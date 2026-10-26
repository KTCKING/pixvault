//! 缩略图后台生成任务。
//!
//! 生成一张缩略图意味着要把原图整解码一遍 —— 一张 4000×3000 的 JPEG 需要
//! 约 50 MB 内存、100 ms 以上。以前这件事在 `get_thumbnail_path` 里同步做，
//! 而且全程握着图库互斥锁，于是滚动浏览大图库时会卡：每张可见的卡片都在等
//! 一次全图解码，整个引擎串行排在后面。
//!
//! 现在把解码搬到独立的后台线程：
//!   * `get_thumbnail_path` 只做一次「文件在不在」的判断，若不需要生成就直接
//!     返回；否则把任务丢进队列并立刻回 `pending`；
//!   * 后台 worker 按批次并行消费队列，结果通过 `thumb-ready` /
//!     `thumb-progress` 事件推给前端。

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rayon::prelude::*;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::engine::thumbnail::ThumbnailEngine;

/// 网格缩略图边长 —— 直接复用 `ThumbnailEngine` 里的常量，避免两处各写一份
/// 然后在改尺寸时忘掉其中一处。
pub use crate::engine::thumbnail::GRID_SIZE as THUMB_SIZE;

/// 单个并行批次的任务数。批次太大内存峰值高，太小线程利用率不足。
const BATCH: usize = 24;

/// 解码并行度上限。
///
/// 一张 4K 图解码后约 36MB，跑满所有核心（16 线程）峰值能到 600MB+，一开库
/// 就很吓人。限制到 4 个线程后峰值约 150MB，而 2400 张图也只是从 8 秒变成
/// 十几秒 —— 反正都在后台跑。
const MAX_PARALLEL: usize = 4;

/// 进度事件的最小间隔（毫秒），避免刷屏。
const PROGRESS_INTERVAL_MS: u128 = 300;

/// 用户交互后的「静默窗口」（毫秒）。
///
/// 在这个窗口内后台只处理 urgent（用户此刻盯着的那一屏），整库预热队列
/// （bulk）整个停下来，把 CPU 让给界面渲染。窗口一过自动恢复满速 ——
/// 所以前端**不需要**再上报一次「我停下来了」。
///
/// 460ms 的取法：比常见滚动惯性停顿（约 300–400ms）略长一点，免得手指连续
/// 滑动的间隙里后台插进来抢 CPU；又不至于长到预热肉眼可见地变慢。
const INTERACTION_QUIET_MS: u64 = 460;

/// 静默窗口内每轮之间让出的时间（毫秒）。
///
/// 不能直接 `continue` 空转 —— 那还是满核空转，等于没让。睡一下才真把 CPU
/// 还回去；60ms 也保证了交互一停就能在下一轮立刻恢复满速。
const THROTTLE_YIELD_MS: u64 = 60;

#[derive(Debug, Clone, Serialize)]
pub struct ThumbReadyItem {
    /// 原始文件路径 —— 前端以它为 key 缓存缩略图。
    pub path: String,
    /// 缩略图文件路径；为 `None` 表示生成失败（例如 RAW/PSD 这类解码器不认
    /// 的格式），前端会退回原图交给 WebView 自己解。
    pub thumb: Option<String>,
}

/// `thumb-placeholder` 事件的载荷元素：一张图的模糊占位图。
///
/// `data` 是 32px JPEG 的 data URL（约 1 KB），前端把它铺满卡片并加一层模糊，
/// 于是「真图还没轮到」的格子看起来是「已经有了、正在变清晰」，而不是一片空白。
#[derive(Debug, Clone, Serialize)]
pub struct ThumbPlaceholder {
    pub path: String,
    pub data: String,
}

/// 任务类型：网格缩略图 / 右侧预览图。两者共用同一条流水线，只是输出尺寸不同。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbKind {
    /// 网格卡片用的 448px 缩略图。
    Grid,
    /// 右侧信息栏用的 1600px 预览图（按需生成，不做整库预热）。
    Preview,
}

/// 网格缩略图的任务键。
pub fn job_key(library_id: &str, file_path: &str) -> String {
    format!("{library_id}|{file_path}")
}

