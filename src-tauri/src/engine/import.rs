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
struct EagleRoot {
    #[serde(default)]
    folders: Vec<EagleFolder>,
}

#[derive(Debug, Deserialize)]
struct EagleFolder {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    children: Vec<EagleFolder>,
}

#[derive(Debug, Deserialize)]
struct EagleItemMeta {
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    folders: Vec<String>,
    #[serde(default)]
    annotation: Option<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    star: Option<i32>,
}

pub fn import_eagle(root: &Path) -> Result<Vec<ImportItem>, String> {
    let root_meta_path = root.join("metadata.json");
    let root_json = std::fs::read_to_string(&root_meta_path)
        .map_err(|e| format!("读取 metadata.json 失败: {}", e))?;
    let eagle_root: EagleRoot = serde_json::from_str(&root_json)
        .map_err(|e| format!("解析 Eagle metadata.json 失败: {}", e))?;

    // Build folder id -> full path name (root folders, then nested).
    let mut folder_paths: HashMap<String, String> = HashMap::new();
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
    walk(&eagle_root.folders, "", &mut folder_paths);

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

        // Resolve the original file: usually a single non-metadata file.
        let original = find_original_file(&item_dir);
        let Some(original) = original else { continue };

        // Tags = explicit tags + folder memberships (as hierarchical tags).
        let mut tags: Vec<String> = meta.tags.clone();
        for fid in &meta.folders {
            if let Some(path) = folder_paths.get(fid) {
                tags.push(path.clone());
            }
        }

        // Notes: prefer annotation, fall back to note.
        let notes = meta.annotation.clone().or(meta.note.clone()).filter(|s| !s.is_empty());

        // Eagle star is 1-5; PixVault uses 0-5 where 0 = none. Keep as-is
        // but treat missing as None.
        let rating = meta.star.filter(|s| *s > 0);

        items.push(ImportItem {
            path: original,
            tags,
            notes,
            rating,
            source: meta.url.clone().filter(|s| !s.is_empty()),
        });
    }

    Ok(items)
}

/// Find the original media file inside an Eagle item subfolder.
fn find_original_file(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<PathBuf> = None;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name == "metadata.json" || name.starts_with("thumbnail") {
                continue;
            }
            if e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                best = Some(e.path());
                break;
            }
        }
    }
    best
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
