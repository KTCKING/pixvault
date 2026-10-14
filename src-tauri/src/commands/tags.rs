use tauri::State;
use crate::AppState;
use crate::engine::db::Tag;
use serde::{Deserialize, Serialize};

#[tauri::command]
pub async fn get_all_tags(
    library_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Tag>, String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();
    lib.db.get_all_tags().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_tag(
    library_id: String,
    name: String,
    parent_id: Option<i64>,
    color: Option<String>,
    state: State<'_, AppState>,
) -> Result<Tag, String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();
    let id = lib.db.create_tag(&name, parent_id, color.as_deref()).map_err(|e| e.to_string())?;
    Ok(Tag { id, name, parent_id, color, icon: None })
}

#[tauri::command]
pub async fn add_tag_to_file(
    library_id: String,
    file_path: String,
    tag_id: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();
    let entry = lib.db.get_file_by_path(&file_path).map_err(|e| e.to_string())?.ok_or("未找到文件")?;
    lib.db.add_file_tag(entry.id, tag_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_tag_from_file(
    library_id: String,
    file_path: String,
    tag_id: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();
    let entry = lib.db.get_file_by_path(&file_path).map_err(|e| e.to_string())?.ok_or("未找到文件")?;
    lib.db.remove_file_tag(entry.id, tag_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn batch_add_tags(
    library_id: String,
    file_paths: Vec<String>,
    tag_ids: Vec<i64>,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();
    let mut errors = Vec::new();

    for file_path in &file_paths {
        if let Ok(Some(entry)) = lib.db.get_file_by_path(file_path) {
            for &tag_id in &tag_ids {
                if let Err(e) = lib.db.add_file_tag(entry.id, tag_id) {
                    errors.push(format!("{}: {}", file_path, e));
                }
            }
        } else {
            errors.push(format!("{}: 未找到文件", file_path));
        }
    }
    Ok(errors)
}