/// 预览图的任务键 —— 与网格缩略图共用同一张任务表，靠 `#preview` 后缀区分。
pub fn preview_job_key(library_id: &str, file_path: &str) -> String {
    format!("{library_id}|{file_path}#preview")
}

#[derive(Debug, Clone, Serialize)]
pub struct ThumbProgress {
    pub total: u64,
    pub done: u64,
    pub running: bool,
}

/// 某个图库的「缩略图预热」进度。
///
/// 打开 / 导入 / 恢复图库时会自动把还缺缩略图的文件排进后台队列 —— 这就是
/// 预热，默认启用，不需要用户点任何按钮。它和用户滚动触发的按需生成走的是
/// 同一条流水线（同一个队列、同一张结果表），只是进度**按库单独统计**，
/// 好让侧边栏在图库名旁边画一个圆形进度环。
#[derive(Debug, Clone, Serialize)]
pub struct WarmProgress {
    pub library_id: String,
    /// 本轮需要补齐的图片总数。
    pub total: u64,
    /// 已处理的数量。
    pub done: u64,
    /// 是否仍在处理（false = 本轮已结束，界面可以收起进度环）。
    pub running: bool,
}

/// 单个库的预热状态。
struct Warm {
    total: u64,
    done: u64,
    /// 属于本轮预热的任务键。
    ///
    /// 用「任务键集合」而不是一个全局计数来统计，是为了把用户滚动触发的按需
    /// 任务排除在外 —— 那些任务同样流经这条流水线，但它们不该算进预热进度里。
    pending: HashSet<String>,
}

#[derive(Clone)]
pub struct ThumbJob {
    /// 唯一键：网格图 `{library_id}|{file_path}`，预览图再加 `#preview` 后缀。
    pub key: String,
    /// 前端使用的键（原始文件路径）。
    pub path: String,
    pub source: PathBuf,
    pub hash: String,
    pub engine: ThumbnailEngine,
    /// 这一步要产出哪一档图。
    pub kind: ThumbKind,
}

struct Inner {
    /// 屏幕上正在看的图（前端滚动到可见时才请求）—— 优先处理。存 key。
    urgent: VecDeque<String>,
    /// 整库补齐的批处理队列（打开/导入图库时一次性排队）。存 key。
    bulk: VecDeque<String>,
    /// key -> 任务本体（排队中或正在解码）。既用于去重，也保证「被漏掉」的
    /// 任务能重新捡回来接着做 —— 只记 key 而不留任务的话，一旦某批任务丢了
    /// （worker 异常退出），这些图就会永远显示不出缩略图。
    jobs: HashMap<String, ThumbJob>,
    /// key -> 缩略图路径；`None` 表示确定生成不出来，别再重试。
    results: HashMap<String, Option<String>>,
    /// library_id -> 该库的预热进度。
    warm: HashMap<String, Warm>,
    total: u64,
    done: u64,
    running: bool,
}

/// 任务优先级。
#[derive(Clone, Copy, PartialEq)]
pub enum Priority {
    /// 用户正在看这张 —— 插队。
    Urgent,
    /// 整库补齐。
    Bulk,
}

pub struct ThumbWorker {
    inner: Mutex<Inner>,
    app: Mutex<Option<AppHandle>>,
    /// 打开 / 导入 / 恢复图库时是否自动在后台补齐缩略图。默认开启。
    prewarm_enabled: AtomicBool,
    /// 专用的 rayon 线程池 —— 不用全局池是为了给解码单独限流，不让它吃满
    /// 所有核心和内存。
    pool: rayon::ThreadPool,
    /// 最近一次「用户正在交互」的时间戳（相对 `epoch` 的毫秒数；0 = 从未上报）。
    interaction_ms: AtomicU64,
    /// `interaction_ms` 的时基。用单调时钟，不受系统时间被调整的影响。
    epoch: std::time::Instant,
}

