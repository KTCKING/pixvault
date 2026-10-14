use tauri::State;
use crate::AppState;
use crate::engine::thumbs::{
    job_key, preview_job_key, Priority, ThumbJob, ThumbKind, ThumbProgress, ThumbWorker, THUMB_SIZE,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileInfo {
    pub id: i64,
    pub path: String,
    pub filename: String,
    pub extension: String,
    pub size_bytes: i64,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub rating: Option<i32>,
    pub color_label: Option<String>,
    pub notes: Option<String>,
    /// File system modification time.
    pub mtime: String,
    /// File system creation time.
    pub ctime: String,
    /// When the file was imported into the library.
    pub created_at: String,
    /// When the library record was last refreshed.
    pub updated_at: String,
    /// 收藏标记（Pixcall 式 ♥）。
    pub favorite: bool,
    pub tags: Vec<crate::engine::db::Tag>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PaginatedFiles {
    pub files: Vec<FileInfo>,
    pub total: i64,
    pub offset: i64,
    pub limit: i64,
}

#[tauri::command]
pub async fn get_files(
    library_id: String,
    offset: i64,
    limit: i64,
    sort_by: String,
    sort_order: String,
    filter_ext: Option<String>,
    filter_tag: Option<i64>,
    filter_folder: Option<String>,
    filter_favorite: Option<bool>,
    filter_color: Option<String>,
    filter_min_rating: Option<i32>,
    state: State<'_, AppState>,
) -> Result<PaginatedFiles, String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();

    let filter = crate::engine::db::FileFilter {
        ext: filter_ext,
        tag_id: filter_tag,
        folder: filter_folder,
        favorite: filter_favorite,
        color: filter_color,
        min_rating: filter_min_rating,
        include_deleted: false,
    };

    let entries = lib.db.get_files_paginated(
        offset, limit, &sort_by, &sort_order, &filter,
    ).map_err(|e| e.to_string())?;

    // Filter out .pixvault metadata files
    let entries: Vec<_> = entries.into_iter()
        .filter(|e| !e.path.contains("/.pixvault/") && !e.path.contains("\\.pixvault\\") && !e.path.contains("/.pixvault") && !e.path.contains("\\.pixvault"))
        .collect();

    let total = lib.db.count_files(&filter)
        .map_err(|e| e.to_string())?;

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

    Ok(PaginatedFiles { files, total, offset, limit })
}

#[tauri::command]
pub async fn get_file_info(
    library_id: String,
    file_path: String,
    state: State<'_, AppState>,
) -> Result<FileInfo, String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();

    let entry = lib.db.get_file_by_path(&file_path)
        .map_err(|e| e.to_string())?
        .ok_or("未找到文件")?;
    let tags = lib.db.get_file_tags(entry.id).unwrap_or_default();

    Ok(FileInfo {
        id: entry.id, path: entry.path, filename: entry.filename, extension: entry.extension,
        size_bytes: entry.size_bytes, width: entry.width, height: entry.height,
        rating: entry.rating, color_label: entry.color_label, notes: entry.notes,
        mtime: entry.mtime, ctime: entry.ctime, created_at: entry.created_at,
        updated_at: entry.updated_at, favorite: entry.favorite, tags,
    })
}

/// 收藏 / 取消收藏一张图片。
#[tauri::command]
pub async fn set_file_favorite(
    library_id: String,
    file_path: String,
    favorite: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();
    lib.db.update_file_favorite(&file_path, favorite).map_err(|e| e.to_string())
}

/// 设置 / 清除颜色标记。`color` 为空表示清除。
#[tauri::command]
pub async fn set_file_color(
    library_id: String,
    file_path: String,
    color: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();
    lib.db
        .update_file_color(&file_path, color.as_deref().filter(|c| !c.is_empty()))
        .map_err(|e| e.to_string())
}

/// 侧边栏「文件夹」树：库内目录层级 + 每个文件夹的直接图片数。
#[tauri::command]
pub async fn get_folder_tree(
    library_id: String,
    state: State<'_, AppState>,
) -> Result<crate::engine::db::FolderNode, String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();
    let root = lib.root_path.to_string_lossy().to_string();
    lib.db.get_folder_tree(&root).map_err(|e| e.to_string())
}

/// 侧边栏智能视图 / 颜色标记的计数徽标。
#[derive(Debug, Serialize, Deserialize)]
pub struct ViewCounts {
    pub all: i64,
    pub favorite: i64,
    pub rated: i64,
    pub trash: i64,
    /// 颜色名 -> 数量（只包含有文件的颜色）。
    pub colors: std::collections::HashMap<String, i64>,
}

