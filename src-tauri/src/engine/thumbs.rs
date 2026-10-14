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

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
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

#[derive(Debug, Clone, Serialize)]
pub struct ThumbReadyItem {
    /// 原始文件路径 —— 前端以它为 key 缓存缩略图。
    pub path: String,
    /// 缩略图文件路径；为 `None` 表示生成失败（例如 RAW/PSD 这类解码器不认
    /// 的格式），前端会退回原图交给 WebView 自己解。
    pub thumb: Option<String>,
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
    /// 专用的 rayon 线程池 —— 不用全局池是为了给解码单独限流，不让它吃满
    /// 所有核心和内存。
    pool: rayon::ThreadPool,
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
            .build()
            .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().build().unwrap());

        ThumbWorker {
            inner: Mutex::new(Inner {
                urgent: VecDeque::new(),
                bulk: VecDeque::new(),
                jobs: HashMap::new(),
                results: HashMap::new(),
                total: 0,
                done: 0,
                running: false,
            }),
            app: Mutex::new(None),
            pool,
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

    /// 入队一批任务，必要时拉起 worker 线程。
    pub fn enqueue(self: &Arc<Self>, jobs: Vec<ThumbJob>) {
        self.enqueue_with(jobs, Priority::Bulk);
    }

    /// 入队并指定优先级。`Urgent` 会插到整库补齐的前面：用户滚到哪儿，哪儿的
    /// 缩略图就先出来，不必等整库的批处理跑完。
    pub fn enqueue_with(self: &Arc<Self>, jobs: Vec<ThumbJob>, priority: Priority) {
        if jobs.is_empty() {
            return;
        }

        let start = {
            let Ok(mut g) = self.inner.lock() else { return };

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
                let queued = g.jobs.contains_key(&key);
                if priority == Priority::Urgent {
                    // 还在批处理队列里的话直接提上来。
                    if let Some(pos) = g.bulk.iter().position(|k| *k == key) {
                        g.bulk.remove(pos);
                        g.urgent.push_back(key);
                        continue;
                    }
                }
                if queued {
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
            let batch: Vec<ThumbJob> = {
                let Ok(mut g) = self.inner.lock() else { return };
                let mut batch: Vec<ThumbJob> = Vec::with_capacity(BATCH);
                while batch.len() < BATCH {
                    let key = match g.urgent.pop_front().or_else(|| g.bulk.pop_front()) {
                        Some(key) => key,
                        None => break,
                    };
                    if let Some(job) = g.jobs.get(&key) {
                        batch.push(job.clone());
                    }
                }
                if batch.is_empty() {
                    // 同一个锁里判断 + 复位，避免和 enqueue 抢跑：要么 enqueue
                    // 已经插了新任务（这里看的队列非空），要么它之后看到
                    // running == false 并重新拉线程。
                    g.running = false;
                    let progress = ThumbProgress { total: g.total, done: g.done, running: false };
                    drop(g);
                    self.emit("thumb-progress", progress);
                    return;
                }
                batch
            };

            // 2. 并行解码 —— 不持有任何锁，也不持有 engine 锁。
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
                        (job.key, job.kind, ThumbReadyItem { path: job.path, thumb })
                    })
                    .collect()
            });

            // 3. 登记结果（同时把任务从 jobs 里摘掉 —— 之后允许重试）。
            let progress = {
                let Ok(mut g) = self.inner.lock() else { return };
                g.done += generated.len() as u64;
                for (key, _, item) in &generated {
                    g.jobs.remove(key);
                    g.results.insert(key.clone(), item.thumb.clone());
                }
                ThumbProgress { total: g.total, done: g.done, running: true }
            };

            // 4. 按类型分别推送：网格图走 `thumb-ready`，预览图走 `preview-ready`。
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
