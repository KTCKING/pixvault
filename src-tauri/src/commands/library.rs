use tauri::State;
use tauri::Emitter;
use crate::AppState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryInfo {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub is_network: bool,
    pub file_count: i64,
    pub thumbnail_cache_size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenLibraryResult {
    pub library: LibraryInfo,
    pub initial_file_count: i64,
}

#[tauri::command]
pub async fn open_library(
    path: String,
    state: State<'_, AppState>,
) -> Result<OpenLibraryResult, String> {
    let root_path = std::path::PathBuf::from(&path);
    if !root_path.exists() {
        return Err(format!("路径不存在: {}", path));
    }

    let mut engine = state.engine.lock().unwrap();
    let id = engine.open_library(&root_path)?;
    save_libraries_config(&engine);

    let lib = engine.get_library(&id).ok_or("打开后未找到图库")?;
    let lib = lib.lock().unwrap();

    let file_count = lib.db.count_files(&crate::engine::db::FileFilter::default()).unwrap_or(0);
    let cache_size = lib.thumbnail.cache_size();

    Ok(OpenLibraryResult {
        library: LibraryInfo {
            id: lib.id.clone(),
            name: lib.name.clone(),
            root_path: lib.root_path.to_string_lossy().to_string(),
            is_network: lib.is_network,
            file_count,
            thumbnail_cache_size: cache_size,
        },
        initial_file_count: file_count,
    })
}

/// Create a brand-new library: makes a folder named `name` inside `parent_dir`
/// and opens it as a library.
#[tauri::command]
pub async fn create_library(
    parent_dir: String,
    name: String,
    state: State<'_, AppState>,
) -> Result<OpenLibraryResult, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("库名称不能为空".to_string());
    }
    if name
        .chars()
        .any(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err("库名称不能包含 \\ / : * ? \" < > | 等字符".to_string());
    }

    let parent = std::path::PathBuf::from(&parent_dir);
    if !parent.exists() {
        return Err(format!("所选文件夹不存在: {}", parent_dir));
    }
    if !parent.is_dir() {
        return Err("所选路径不是文件夹".to_string());
    }

    let root_path = parent.join(&name);
    std::fs::create_dir_all(&root_path).map_err(|e| format!("创建文件夹失败: {e}"))?;

    let mut engine = state.engine.lock().unwrap();
    let id = engine.open_library(&root_path)?;
    save_libraries_config(&engine);

    let lib = engine.get_library(&id).ok_or("创建后未找到图库")?;
    let lib = lib.lock().unwrap();

    let file_count = lib.db.count_files(&crate::engine::db::FileFilter::default()).unwrap_or(0);
    let cache_size = lib.thumbnail.cache_size();

    Ok(OpenLibraryResult {
        library: LibraryInfo {
            id: lib.id.clone(),
            name: lib.name.clone(),
            root_path: lib.root_path.to_string_lossy().to_string(),
            is_network: lib.is_network,
            file_count,
            thumbnail_cache_size: cache_size,
        },
        initial_file_count: file_count,
    })
}

#[tauri::command]
pub async fn close_library(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let mut engine = state.engine.lock().unwrap();
        engine.close_library(&id);
        save_libraries_config(&engine);
    }
    // 关掉的库不该在侧边栏留一个永远不动的进度环。
    state.thumbs.forget_warm(&id);
    Ok(())
}

/// Rename a library (updates the in-memory display name; the on-disk folder
/// name is intentionally left untouched).
#[tauri::command]
pub async fn rename_library(
    id: String,
    new_name: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let name = new_name.trim().to_string();
    if name.is_empty() {
        return Err("库名称不能为空".to_string());
    }
    // 与 create_library 用同一套字符表。虽然名字不落到磁盘（文件夹名不变），
    // 但库名会被写进 libraries.json，路径分隔符混进去后面排查会很痛苦。
    if name
        .chars()
        .any(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err("库名称不能包含 \\ / : * ? \" < > | 等字符".to_string());
    }
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&id).ok_or("未找到图库")?;
    let mut lib = lib.lock().unwrap();
    lib.name = name;
    drop(lib);
    save_libraries_config(&engine);
    Ok(())
}