#[tauri::command]
pub async fn get_view_counts(
    library_id: String,
    state: State<'_, AppState>,
) -> Result<ViewCounts, String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();

    let default_filter = crate::engine::db::FileFilter::default();
    let all = lib.db.count_files(&default_filter).map_err(|e| e.to_string())?;

    let favorite = lib
        .db
        .count_files(&crate::engine::db::FileFilter { favorite: Some(true), ..Default::default() })
        .map_err(|e| e.to_string())?;

    let rated = lib
        .db
        .count_files(&crate::engine::db::FileFilter { min_rating: Some(1), ..Default::default() })
        .map_err(|e| e.to_string())?;

    let trash = lib
        .db
        .count_files(&crate::engine::db::FileFilter { include_deleted: true, ..Default::default() })
        .map_err(|e| e.to_string())?;

    let mut colors = std::collections::HashMap::new();
    for (name, _hex) in crate::engine::db::COLOR_LABELS {
        let n = lib
            .db
            .count_files(&crate::engine::db::FileFilter {
                color: Some(name.to_string()),
                ..Default::default()
            })
            .map_err(|e| e.to_string())?;
        if n > 0 {
            colors.insert(name.to_string(), n);
        }
    }

    Ok(ViewCounts { all, favorite, rated, trash, colors })
}

