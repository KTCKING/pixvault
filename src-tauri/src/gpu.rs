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
//! 恢复硬件渲染：**设置 → 渲染 → 硬件加速**（持久化，重启生效），或者用环境
//! 变量 `PRISMVAULT_FORCE_GPU=1` 启动一次（等价于选择「硬件加速」）。
//! 强制软件渲染：环境变量 `PRISMVAULT_FORCE_SOFTWARE_RENDER=1`。
//!
//! 为什么要有界面开关：环境变量对桌面应用的用户基本不可达（快捷方式双击启动
//! 的进程读不到），一旦自动降级就等于**永久**锁在软件渲染上 —— 后来驱动修好了
//! 也没法自己恢复。界面里的「硬件加速」提供了这条出路，同时保留哨兵：选了
//! 硬件加速但真的崩了，下次启动会自动退回并说明原因，不会把用户卡在白屏里。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// wry 在 Windows 上默认会传给 WebView2 的参数。
/// **我们自己传时必须原样带上** —— wry 的逻辑是「配置了就用配置的」，
/// 不配置才用这份默认值，所以自建参数会覆盖掉它，漏了就会丢掉这些特性开关。
pub const WRY_DEFAULT_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// 软件渲染时追加的参数：不启动 GPU 进程，也不做 GPU 合成。
pub const SOFTWARE_ARGS: &str = "--disable-gpu --disable-gpu-compositing";

/// 渲染方式的选择（用户在设置里定的，持久化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuMode {
    /// 自动：跟着崩溃哨兵走 —— 能起 GPU 就用，崩过一次就退软件渲染。
    /// **选它不会改变已有结论**（不会主动去重试硬件渲染）。
    #[default]
    Auto,
    /// 用户显式要求硬件渲染。下次启动就会尝试，即使上次判定过 GPU 不可用。
    /// 哨兵仍然生效：真崩了就自动退回 [`GpuMode::Auto`] + 软件渲染。
    Hardware,
    /// 用户显式要求软件渲染（省电、排查画面问题）。
    Software,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GpuState {
    /// 已确认本机 GPU 初始化不可用 → 固定使用软件渲染。
    #[serde(default)]
    pub software_rendering: bool,
    /// 上一次启动没有走到「界面就绪」，说明中途异常退出。
    #[serde(default)]
    pub crashed_last_run: bool,
    /// 用户选择的渲染方式。老状态文件没有这个字段 → 落到 `Auto`，行为不变。
    #[serde(default)]
    pub mode: GpuMode,
}

/// 给前端看的渲染状态。
#[derive(Debug, Clone, Serialize)]
pub struct GpuStatus {
    /// 用户选的方式。
    pub mode: GpuMode,
    /// 当前生效的是不是软件渲染。
    pub software_rendering: bool,
    /// 是否有过「启动没走到界面就绪」的记录（自动降级的原因）。
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
        // 顺手把选择持久化：README 里承诺「启动一次即可恢复硬件加速」，
        // 只改本次的话用户下次还得再设一遍环境变量。
        write_state(data_dir, &GpuState { mode: GpuMode::Hardware, ..Default::default() });
        return false;
    }
    if std::env::var("PRISMVAULT_FORCE_SOFTWARE_RENDER").is_ok() {
        write_state(
            data_dir,
            &GpuState { software_rendering: true, mode: GpuMode::Software, ..Default::default() },
        );
        return true;
    }

    let st = read_state(data_dir);

    // 用户在设置里明确选了软件渲染 → 保持稳态，也不重置 crashed_last_run
    // （免得下次又去试一次硬件渲染再崩一遍）。
    if st.mode == GpuMode::Software {
        return true;
    }

    // 上次没跑到「界面就绪」→ 自动降级，并记下结论。
    //
    // 注意这里同时把 `mode` 退回 `Auto`：即使上一轮用户显式选的是「硬件加速」，
    // 崩了之后也必须退回，否则会变成死循环 —— 白屏时用户连设置页都进不去，
    // 那个选项就永远改不回来了。界面会显示降级原因，用户想再试随时能再选一次。
    if st.crashed_last_run {
        write_state(
            data_dir,
            &GpuState { software_rendering: true, crashed_last_run: true, mode: GpuMode::Auto },
        );
        return true;
    }

    // `Auto` 模式下如果上一轮已经判定过 GPU 不可用，就沿用结论，不再反复试崩。
    // （`Hardware` 模式在 `set_gpu_mode` 里已经把这个标志清掉了，所以能走到下面。）
    if st.mode == GpuMode::Auto && st.software_rendering {
        return true;
    }

    // 首次启动 / 上次正常 / 用户要求硬件渲染：先试硬件渲染，标记为「尚未就绪」。
    write_state(
        data_dir,
        &GpuState { software_rendering: false, crashed_last_run: true, mode: st.mode },
    );
    false
}