/// Permanently delete a library. When `delete_files` is true the on-disk files
/// (and the `.pixvault` metadata folder) are also removed from disk. When false
/// the library is only closed and forgotten by the app.
///
/// `delete_files` 走的是 `remove_dir_all`，不可逆也不进回收站，所以除了前端
/// 那个要手打库名的弹窗，这里再复核一次 `confirm_name`，并过一道 `ensure_deletable`
/// 的路径闸。
#[tauri::command]
pub async fn delete_library(
    id: String,
    delete_files: bool,
    confirm_name: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // 先把目标定位清楚。校验失败就直接返回，库仍留在列表里 —— 不能出现
    // 「删到一半失败、入口却已经没了」这种状态。
    let (root_path, name, is_network) = {
        let engine = state.engine.lock().unwrap();
        let lib = engine.get_library(&id).ok_or("未找到图库")?;
        let lib = lib.lock().unwrap();
        (lib.root_path.clone(), lib.name.clone(), lib.is_network)
    };

    if delete_files {
        if confirm_name.as_deref().map(str::trim) != Some(name.as_str()) {
            return Err("库名不匹配，已取消删除".to_string());
        }
        ensure_deletable(&root_path, is_network)?;
    }

    {
        let mut engine = state.engine.lock().unwrap();
        engine.close_library(&id);
        save_libraries_config(&engine);
    }
    // 与 close_library 命令一致：别在侧边栏留一个永远不动的进度环。
    state.thumbs.forget_warm(&id);

    if delete_files && root_path.exists() {
        std::fs::remove_dir_all(&root_path).map_err(|e| {
            format!(
                "库已从列表移除，但文件夹没删干净：{e}\n可能还有程序占用着 {}，关掉之后手动删除即可。",
                root_path.display()
            )
        })?;
    }

    Ok(())
}

/// 删除库目录前的路径闸。这个操作不可逆、不进回收站，宁可多拦几次。
///
/// 拦三类：磁盘根 / 用户主目录及其常见子目录 / 里面没有 `.pixvault` 的目录
/// （说明它不是本应用建出来的库根，而是库根的上级或用户随手选的目录）。
fn ensure_deletable(root: &std::path::Path, is_network: bool) -> Result<(), String> {
    if !root.exists() {
        // 磁盘上本来就不在了，当成「从列表移除」处理即可，不必拦。
        return Ok(());
    }
    if !root.is_dir() {
        return Err(format!("库根不是文件夹，已拒绝删除：{}", root.display()));
    }

    // 盘根不删。注意别用 `components().count() < 2` 来判断 —— Windows 上
    // `C:\` 是 [Prefix, RootDir] 两个组件，count 正好是 2，那条判断会放行
    // 整个磁盘（这个 bug 是单元测试抓出来的）。看有没有实际目录名才准。
    let named = root
        .components()
        .filter(|c| matches!(c, std::path::Component::Normal(_)))
        .count();
    if named == 0 {
        return Err(format!(
            "拒绝删除磁盘根目录（{}）——请把图库放在一个单独的文件夹里",
            root.display()
        ));
    }
    // `\\server\share` 是共享根（Prefix + RootDir + 1 个 Normal），删它等于
    // 清空整个网络共享，同样不干。
    if is_network && named <= 1 {
        return Err(format!(
            "拒绝删除网络共享根目录（{}）——请指向共享下的某个具体文件夹",
            root.display()
        ));
    }

    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if is_sensitive_dir(&canonical) {
        return Err(format!(
            "{} 是用户主目录或系统目录，删除它会把桌面 / 文档一起带走，已拒绝。\n请把图库放在单独的子文件夹里（比如 D:\\图库\\我的素材）",
            root.display()
        ));
    }

    // 本地库根下一定有 `.pixvault`（open_library 建的）。网络库的元数据存在
    // %APPDATA%/libraries/<id>，所以跳过这项。
    if !is_network && !crate::utils::winattr::metadata_dir(root).is_dir() {
        return Err(format!(
            "{} 里没有 .pixvault 元数据，看起来不是图库根目录，已拒绝删除",
            root.display()
        ));
    }

    Ok(())
}

