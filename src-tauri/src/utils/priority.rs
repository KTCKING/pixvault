//! 进程 / 线程调度优先级。
//!
//! 缩略图预热是典型的「后台可延迟任务」：整库跑满解码线程时能把 CPU 吃到
//! 100% 持续一两分钟，用户滚动、切图、拖窗口的手感就跟着一起掉。问题在于
//! Windows 默认把进程放在 `NORMAL_PRIORITY_CLASS` —— 和用户正在交互的浏览器、
//! 编辑器完全平权，后台任务能实打实地抢走一半时间片。
//!
//! 这里做两件事：
//!
//! 1. **进程**降到 `BELOW_NORMAL_PRIORITY_CLASS`：只要别的程序要 CPU 我们就
//!    让路；系统空闲时照旧拿满全部核心（不是 `IDLE`，不会饿死在那儿）。
//! 2. **缩略图解码线程**在进程的基础上再降一档：保证进程内部也是「界面 /
//!    事件循环 > 解码」，而不只是对外部程序让路。这一档在 `engine/thumbs.rs`
//!    建池时通过 rayon 的 `start_handler` 挂上。
//!
//! 和 `winattr.rs` 一样，为了这一个系统调用不值得引进一个依赖，直接声明
//! kernel32 / libSystem 的符号即可。
//!
//! 想关掉（性能对比、排查问题）：环境变量 `PRISMVAULT_PRIORITY=normal`。

/// 用户显式要求不降级时返回 true。
///
/// 只在进程启动阶段和建线程池时各查一次，开销可以忽略，所以不做缓存 ——
/// 缓存反而要引入一个静态变量和各处调用的先后顺序约束。
fn opted_out() -> bool {
    matches!(std::env::var("PRISMVAULT_PRIORITY").ok().as_deref(), Some("normal") | Some("high"))
}

#[cfg(windows)]
mod imp {
    use core::ffi::c_void;

    /// `BELOW_NORMAL_PRIORITY_CLASS`。
    ///
    /// 刻意不用更激进的 `IDLE_PRIORITY_CLASS`：那个档位只在系统完全空闲时才
    /// 会被调度，实测整库预热会被拖到几倍时长。`BELOW_NORMAL` 是「让路但仍
    /// 全力」的档位，正好对上这里的需求。
    const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
    /// `THREAD_PRIORITY_BELOW_NORMAL`（相对进程基优先级 -1）。
    const THREAD_PRIORITY_BELOW_NORMAL: i32 = -1;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        /// 返回当前进程的伪句柄，只在本进程内有效，不需要 CloseHandle。
        fn GetCurrentProcess() -> *mut c_void;
        /// 返回当前线程的伪句柄，**只在本线程内有效**，不需要 CloseHandle。
        fn GetCurrentThread() -> *mut c_void;
        fn SetPriorityClass(process: *mut c_void, priority_class: u32) -> i32;
        fn SetThreadPriority(thread: *mut c_void, priority: i32) -> i32;
    }

    /// 把整个进程降到 Below Normal。失败（极少见，权限或受保护进程）返回 false。
    pub fn lower_process() -> bool {
        unsafe { SetPriorityClass(GetCurrentProcess(), BELOW_NORMAL_PRIORITY_CLASS) != 0 }
    }

    /// 把**调用它的那个线程**降到 Below Normal。
    pub fn lower_current_thread() -> bool {
        unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) != 0 }
    }
}

#[cfg(not(windows))]
mod imp {
    /// `PRIO_PROCESS`
    const PRIO_PROCESS: i32 = 0;
    /// nice 目标值。默认 0，越大越「谦让」。
    const NICE_DELTA: i32 = 10;

    unsafe extern "C" {
        fn setpriority(which: i32, who: u32, prio: i32) -> i32;
    }

    /// macOS / Linux：把进程 nice 值抬到 10。
    ///
    /// 只有**提高**优先级（把 nice 调小）才需要 root，调大不需要，所以这条
    /// 不会因为权限而失败。
    pub fn lower_process() -> bool {
        unsafe { setpriority(PRIO_PROCESS, 0, NICE_DELTA) == 0 }
    }

    /// macOS 没有「单线程 nice」这种粒度；Linux 的 `setpriority(gettid())` 虽然
    /// 存在，但和进程 nice 共用同一个字段，单独改线程会顺带影响整个进程 ——
    /// 那就等于重复第 1 步。所以这里直接交给进程级那一档，返回 false 表示
    /// 「没有额外动作」。
    pub fn lower_current_thread() -> bool {
        false
    }
}

/// 把整个进程降到 Below Normal。
///
/// **调用时机很关键**：必须在主窗口建好之后（见 `main.rs`）。WebView2 会派生
/// 自己的浏览器 / 渲染 / GPU 进程，而子进程默认继承父进程的优先级类 —— 提前
/// 降级会把 WebView 的渲染进程一起拖到 Below Normal，正好和「让界面更跟手」
/// 的目标相反。
///
/// 返回是否真的设置成功（`PRISMVAULT_PRIORITY=normal` 时为 false）。
pub fn lower_process() -> bool {
    if opted_out() {
        return false;
    }
    imp::lower_process()
}

/// 后台解码线程启动时调用：在当前线程上再降一档。
///
/// 返回是否真的设置成功。
pub fn lower_current_thread() -> bool {
    if opted_out() {
        return false;
    }
    imp::lower_current_thread()
}
