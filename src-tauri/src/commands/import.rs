use tauri::State;
use crate::AppState;
use crate::engine::import::{self, LibraryKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSummary {
    pub kind: String,
    pub detected: bool,
    pub total_items: usize,
    pub imported_files: usize,
    pub created_tags: usize,
    pub errors: Vec<String>,
    /// The newly-created (or reused) target library.
    pub library: crate::commands::library::LibraryInfo,
}

#[tauri::command]
pub async fn detect_library_kind(
    path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let _ = state;
    let root = std::path::Path::new(&path);
    match import::detect_kind(root) {
        Some(LibraryKind::Eagle) => Ok("eagle".to_string()),
        Some(LibraryKind::Pixcall) => Ok("pixcall".to_string()),
        None => Ok("unknown".to_string()),
    }
}

#[tauri::command]
pub async fn import_eagle_library(
    source_path: String,
    state: State<'_, AppState>,
) -> Result<ImportSummary, String> {
    import_library(source_path, LibraryKind::Eagle, state).await
}

#[tauri::command]
pub async fn import_pixcall_library(
    source_path: String,
    state: State<'_, AppState>,
) -> Result<ImportSummary, String> {
    import_library(source_path, LibraryKind::Pixcall, state).await
}

async fn import_library(
    source_path: String,
    kind: LibraryKind,
    state: State<'_, AppState>,
) -> Result<ImportSummary, String> {
    let root = std::path::PathBuf::from(&source_path);
    if !root.exists() {
        return Err(format!("路径不存在: {}", source_path));
    }

    // Parse outside the lock (may be slow for large libraries).
    let items = match kind {
        LibraryKind::Eagle => import::import_eagle(&root),
        LibraryKind::Pixcall => import::import_pixcall(&root),
    }?;

    let total_items = items.len();

    // Create a dedicated, brand-new library for the imported source so its
    // contents are NOT merged into whatever library is currently open.
    let library_name = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| match kind {
            LibraryKind::Eagle => "Eagle 导入".to_string(),
            LibraryKind::Pixcall => "Pixcall 导入".to_string(),
        });

    let mut engine = state.engine.lock().unwrap();
    let library_id = engine.open_library(&root)?;
    crate::commands::library::save_libraries_config(&engine);

    let mut imported_files = 0usize;
    let mut created_tags = 0usize;
    let mut errors: Vec<String> = Vec::new();

    {
        let lib = engine.get_library(&library_id).ok_or("创建图库失败")?;
        let lib = lib.lock().unwrap();

        let before_tags = lib.db.get_all_tags().map(|t| t.len()).unwrap_or(0);

        for item in &items {
            let Some(entry) = crate::engine::indexer::Indexer::build_entry(&item.path) else {
                continue;
            };

            let path_str = item.path.to_string_lossy().to_string();

            if let Err(e) = lib.db.insert_file(&entry) {
                errors.push(format!("{}: 写入失败 {}", path_str, e));
                continue;
            }

            // Notes
            if let Some(notes) = &item.notes {
                let _ = lib.db.update_file_notes(&path_str, notes);
            }
            // Rating
            if let Some(rating) = item.rating {
                let _ = lib.db.update_file_rating(&path_str, rating.clamp(0, 5));
            }
            // Tags (folder hierarchy + explicit tags)
            for tag_name in &item.tags {
                match lib.db.get_or_create_tag(tag_name) {
                    Ok(tag_id) => {
                        if let Ok(Some(entry)) = lib.db.get_file_by_path(&path_str) {
                            let _ = lib.db.add_file_tag(entry.id, tag_id);
                        }
                    }
                    Err(e) => errors.push(format!("{}: 标签失败 {}", path_str, e)),
                }
            }

            imported_files += 1;
        }

        let after_tags = lib.db.get_all_tags().map(|t| t.len()).unwrap_or(0);
        created_tags = after_tags.saturating_sub(before_tags);
    }

    // Build the library info for the response.
    let lib = engine.get_library(&library_id).ok_or("创建图库失败")?;
    let lib = lib.lock().unwrap();
    let library = crate::commands::library::LibraryInfo {
        id: lib.id.clone(),
        name: library_name,
        root_path: lib.root_path.to_string_lossy().to_string(),
        is_network: lib.is_network,
        file_count: imported_files as i64,
        thumbnail_cache_size: lib.thumbnail.cache_size(),
    };
    // 导入完就排队生成缩略图，避免用户翻到哪张才现解哪张。
    crate::commands::files::enqueue_missing_thumbnails(&state.thumbs, &lib, &library_id);
    drop(lib);

    Ok(ImportSummary {
        kind: match kind {
            LibraryKind::Eagle => "eagle".to_string(),
            LibraryKind::Pixcall => "pixcall".to_string(),
        },
        detected: true,
        total_items,
        imported_files,
        created_tags,
        errors,
        library,
    })
}
