//! Importers for third-party asset libraries.
//!
//! Two formats are supported:
//! - **Eagle** (`.library`): a folder containing a root `metadata.json` plus an
//!   `images/` folder where each item lives in an id-named subfolder with its
//!   own `metadata.json`.
//! - **Pixcall** (`.pixcall`): a SQLite database (`database/main.db`) plus the
//!   plain files, organised via `entries`/`folders`/`tags` tables.
//!
//! Both store the actual files as ordinary files, so import means: read the
//! metadata, map folder hierarchy + tags into PixVault tags, and record
//! notes / rating / source link against each file path.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// A single imported item, resolved to an absolute file path plus metadata.
#[derive(Debug, Clone)]
pub struct ImportItem {
    /// Absolute path to the original file on disk.
    pub path: PathBuf,
    /// Tag names, including hierarchy expressed as `parent/child` paths.
    pub tags: Vec<String>,
    /// Free-form notes / description.
    pub notes: Option<String>,
    /// Star rating 0-5 (or Eagle's 1-5 mapped in).
    pub rating: Option<i32>,
    /// Source / origin URL.
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryKind {
    Eagle,
    Pixcall,
}

/// Detect which library format a directory is.
pub fn detect_kind(root: &Path) -> Option<LibraryKind> {
    if root.join("metadata.json").is_file() && root.join("images").is_dir() {
        return Some(LibraryKind::Eagle);
    }
    if root.join(".pixcall").join("database").join("main.db").is_file() {
        return Some(LibraryKind::Pixcall);
    }
    // Eagle library folder itself carries a `.library` extension but the user
    // may have navigated into it; accept a child `images` + `metadata.json`.
    None
}

// ───────────────────────── Eagle ─────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct EagleRoot {
    #[serde(default)]
    pub(crate) folders: Vec<EagleFolder>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EagleFolder {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) children: Vec<EagleFolder>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EagleItemMeta {
    #[serde(default)]
    pub(crate) id: Option<String>,
    /// 原始文件名（**不含扩展名**，Eagle 就是这样存的）。
    #[serde(default)]
    pub(crate) name: Option<String>,
    /// 扩展名，不含点。
    #[serde(default)]
    pub(crate) ext: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) folders: Vec<String>,
    #[serde(default)]
    pub(crate) annotation: Option<String>,
    #[serde(default)]
    pub(crate) note: Option<String>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) star: Option<i32>,
    /// 回收站里的条目仍会留在包内，导入时要跳过。
    #[serde(default, rename = "isDeleted")]
    pub(crate) is_deleted: bool,
}

/// 把 Eagle 的文件夹树摊平成 `id -> "父/子"` 的映射。
///
/// Eagle 的文件夹是**虚拟**的：磁盘上所有素材扁平地躺在 `images/<ID>.info/`，
/// 归属关系只记在条目自己的 `folders: [id]` 里。所以要还原层级，只能靠这棵树。
pub(crate) fn folder_path_map(folders: &[EagleFolder]) -> HashMap<String, String> {
    fn walk(folders: &[EagleFolder], prefix: &str, out: &mut HashMap<String, String>) {
        for f in folders {
            let full = if prefix.is_empty() {
                f.name.clone()
            } else {
                format!("{}/{}", prefix, f.name)
            };
            out.insert(f.id.clone(), full.clone());
            walk(&f.children, &full, out);
        }
    }
    let mut out = HashMap::new();
    walk(folders, "", &mut out);
    out
}

/// 一条 Eagle 元数据 → PixVault 的（标签, 备注, 评分, 来源）。
///
/// Eagle 库（.library）和 eaglepack 共用这一套映射。抽出来是为了避免两边规则
/// 各写一份后漂移 —— 比如「annotation 优先于 note」「文件夹当层级标签」这两条，
/// 只有一处说了算。
pub(crate) fn map_meta(
    meta: &EagleItemMeta,
    folder_paths: &HashMap<String, String>,
) -> (Vec<String>, Option<String>, Option<i32>, Option<String>) {
    let mut tags = meta.tags.clone();
    for fid in &meta.folders {
        if let Some(path) = folder_paths.get(fid) {
            if !path.is_empty() && !tags.iter().any(|t| t == path) {
                tags.push(path.clone());
            }
        }
    }
    // Notes: prefer annotation, fall back to note.
    let notes = meta
        .annotation
        .clone()
        .or_else(|| meta.note.clone())
        .filter(|s| !s.is_empty());
    // Eagle star 是 1-5，PixVault 用 0-5 且 0 表示未评分 → 0 视作 None。
    let rating = meta.star.filter(|s| *s > 0);
    let source = meta.url.clone().filter(|s| !s.is_empty());
    (tags, notes, rating, source)
}