/// 用户主目录本身，以及它下面那些「删了就出大事」的目录。
fn is_sensitive_dir(canonical: &std::path::Path) -> bool {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(std::path::PathBuf::from);

    if let Some(home) = home {
        let mut guards = vec![home.clone()];
        // 中文 Windows 下资源管理器显示的是「桌面」「文档」等，但真实目录名
        // 仍是这些英文名，所以按英文名挡。
        for sub in [
            "Desktop", "Documents", "Downloads", "Pictures", "Music", "Videos",
            "AppData", "Library", "Movies",
        ] {
            guards.push(home.join(sub));
        }
        for g in guards {
            if g.canonicalize().map(|c| c == canonical).unwrap_or(false) {
                return true;
            }
        }
    }

    false
}

#[tauri::command]
pub async fn get_libraries(
    state: State<'_, AppState>,
) -> Result<Vec<LibraryInfo>, String> {
    let engine = state.engine.lock().unwrap();
    let mut libraries = Vec::new();

    // Iterate in the persisted user-defined order, not HashMap order.
    for id in engine.ordered_ids() {
        let Some(lib) = engine.libraries.get(&id) else { continue };
        let lib = lib.lock().unwrap();
        let file_count = lib.db.count_files(&crate::engine::db::FileFilter::default()).unwrap_or(0);
        libraries.push(LibraryInfo {
            id: lib.id.clone(),
            name: lib.name.clone(),
            root_path: lib.root_path.to_string_lossy().to_string(),
            is_network: lib.is_network,
            file_count,
            thumbnail_cache_size: lib.thumbnail.cache_size(),
        });
    }

    Ok(libraries)
}

/// Persist a new display order for the opened libraries.
#[tauri::command]
pub async fn reorder_libraries(
    ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut engine = state.engine.lock().unwrap();
    engine.reorder_libraries(ids);
    save_libraries_config(&engine);
    Ok(())
}

