use tauri::State;
use crate::AppState;
use crate::engine::thumbs::{
    job_key, preview_job_key, Priority, ThumbJob, ThumbKind, ThumbProgress, ThumbWorker, WarmProgress,
    THUMB_SIZE,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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

/// 回收站里的一个条目：文件信息 + 它来自哪个库。
///
/// 回收站是**逻辑库**，不搬文件、没有自己的目录。每条都带着来源库，
/// 恢复 / 彻底删除时按 `library_id` 回原库执行，不存在跨库写入。
#[derive(Debug, Serialize, Deserialize)]
pub struct TrashEntry {
    #[serde(flatten)]
    pub file: FileInfo,
    pub library_id: String,
    pub library_name: String,
}

/// 回收站里某个来源库的汇总，用于顶部「全部 / 某个库」筛选条。
#[derive(Debug, Serialize, Deserialize)]
pub struct TrashSource {
    pub library_id: String,
    pub library_name: String,
    pub count: usize,
}

/// 回收站一次聚合查询的完整结果。
#[derive(Debug, Serialize, Deserialize)]
pub struct TrashPayload {
    pub entries: Vec<TrashEntry>,
    pub sources: Vec<TrashSource>,
    /// 读不出来的库名（NAS 离线、盘不在）。UI 需要标注，不能假装它没有回收站。
    pub unavailable: Vec<String>,
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
    // 「最近新增」的时间窗口（天数）：只看最近 N 天内导入的文件。
    filter_recent_days: Option<i64>,
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
        recent_days: filter_recent_days,
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

/// 把 db 层的行转成前端用的 `FileInfo`（顺带取标签）。
fn entry_to_info(db: &crate::engine::db::Database, e: crate::engine::db::FileEntry) -> FileInfo {
    let tags = db.get_file_tags(e.id).unwrap_or_default();
    FileInfo {
        id: e.id, path: e.path, filename: e.filename, extension: e.extension,
        size_bytes: e.size_bytes, width: e.width, height: e.height,
        rating: e.rating, color_label: e.color_label, notes: e.notes,
        mtime: e.mtime, ctime: e.ctime, created_at: e.created_at, updated_at: e.updated_at,
        favorite: e.favorite, tags,
    }
}

/// 当前库的回收站（保留给内部调用；UI 已改用跨库的 `get_trash`）。
#[tauri::command]
pub async fn get_deleted_files(
    library_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<FileInfo>, String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();

    let entries = lib.db.get_deleted_files().map_err(|e| e.to_string())?;
    Ok(entries.into_iter().map(|e| entry_to_info(&lib.db, e)).collect())
}

/// 跨库聚合回收站 —— 回收站作为「逻辑库」的唯一数据源。
///
/// 性能依据：`restore_libraries` 在启动时就把 `libraries.json` 里的每个库都
/// `open_library` 了一遍，所以 Engine 内存里常驻着全部库的 SQLite 连接。
/// 这里只是遍历内存里的 map，每个库跑一句走 `idx_files_deleted` 的查询，
/// 没有冷启动、没有重新建连接。典型 10 个库、几百条记录，耗时在毫秒级。
///
/// 某个库读失败（NAS 掉线 / 盘拔了）不整体失败，只记进 `unavailable`，
/// 让 UI 明确告诉用户「还有一个库现在读不到」，而不是假装它没有回收站。
#[tauri::command]
pub async fn get_trash(state: State<'_, AppState>) -> Result<TrashPayload, String> {
    let engine = state.engine.lock().unwrap();

    // 按用户在侧栏里排好的顺序遍历，回收站里的分组顺序才和侧栏一致。
    let mut ids: Vec<String> = engine
        .library_order
        .iter()
        .filter(|id| engine.libraries.contains_key(*id))
        .cloned()
        .collect();
    for id in engine.libraries.keys() {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }

    let mut entries: Vec<TrashEntry> = Vec::new();
    let mut sources: Vec<TrashSource> = Vec::new();
    let mut unavailable: Vec<String> = Vec::new();

    for id in ids {
        let lib = match engine.libraries.get(&id) {
            Some(l) => l,
            None => continue,
        };
        let lib = lib.lock().unwrap();
        match lib.db.get_deleted_files() {
            Ok(list) => {
                let count = list.len();
                if count > 0 {
                    sources.push(TrashSource {
                        library_id: id.clone(),
                        library_name: lib.name.clone(),
                        count,
                    });
                    for e in list {
                        entries.push(TrashEntry {
                            file: entry_to_info(&lib.db, e),
                            library_id: id.clone(),
                            library_name: lib.name.clone(),
                        });
                    }
                }
            }
            Err(_) => unavailable.push(lib.name.clone()),
        }
    }

    Ok(TrashPayload { entries, sources, unavailable })
}

/// 校验 path 确实位于库根之内。
///
/// `canonicalize` 会解开符号链接并按真实路径比较，所以库内指向库外的软链
/// 会被拒绝（否则「彻底删除」可能被构造的路径带到库外去删东西）。
/// 顺带解决「原文件是否还在」—— canonicalize 失败就是文件不在了。
fn ensure_in_library(lib_root: &std::path::Path, path: &str) -> Result<std::path::PathBuf, String> {
    let root = lib_root
        .canonicalize()
        .map_err(|_| "库目录当前不可访问".to_string())?;
    let target = std::path::Path::new(path)
        .canonicalize()
        .map_err(|_| format!("文件已不在磁盘上：{}", path))?;
    if !target.starts_with(&root) {
        return Err("文件不在该库目录内，已拒绝操作".into());
    }
    Ok(target)
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

    // 恢复前确认文件仍在原位：否则恢复出来只是一条指向空气的记录。
    // 文件没了就让用户去「彻底删除」把它从库里移除。
    ensure_in_library(&lib.root_path, &file_path)?;
    lib.db.restore_file(&file_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn purge_file(
    library_id: String,
    file_path: String,
    also_delete_disk: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();

    if also_delete_disk {
        // 先校验再删：校验不过就整单拒绝，绝不留下「文件删了但记录还在」的中间态。
        let target = ensure_in_library(&lib.root_path, &file_path)?;
        std::fs::remove_file(&target)
            .map_err(|e| format!("删除磁盘文件失败，已保留库内记录：{}", e))?;
    }
    lib.db.purge_file(&file_path).map_err(|e| e.to_string())
}

/// 跨库回收站的总条目数。
///
/// 侧栏的回收站徽标要显示「所有库加起来多少张」，但它不该为此拉全量条目
/// （`get_trash` 还要逐条取标签）。这里每库只跑一句 `COUNT(*)`，
/// 走 `idx_files_deleted` 索引，代价可以忽略。
#[tauri::command]
pub async fn get_trash_count(state: State<'_, AppState>) -> Result<usize, String> {
    let engine = state.engine.lock().unwrap();
    let mut total = 0usize;
    for lib in engine.libraries.values() {
        let lib = lib.lock().unwrap();
        if let Ok(n) = lib.db.count_deleted() {
            total += n;
        }
    }
    Ok(total)
}

/// 清空一个库的回收站时返回的汇总。
#[derive(Debug, Serialize, Deserialize)]
pub struct EmptyTrashResult {
    /// 从库里移除的记录数。
    pub removed: usize,
    /// 删不掉的文件（原样保留记录，用户还能重试）。
    pub failed: Vec<String>,
}

/// 清空**一个库**的回收站。
///
/// `also_delete_disk = true` 时逐个删磁盘文件，只摘掉删成功的记录；
/// 删失败的记录保留 —— 「文件还在但记录没了」比「文件没删掉」更难排查。
fn purge_library_trash(
    lib: &crate::engine::Library,
    also_delete_disk: bool,
) -> Result<EmptyTrashResult, String> {
    if !also_delete_disk {
        let removed = lib.db.purge_all_deleted().map_err(|e| e.to_string())?;
        return Ok(EmptyTrashResult { removed, failed: Vec::new() });
    }

    let list = lib.db.get_deleted_files().map_err(|e| e.to_string())?;
    let mut ok_paths: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    for e in &list {
        match ensure_in_library(&lib.root_path, &e.path) {
            Ok(target) => match std::fs::remove_file(&target) {
                Ok(_) => ok_paths.push(e.path.clone()),
                Err(err) => failed.push(format!("{}（{}）", e.path, err)),
            },
            Err(err) => failed.push(format!("{}（{}）", e.path, err)),
        }
    }
    let removed = lib.db.purge_paths(&ok_paths).map_err(|e| e.to_string())?;
    Ok(EmptyTrashResult { removed, failed })
}

#[tauri::command]
pub async fn empty_trash(
    library_id: String,
    also_delete_disk: bool,
    state: State<'_, AppState>,
) -> Result<EmptyTrashResult, String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();
    purge_library_trash(&lib, also_delete_disk)
}

/// 清空**所有库**的回收站 —— 回收站作为逻辑库的「清空」动作。
///
/// 单个库失败不影响其他库；读不到的库记进 `unavailable`，由 UI 告知用户。
#[tauri::command]
pub async fn empty_trash_all(
    also_delete_disk: bool,
    state: State<'_, AppState>,
) -> Result<EmptyTrashResult, String> {
    let engine = state.engine.lock().unwrap();
    let ids: Vec<String> = engine.libraries.keys().cloned().collect();

    let mut removed = 0usize;
    let mut failed: Vec<String> = Vec::new();
    for id in ids {
        if let Some(lib) = engine.libraries.get(&id) {
            let lib = lib.lock().unwrap();
            match purge_library_trash(&lib, also_delete_disk) {
                Ok(r) => {
                    removed += r.removed;
                    // 失败条目带上库名，跨库清空时才分得清是哪个库的文件。
                    failed.extend(r.failed.into_iter().map(|f| format!("[{}] {}", lib.name, f)));
                }
                Err(e) => failed.push(format!("[{}] {}", lib.name, e)),
            }
        }
    }
    Ok(EmptyTrashResult { removed, failed })
}

/// 缩略图查询结果。
#[derive(Debug, Serialize, Deserialize)]
pub struct ThumbnailResult {
    /// `ready` —— path 是缩略图；`fallback` —— 生成不出来，用原图；
    /// `pending` —— 已排进后台队列，稍后通过 `thumb-ready` 事件推送。
    pub status: String,
    pub path: String,
}

/// 批量缩略图查询的单条结果（语义同 [`ThumbnailResult`]，外加占位图）。
#[derive(Debug, Serialize, Deserialize)]
pub struct ThumbBatchItem {
    pub path: String,
    /// `ready` / `fallback` / `pending`，语义同 [`ThumbnailResult`]。
    pub status: String,
    /// `ready` / `fallback` 时的图片地址。
    pub thumb: String,
    /// 已缓存的模糊占位图（32px data URL）；没有则为空串。
    pub placeholder: String,
}

/// 把某个图库里所有「还没有缩略图」的文件排进后台队列。
///
/// 只做一次数据库查询 + 每个文件一次 `exists()`，**不做任何解码**，所以可以
/// 放心地在持有图库锁时调用。
///
/// `force = false` 时尊重「打开图库时后台补齐缩略图」这个开关（默认开）；
/// 手动触发的「补齐缺失缩略图」传 `true`，不受开关影响。
///
/// 排完队会登记本轮预热进度（见 [`ThumbWorker::begin_warm]`），侧边栏据此在
/// 库名旁边画圆环。
pub fn enqueue_missing_thumbnails(
    thumbs: &Arc<ThumbWorker>,
    lib: &crate::engine::Library,
    library_id: &str,
    force: bool,
) -> usize {
    if !force && !thumbs.prewarm_enabled() {
        return 0;
    }

    // 尺寸 / 质量参数变了就先清掉上一版缓存（内部有版本哨兵，平时只读一个小
    // 文件），否则磁盘上会同时躺着 `_512.jpg` 和 `_448.jpg` 两套缩略图。
    lib.thumbnail.ensure_cache_version();

    let candidates = match lib.db.list_files_for_thumbs() {
        Ok(c) => c,
        Err(_) => return 0,
    };

    let mut jobs: Vec<ThumbJob> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    for (path, hash) in candidates {
        // 视频不排缩略图任务：解码器不认 mp4，硬排只会每轮开库都白试一遍。
        // 网格里视频卡片显示的是播放角标（见 ThumbnailCard）。
        if crate::bridge::is_video_ext(
            std::path::Path::new(&path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or(""),
        ) {
            continue;
        }
        let hash = hash
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| crate::utils::hash::blake3_hash_bytes(path.as_bytes()));
        if lib.thumbnail.thumbnail_path(&hash, THUMB_SIZE).exists() {
            continue;
        }
        let key = job_key(library_id, &path);
        keys.push(key.clone());
        jobs.push(ThumbJob {
            key,
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
    // 即使 n == 0 也要登记：这样会把上一轮残留的进度收成「本轮无待办」，
    // 界面上的进度环随之收起。
    thumbs.begin_warm(library_id, keys);
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

/// 批量版 [`get_thumbnail_path`]：一次 IPC 拿一整屏的图。
///
/// 虚拟化网格每次滚动都会重算渲染窗口，逐张 IPC 的话一屏就是几十次往返；
/// 合成一次之后，前端「滚动 → 120ms 去抖 → 一次提交整屏」的模式才成立。
///
/// `window` 是前端声明的「当前视口路径集合」：除了给这些图插队，还会把窗口
/// 之外的过期 urgent 任务降级回整库队列（见 [`ThumbWorker::enqueue_window`]）。
#[tauri::command]
pub async fn get_thumbnails(
    library_id: String,
    paths: Vec<String>,
    window: Option<Vec<String>>,
    state: State<'_, AppState>,
) -> Result<Vec<ThumbBatchItem>, String> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let mut out: Vec<ThumbBatchItem> = Vec::with_capacity(paths.len());
    let mut jobs: Vec<ThumbJob> = Vec::new();
    // 库里查不到的路径（刚被挪走 / 删掉的），最后统一按 pending 回，免得前端
    // 把它当成「这张生成不出来」而退回原图。
    let mut unknown: Vec<String> = Vec::new();
    let declared: Option<HashSet<String>> = window.map(|w| {
        w.into_iter().map(|p| job_key(&library_id, &p)).collect()
    });

    {
        let engine = state.engine.lock().map_err(|_| "引擎锁失效".to_string())?;
        let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
        let lib = lib.lock().map_err(|_| "图库锁失效".to_string())?;

        for file_path in &paths {
            let key = job_key(&library_id, file_path);

            // ① 内存里已有结论的，直接回答，连数据库都不用碰。
            if let Some(resolved) = state.thumbs.lookup(&key) {
                out.push(match resolved {
                    Some(thumb) => ThumbBatchItem {
                        path: file_path.clone(),
                        status: "ready".into(),
                        thumb,
                        placeholder: String::new(),
                    },
                    None => ThumbBatchItem {
                        path: file_path.clone(),
                        status: "fallback".into(),
                        thumb: file_path.clone(),
                        placeholder: String::new(),
                    },
                });
                continue;
            }

            let Some(entry) = lib.db.get_file_by_path(file_path).map_err(|e| e.to_string())? else {
                unknown.push(file_path.clone());
                continue;
            };

            // 索引阶段没有算过内容哈希，退化成「路径哈希」—— 与后台任务保持一致。
            let hash = entry
                .blake3_hash
                .filter(|h| !h.is_empty())
                .unwrap_or_else(|| crate::utils::hash::blake3_hash_bytes(file_path.as_bytes()));

            let thumb_path = lib.thumbnail.thumbnail_path(&hash, THUMB_SIZE);
            if thumb_path.exists() {
                let thumb = thumb_path.to_string_lossy().to_string();
                state.thumbs.remember(&key, thumb.clone());
                out.push(ThumbBatchItem {
                    path: file_path.clone(),
                    status: "ready".into(),
                    thumb,
                    placeholder: String::new(),
                });
                continue;
            }

            // ② 真图还没生成：顺手把**已经缓存**的模糊占位图带上（纯磁盘读，
            //    约 0.5 KB），卡片立刻有东西可显示，不必等解码。
            out.push(ThumbBatchItem {
                path: file_path.clone(),
                status: "pending".into(),
                thumb: String::new(),
                placeholder: lib.thumbnail.read_placeholder(&hash).unwrap_or_default(),
            });

            jobs.push(ThumbJob {
                key,
                path: file_path.clone(),
                source: std::path::PathBuf::from(file_path),
                hash,
                engine: lib.thumbnail.clone(),
                kind: ThumbKind::Grid,
            });
        }
    } // 锁在这里释放，解码过程绝不持锁

    // 即使没有新任务也要调用：窗口外的过期 urgent 任务需要被降级回整库队列。
    match declared {
        Some(win) => state.thumbs.enqueue_window(jobs, &win),
        None => state.thumbs.enqueue_with(jobs, Priority::Urgent),
    }

    for file_path in unknown {
        out.push(ThumbBatchItem {
            path: file_path,
            status: "pending".into(),
            thumb: String::new(),
            placeholder: String::new(),
        });
    }

    Ok(out)
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
///
/// 与打开图库时的自动预热是同一条路径，区别是**不受预热开关影响** —— 用户
/// 关掉了自动预热之后，仍然可以在这里手动跑一次。
#[tauri::command]
pub async fn build_missing_thumbnails(
    library_id: String,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let engine = state.engine.lock().map_err(|_| "引擎锁失效".to_string())?;
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().map_err(|_| "图库锁失效".to_string())?;
    Ok(enqueue_missing_thumbnails(&state.thumbs, &lib, &library_id, true))
}

/// 所有图库的缩略图预热进度（侧边栏库名旁的圆环用它）。
#[tauri::command]
pub async fn get_warm_progress(state: State<'_, AppState>) -> Result<Vec<WarmProgress>, String> {
    Ok(state.thumbs.all_warm_progress())
}

/// 读取「打开图库时后台补齐缩略图」的开关状态。
#[tauri::command]
pub async fn get_prewarm_enabled(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.thumbs.prewarm_enabled())
}

/// 设置「打开图库时后台补齐缩略图」。默认开启。
///
/// 关掉之后：打开 / 导入 / 恢复图库不会再自动排队，但滚动到哪张生成哪张的
/// 按需路径完全不受影响，手动「补齐缺失缩略图」也仍然可用。
#[tauri::command]
pub async fn set_prewarm_enabled(enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
    state.thumbs.set_prewarm_enabled(enabled);
    Ok(())
}

/// 前端上报「用户正在滚动 / 拖拽」——触发后台的交互感知节流。
///
/// 后台收到后会在接下来约 0.5 秒内**暂停整库预热队列**，只继续做用户当前
/// 视口（urgent）的任务，把 CPU 让给界面渲染。不需要「交互结束」的上报：
/// 静默窗口到期自动恢复满速，所以前端可以在滚动回调里放心频繁调用
/// （前端已做约 120ms 的限流）。
#[tauri::command]
pub async fn thumbs_interaction(state: State<'_, AppState>) -> Result<(), String> {
    state.thumbs.note_interaction();
    Ok(())
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