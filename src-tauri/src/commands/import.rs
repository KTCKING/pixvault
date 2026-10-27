use std::path::PathBuf;
use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::engine::eaglepack::{self, PackProbe};
use crate::engine::import::{self, ImportItem, LibraryKind};
use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSummary {
    pub kind: String,
    pub detected: bool,
    pub total_items: usize,
    pub imported_files: usize,
    pub created_tags: usize,
    pub errors: Vec<String>,
    /// 解压/跳过相关的补充信息（仅 eaglepack 有值）
    #[serde(default)]
    pub unpacked_bytes: u64,
    #[serde(default)]
    pub skipped: usize,
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

// ───────────────────────── eaglepack ─────────────────────────

/// 开包前先看一眼：布局、条目数、文件夹数、解压后多大。
///
/// 前端用它实现「选完文件先给个统计，再让用户决定要不要解」——读的是 zip 中央
/// 目录，几百 MB 的包也是毫秒级返回。
#[tauri::command]
pub async fn probe_eaglepack(pack_path: String) -> Result<PackProbe, String> {
    let pack = PathBuf::from(&pack_path);
    if !pack.is_file() {
        return Err(format!("文件不存在: {}", pack_path));
    }
    tauri::async_runtime::spawn_blocking(move || eaglepack::probe(&pack))
        .await
        .map_err(|e| format!("读取素材包失败: {}", e))?
}

/// 取消正在进行的 eaglepack 解压。
#[tauri::command]
pub fn cancel_eaglepack_import(state: State<'_, AppState>) -> Result<(), String> {
    state.pack_cancel.store(true, Ordering::SeqCst);
    Ok(())
}

/// 把 `.eaglepack` 流式解到 `dest_dir`，然后把这个目录当作新图库打开。
///
/// 语义与 Eagle / Pixcall 导入一致：**源目录直接当库根，零复制**。所以目标目录
/// 必须是一个空文件夹（或尚不存在），解出来的东西就是图库的全部内容 —— 取消或
/// 失败时会把半成品整个删掉，不会留一个半截库。
#[tauri::command]
pub async fn import_eaglepack(
    pack_path: String,
    dest_dir: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<ImportSummary, String> {
    let pack = PathBuf::from(&pack_path);
    let dest = PathBuf::from(&dest_dir);
    if !pack.is_file() {
        return Err(format!("文件不存在: {}", pack_path));
    }

    // 目标目录必须空着 —— 我们不会往用户已有的文件夹里掺东西
    if dest.exists() {
        let mut rd = std::fs::read_dir(&dest).map_err(|e| format!("读取目标目录失败: {}", e))?;
        if rd.next().is_some() {
            return Err("目标文件夹不为空。导入会把它当作新图库的根，请换一个空文件夹。".to_string());
        }
    }

    let probe = {
        let p = pack.clone();
        tauri::async_runtime::spawn_blocking(move || eaglepack::probe(&p))
            .await
            .map_err(|e| format!("读取素材包失败: {}", e))??
    };
    if probe.layout == "unknown" {
        return Err("无法识别这个包：既没有 pack.json，也没有 Eagle 库的 metadata.json + images 目录。".to_string());
    }

    state.pack_cancel.store(false, Ordering::SeqCst);
    let cancel = state.pack_cancel.clone();

    let app_emit = app.clone();
    let pack_for_task = pack.clone();
    let dest_for_task = dest.clone();
    let unpack_result = tauri::async_runtime::spawn_blocking(move || {
        eaglepack::unpack(&pack_for_task, &dest_for_task, |p| {
            let _ = app_emit.emit("eaglepack-progress", p);
            !cancel.load(Ordering::SeqCst)
        })
    })
    .await
    .map_err(|e| format!("解压任务异常: {}", e))?;

    let report = match unpack_result {
        Ok(r) => r,
        Err(e) => {
            // 取消或失败：把半成品整棵删掉，别把一堆散图留在用户的硬盘上
            let _ = std::fs::remove_dir_all(&dest);
            return Err(e);
        }
    };

    let _ = app.emit(
        "eaglepack-progress",
        eaglepack::PackProgress {
            phase: "parse",
            done: 0,
            total: probe.item_count as u64,
            current: String::new(),
        },
    );

    let dest_for_parse = dest.clone();
    let (items, parse_report) =
        tauri::async_runtime::spawn_blocking(move || eaglepack::collect_items(&dest_for_parse))
            .await
            .map_err(|e| format!("解析素材包失败: {}", e))?
            .map_err(|e| {
                format!("{}（文件已解压到 {}，你可以直接把它当普通文件夹打开）", e, dest_dir)
            })?;

    // 库名优先用「用户刚选的那个目标目录名」——那才是他在资源管理器里看到的、
    // 下次还要找回来的名字。退回包文件名 stem（很多网上下来的包叫 Untitled.eaglepack，
    // 拿它当库名会让人一头雾水）。
    let library_name = dest
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| pack.file_stem().map(|s| s.to_string_lossy().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Eagle 素材包".to_string());

    let mut summary = finish_import(dest, library_name, "eaglepack", items, &state).await?;
    summary.unpacked_bytes = report.bytes;
    summary.skipped =
        report.skipped + parse_report.skipped_deleted + parse_report.skipped_bookmark + parse_report.skipped_missing;

    if parse_report.skipped_deleted > 0 {
        summary
            .errors
            .push(format!("已跳过 {} 个回收站条目", parse_report.skipped_deleted));
    }
    if parse_report.skipped_bookmark > 0 {
        summary.errors.push(format!(
            "已跳过 {} 个网页书签条目（只有链接和缩略图，没有原文件）",
            parse_report.skipped_bookmark
        ));
    }

    Ok(summary)
}

// ───────────────────────── 共用 ─────────────────────────

async fn import_library(
    source_path: String,
    kind: LibraryKind,
    state: State<'_, AppState>,
) -> Result<ImportSummary, String> {
    let root = PathBuf::from(&source_path);
    if !root.exists() {
        return Err(format!("路径不存在: {}", source_path));
    }

    // Parse outside the lock (may be slow for large libraries).
    let items = match kind {
        LibraryKind::Eagle => import::import_eagle(&root),
        LibraryKind::Pixcall => import::import_pixcall(&root),
    }?;

    let library_name = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| match kind {
            LibraryKind::Eagle => "Eagle 导入".to_string(),
            LibraryKind::Pixcall => "Pixcall 导入".to_string(),
        });

    let kind_label = match kind {
        LibraryKind::Eagle => "eagle",
        LibraryKind::Pixcall => "pixcall",
    };

    finish_import(root, library_name, kind_label, items, &state).await
}

/// 把解析好的条目写进一个**新建的**图库，并排上缩略图任务。
///
/// Eagle 目录导入、Pixcall 导入、eaglepack 导入三条路径共用这一段 —— 之前这段
/// 逻辑三次复制的话，改一处忘两处是迟早的事。
async fn finish_import(
    root: PathBuf,
    library_name: String,
    kind_label: &str,
    items: Vec<ImportItem>,
    state: &State<'_, AppState>,
) -> Result<ImportSummary, String> {
    let total_items = items.len();

    let mut engine = state.engine.lock().unwrap();
    let library_id = engine.open_library(&root)?;
    crate::commands::library::save_libraries_config(&engine);

    let mut imported_files = 0usize;
    let created_tags: usize;
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

            if let Some(notes) = &item.notes {
                let _ = lib.db.update_file_notes(&path_str, notes);
            }
            if let Some(rating) = item.rating {
                let _ = lib.db.update_file_rating(&path_str, rating.clamp(0, 5));
            }
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
    crate::commands::files::enqueue_missing_thumbnails(&state.thumbs, &lib, &library_id, false);
    drop(lib);

    Ok(ImportSummary {
        kind: kind_label.to_string(),
        detected: true,
        total_items,
        imported_files,
        created_tags,
        errors,
        unpacked_bytes: 0,
        skipped: 0,
        library,
    })
}