#[tauri::command]
pub async fn index_library(
    app_handle: tauri::AppHandle,
    library_id: String,
    state: State<'_, AppState>,
) -> Result<IndexProgress, String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;

    // 扫描目录（只读文件元数据，不解码图片），不持有图库锁。
    let files = {
        let lib = lib.lock().unwrap();
        lib.indexer.scan_directory(&lib.root_path)
    };
    let total = files.len() as u64;

    let _ = app_handle.emit("index-progress", IndexProgress {
        total, processed: 0, current_file: String::new(), stage: "scanning".to_string(),
    });

    // Fast path: single transaction, metadata only, skip per-file DB check
    {
        let lib = lib.lock().unwrap();
        let conn = lib.db.conn();
        conn.execute("BEGIN TRANSACTION", []).map_err(|e| e.to_string())?;

        let result = (|| -> Result<(), String> {
            for (i, file_path) in files.iter().enumerate() {
                if let Some(entry) = crate::engine::indexer::Indexer::build_entry(file_path) {
                    lib.db.insert_file(&entry).map_err(|e| e.to_string())?;
                }

                if i % 50 == 0 || i == files.len() - 1 {
                    let _ = app_handle.emit("index-progress", IndexProgress {
                        total, processed: (i + 1) as u64,
                        current_file: file_path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                        stage: "indexing".to_string(),
                    });
                }
            }
            Ok(())
        })();

        if result.is_ok() {
            conn.execute("COMMIT", []).map_err(|e| e.to_string())?;
        } else {
            conn.execute("ROLLBACK", []).map_err(|e| e.to_string())?;
            return Err(result.unwrap_err());
        }
    }

    // 真实文件数（供侧边栏 / libraries.json 使用）。索引写入的数据以前从不回
    // 写配置，导致重启后图库计数恒为 0 —— 这里补上。
    let file_count = {
        let lib = lib.lock().unwrap();
        lib.db.count_files(&crate::engine::db::FileFilter::default()).unwrap_or(total as i64)
    };
    save_libraries_config(&engine);

    // 把还没生成缩略图的文件排进后台队列（只查库 + 判存在，毫秒级）。
    let queued = {
        let lib = lib.lock().unwrap();
        crate::commands::files::enqueue_missing_thumbnails(&state.thumbs, &lib, &library_id, false)
    };
    drop(engine);

    let _ = app_handle.emit("index-progress", IndexProgress {
        total: file_count.max(0) as u64, processed: file_count.max(0) as u64,
        current_file: String::new(), stage: "complete".to_string(),
    });

    tracing::info!("索引完成：{file_count} 个文件，排队生成 {queued} 张缩略图");

    Ok(IndexProgress {
        total: file_count.max(0) as u64,
        processed: file_count.max(0) as u64,
        current_file: String::new(),
        stage: "complete".to_string(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexProgress {
    pub total: u64,
    pub processed: u64,
    pub current_file: String,
    pub stage: String,
}

#[tauri::command]
pub async fn get_nas_status(
    library_id: String,
    state: State<'_, AppState>,
) -> Result<NasStatus, String> {
    let engine = state.engine.lock().unwrap();
    let lib = engine.get_library(&library_id).ok_or("未找到图库")?;
    let lib = lib.lock().unwrap();

    if !lib.is_network {
        return Ok(NasStatus {
            status: "local".to_string(),
            latency_ms: 0.0,
        });
    }

    let start = std::time::Instant::now();
    match std::fs::metadata(&lib.root_path) {
        Ok(_) => {
            let latency = start.elapsed().as_secs_f64() * 1000.0;
            let status = if latency > 100.0 {
                "degraded".to_string()
            } else {
                "online".to_string()
            };
            Ok(NasStatus { status, latency_ms: latency })
        }
        Err(_) => Ok(NasStatus {
            status: "offline".to_string(),
            latency_ms: 0.0,
        }),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NasStatus {
    pub status: String,
    pub latency_ms: f64,
}

/// Save library paths + cached file counts to config
#[derive(Serialize, Deserialize)]
struct LibraryConfigEntry {
    path: String,
    file_count: i64,
}

pub(crate) fn save_libraries_config(engine: &crate::engine::Engine) {
    let config_path = engine.data_dir.join("libraries.json");
    let entries: Vec<LibraryConfigEntry> = engine.ordered_ids().iter()
        .filter_map(|id| engine.libraries.get(id))
        .filter_map(|lib| lib.lock().ok())
        .map(|lib| LibraryConfigEntry {
            path: lib.root_path.to_string_lossy().to_string(),
            file_count: lib.db.count_files(&crate::engine::db::FileFilter::default()).unwrap_or(0),
        })
        .collect();
    if let Ok(json) = serde_json::to_string(&entries) {
        std::fs::write(&config_path, json).ok();
    }
}

/// Load saved library config (path + cached count)
fn load_libraries_config(data_dir: &std::path::Path) -> Vec<LibraryConfigEntry> {
    let config_path = data_dir.join("libraries.json");
    if let Ok(json) = std::fs::read_to_string(&config_path) {
        serde_json::from_str(&json).unwrap_or_default()
    } else {
        Vec::new()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RestoreResult {
    pub libraries: Vec<LibraryInfo>,
    pub failed_paths: Vec<String>,
}

#[tauri::command]
pub async fn restore_libraries(
    state: State<'_, AppState>,
) -> Result<RestoreResult, String> {
    let mut engine = state.engine.lock().unwrap();
    let config = load_libraries_config(&engine.data_dir);
    let mut result = Vec::new();
    let mut failed = Vec::new();
    let mut need_reindex = Vec::new();

    for entry in &config {
        let root_path = std::path::PathBuf::from(&entry.path);
        if !root_path.exists() {
            failed.push(entry.path.clone());
            continue;
        }
        if let Ok(id) = engine.open_library(&root_path) {
            let lib = engine.get_library(&id).unwrap();

            // 真实计数：`libraries.json` 里的缓存值只会在打开/关闭库时写入，
            // 索引之后从来没人回写，所以它长期是 0 —— 重启后侧边栏就显示不出
            // 图片数量。SQLite 的 COUNT 走索引，代价可以忽略。
            let (count, cache_size, name, is_network, lib_root) = {
                let lib = lib.lock().unwrap();
                (
                    lib.db.count_files(&crate::engine::db::FileFilter::default()).unwrap_or(0),
                    lib.thumbnail.cache_size(),
                    lib.name.clone(),
                    lib.is_network,
                    lib.root_path.to_string_lossy().to_string(),
                )
            };

            result.push(LibraryInfo {
                id: id.clone(),
                name,
                root_path: lib_root,
                is_network,
                file_count: count,
                thumbnail_cache_size: cache_size,
            });

            if count == 0 {
                need_reindex.push(id);
            } else {
                // 数据库里已经有东西了 —— 顺手把缺的缩略图排进后台队列。
                let lib = lib.lock().unwrap();
                crate::commands::files::enqueue_missing_thumbnails(&state.thumbs, &lib, &id, false);
            }
        }
    }

    // 回写真实计数，避免下次启动又把它当成空库。
    save_libraries_config(&engine);

    // Spawn background re-index for empty libraries (non-blocking)
    if !need_reindex.is_empty() {
        let engine = state.engine.clone();
        let thumbs = state.thumbs.clone();
        tauri::async_runtime::spawn(async move {
            for id in need_reindex {
                reindex_in_background(&engine, &thumbs, &id);
            }
        });
    }

    Ok(RestoreResult { libraries: result, failed_paths: failed })
}

/// 后台重扫一个库并回写计数，随后排队生成缺失的缩略图。
fn reindex_in_background(
    engine: &std::sync::Arc<std::sync::Mutex<crate::engine::Engine>>,
    thumbs: &std::sync::Arc<crate::engine::thumbs::ThumbWorker>,
    library_id: &str,
) {
    let Ok(engine) = engine.lock() else { return };
    let Some(lib) = engine.get_library(library_id) else { return };

    {
        let lib = lib.lock().unwrap();
        let files = lib.indexer.scan_directory(&lib.root_path);
        let conn = lib.db.conn();
        conn.execute("BEGIN TRANSACTION", []).ok();
        for file_path in &files {
            if let Some(entry) = crate::engine::indexer::Indexer::build_entry(file_path) {
                lib.db.insert_file(&entry).ok();
            }
        }
        conn.execute("COMMIT", []).ok();
    }

    save_libraries_config(&engine);

    let lib = lib.lock().unwrap();
    crate::commands::files::enqueue_missing_thumbnails(thumbs, &lib, library_id, false);
}
#[cfg(test)]
mod delete_guard_tests {
    //! `ensure_deletable` 是「删除库及所有文件」前面唯一的机械闸 —— 前端那个
    //! 输库名的弹窗挡不住程序 bug，这里必须自己站得住。
    use super::ensure_deletable;

    /// 造一个空目录（先清掉上次残留）。
    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pixvault-guard-{}", tag));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn refuses_volume_root() {
        let root = if cfg!(windows) {
            std::path::Path::new("C://")
        } else {
            std::path::Path::new("/")
        };
        let err = ensure_deletable(root, false).unwrap_err();
        assert!(err.contains("磁盘根目录"), "实际错误: {}", err);
    }

    #[test]
    fn refuses_folder_without_metadata() {
        let dir = scratch("plain");
        // 本地库根下一定有 .pixvault；没有就说明这不是库根，不能删。
        let err = ensure_deletable(&dir, false).unwrap_err();
        assert!(err.contains(".pixvault"), "实际错误: {}", err);

        // 建出元数据目录后应当放行。
        std::fs::create_dir_all(dir.join(".pixvault")).unwrap();
        assert!(ensure_deletable(&dir, false).is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn network_library_skips_metadata_check() {
        let dir = scratch("network");
        // 网络库的元数据在 %APPDATA%/libraries/<id>，库根下没有 .pixvault。
        assert!(ensure_deletable(&dir, true).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_home_directory() {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap();
        let home = std::path::PathBuf::from(home);
        if !home.is_dir() {
            return;
        }
        let err = ensure_deletable(&home, true).unwrap_err();
        assert!(err.contains("用户主目录"), "实际错误: {}", err);
    }

    #[test]
    fn missing_path_is_treated_as_already_gone() {
        let dir = std::env::temp_dir().join("pixvault-guard-not-there");
        let _ = std::fs::remove_dir_all(&dir);
        // 磁盘上本来就没有 → 不该拦住「从列表移除」。
        assert!(ensure_deletable(&dir, false).is_ok());
    }
}