pub fn import_eagle(root: &Path) -> Result<Vec<ImportItem>, String> {
    let root_meta_path = root.join("metadata.json");
    let root_json = std::fs::read_to_string(&root_meta_path)
        .map_err(|e| format!("读取 metadata.json 失败: {}", e))?;
    let eagle_root: EagleRoot = serde_json::from_str(&root_json)
        .map_err(|e| format!("解析 Eagle metadata.json 失败: {}", e))?;

    let folder_paths = folder_path_map(&eagle_root.folders);

    let images_dir = root.join("images");
    let mut items: Vec<ImportItem> = Vec::new();

    for entry in std::fs::read_dir(&images_dir)
        .map_err(|e| format!("读取 images 目录失败: {}", e))?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let item_dir = entry.path();
        let meta_path = item_dir.join("metadata.json");
        let Ok(json) = std::fs::read_to_string(&meta_path) else { continue };
        let Ok(meta) = serde_json::from_str::<EagleItemMeta>(&json) else { continue };
        // 回收站里的条目仍在包里，不入库
        if meta.is_deleted {
            continue;
        }

        // Resolve the original file, using the recorded name/ext as a hint.
        let Some(original) =
            find_original_file(&item_dir, meta.name.as_deref(), meta.ext.as_deref())
        else {
            continue;
        };

        let (tags, notes, rating, source) = map_meta(&meta, &folder_paths);

        items.push(ImportItem { path: original, tags, notes, rating, source });
    }

    Ok(items)
}

/// 判断文件名是不是 Eagle 的附属文件（元数据 / 缩略图 / 网页快捷方式）。
///
/// 缩略图有**两种**命名，都真实存在过：`thumbnail.png` 与 `<原名>_thumbnail.png`。
/// 老代码只跳过前缀式，后缀式的会被当成原图导进图库 —— 用户看到的是「每张图
/// 都多出一条缩略图记录」。这里两种都认。
pub(crate) fn is_sidecar_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    if lower == "metadata.json" || lower == ".ds_store" {
        return true;
    }
    // 网页书签条目会附带 Windows / macOS 快捷方式
    if lower.ends_with(".url") || lower.ends_with(".webloc") {
        return true;
    }
    let stem = match lower.rsplit_once('.') {
        Some((s, _)) => s,
        None => lower.as_str(),
    };
    stem == "thumbnail" || stem.ends_with("_thumbnail")
}

/// 在 Eagle 条目目录里找出原始素材文件。
///
/// 目录里除原图外还有 metadata.json、缩略图和快捷方式。挑选顺序：① 文件名精确
/// 等于 metadata 的 `name.ext` ② 同名任意扩展名 ③ 体积最大的那个。
///
/// 为什么不能像以前那样「取第一个非元数据文件就 break」：`read_dir` 的返回顺序
/// 是文件系统给的，没有保证。同一份包在两台机器上可能选中不同的文件，取到缩略图
/// 就是随机事故 —— 而且是那种「别人机器上正常、我这就糊」的玄学 bug。
pub(crate) fn find_original_file(
    dir: &Path,
    name_hint: Option<&str>,
    ext_hint: Option<&str>,
) -> Option<PathBuf> {
    let mut candidates: Vec<(PathBuf, u64)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if !e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let file_name = e.file_name().to_string_lossy().to_string();
            if is_sidecar_name(&file_name) {
                continue;
            }
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            candidates.push((e.path(), size));
        }
    }
    if candidates.is_empty() {
        return None;
    }

    if let Some(hint) = name_hint.filter(|s| !s.is_empty()) {
        if let Some(ext) = ext_hint.filter(|s| !s.is_empty()) {
            let want = format!("{}.{}", hint, ext).to_lowercase();
            if let Some((p, _)) = candidates.iter().find(|(p, _)| {
                p.file_name()
                    .map(|n| n.to_string_lossy().to_lowercase() == want)
                    .unwrap_or(false)
            }) {
                return Some(p.clone());
            }
        }
        let hint_lower = hint.to_lowercase();
        if let Some((p, _)) = candidates.iter().find(|(p, _)| {
            p.file_stem()
                .map(|s| s.to_string_lossy().to_lowercase() == hint_lower)
                .unwrap_or(false)
        }) {
            return Some(p.clone());
        }
    }

    candidates.sort_by(|a, b| b.1.cmp(&a.1));
    candidates.first().map(|(p, _)| p.clone())
}

