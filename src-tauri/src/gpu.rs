//! GPU 兼容性兜底。
//!
//! WebView2 会为每个宿主进程起一个 GPU 子进程。在某些环境里这个子进程起不来
//! （安全软件拦截、显卡驱动异常、WebView2 运行时状态损坏……），Chromium 会走到
//! `gpu_data_manager_impl_private.cc` 里的
//! `LOG_FATAL: GPU process isn't usable. Goodbye.` —— **浏览器进程直接中止**。
//! 宿主窗口于是只剩白屏，Windows 还会弹一个
//! 「msedgewebview2 已停止工作 / Error launching CrashSender.exe」。
//!
//! 这类崩溃无法在进程内捕获（进程已经没了），所以这里用「哨兵文件」的思路：
//!
//! 1. 每次启动**先把状态写成「本次尚未就绪」**，然后才创建窗口；
//! 2. 前端渲染成功后回调 [`ui_ready`]，把状态改回「正常」；
//! 3. 如果上次启动留下了「尚未就绪」，说明上次是崩溃退出 → 本次改用软件渲染
//!    （`--disable-gpu`，压根不创建 GPU 子进程），并把结论**持久化**，
//!    之后每次启动都直接走软件渲染，不再反复崩溃。
//!
//! 恢复硬件渲染：设置环境变量 `PRISMVAULT_FORCE_GPU=1` 启动一次即可
//! （也会順便把状态重置）。
//! 强制软件渲染：`PRISMVAULT_FORCE_SOFTWARE_RENDER=1`。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// wry 在 Windows 上默认会传给 WebView2 的参数。
/// **我们自己传时必须原样带上** —— wry 的逻辑是「配置了就用配置的」，
/// 不配置才用这份默认值，所以自建参数会覆盖掉它，漏了就会丢掉这些特性开关。
pub const WRY_DEFAULT_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// 软件渲染时追加的参数：不启动 GPU 进程，也不做 GPU 合成。
pub const SOFTWARE_ARGS: &str = "--disable-gpu --disable-gpu-compositing";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GpuState {
    /// 已确认本机 GPU 初始化不可用 → 固定使用软件渲染。
    #[serde(default)]
    pub software_rendering: bool,
    /// 上一次启动没有走到「界面就绪」，说明中途异常退出。
    #[serde(default)]
    pub crashed_last_run: bool,
}

fn state_path(data_dir: &Path) -> PathBuf {
    data_dir.join("gpu-state.json")
}

/// 渲染诊断日志（每次启动清空重写）。
///
/// 白屏这类问题在 WebView 内部发生，外部看不到任何东西；把「软件渲染开关」
/// 「页面加载事件」「前端上报的视口/挂载状态」落到同一个文件里，排查时一眼
/// 就能看出卡在哪一步。
pub fn render_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("render-debug.log")
}

pub fn reset_render_log(data_dir: &Path) {
    let _ = std::fs::create_dir_all(data_dir);
    let _ = std::fs::write(render_log_path(data_dir), b"");
}