impl ThumbWorker {
    pub fn new() -> Self {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2)
            .clamp(1, MAX_PARALLEL);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("pixvault-thumb-{i}"))
            // 解码线程在进程优先级的基础上再降一档，保证进程内部也是
            // 「界面 / 事件循环 > 解码」。用 `start_handler` 而不是
            // `spawn_handler`：不接管线程创建，线程名、栈大小仍归 rayon 管，
            // 少一处可能出错的地方。
            .start_handler(|_| {
                crate::utils::priority::lower_current_thread();
            })
            .build()
            .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().build().unwrap());

        ThumbWorker {
            inner: Mutex::new(Inner {
                urgent: VecDeque::new(),
                bulk: VecDeque::new(),
                jobs: HashMap::new(),
                results: HashMap::new(),
                warm: HashMap::new(),
                total: 0,
                done: 0,
                running: false,
            }),
            app: Mutex::new(None),
            prewarm_enabled: AtomicBool::new(true),
            pool,
            interaction_ms: AtomicU64::new(0),
            epoch: std::time::Instant::now(),
        }
    }

    pub fn set_app(&self, app: AppHandle) {
        if let Ok(mut slot) = self.app.lock() {
            *slot = Some(app);
        }
    }

    fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) {
        let app = self.app.lock().ok().and_then(|g| g.clone());
        if let Some(app) = app {
            let _ = app.emit(event, payload);
        }
    }

    /// 已确定的结果：`Some(Some(path))` 表示缩略图就绪，`Some(None)` 表示
    /// 这个文件注定生成不出缩略图，`None` 表示还没结果。
    pub fn lookup(&self, key: &str) -> Option<Option<String>> {
        self.inner.lock().ok().and_then(|g| g.results.get(key).cloned())
    }

    /// 把「本来就已经存在」的缩略图登记进结果表，省掉后续的数据库查询。
    pub fn remember(&self, key: &str, thumb: String) {
        if let Ok(mut g) = self.inner.lock() {
            if !g.results.contains_key(key) {
                g.jobs.remove(key);
                g.results.insert(key.to_string(), Some(thumb));
            }
        }
    }

    pub fn stats(&self) -> ThumbProgress {
        self.inner
            .lock()
            .map(|g| ThumbProgress { total: g.total, done: g.done, running: g.running })
            .unwrap_or(ThumbProgress { total: 0, done: 0, running: false })
    }

    // ── 预热开关 ─────────────────────────────────────

    /// 打开图库时是否自动补齐缩略图。默认开启；关掉后按需生成（滚动到哪张
    /// 做哪张）仍然照常工作，手动「补齐缺失缩略图」也仍然可用。
    pub fn set_prewarm_enabled(&self, on: bool) {
        self.prewarm_enabled.store(on, Ordering::Relaxed);
    }

    pub fn prewarm_enabled(&self) -> bool {
        self.prewarm_enabled.load(Ordering::Relaxed)
    }

    // ── 交互感知节流 ─────────────────────────────────

    /// 前端上报「用户正在滚动 / 拖拽」。
    ///
    /// 后台据此在接下来 [`INTERACTION_QUIET_MS`] 内暂停整库预热，把 CPU 让给
    /// 界面。判断完全基于时间戳，静默窗口到期自动恢复满速，所以既不需要
    /// 「交互结束」的上报，也不需要加锁 —— 前端可以放心地在滚动回调里频繁调，
    /// 限流放在前端（约 120ms 一次）。
    pub fn note_interaction(&self) {
        self.interaction_ms
            .store(self.epoch.elapsed().as_millis() as u64, Ordering::Relaxed);
    }

    /// 是否处于「用户正在交互」的静默窗口内。
    fn is_interacting(&self) -> bool {
        let last = self.interaction_ms.load(Ordering::Relaxed);
        if last == 0 {
            return false;
        }
        (self.epoch.elapsed().as_millis() as u64).saturating_sub(last) < INTERACTION_QUIET_MS
    }

    // ── 预热进度 ─────────────────────────────────────

    /// 登记一轮预热，返回本轮实际需要补齐的数量。
    ///
    /// `keys` 是候选任务键。已经在结果表里出过结论的（上一轮就生成过、
    /// 或已被判定为生成不出来）直接算作已完成，只把真正待办的那些挂进
    /// `pending` —— 否则进度环会永远差那么几个到不了 100%。
    pub fn begin_warm(&self, library_id: &str, keys: Vec<String>) -> u64 {
        let Ok(mut g) = self.inner.lock() else { return 0 };

        let mut pending: HashSet<String> = HashSet::new();
        let mut done: u64 = 0;
        for key in keys {
            if g.results.contains_key(&key) {
                done += 1;
            } else {
                pending.insert(key);
            }
        }
        let total = done + pending.len() as u64;

        let changed = g
            .warm
            .get(library_id)
            .map(|w| w.total != total || w.done != done)
            .unwrap_or(true);
        g.warm.insert(library_id.to_string(), Warm { total, done, pending });
        drop(g);

        if changed {
            self.emit("thumb-warm", self.all_warm_progress());
        }
        total
    }

    /// 某个库的预热进度；没有记录时返回一个空进度。
    pub fn warm_progress(&self, library_id: &str) -> WarmProgress {
        let slot = self.inner.lock().ok().and_then(|g| {
            g.warm.get(library_id).map(|w| (w.total, w.done, w.pending.is_empty()))
        });
        match slot {
            Some((total, done, finished)) => WarmProgress {
                library_id: library_id.to_string(),
                total,
                done,
                running: !finished,
            },
            None => WarmProgress {
                library_id: library_id.to_string(),
                total: 0,
                done: 0,
                running: false,
            },
        }
    }

    /// 所有库的预热进度 —— 前端订阅一次就能画出侧边栏上所有进度环。
    pub fn all_warm_progress(&self) -> Vec<WarmProgress> {
        self.inner
            .lock()
            .map(|g| {
                g.warm
                    .iter()
                    .map(|(id, w)| WarmProgress {
                        library_id: id.clone(),
                        total: w.total,
                        done: w.done,
                        running: !w.pending.is_empty(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 忘掉某个库的预热进度（关库时调用，避免进度环留在已关闭的库上）。
    pub fn forget_warm(&self, library_id: &str) {
        let removed = self
            .inner
            .lock()
            .map(|mut g| g.warm.remove(library_id).is_some())
            .unwrap_or(false);
        if removed {
            self.emit("thumb-warm", self.all_warm_progress());
        }
    }

    /// 入队一批任务，必要时拉起 worker 线程。
    pub fn enqueue(self: &Arc<Self>, jobs: Vec<ThumbJob>) {
        self.submit(jobs, Priority::Bulk, None);
    }

    /// 入队并指定优先级。`Urgent` 会插到整库补齐的前面：用户滚到哪儿，哪儿的
    /// 缩略图就先出来，不必等整库的批处理跑完。
    pub fn enqueue_with(self: &Arc<Self>, jobs: Vec<ThumbJob>, priority: Priority) {
        self.submit(jobs, priority, None);
    }

    /// 以「当前视口窗口」的身份入队。
    ///
    /// 除了插队，还做一件关键的事：**把窗口外的 urgent 任务降级回整库队列**。
    /// 没有这一步的话，快速下滑会把滑过去的每一屏都塞进 urgent，而 urgent 是
    /// 按入队顺序消费的 —— 用户真正盯着的这一屏反而排在那几百张后面，于是
    /// 「滑得越快，白块越久」。降级之后，队列里只剩当前窗口，滑过去没看到的
    /// 那些交给整库补齐慢慢做（滑回来时会重新提权）。
    pub fn enqueue_window(self: &Arc<Self>, jobs: Vec<ThumbJob>, window: &HashSet<String>) {
        self.submit(jobs, Priority::Urgent, Some(window));
    }

    /// 入队的统一入口：窗口降级 → 自我修复 → 去重入队 → 必要时拉起 worker。
    fn submit(
        self: &Arc<Self>,
        jobs: Vec<ThumbJob>,
        priority: Priority,
        window: Option<&HashSet<String>>,
    ) {
        // 没有新任务、也没有要维护的窗口，就没必要碰锁。
        if jobs.is_empty() && window.is_none() {
            return;
        }

        let start = {
            let Ok(mut g) = self.inner.lock() else { return };

            if let Some(window) = window {
                let mut kept: VecDeque<String> = VecDeque::new();
                let mut demoted: Vec<String> = Vec::new();
                for key in g.urgent.drain(..) {
                    if window.contains(&key) {
                        kept.push_back(key);
                    } else {
                        demoted.push(key);
                    }
                }
                g.urgent = kept;
                for key in demoted {
                    if g.jobs.contains_key(&key) {
                        g.bulk.push_back(key);
                    }
                }
            }

            // 自我修复：队列空了但还有「排队中/解码中」的任务、worker 又没在跑，
            // 说明上一批任务丢了（线程异常退出）。把它们重新排进去，
            // 否则这些卡片会永远停在骨架屏上。
            if !g.running && g.urgent.is_empty() && g.bulk.is_empty() && !g.jobs.is_empty() {
                let orphaned: Vec<String> = g.jobs.keys().cloned().collect();
                for key in orphaned {
                    g.bulk.push_back(key);
                }
            }

            for job in jobs {
                let key = job.key.clone();
                // 已经有结论的（无论成功还是确定失败）都不再重复处理。
                if g.results.contains_key(&key) {
                    continue;
                }
                if priority == Priority::Urgent {
                    // 还在批处理队列里的话直接提上来。
                    if let Some(pos) = g.bulk.iter().position(|k| *k == key) {
                        g.bulk.remove(pos);
                        g.urgent.push_back(key);
                        continue;
                    }
                }
                if g.jobs.contains_key(&key) {
                    continue;
                }
                g.jobs.insert(key.clone(), job);
                g.total += 1;
                match priority {
                    Priority::Urgent => g.urgent.push_back(key),
                    Priority::Bulk => g.bulk.push_back(key),
                }
            }

            let has_work = !g.urgent.is_empty() || !g.bulk.is_empty();
            if has_work && !g.running {
                g.running = true;
                true
            } else {
                false
            }
        };

        if !start {
            // 已有 worker 在跑（或没活干），它会自己接着消费新任务。
            return;
        }
        self.emit("thumb-progress", self.stats());

        let me = Arc::clone(self);
        std::thread::Builder::new()
            .name("pixvault-thumbs".into())
            .spawn(move || me.run())
            .ok();
    }

    fn run(self: Arc<Self>) {
        let mut last_emit = std::time::Instant::now();

        loop {
            // 1. 取一批任务（urgent 优先）；两个队列都空则收工。
            //
            //    用户正在滚动 / 拖拽时（`is_interacting`）**只取 urgent**：bulk 是
            //    「整库慢慢补齐」，晚几百毫秒无所谓；urgent 是用户此刻盯着的那一屏，
            //    必须继续做。这就是「交互感知节流」——后台在最需要 CPU 的时候让路，
            //    人一停手自动恢复满速。
            let interacting = self.is_interacting();
            let (batch, yielded): (Vec<ThumbJob>, bool) = {
                let Ok(mut g) = self.inner.lock() else { return };
                let mut batch: Vec<ThumbJob> = Vec::with_capacity(BATCH);
                while batch.len() < BATCH {
                    // urgent 用**后进先出**：最近提交的窗口就是用户此刻盯着的那一屏，
                    // 先解它们。整库补齐（bulk）保持先进先出，按 mtime 顺序推进。
                    let key = match g.urgent.pop_back() {
                        Some(key) => Some(key),
                        // 交互期间不碰 bulk —— 直接结束本批。
                        None if interacting => None,
                        None => g.bulk.pop_front(),
                    };
                    let key = match key {
                        Some(key) => key,
                        None => break,
                    };
                    if let Some(job) = g.jobs.get(&key) {
                        batch.push(job.clone());
                    }
                }

                if !batch.is_empty() {
                    (batch, false)
                } else if !g.bulk.is_empty() {
                    // 还有活，只是被交互让出去了。这里**必须保持 running**，
                    // 不能走下面「两个队列都空」的收工分支 —— 否则预热会永久停摆，
                    // 只能等下次入队才重新拉起 worker。
                    (Vec::new(), true)
                } else {
                    // 同一个锁里判断 + 复位，避免和 enqueue 抢跑：要么 enqueue
                    // 已经插了新任务（这里看的队列非空），要么它之后看到
                    // running == false 并重新拉线程。
                    g.running = false;
                    let progress = ThumbProgress { total: g.total, done: g.done, running: false };
                    drop(g);
                    self.emit("thumb-progress", progress);
                    return;
                }
            };

            if yielded {
                std::thread::sleep(std::time::Duration::from_millis(THROTTLE_YIELD_MS));
                continue;
            }

            // 2. 占位图快扫 —— 必须在解码**之前**推出去。
            //
            //    只读文件头取 EXIF 内嵌缩略图（毫秒级，不解原图），压成 32px 后
            //    推给前端当模糊底。这样即使这张图的 448px 缩略图还要排队几百毫秒，
            //    用户也已经"看见轮廓"了。没有内嵌图的（PNG / 截图）这一步会失败，
            //    等第 3 步解码完用缩略图补，下一次滑到这片区域就是即时命中。
            let placeholders: Vec<ThumbPlaceholder> = self.pool.install(|| {
                batch
                    .par_iter()
                    .filter(|job| job.kind == ThumbKind::Grid)
                    .filter_map(|job| {
                        job.engine
                            .make_placeholder(&job.source, &job.hash)
                            .ok()
                            .map(|data| ThumbPlaceholder { path: job.path.clone(), data })
                    })
                    .collect()
            });
            if !placeholders.is_empty() {
                self.emit("thumb-placeholder", placeholders);
            }

            // 3. 并行解码 —— 不持有任何锁，也不持有 engine 锁。
            //    单个文件解码失败（RAW / PSD / 损坏文件）只影响它自己。
            let generated: Vec<(String, ThumbKind, ThumbReadyItem)> = self.pool.install(|| {
                batch
                    .into_par_iter()
                    .map(|job| {
                        let produced = match job.kind {
                            ThumbKind::Grid => job.engine.generate(&job.source, &job.hash),
                            ThumbKind::Preview => job.engine.generate_preview(&job.source, &job.hash),
                        };
                        let thumb = produced
                            .ok()
                            .filter(|p| p.exists())
                            .map(|p| p.to_string_lossy().to_string());
                        // 没有 EXIF 内嵌图的那批（PNG / 截图）在这里补占位图缓存：
                        // 解码一张 448px 的 JPEG 只要几毫秒，代价可以忽略，换来
                        // 的是下次滑到这片区域立刻有模糊底。
                        if let (ThumbKind::Grid, Some(t)) = (job.kind, thumb.as_deref()) {
                            let _ = job.engine.placeholder_from_thumb(std::path::Path::new(t), &job.hash);
                        }
                        (job.key, job.kind, ThumbReadyItem { path: job.path, thumb })
                    })
                    .collect()
            });

            // 4. 登记结果（同时把任务从 jobs 里摘掉 —— 之后允许重试），并推进
            //    各图库的预热进度。
            let (progress, warm_changed) = {
                let Ok(mut g) = self.inner.lock() else { return };
                g.done += generated.len() as u64;
                for (key, _, item) in &generated {
                    g.jobs.remove(key);
                    g.results.insert(key.clone(), item.thumb.clone());
                }

                // 预热进度只统计属于本轮预热批次的任务 —— 用户滚动触发的按需
                // 任务压根不会出现在任何 `warm.pending` 里，自然不会误算。
                let mut warm_changed = false;
                for (key, _, _) in &generated {
                    for w in g.warm.values_mut() {
                        if w.pending.remove(key) {
                            w.done += 1;
                            warm_changed = true;
                        }
                    }
                }

                (
                    ThumbProgress { total: g.total, done: g.done, running: true },
                    warm_changed,
                )
            };
            if warm_changed {
                self.emit("thumb-warm", self.all_warm_progress());
            }

            // 5. 按类型分别推送：网格图走 `thumb-ready`，预览图走 `preview-ready`。
            let mut grid: Vec<ThumbReadyItem> = Vec::new();
            let mut preview: Vec<ThumbReadyItem> = Vec::new();
            for (_, kind, item) in generated {
                match kind {
                    ThumbKind::Grid => grid.push(item),
                    ThumbKind::Preview => preview.push(item),
                }
            }
            if !grid.is_empty() {
                self.emit("thumb-ready", grid);
            }
            if !preview.is_empty() {
                self.emit("preview-ready", preview);
            }

            if last_emit.elapsed().as_millis() >= PROGRESS_INTERVAL_MS {
                last_emit = std::time::Instant::now();
                self.emit("thumb-progress", progress);
            }
        }
    }
}

impl Default for ThumbWorker {
    fn default() -> Self {
        Self::new()
    }
}
