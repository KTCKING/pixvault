use tauri::State;
use crate::AppState;
use crate::commands::files::FileInfo;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchParams {
    pub text: Option<String>,
    pub tags: Vec<String>,
    pub extensions: Vec<String>,
    pub min_rating: Option<i32>,
    pub color_label: Option<String>,
    pub min_width: Option<i32>,
    pub min_height: Option<i32>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub files: Vec<FileInfo>,
    pub total: i64,
    pub query_time_ms: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DuplicateGroupInfo {
    pub hash: String,
    pub file_count: i64,
    pub wasted_bytes: i64,
    pub match_type: String,
    pub files: Vec<FileInfo>,
}

#[tauri::command]
pub async fn search_files(
    library_id: String,
    text: Option<String>,
    state: State<'_, AppState>,
) -> Result<SearchResult, String> {
    let start = std::time::Instant::now();
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();

    let entries = if let Some(ref query) = text {
        lib.db.search_fts(query, 200).map_err(|e| e.to_string())?
    } else {
        lib.db.get_files_paginated(0, 200, "mtime", "DESC", &crate::engine::db::FileFilter::default()).map_err(|e| e.to_string())?
    };

    // Filter out .pixvault metadata files
    let entries: Vec<_> = entries.into_iter()
        .filter(|e| !e.path.contains("/.pixvault/") && !e.path.contains("\\.pixvault\\") && !e.path.contains("/.pixvault") && !e.path.contains("\\.pixvault"))
        .collect();

    let files: Vec<FileInfo> = entries.into_iter().map(|e| {
        let tags = lib.db.get_file_tags(e.id).unwrap_or_default();
        FileInfo {
            id: e.id, path: e.path, filename: e.filename, extension: e.extension,
            size_bytes: e.size_bytes, width: e.width, height: e.height,
            rating: e.rating, color_label: e.color_label, notes: e.notes,
            mtime: e.mtime, ctime: e.ctime, created_at: e.created_at, updated_at: e.updated_at,
            favorite: e.favorite, tags,
        }
    }).collect();

    let total = files.len() as i64;
    let query_time = start.elapsed().as_secs_f64() * 1000.0;

    Ok(SearchResult { total, files, query_time_ms: query_time })
}

#[tauri::command]
pub async fn search_by_color(
    library_id: String,
    color_hex: String,
    state: State<'_, AppState>,
) -> Result<Vec<FileInfo>, String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();

    let entries = lib.db.get_files_paginated(0, 100, "mtime", "DESC", &crate::engine::db::FileFilter::default()).map_err(|e| e.to_string())?;
    Ok(entries.into_iter()
        .filter(|e| e.color_label.as_deref() == Some(&color_hex))
        .map(|e| {
            let tags = lib.db.get_file_tags(e.id).unwrap_or_default();
            FileInfo {
                id: e.id, path: e.path, filename: e.filename, extension: e.extension,
                size_bytes: e.size_bytes, width: e.width, height: e.height,
                rating: e.rating, color_label: e.color_label, notes: e.notes,
                mtime: e.mtime, ctime: e.ctime, created_at: e.created_at, updated_at: e.updated_at,
                favorite: e.favorite, tags,
            }
        })
        .collect())
}

#[tauri::command]
pub async fn find_duplicates(
    library_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<DuplicateGroupInfo>, String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();

    let entries = lib.db.get_files_paginated(0, 10000, "mtime", "DESC", &crate::engine::db::FileFilter::default()).map_err(|e| e.to_string())?;
    let groups = lib.dedup.find_exact_duplicates(&entries);

    Ok(groups.into_iter().map(|g| DuplicateGroupInfo {
        hash: g.hash,
        file_count: g.files.len() as i64,
        wasted_bytes: g.total_size_wasted,
        match_type: match g.match_type {
            crate::engine::dedup::MatchType::Exact => "exact".to_string(),
            crate::engine::dedup::MatchType::Perceptual => "perceptual".to_string(),
        },
        files: g.files.into_iter().map(|f| FileInfo {
            id: f.id, path: f.path, filename: f.filename, extension: f.extension,
            size_bytes: f.size_bytes, width: f.width, height: f.height,
            rating: f.rating, color_label: f.color_label, notes: f.notes,
            mtime: f.mtime, ctime: f.ctime, created_at: f.created_at,
            updated_at: f.updated_at, favorite: f.favorite, tags: Vec::new(),
        }).collect(),
    }).collect())
}