// ───────────────────────── Pixcall ─────────────────────────

pub fn import_pixcall(root: &Path) -> Result<Vec<ImportItem>, String> {
    let db_path = root.join(".pixcall").join("database").join("main.db");
    if !db_path.is_file() {
        return Err("未找到 Pixcall 数据库 (main.db)".to_string());
    }

    // Open read-only against the live WAL so unflushed data is visible.
    let conn = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("打开 Pixcall 数据库失败: {}", e))?;

    // Load folders: id -> (parent_id, name)
    let mut folder_map: HashMap<i64, (i64, String)> = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT id, parent_id, name FROM entries WHERE kind = 0")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?))
            })
            .map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            folder_map.insert(row.0, (row.1, row.2));
        }
    }

    // Load tags: id -> name
    let mut tag_map: HashMap<i64, String> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT id, name FROM tags").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            tag_map.insert(row.0, row.1);
        }
    }

    // Resolve a folder's full hierarchical path. The virtual roots (parent_id
    // < 0) represent "Pixcall" and "Trash" — they are not real folders on
    // disk, so they map to an empty path.
    fn folder_full_path(
        id: i64,
        map: &HashMap<i64, (i64, String)>,
        memo: &mut HashMap<i64, String>,
    ) -> String {
        if let Some(cached) = memo.get(&id) {
            return cached.clone();
        }
        let (parent, name) = match map.get(&id) {
            Some(v) => v.clone(),
            None => return String::new(),
        };
        let full = if parent < 0 {
            String::new()
        } else {
            let p = folder_full_path(parent, map, memo);
            if p.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", p, name)
            }
        };
        memo.insert(id, full.clone());
        full
    }

    let mut memo: HashMap<i64, String> = HashMap::new();

    // Load files: kind = 1
    let mut items: Vec<ImportItem> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT id, parent_id, name, description, link, tags, rating \
                 FROM entries WHERE kind = 1 AND is_deleted = 0 AND parent_id != 2",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<i32>>(6)?,
                ))
            })
            .map_err(|e| e.to_string())?;

        for row in rows {
            let Ok((_id, parent_id, name, description, link, tags_raw, rating)) = row else {
                continue;
            };

            // Resolve physical path from the folder chain.
            let rel_dir = if parent_id >= 0 {
                folder_full_path(parent_id, &folder_map, &mut memo)
            } else {
                String::new()
            };
            let path = if rel_dir.is_empty() {
                root.join(&name)
            } else {
                root.join(&rel_dir).join(&name)
            };
            if !path.exists() {
                continue;
            }

            // Tags: explicit tag ids (comma/JSON) + folder path.
            let mut tags: Vec<String> = Vec::new();
            if let Some(raw) = &tags_raw {
                for t in parse_pixcall_tags(raw, &tag_map) {
                    tags.push(t);
                }
            }
            if !rel_dir.is_empty() {
                tags.push(rel_dir.clone());
            }

            items.push(ImportItem {
                path,
                tags,
                notes: description.clone().filter(|s| !s.is_empty()),
                rating: rating.filter(|r| *r > 0),
                source: link.clone().filter(|s| !s.is_empty()),
            });
        }
    }

    Ok(items)
}

/// Parse Pixcall `entries.tags`: it may be a JSON array of ids, or a
/// comma-separated id list, or already tag names.
fn parse_pixcall_tags(raw: &str, tag_map: &HashMap<i64, String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return out;
    }

    // Try JSON array first.
    if trimmed.starts_with('[') {
        if let Ok(arr) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(list) = arr.as_array() {
                for v in list {
                    if let Some(id) = v.as_i64() {
                        if let Some(name) = tag_map.get(&id) {
                            out.push(name.clone());
                        }
                    } else if let Some(s) = v.as_str() {
                        out.push(s.to_string());
                    }
                }
                return out;
            }
        }
    }

    // Comma-separated numeric ids.
    for part in trimmed.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        if let Ok(id) = p.parse::<i64>() {
            if let Some(name) = tag_map.get(&id) {
                out.push(name.clone());
            }
        } else {
            out.push(p.to_string());
        }
    }
    out
}