/// 前端渲染成功 → 清除「尚未就绪」标记。
pub fn mark_ready(data_dir: &Path) {
    let st = read_state(data_dir);
    // 软件渲染是我们主动选择的稳态，不去动它。
    // 用户选的 `mode` 必须原样带过去 —— 早期版本这里写的是 `GpuState::default()`，
    // 会把设置里选的「硬件加速」在第一次成功启动后就悄悄改回「自动」。
    if !st.software_rendering {
        write_state(data_dir, &GpuState { mode: st.mode, ..Default::default() });
    }
}

/// 读取当前渲染状态（设置页用）。
pub fn status(data_dir: &Path) -> GpuStatus {
    let st = read_state(data_dir);
    GpuStatus {
        mode: st.mode,
        software_rendering: st.software_rendering,
        crashed_last_run: st.crashed_last_run,
    }
}

/// 切换渲染方式。返回切换后的状态。
///
/// 立刻落盘，但**本次启动的渲染方式已经定死了** —— WebView2 的启动参数只能在
/// 创建窗口时传，所以必须重启才生效（界面上有说明）。
pub fn set_mode(data_dir: &Path, mode: GpuMode) -> GpuStatus {
    let mut st = read_state(data_dir);
    st.mode = mode;
    match mode {
        // 切到硬件渲染 = 用户主动要求再试一次：必须清掉上次的崩溃结论，
        // 否则下次启动会立刻被判成「又崩了」直接退回软件渲染，切换毫无效果。
        GpuMode::Hardware => {
            st.software_rendering = false;
            st.crashed_last_run = false;
        }
        GpuMode::Software => {
            st.software_rendering = true;
            st.crashed_last_run = false;
        }
        // 交还给哨兵：保留当前结论（不主动重试），也不清崩溃记录。
        GpuMode::Auto => {}
    }
    write_state(data_dir, &st);
    status(data_dir)
}

/// 传给 WebView2 的启动参数（仅 Windows 有意义）。
pub fn browser_args(software: bool) -> String {
    if software {
        format!("{WRY_DEFAULT_ARGS} {SOFTWARE_ARGS}")
    } else {
        WRY_DEFAULT_ARGS.to_string()
    }
}

/// 从 `AppState` 取应用数据目录。
///
/// 抽出来是因为下面几个命令都要用，而这些命令都会在「引擎锁已失效」这种
/// 极端情况下被调用 —— 那时退回当前目录，至少不要 panic。
fn data_dir_of(state: &tauri::State<'_, crate::AppState>) -> PathBuf {
    state
        .engine
        .lock()
        .map(|g| g.data_dir.clone())
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// 当前渲染方式、是否软件渲染、是否有过自动降级（设置页展示用）。
#[tauri::command]
pub fn get_gpu_status(state: tauri::State<'_, crate::AppState>) -> GpuStatus {
    status(&data_dir_of(&state))
}

/// 切换渲染方式：`auto`（自动）/ `hardware`（硬件加速）/ `software`（软件渲染）。
///
/// **重启后生效** —— WebView2 的启动参数只在创建窗口时能传，这一点在界面上
/// 有明确提示。返回切换后的状态，前端拿来刷新显示。
#[tauri::command]
pub fn set_gpu_mode(state: tauri::State<'_, crate::AppState>, mode: String) -> GpuStatus {
    let mode = match mode.as_str() {
        "hardware" => GpuMode::Hardware,
        "software" => GpuMode::Software,
        _ => GpuMode::Auto,
    };
    set_mode(&data_dir_of(&state), mode)
}

/// 前端挂载完成、界面确实渲染出来了 —— 调用它注销崩溃哨兵。
#[tauri::command]
pub fn ui_ready(state: tauri::State<'_, crate::AppState>, diag: Option<String>) {
    let data_dir = data_dir_of(&state);
    if let Some(d) = diag {
        render_log(&data_dir, &format!("[ui_ready] {d}"));
    }
    mark_ready(&data_dir);
}

/// 后端注入的探针把页面内部状态回传到这里。
#[tauri::command]
pub fn render_probe(state: tauri::State<'_, crate::AppState>, info: String) {
    let data_dir = data_dir_of(&state);
    render_log(&data_dir, &format!("[probe] {info}"));
}

/// 前端把诊断信息写进 render-debug.log（图片加载失败、缩略图请求结果等）。
///
/// 图片看不见这类问题多半卡在 WebView 内部：`<img>` 的 onError 只在 JS 侧可见，
/// 不落到文件里就只能靠猜。前端只在「前若干个请求 + 全部失败」时上报，避免刷屏。
#[tauri::command]
pub fn diag_log(state: tauri::State<'_, crate::AppState>, tag: String, msg: String) {
    let data_dir = data_dir_of(&state);
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
