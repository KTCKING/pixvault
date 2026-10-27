pub mod engine;
pub mod commands;
pub mod utils;
pub mod gpu;

use std::sync::Arc;
use std::sync::Mutex;

pub struct AppState {
    pub engine: Arc<Mutex<engine::Engine>>,
    /// 后台缩略图生成任务（全局一个，按库区分缓存目录）。
    pub thumbs: Arc<engine::thumbs::ThumbWorker>,
    /// eaglepack 解压的取消标志。前端置位后，解压循环在下一个回报点退出。
    pub pack_cancel: Arc<std::sync::atomic::AtomicBool>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            engine: Arc::new(Mutex::new(engine::Engine::new())),
            thumbs: Arc::new(engine::thumbs::ThumbWorker::new()),
            pack_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}