pub fn render_log(data_dir: &Path, line: &str) {
    use std::io::Write;
    let _ = std::fs::create_dir_all(data_dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(render_log_path(data_dir))
    {
        let _ = writeln!(f, "{line}");
    }
}

fn read_state(data_dir: &Path) -> GpuState {
    std::fs::read_to_string(state_path(data_dir))
        .ok()
        .and_then(|s| serde_json::from_str::<GpuState>(&s).ok())
        .unwrap_or_default()
}

fn write_state(data_dir: &Path, st: &GpuState) {
    let _ = std::fs::create_dir_all(data_dir);
    if let Ok(json) = serde_json::to_string_pretty(st) {
        let _ = std::fs::write(state_path(data_dir), json);
    }
}

/// 本次启动是否使用软件渲染。
///
/// 副作用：把状态标记为「尚未就绪」—— 正常渲染成功后由 [`ui_ready`] 清除。
pub fn decide_software_rendering(data_dir: &Path) -> bool {
    // 环境变量优先级最高，便于人工排查 / 临时切换。
    if std::env::var("PRISMVAULT_FORCE_GPU").is_ok() {
        write_state(data_dir, &GpuState::default());
        return false;
    }
    if std::env::var("PRISMVAULT_FORCE_SOFTWARE_RENDER").is_ok() {
        write_state(
            data_dir,
            &GpuState { software_rendering: true, crashed_last_run: false },
        );
        return true;
    }

    let st = read_state(data_dir);
    if st.software_rendering {
        // 已经判定过 GPU 不可用，保持现状（不重置 crashed_last_run，
        // 免得下次又去试一次硬件渲染再崩一遍）。
        return true;
    }
    if st.crashed_last_run {
        // 上次没跑到「界面就绪」→ 自动降级，并记下结论。
        write_state(
            data_dir,
            &GpuState { software_rendering: true, crashed_last_run: true },
        );
        return true;
    }

    // 首次启动（或上次正常）：先试硬件渲染，标记为「尚未就绪」。
    write_state(
        data_dir,
        &GpuState { software_rendering: false, crashed_last_run: true },
    );
    false
}

/// 前端渲染成功 → 清除「尚未就绪」标记。
pub fn mark_ready(data_dir: &Path) {
    let st = read_state(data_dir);
    // 软件渲染是我们主动选择的稳态，不去动它。
    if !st.software_rendering {
        write_state(data_dir, &GpuState::default());
    }
}

/// 传给 WebView2 的启动参数（仅 Windows 有意义）。
pub fn browser_args(software: bool) -> String {
    if software {
        format!("{WRY_DEFAULT_ARGS} {SOFTWARE_ARGS}")
    } else {
        WRY_DEFAULT_ARGS.to_string()
    }
}

/// 前端挂载完成、界面确实渲染出来了 —— 调用它注销崩溃哨兵。
#[tauri::command]
pub fn ui_ready(state: tauri::State<'_, crate::AppState>, diag: Option<String>) {
    let data_dir = state
        .engine
        .lock()
        .map(|g| g.data_dir.clone())
        .unwrap_or_else(|_| PathBuf::from("."));
    if let Some(d) = diag {
        render_log(&data_dir, &format!("[ui_ready] {d}"));
    }
    mark_ready(&data_dir);
}

/// 后端注入的探针把页面内部状态回传到这里。
#[tauri::command]
pub fn render_probe(state: tauri::State<'_, crate::AppState>, info: String) {
    let data_dir = state
        .engine
        .lock()
        .map(|g| g.data_dir.clone())
        .unwrap_or_else(|_| PathBuf::from("."));
    render_log(&data_dir, &format!("[probe] {info}"));
}

/// 前端把诊断信息写进 render-debug.log（图片加载失败、缩略图请求结果等）。
///
/// 图片看不见这类问题多半卡在 WebView 内部：`<img>` 的 onError 只在 JS 侧可见，
/// 不落到文件里就只能靠猜。前端只在「前若干个请求 + 全部失败」时上报，避免刷屏。
#[tauri::command]
pub fn diag_log(state: tauri::State<'_, crate::AppState>, tag: String, msg: String) {
    let data_dir = state
        .engine
        .lock()
        .map(|g| g.data_dir.clone())
        .unwrap_or_else(|_| PathBuf::from("."));
    render_log(&data_dir, &format!("[fe:{tag}] {msg}"));
}

/// 注入到页面里执行的诊断脚本：把 WebView 内部状态回传给后端。
///
/// 走 `__TAURI_INTERNALS__.invoke` 直接调用命令，**不依赖前端 bundle 是否加载
/// 成功** —— 这样即使打包产物本身有问题，也能知道 JS 引擎是活的。
pub const RENDER_PROBE_JS: &str = r#"(function(){
  function send(s){ try { window.__TAURI_INTERNALS__.invoke('render_probe', { info: String(s) }); } catch(e){} }
  try {
    var root = document.getElementById('root');
    send(JSON.stringify({
      href: location.href,
      ready: document.readyState,
      rootChildren: root ? root.childElementCount : -1,
      bodyLen: document.body ? (document.body.innerText || '').length : -1,
      bodyText: document.body ? (document.body.innerText || '').slice(0, 120) : '',
      scripts: Array.prototype.map.call(document.querySelectorAll('script'), function(s){ return s.src || 'inline'; }),
      stylesheets: document.querySelectorAll('link[rel=stylesheet]').length,
      w: window.innerWidth, h: window.innerHeight, dpr: window.devicePixelRatio
    }));
  } catch (e) { send('PROBE_ERROR: ' + e); }
})();"#;
