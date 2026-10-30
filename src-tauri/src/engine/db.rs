use rusqlite::{Connection, params, Result as SqlResult};
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// 颜色标记的固定色板（Pixcall 式 7 色）。前端镜像同一份列表。
pub const COLOR_LABELS: &[(&str, &str)] = &[
    ("red", "#e03131"),
    ("orange", "#f76707"),
    ("yellow", "#f59f00"),
    ("green", "#2f9e44"),
    ("blue", "#1971c2"),
    ("purple", "#9c36b5"),
    ("gray", "#868e96"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub id: i64,
    pub path: String,
    pub filename: String,
    pub extension: String,
    pub size_bytes: i64,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub blake3_hash: Option<String>,
    pub phash: Option<String>,
    pub mtime: String,
    pub ctime: String,
    pub rating: Option<i32>,
    pub color_label: Option<String>,
    pub notes: Option<String>,
    pub is_deleted: bool,
    /// When the file was first imported into the library (DB insert time).
    pub created_at: String,
    /// When this DB record was last refreshed (re-index / metadata edit).
    pub updated_at: String,
    /// 收藏（Pixcall 式快速标记）。
    pub favorite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub color: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTag {
    pub file_id: i64,
    pub tag_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartFolder {
    pub id: i64,
    pub name: String,
    pub rules_json: String,
    pub icon: Option<String>,
}

/// 侧边栏「文件夹」树的一个节点。`file_count` 只统计**直接**放在该文件夹
/// 里的图片（子文件夹的不算），与资源管理器的观感一致。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderNode {
    pub name: String,
    /// 该文件夹的完整路径（用系统原生分隔符，和 files.path 一致）。
    pub path: String,
    pub file_count: i64,
    pub children: Vec<FolderNode>,
}

/// 文件查询过滤器（智能视图 / 文件夹 / 标签 / 颜色 / 评分都靠它组合）。
#[derive(Debug, Clone, Default)]
pub struct FileFilter {
    pub ext: Option<String>,
    pub tag_id: Option<i64>,
    /// 只看某个文件夹（含子文件夹）下的文件。
    pub folder: Option<String>,
    /// 只看收藏 / 只看未收藏。
    pub favorite: Option<bool>,
    /// 只看某个颜色标记（`red` / `orange` / …）。
    pub color: Option<String>,
    /// 评分下限（含），1 表示「已评分」。
    pub min_rating: Option<i32>,
    /// 只看最近 N 天内**导入**的文件（按 `created_at`，不是文件系统时间）。
    /// 「最近新增」视图用它做时间窗口，N 由前端设置决定（7 / 30）。
    pub recent_days: Option<i64>,
    /// 是否只包含回收站里的文件（默认 false）。
    pub include_deleted: bool,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> SqlResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let conn = Connection::open(path)?;
        
        // Performance pragmas
        conn.execute_batch("
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA cache_size = -64000;  -- 64MB cache
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;
            PRAGMA temp_store = MEMORY;
            PRAGMA mmap_size = 268435456;  -- 256MB mmap
        ")?;

        let db = Database { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> SqlResult<()> {
        self.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS files (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                path        TEXT NOT NULL UNIQUE,
                filename    TEXT NOT NULL,
                extension   TEXT NOT NULL DEFAULT '',
                size_bytes  INTEGER NOT NULL DEFAULT 0,
                width       INTEGER,
                height      INTEGER,
                blake3_hash TEXT,
                phash       TEXT,
                mtime       TEXT NOT NULL DEFAULT '',
                ctime       TEXT NOT NULL DEFAULT '',
                rating      INTEGER DEFAULT 0,
                color_label TEXT,
                notes       TEXT,
                is_deleted  INTEGER DEFAULT 0,
                created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
                favorite    INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);
            CREATE INDEX IF NOT EXISTS idx_files_hash ON files(blake3_hash);
            CREATE INDEX IF NOT EXISTS idx_files_mtime ON files(mtime);
            CREATE INDEX IF NOT EXISTS idx_files_extension ON files(extension);
            CREATE INDEX IF NOT EXISTS idx_files_rating ON files(rating);
            CREATE INDEX IF NOT EXISTS idx_files_deleted ON files(is_deleted);
            CREATE INDEX IF NOT EXISTS idx_files_favorite ON files(favorite);

            CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(
                filename,
                path,
                notes,
                tokenize = 'unicode61'
            );

            CREATE TABLE IF NOT EXISTS tags (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                name      TEXT NOT NULL UNIQUE,
                parent_id INTEGER,
                color     TEXT,
                icon      TEXT,
                FOREIGN KEY (parent_id) REFERENCES tags(id) ON DELETE SET NULL
            );

            CREATE INDEX IF NOT EXISTS idx_tags_name ON tags(name);
            CREATE INDEX IF NOT EXISTS idx_tags_parent ON tags(parent_id);

            CREATE TABLE IF NOT EXISTS file_tags (
                file_id INTEGER NOT NULL,
                tag_id  INTEGER NOT NULL,
                PRIMARY KEY (file_id, tag_id),
                FOREIGN KEY (file_id) REFERENCES files(id) ON DELETE CASCADE,
                FOREIGN KEY (tag_id)  REFERENCES tags(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_file_tags_file ON file_tags(file_id);
            CREATE INDEX IF NOT EXISTS idx_file_tags_tag ON file_tags(tag_id);

            CREATE TABLE IF NOT EXISTS smart_folders (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                name       TEXT NOT NULL,
                rules_json TEXT NOT NULL DEFAULT '[]',
                icon       TEXT
            );

            CREATE TABLE IF NOT EXISTS snapshots (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                file_id       INTEGER NOT NULL,
                snapshot_path TEXT NOT NULL,
                created_at    TEXT NOT NULL DEFAULT (datetime('now')),
                trigger       TEXT NOT NULL DEFAULT 'auto',
                FOREIGN KEY (file_id) REFERENCES files(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_snapshots_file ON snapshots(file_id);

            CREATE TABLE IF NOT EXISTS library_meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
        ")?;

        // ── Migrations for libraries created by older releases ──────────
        // `updated_at` was introduced in schema v2. Add it in place for
        // existing libraries and back-fill from `created_at`.
        let existing_cols: Vec<String> = {
            let mut stmt = self.conn.prepare("PRAGMA table_info(files)")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
            rows.filter_map(|r| r.ok()).collect()
        };
        if !existing_cols.iter().any(|c| c == "updated_at") {
            self.conn.execute(
                "ALTER TABLE files ADD COLUMN updated_at TEXT NOT NULL DEFAULT ''",
                [],
            )?;
            self.conn.execute(
                "UPDATE files SET updated_at = created_at WHERE updated_at = ''",
                [],
            )?;
        }

        // schema v3: `favorite`（收藏标记）。
        if !existing_cols.iter().any(|c| c == "favorite") {
            self.conn.execute(
                "ALTER TABLE files ADD COLUMN favorite INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }

        // Insert / bump schema version
        self.conn.execute(
            "INSERT INTO library_meta (key, value) VALUES ('schema_version', '3')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )?;

        Ok(())
    }

    // ── File CRUD ──────────────────────────────────────

    /// 把一行查询结果映射成 FileEntry（18 列，顺序见各 SELECT）。
    fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<FileEntry> {
        Ok(FileEntry {
            id: row.get(0)?,
            path: row.get(1)?,
            filename: row.get(2)?,
            extension: row.get(3)?,
            size_bytes: row.get(4)?,
            width: row.get(5)?,
            height: row.get(6)?,
            blake3_hash: row.get(7)?,
            phash: row.get(8)?,
            mtime: row.get(9)?,
            ctime: row.get(10)?,
            rating: row.get(11)?,
            color_label: row.get(12)?,
            notes: row.get(13)?,
            is_deleted: row.get(14)?,
            created_at: row.get(15)?,
            updated_at: row.get(16)?,
            favorite: row.get(17)?,
        })
    }

    pub fn insert_file(&self, entry: &FileEntry) -> SqlResult<i64> {
        // Upsert keyed on `path`. On a re-index we must NOT clobber user data
        // (rating / notes / color_label / favorite) nor the original import
        // time (`created_at`); only file-derived fields are refreshed and
        // `updated_at` is bumped.
        self.conn.execute(
            "INSERT INTO files
             (path, filename, extension, size_bytes, width, height, blake3_hash, phash,
              mtime, ctime, rating, color_label, notes, favorite, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
             ON CONFLICT(path) DO UPDATE SET
                filename    = excluded.filename,
                extension   = excluded.extension,
                size_bytes  = excluded.size_bytes,
                width       = excluded.width,
                height      = excluded.height,
                blake3_hash = excluded.blake3_hash,
                phash       = excluded.phash,
                mtime       = excluded.mtime,
                ctime       = excluded.ctime,
                updated_at  = excluded.updated_at",
            params![
                entry.path,
                entry.filename,
                entry.extension,
                entry.size_bytes,
                entry.width,
                entry.height,
                entry.blake3_hash,
                entry.phash,
                entry.mtime,
                entry.ctime,
                entry.rating,
                entry.color_label,
                entry.notes,
                entry.favorite,
                entry.created_at,
                entry.updated_at,
            ],
        )?;

        let id: i64 = self.conn.query_row(
            "SELECT id FROM files WHERE path = ?1",
            params![entry.path],
            |row| row.get(0),
        )?;

        // Keep the FTS mirror in sync from the canonical `files` row so notes
        // edited elsewhere are never lost by a re-index.
        self.conn.execute(
            "INSERT OR REPLACE INTO files_fts(rowid, filename, path, notes)
             SELECT id, filename, path, COALESCE(notes, '') FROM files WHERE id = ?1",
            params![id],
        )?;
        Ok(id)
    }

    pub fn get_file_by_path(&self, path: &str) -> SqlResult<Option<FileEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, filename, extension, size_bytes, width, height, 
                    blake3_hash, phash, mtime, ctime, rating, color_label, notes, 
                    is_deleted, created_at, updated_at, favorite
             FROM files WHERE path = ?1 AND is_deleted = 0"
        )?;
        
        let mut rows = stmt.query_map(params![path], Self::row_to_entry)?;

        match rows.next() {
            Some(Ok(entry)) => Ok(Some(entry)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    /// 组装 WHERE 子句与参数（files 表别名 `f`）。
    fn build_where(filter: &FileFilter) -> (String, Vec<Box<dyn rusqlite::types::ToSql>>) {
        let mut sql = String::new();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if filter.include_deleted {
            sql.push_str("f.is_deleted = 1");
        } else {
            sql.push_str("f.is_deleted = 0");
        }

        if let Some(ext) = &filter.ext {
            sql.push_str(" AND f.extension = ?");
            params.push(Box::new(ext.clone()));
        }

        if let Some(tag_id) = filter.tag_id {
            sql.push_str(" AND EXISTS (SELECT 1 FROM file_tags ft WHERE ft.file_id = f.id AND ft.tag_id = ?)");
            params.push(Box::new(tag_id));
        }

        if let Some(folder) = &filter.folder {
            // 前缀匹配：不用 LIKE，避开 % / _ 转义问题。
            // `folder + sep` ≤ path < folder + (sep+1) 恰好覆盖该文件夹全部子孙
            // （sep 与 sep+1 之间没有别的字符）。
            let sep = std::path::MAIN_SEPARATOR;
            let lower = format!("{}{}", folder, sep);
            let upper = format!("{}{}", folder, (sep as u8 + 1) as char);
            sql.push_str(" AND f.path >= ? AND f.path < ?");
            params.push(Box::new(lower));
            params.push(Box::new(upper));
        }

        if let Some(fav) = filter.favorite {
            sql.push_str(" AND f.favorite = ?");
            params.push(Box::new(fav as i64));
        }

        if let Some(color) = &filter.color {
            sql.push_str(" AND f.color_label = ?");
            params.push(Box::new(color.clone()));
        }

        if let Some(min) = filter.min_rating {
            sql.push_str(" AND COALESCE(f.rating, 0) >= ?");
            params.push(Box::new(min));
        }

        if let Some(days) = filter.recent_days {
            // created_at 是 `datetime('now')` 写进去的 UTC 文本，直接让 SQLite
            // 用同一套时钟做减法，避免前端算时间戳时踩时区 / 格式不一致。
            sql.push_str(" AND f.created_at >= datetime('now', ?)");
            params.push(Box::new(format!("-{} days", days)));
        }

        (sql, params)
    }

    pub fn get_files_paginated(
        &self,
        offset: i64,
        limit: i64,
        sort_by: &str,
        sort_order: &str,
        filter: &FileFilter,
    ) -> SqlResult<Vec<FileEntry>> {
        let order_clause = match sort_by {
            "mtime" => format!("f.mtime {}", sort_order),
            "size" => format!("f.size_bytes {}", sort_order),
            "name" => format!("f.filename {}", sort_order),
            "rating" => format!("f.rating {}", sort_order),
            "created_at" => format!("f.created_at {}", sort_order),
            _ => format!("f.mtime DESC"),
        };

        let (where_clause, param_values) = Self::build_where(filter);

        let mut sql = format!(
            "SELECT f.id, f.path, f.filename, f.extension, f.size_bytes, f.width, f.height,
                    f.blake3_hash, f.phash, f.mtime, f.ctime, f.rating, f.color_label, f.notes,
                    f.is_deleted, f.created_at, f.updated_at, f.favorite
             FROM files f
             WHERE {}",
            where_clause
        );

        sql.push_str(&format!(" ORDER BY {}", order_clause));
        sql.push_str(&format!(" LIMIT {} OFFSET {}", limit, offset));

        let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
        
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), Self::row_to_entry)?;

        rows.collect::<Result<Vec<_>, _>>()
    }

    pub fn count_files(&self, filter: &FileFilter) -> SqlResult<i64> {
        let (where_clause, params) = Self::build_where(filter);
        let sql = format!("SELECT COUNT(*) FROM files f WHERE {}", where_clause);
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        self.conn.query_row(&sql, params_refs.as_slice(), |row| row.get(0))
    }

    /// Every live file path (plus whatever content hash it has) — used to work
    /// out which thumbnails still need to be generated. Metadata only: this is
    /// a single indexed query and never touches the image files themselves.
    ///
    /// 按 `mtime DESC` 排序 —— 与网格默认排序一致，这样后台任务会先处理用户
    /// 一打开就看到的那些图，首屏几秒内就填满，而不是等最后一屏。
    pub fn list_files_for_thumbs(&self) -> SqlResult<Vec<(String, Option<String>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, blake3_hash FROM files WHERE is_deleted = 0 ORDER BY mtime DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn update_file_rating(&self, path: &str, rating: i32) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE files SET rating = ?1, updated_at = datetime('now') WHERE path = ?2",
            params![rating, path],
        )?;
        Ok(())
    }

    pub fn update_file_notes(&self, path: &str, notes: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE files SET notes = ?1, updated_at = datetime('now') WHERE path = ?2",
            params![notes, path],
        )?;
        // Also update FTS index
        self.conn.execute(
            "UPDATE files_fts SET notes = ?1 WHERE rowid = (SELECT id FROM files WHERE path = ?2)",
            params![notes, path],
        )?;
        Ok(())
    }

    /// 收藏 / 取消收藏。
    pub fn update_file_favorite(&self, path: &str, favorite: bool) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE files SET favorite = ?1, updated_at = datetime('now') WHERE path = ?2",
            params![favorite as i64, path],
        )?;
        Ok(())
    }

    /// 设置颜色标记（None = 清除）。
    pub fn update_file_color(&self, path: &str, color: Option<&str>) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE files SET color_label = ?1, updated_at = datetime('now') WHERE path = ?2",
            params![color, path],
        )?;
        Ok(())
    }

    pub fn soft_delete_file(&self, path: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE files SET is_deleted = 1 WHERE path = ?1",
            params![path],
        )?;
        Ok(())
    }

    /// List all soft-deleted files (trash contents).
    pub fn get_deleted_files(&self) -> SqlResult<Vec<FileEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, filename, extension, size_bytes, width, height,
                    blake3_hash, phash, mtime, ctime, rating, color_label, notes,
                    is_deleted, created_at, updated_at, favorite
             FROM files WHERE is_deleted = 1 ORDER BY mtime DESC"
        )?;
        let rows = stmt.query_map([], Self::row_to_entry)?;
        rows.collect::<Result<Vec<_>, _>>()
    }

    /// Restore a soft-deleted file back to the active library.
    pub fn restore_file(&self, path: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE files SET is_deleted = 0 WHERE path = ?1",
            params![path],
        )?;
        Ok(())
    }

    /// Permanently remove a file (and its FTS entry) from the database.
    pub fn purge_file(&self, path: &str) -> SqlResult<()> {
        // Remove FTS row first (referenced by files.id rowid).
        self.conn.execute(
            "DELETE FROM files_fts WHERE rowid = (SELECT id FROM files WHERE path = ?1)",
            params![path],
        )?;
        self.conn.execute(
            "DELETE FROM files WHERE path = ?1",
            params![path],
        )?;
        Ok(())
    }

    /// 回收站里的条目数（只 COUNT，不取行 —— 侧栏徽标频繁调用）。
    pub fn count_deleted(&self) -> SqlResult<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files WHERE is_deleted = 1", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// 按路径批量移除记录。
    ///
    /// 「彻底删除时一并删磁盘文件」这条路径用它：只把**真正删成功的文件**
    /// 从库里摘掉，删失败的那些记录留着，用户还能再试一次。
    pub fn purge_paths(&self, paths: &[String]) -> SqlResult<usize> {
        let mut n = 0;
        for p in paths {
            self.purge_file(p)?;
            n += 1;
        }
        Ok(n)
    }

    /// Permanently remove every soft-deleted file (empty the trash).
    pub fn purge_all_deleted(&self) -> SqlResult<usize> {
        self.conn.execute(
            "DELETE FROM files_fts WHERE rowid IN (SELECT id FROM files WHERE is_deleted = 1)",
            [],
        )?;
        let n = self.conn.execute(
            "DELETE FROM files WHERE is_deleted = 1",
            [],
        )?;
        Ok(n)
    }

    pub fn get_files_by_hash(&self, hash: &str) -> SqlResult<Vec<FileEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, filename, extension, size_bytes, width, height,
                    blake3_hash, phash, mtime, ctime, rating, color_label, notes,
                    is_deleted, created_at, updated_at, favorite
             FROM files WHERE blake3_hash = ?1 AND is_deleted = 0"
        )?;
        let rows = stmt.query_map(params![hash], Self::row_to_entry)?;
        rows.collect::<Result<Vec<_>, _>>()
    }

    // ── Folder tree (sidebar "文件夹" section) ─────────

    /// 库内文件夹树。只读 files.path（一条查询），在内存里聚合；
    /// `file_count` 为直接子文件数。库根自身作为返回树的根节点。
    pub fn get_folder_tree(&self, root: &str) -> SqlResult<FolderNode> {
        let mut stmt = self.conn.prepare("SELECT path FROM files WHERE is_deleted = 0")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut paths: Vec<String> = Vec::new();
        for row in rows {
            paths.push(row?);
        }

        let sep = std::path::MAIN_SEPARATOR;
        let root_norm = root.trim_end_matches(['/', '\\']).to_string();

        // path -> (name, direct_file_count)
        let mut dirs: std::collections::BTreeMap<String, (String, i64)> =
            std::collections::BTreeMap::new();

        for p in &paths {
            // 元数据目录不进树。
            if p.split(['/', '\\']).any(|c| c == ".pixvault" || c == ".prism") {
                continue;
            }
            let Some(idx) = p.rfind(sep) else { continue };
            // Eagle 的素材是「一条一个壳目录」：`<ITEM_ID>.info/photo.jpg`。
            // 那一层不是用户概念里的文件夹，只是文件自己的容器 —— 不折叠掉的话，
            // 侧栏「文件夹」会被几百个 MVQ55RU1NL8GV.info 淹掉。
            // 这里只影响侧栏展示与筛选归属，磁盘结构原样不动。
            let mut dir = &p[..idx];
            while let Some(name) = dir.rsplit(sep).next() {
                if !is_item_container(name) {
                    break;
                }
                match dir.rfind(sep) {
                    Some(i) => dir = &dir[..i],
                    None => {
                        dir = "";
                        break;
                    }
                }
            }
            if dir.is_empty() {
                continue;
            }
            // 登记这条路径上的每一级目录（直到库根）。
            let mut cur = dir.to_string();
            loop {
                if cur == root_norm || cur.is_empty() {
                    break;
                }
                let name = cur
                    .rsplit(sep)
                    .next()
                    .unwrap_or(cur.as_str())
                    .to_string();
                dirs.entry(cur.clone()).or_insert((name, 0));
                match cur.rfind(sep) {
                    Some(i) => cur = cur[..i].to_string(),
                    None => break,
                }
            }
            if let Some(entry) = dirs.get_mut(dir) {
                entry.1 += 1;
            }
        }

        // 递归组装（BTreeMap 有序，子节点天然按名称排序）。
        fn assemble(
            dirs: &std::collections::BTreeMap<String, (String, i64)>,
            path: &str,
            name: String,
            count: i64,
        ) -> FolderNode {
            let sep = std::path::MAIN_SEPARATOR;
            let prefix = format!("{}{}", path, sep);
            let children: Vec<FolderNode> = dirs
                .iter()
                .filter(|(k, _)| k.starts_with(&prefix) && !k[prefix.len()..].contains(sep))
                .map(|(k, (n, c))| assemble(dirs, k, n.clone(), *c))
                .collect();
            FolderNode { name, path: path.to_string(), file_count: count, children }
        }

        let root_name = root_norm
            .rsplit(sep)
            .next()
            .unwrap_or(root_norm.as_str())
            .to_string();
        let root_count = dirs.get(&root_norm).map(|(_, c)| *c).unwrap_or(0);
        // 库根本身可能不在 dirs 里（所有图片都在子文件夹时）—— 补一个占位。
        dirs.entry(root_norm.clone()).or_insert((root_name.clone(), 0));

        Ok(assemble(&dirs, &root_norm, root_name, root_count))
    }

    // ── Tag CRUD ───────────────────────────────────────

    pub fn create_tag(&self, name: &str, parent_id: Option<i64>, color: Option<&str>) -> SqlResult<i64> {
        self.conn.execute(
            "INSERT OR IGNORE INTO tags (name, parent_id, color) VALUES (?1, ?2, ?3)",
            params![name, parent_id, color],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Find a tag by name (case-sensitive). Returns its id if it exists.
    pub fn find_tag_by_name(&self, name: &str) -> SqlResult<Option<i64>> {
        let mut stmt = self.conn.prepare("SELECT id FROM tags WHERE name = ?1")?;
        let mut rows = stmt.query_map(params![name], |row| row.get::<_, i64>(0))?;
        Ok(rows.next().transpose()?)
    }

    /// Get or create a tag, returning its id. Handles hierarchical `a/b/c`
    /// names by creating parent tags as needed.
    pub fn get_or_create_tag(&self, name: &str) -> SqlResult<i64> {
        if let Some(id) = self.find_tag_by_name(name)? {
            return Ok(id);
        }
        // Hierarchical path: ensure parents exist first.
        let parts: Vec<&str> = name.split('/').filter(|s| !s.is_empty()).collect();
        let mut parent_id: Option<i64> = None;
        let mut current_path = String::new();
        for (i, part) in parts.iter().enumerate() {
            current_path = if i == 0 {
                part.to_string()
            } else {
                format!("{}/{}", current_path, part)
            };
            let existing = self.find_tag_by_name(&current_path)?;
            match existing {
                Some(id) => parent_id = Some(id),
                None => {
                    self.conn.execute(
                        "INSERT INTO tags (name, parent_id) VALUES (?1, ?2)",
                        params![current_path, parent_id],
                    )?;
                    parent_id = Some(self.conn.last_insert_rowid());
                }
            }
        }
        Ok(parent_id.unwrap_or_else(|| {
            // Fallback: plain insert
            self.conn
                .execute("INSERT INTO tags (name) VALUES (?1)", params![name])
                .ok();
            self.conn.last_insert_rowid()
        }))
    }

    pub fn get_all_tags(&self) -> SqlResult<Vec<Tag>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, parent_id, color, icon FROM tags ORDER BY name"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                color: row.get(3)?,
                icon: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
    }

    pub fn add_file_tag(&self, file_id: i64, tag_id: i64) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO file_tags (file_id, tag_id) VALUES (?1, ?2)",
            params![file_id, tag_id],
        )?;
        Ok(())
    }

    pub fn remove_file_tag(&self, file_id: i64, tag_id: i64) -> SqlResult<()> {
        self.conn.execute(
            "DELETE FROM file_tags WHERE file_id = ?1 AND tag_id = ?2",
            params![file_id, tag_id],
        )?;
        Ok(())
    }

    pub fn get_file_tags(&self, file_id: i64) -> SqlResult<Vec<Tag>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.name, t.parent_id, t.color, t.icon
             FROM tags t
             JOIN file_tags ft ON t.id = ft.tag_id
             WHERE ft.file_id = ?1
             ORDER BY t.name"
        )?;
        let rows = stmt.query_map(params![file_id], |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                color: row.get(3)?,
                icon: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
    }

    // ── Search ─────────────────────────────────────────

    pub fn search_fts(&self, query: &str, limit: i64) -> SqlResult<Vec<FileEntry>> {
        // Try FTS5 first with wildcard
        let fts_query = if query.contains('*') || query.contains('"') {
            query.to_string()
        } else {
            // Add prefix wildcard for each word
            query.split_whitespace()
                .map(|w| format!("{}*", w))
                .collect::<Vec<_>>()
                .join(" ")
        };
        
        let mut stmt = self.conn.prepare(
            "SELECT f.id, f.path, f.filename, f.extension, f.size_bytes, f.width, f.height,
                    f.blake3_hash, f.phash, f.mtime, f.ctime, f.rating, f.color_label, f.notes,
                    f.is_deleted, f.created_at, f.updated_at, f.favorite
             FROM files_fts fts
             JOIN files f ON fts.rowid = f.id
             WHERE files_fts MATCH ?1 AND f.is_deleted = 0
             ORDER BY rank
             LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![fts_query, limit], Self::row_to_entry)?;
        let results: Vec<FileEntry> = rows.collect::<Result<Vec<_>, _>>()?;
        
        // Fallback to LIKE if FTS returns nothing
        if results.is_empty() {
            return self.search_like(query, limit);
        }
        Ok(results)
    }

    fn search_like(&self, query: &str, limit: i64) -> SqlResult<Vec<FileEntry>> {
        let pattern = format!("%{}%", query);
        let mut stmt = self.conn.prepare(
            "SELECT id, path, filename, extension, size_bytes, width, height,
                    blake3_hash, phash, mtime, ctime, rating, color_label, notes,
                    is_deleted, created_at, updated_at, favorite
             FROM files
             WHERE is_deleted = 0 AND (filename LIKE ?1 OR path LIKE ?1 OR notes LIKE ?1)
             ORDER BY mtime DESC
             LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![pattern, limit], Self::row_to_entry)?;
        rows.collect::<Result<Vec<_>, _>>()
    }

    // ── Snapshots ──────────────────────────────────────

    pub fn create_snapshot(&self, file_id: i64, snapshot_path: &str, trigger: &str) -> SqlResult<i64> {
        self.conn.execute(
            "INSERT INTO snapshots (file_id, snapshot_path, trigger) VALUES (?1, ?2, ?3)",
            params![file_id, snapshot_path, trigger],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_snapshots_for_file(&self, file_id: i64) -> SqlResult<Vec<(i64, String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, snapshot_path, created_at, trigger FROM snapshots WHERE file_id = ?1 ORDER BY created_at DESC"
        )?;
        let rows = stmt.query_map(params![file_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>()
    }

    // ── Transaction support ────────────────────────────

    pub fn transaction<F, T>(&mut self, f: F) -> SqlResult<T>
    where
        F: FnOnce(&Connection) -> SqlResult<T>,
    {
        self.conn.execute("BEGIN IMMEDIATE", [])?;
        match f(&self.conn) {
            Ok(result) => {
                self.conn.execute("COMMIT", [])?;
                Ok(result)
            }
            Err(e) => {
                self.conn.execute("ROLLBACK", []).ok();
                Err(e)
            }
        }
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

/// 判断一个目录名是不是「一条素材自己的壳」。
///
/// Eagle 的库与素材包都是这个结构：每条素材一个 `<ITEM_ID>.info/` 目录，
/// 原图、缩略图、metadata.json 都塞在里面。它不是用户建的文件夹，只是文件的容器，
/// 所以在文件夹树里要折叠掉 —— 否则一个几百条素材的库，侧栏会列出几百个
/// `MVQ55RU1NL8GV.info`。
///
/// 只认 `.info` 后缀这一个特征。真有人把文件夹起名叫 `photos.info` 会被误折叠，
/// 但代价只是「这层不在侧栏出现」，文件本身照样在库根下可见，不会丢。
fn is_item_container(name: &str) -> bool {
    name.len() > 5 && name.to_ascii_lowercase().ends_with(".info")
}
