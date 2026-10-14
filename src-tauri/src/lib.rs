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
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            engine: Arc::new(Mutex::new(engine::Engine::new())),
            thumbs: Arc::new(engine::thumbs::ThumbWorker::new()),
        }
    }
}