#[tauri::command]
pub async fn update_file_rating(
    library_id: String,
    file_path: String,
    rating: i32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();
    lib.db.update_file_rating(&file_path, rating).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_file_notes(
    library_id: String,
    file_path: String,
    notes: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();
    lib.db.update_file_notes(&file_path, &notes).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_file(
    library_id: String,
    file_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();
    lib.db.soft_delete_file(&file_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn trash_files(
    library_id: String,
    file_paths: Vec<String>,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();
    let mut n = 0;
    for p in &file_paths {
        lib.db.soft_delete_file(p).map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

#[tauri::command]
pub async fn get_deleted_files(
    library_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<FileInfo>, String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();

    let entries = lib.db.get_deleted_files().map_err(|e| e.to_string())?;
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
    Ok(files)
}

#[tauri::command]
pub async fn restore_file(
    library_id: String,
    file_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();
    lib.db.restore_file(&file_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn purge_file(
    library_id: String,
    file_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();
    lib.db.purge_file(&file_path).map_err(|e| e.to_string())
}

/// Permanently delete every file in the trash (empty the trash).
#[tauri::command]
pub async fn empty_trash(
    library_id: String,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();
    lib.db.purge_all_deleted().map_err(|e| e.to_string())
}

/// 缩略图查询结果。
#[derive(Debug, Serialize, Deserialize)]
pub struct ThumbnailResult {
    /// `ready` —— path 是缩略图；`fallback` —— 生成不出来，用原图；
    /// `pending` —— 已排进后台队列，稍后通过 `thumb-ready` 事件推送。
    pub status: String,
    pub path: String,
}

/// 把某个图库里所有「还没有缩略图」的文件排进后台队列。
///
/// 只做一次数据库查询 + 每个文件一次 `exists()`，**不做任何解码**，所以可以
/// 放心地在持有图库锁时调用。
pub fn enqueue_missing_thumbnails(
    thumbs: &Arc<ThumbWorker>,
    lib: &crate::engine::Library,
    library_id: &str,
) -> usize {
    // 尺寸 / 质量参数变了就先清掉上一版缓存（内部有版本哨兵，平时只读一个小
    // 文件），否则磁盘上会同时躺着 `_512.jpg` 和 `_448.jpg` 两套缩略图。
    lib.thumbnail.ensure_cache_version();

    let candidates = match lib.db.list_files_for_thumbs() {
        Ok(c) => c,
        Err(_) => return 0,
    };

    let mut jobs: Vec<ThumbJob> = Vec::new();
    for (path, hash) in candidates {
        let hash = hash
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| crate::utils::hash::blake3_hash_bytes(path.as_bytes()));
        if lib.thumbnail.thumbnail_path(&hash, THUMB_SIZE).exists() {
            continue;
        }
        jobs.push(ThumbJob {
            key: job_key(library_id, &path),
            path: path.clone(),
            source: std::path::PathBuf::from(&path),
            hash,
            engine: lib.thumbnail.clone(),
            kind: ThumbKind::Grid,
        });
    }

    let n = jobs.len();
    if n > 0 {
        thumbs.enqueue(jobs);
    }
    n
}

/// 取一张图的缩略图路径。
///
/// 这里**只判断文件存不存在**，需要生成时把任务丢给后台线程后立刻返回
/// `pending`。以前这个命令会同步整解码原图，而且全程握着图库锁 —— 滚动大图库
/// 时每张卡片都在等一次全图解码，整个引擎串行排队，这就是卡顿的根源。
#[tauri::command]
pub async fn get_thumbnail_path(
    library_id: String,
    file_path: String,
    state: State<'_, AppState>,
) -> Result<ThumbnailResult, String> {
    let key = job_key(&library_id, &file_path);

    // 已经出过结果的直接回答，连数据库都不用查。
    if let Some(resolved) = state.thumbs.lookup(&key) {
        return Ok(match resolved {
            Some(thumb) => ThumbnailResult { status: "ready".into(), path: thumb },
            None => ThumbnailResult { status: "fallback".into(), path: file_path },
        });
    }

    let job = {
        let engine = state.engine.lock().map_err(|_| "引擎锁失效".to_string())?;
        let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
        let lib = lib.lock().map_err(|_| "图库锁失效".to_string())?;

        let entry = lib
            .db
            .get_file_by_path(&file_path)
            .map_err(|e| e.to_string())?
            .ok_or("未找到文件")?;

        // 索引阶段没有算过内容哈希，退化成「路径哈希」—— 与后台任务保持一致。
        let hash = entry
            .blake3_hash
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| crate::utils::hash::blake3_hash_bytes(file_path.as_bytes()));

        let thumb_path = lib.thumbnail.thumbnail_path(&hash, THUMB_SIZE);
        if thumb_path.exists() {
            let thumb = thumb_path.to_string_lossy().to_string();
            state.thumbs.remember(&key, thumb.clone());
            return Ok(ThumbnailResult { status: "ready".into(), path: thumb });
        }

        ThumbJob {
            key,
            path: file_path.clone(),
            source: std::path::PathBuf::from(&file_path),
            hash,
            engine: lib.thumbnail.clone(),
            kind: ThumbKind::Grid,
        }
    }; // 锁在这里释放，解码过程绝不持锁

    // 屏幕上看得到的图插队，不必等整库补齐的批处理跑完。
    state.thumbs.enqueue_with(vec![job], Priority::Urgent);
    Ok(ThumbnailResult { status: "pending".into(), path: String::new() })
}

/// 取一张图的**右侧预览图**路径（1600px）。
///
/// 和 `get_thumbnail_path` 同一套路：命中就立刻返回，未命中就排进后台队列并回
/// `pending`，结果通过 `preview-ready` 事件推回来。区别是预览图**不做整库
/// 预热** —— 只在用户真的打开某张图时才生成，因为 1600px 的图单张约 250 KB，
/// 一个 2400 张的库全量生成就是 600 MB，而绝大部分图一辈子不会被点开。
#[tauri::command]
pub async fn get_preview_path(
    library_id: String,
    file_path: String,
    state: State<'_, AppState>,
) -> Result<ThumbnailResult, String> {
    let key = preview_job_key(&library_id, &file_path);

    if let Some(resolved) = state.thumbs.lookup(&key) {
        return Ok(match resolved {
            Some(preview) => ThumbnailResult { status: "ready".into(), path: preview },
            None => ThumbnailResult { status: "fallback".into(), path: file_path },
        });
    }

    let job = {
        let engine = state.engine.lock().map_err(|_| "引擎锁失效".to_string())?;
        let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
        let lib = lib.lock().map_err(|_| "图库锁失效".to_string())?;

        let entry = lib
            .db
            .get_file_by_path(&file_path)
            .map_err(|e| e.to_string())?
            .ok_or("未找到文件")?;

        let hash = entry
            .blake3_hash
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| crate::utils::hash::blake3_hash_bytes(file_path.as_bytes()));

        let preview_path = lib.thumbnail.preview_path(&hash);
        if preview_path.exists() {
            let preview = preview_path.to_string_lossy().to_string();
            state.thumbs.remember(&key, preview.clone());
            return Ok(ThumbnailResult { status: "ready".into(), path: preview });
        }

        ThumbJob {
            key,
            path: file_path.clone(),
            source: std::path::PathBuf::from(&file_path),
            hash,
            engine: lib.thumbnail.clone(),
            kind: ThumbKind::Preview,
        }
    }; // 锁在这里释放，解码过程绝不持锁

    state.thumbs.enqueue_with(vec![job], Priority::Urgent);
    Ok(ThumbnailResult { status: "pending".into(), path: String::new() })
}

/// 当前缩略图后台任务的整体进度（用于状态栏/调试）。
#[tauri::command]
pub async fn get_thumbnail_progress(state: State<'_, AppState>) -> Result<ThumbProgress, String> {
    Ok(state.thumbs.stats())
}

/// 兜底通道：把缩略图文件直接读成 data URL 返回给前端。
///
/// 正常路径走 asset 协议（`convertFileSrc` → `http://asset.localhost/…`），
/// 但在某些 WebView2 环境里这条路径会静默失败，卡片就只剩一片空白。这个命令
/// 完全绕开协议层（512px 缩略图约 25KB，base64 后 33KB，代价可接受），只在
/// asset 加载失败时由前端调用。
#[tauri::command]
pub async fn read_thumb_data_url(
    library_id: String,
    file_path: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    use base64::Engine as _;

    let thumb_path = {
        let engine = state.engine.lock().map_err(|_| "引擎锁失效".to_string())?;
        let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
        let lib = lib.lock().map_err(|_| "图库锁失效".to_string())?;

        let entry = lib
            .db
            .get_file_by_path(&file_path)
            .map_err(|e| e.to_string())?
            .ok_or("未找到文件")?;

        let hash = entry
            .blake3_hash
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| crate::utils::hash::blake3_hash_bytes(file_path.as_bytes()));

        lib.thumbnail.thumbnail_path(&hash, THUMB_SIZE)
    }; // 读文件前先放锁

    let bytes = match std::fs::read(&thumb_path) {
        Ok(b) if !b.is_empty() => b,
        _ => return Ok(None),
    };
    Ok(Some(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    )))
}

/// 手动触发一次「补齐缺失缩略图」。
#[tauri::command]
pub async fn build_missing_thumbnails(
    library_id: String,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let engine = state.engine.lock().map_err(|_| "引擎锁失效".to_string())?;
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().map_err(|_| "图库锁失效".to_string())?;
    Ok(enqueue_missing_thumbnails(&state.thumbs, &lib, &library_id))
}

#[tauri::command]
pub async fn open_in_explorer(file_path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let path = std::path::Path::new(&file_path);
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        // `open -R` reveals the file in Finder.
        std::process::Command::new("open")
            .arg("-R")
            .arg(&file_path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn open_file(file_path: String) -> Result<(), String> {
    opener::open(&file_path).map_err(|e| e.to_string())
}

/// Copy image files to the OS clipboard as files so they can be pasted into
/// Explorer / QQ / WeChat etc. On Windows we shell out to PowerShell's
/// Set-Clipboard (avoids pulling in a heavy clipboard crate).
#[tauri::command]
pub async fn copy_files_to_clipboard(file_paths: Vec<String>) -> Result<usize, String> {
    if file_paths.is_empty() {
        return Ok(0);
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

        // Join paths with commas; PowerShell treats a comma-separated list of
        // file paths as a file-collection clipboard payload.
        let list = file_paths
            .iter()
            .map(|p| format!("'{}'", p.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(", ");
        let script = format!("Set-Clipboard -Path {}", list);
        // `CREATE_NO_WINDOW` + `-WindowStyle Hidden` keep this fully silent so
        // no console flashes when copying.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| format!("启动 PowerShell 失败: {e}"))?;
        if !status.success() {
            return Err("复制到剪贴板失败".to_string());
        }
    }

    #[cfg(target_os = "macos")]
    {
        // Put real file references on the macOS pasteboard so they can be
        // pasted into Finder / Mail / other apps. `POSIX file` yields an
        // alias/file object; a list of them works for multi-selection.
        let alias_list = file_paths
            .iter()
            .map(|p| format!("(POSIX file \"{}\")", p.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(", ");
        let script = format!("set the clipboard to {{{}}}", alias_list);
        let status = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .status()
            .map_err(|e| format!("启动 osascript 失败: {e}"))?;
        if !status.success() {
            return Err("复制到剪贴板失败".to_string());
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Linux: no portable way to copy *files* to the clipboard without an
        // extra crate, so report unsupported.
        return Err("当前平台暂不支持复制图片到剪贴板".to_string());
    }

    Ok(file_paths.len())
}