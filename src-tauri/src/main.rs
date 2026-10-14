#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use pixvault_lib::{commands, gpu, AppState};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let app_state = AppState::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .setup(|app| {
            // 后台缩略图任务需要 AppHandle 才能把进度/结果推给前端。
            app.state::<AppState>().thumbs.set_app(app.handle().clone());

            // ── 主窗口 ────────────────────────────────────────────────
            // 窗口刻意不写在 tauri.conf.json 里：WebView2 的启动参数只能在
            // *创建时* 传入，而是否禁用 GPU 取决于上一次运行有没有崩溃到底。
            let data_dir = app
                .state::<AppState>()
                .engine
                .lock()
                .map(|g| g.data_dir.clone())
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            let software = gpu::decide_software_rendering(&data_dir);
            gpu::reset_render_log(&data_dir);
            gpu::render_log(
                &data_dir,
                &format!("[startup] software_rendering={software} pid={}", std::process::id()),
            );

            let log_dir = data_dir.clone();
            #[allow(unused_mut)]
            let mut builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("PixVault")
                .inner_size(1400.0, 900.0)
                .min_inner_size(900.0, 600.0)
                .resizable(true)
                .fullscreen(false)
                .decorations(true)
                // 注意：不要在这里调 `.transparent(false)`。Tauri 把
                // `WebviewWindowBuilder::transparent` 门控在
                // `#[cfg(any(not(target_os = "macos"), feature = "macos-private-api"))]`，
                // 于是 Windows 上能编过、macOS 上直接 E0599 编译失败。
                // 而且它默认就是 false，本来也不需要显式关一次。
                .center()
                .on_page_load(move |_window, payload| {
                    gpu::render_log(
                        &log_dir,
                        &format!("[page_load] {:?} {}", payload.event(), payload.url()),
                    );
                });

            #[cfg(target_os = "windows")]
            {
                builder = builder.additional_browser_args(&gpu::browser_args(software));
            }
            #[cfg(not(target_os = "windows"))]
            let _ = software;

            builder.build()?;

            // 8 秒后注入探针：不管前端 bundle 有没有跑起来，都把 WebView 内部
            // 状态回传一次，写进 render-debug.log。
            let probe_dir = data_dir.clone();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(8));
                gpu::render_log(&probe_dir, "[probe] injecting");
                if let Some(win) = handle.get_webview_window("main") {
                    if let Err(e) = win.eval(gpu::RENDER_PROBE_JS) {
                        gpu::render_log(&probe_dir, &format!("[probe] eval failed: {e}"));
                    }
                } else {
                    gpu::render_log(&probe_dir, "[probe] window not found");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            gpu::ui_ready,
            gpu::render_probe,
            gpu::diag_log,
            commands::library::open_library,
            commands::library::create_library,
            commands::library::close_library,
            commands::library::get_libraries,
            commands::library::index_library,
            commands::library::get_nas_status,
            commands::library::restore_libraries,
            commands::library::reorder_libraries,
            commands::library::rename_library,
            commands::library::delete_library,
            commands::files::get_files,
            commands::files::get_file_info,
            commands::files::update_file_rating,
            commands::files::update_file_notes,
            commands::files::set_file_favorite,
            commands::files::set_file_color,
            commands::files::get_folder_tree,
            commands::files::get_view_counts,
            commands::files::delete_file,
            commands::files::trash_files,
            commands::files::get_deleted_files,
            commands::files::restore_file,
            commands::files::purge_file,
            commands::files::empty_trash,
            commands::files::get_thumbnail_path,
            commands::files::get_thumbnail_progress,
            commands::files::get_preview_path,
            commands::files::read_thumb_data_url,
            commands::files::build_missing_thumbnails,
            commands::files::open_in_explorer,
            commands::files::open_file,
            commands::files::copy_files_to_clipboard,
            commands::tags::get_all_tags,
            commands::tags::create_tag,
            commands::tags::add_tag_to_file,
            commands::tags::remove_tag_from_file,
            commands::tags::batch_add_tags,
            commands::search::search_files,
            commands::search::search_by_color,
            commands::search::find_duplicates,
            commands::import::detect_library_kind,
            commands::import::import_eagle_library,
            commands::import::import_pixcall_library,
        ])
        .run(tauri::generate_context!())
        .expect("error while running PixVault");
}