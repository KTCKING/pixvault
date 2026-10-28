//! 浏览器扩展桥接：把网页上的图片 / 视频直接送进当前图库。
//!
//! 为什么要在应用里起一个 HTTP 服务，而不是让扩展「下载到某个文件夹再让应用
//! 去扫」？因为浏览器才知道那张图真正的字节是什么：带 cookie 的防盗链、`blob:`
//! 地址、canvas 画出来的图、需要 Referer 的图床 —— 只有扩展能拿到。所以分工是
//! **扩展负责取字节，应用负责落盘 + 入库**，两边只用一个 POST 连起来。
//!
//! 这条链路刻意不引入任何 HTTP 框架：需要的只是「读请求头 → 读定长 body →
//! 回一段 JSON」，手写一百多行比拉一整套 hyper/axum 更省事，也不会给 Windows
//! GNU 工具链添新的 C 依赖。
//!
//! 安全上按「本机服务等于半个文件写入接口」来对待：
//! - 只监听 `127.0.0.1`，不碰 `0.0.0.0`；
//! - Origin 白名单（只放行浏览器扩展的 origin）；
//! - 每次启动生成一个 token，扩展填一次才认；
//! - 文件名逐字符清洗，路径只允许落在当前图库根目录之内。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// 固定端口。Eagle 用 41595，我们紧挨着取 41596 —— 两个软件可以同时装着。
pub const DEFAULT_PORT: u16 = 41596;

/// 单次上传上限。视频动辄上百 MB，但再大就该走「文件夹导入」了。
const MAX_BODY: usize = 512 * 1024 * 1024;
const MAX_HEADER: usize = 64 * 1024;

/// 视频扩展名。这些文件能入库、能播，但不进缩略图队列（解不出来）。
pub const VIDEO_EXTS: &[&str] = &[
    "mp4", "m4v", "webm", "mov", "mkv", "avi", "wmv", "flv", "ogv", "mpg", "mpeg", "ts", "3gp",
];

pub fn is_video_ext(ext: &str) -> bool {
    let e = ext.trim_start_matches('.').to_ascii_lowercase();
    VIDEO_EXTS.contains(&e.as_str())
}

// ───────────────────────── 配置 ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    pub enabled: bool,
    pub port: u16,
    pub token: String,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        BridgeConfig {
            enabled: true,
            port: DEFAULT_PORT,
            token: new_token(),
        }
    }
}

fn new_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..16].to_string()
}

/// 一条摘取记录，用于设置面板里展示「刚刚存进来的东西」。
#[derive(Debug, Clone, Serialize)]
pub struct Ingested {
    pub path: String,
    pub filename: String,
    pub size: u64,
    pub page: String,
    pub at: String,
    pub library_id: String,
}

/// 设置面板 + `/api/ping` 共用的状态快照。
#[derive(Debug, Clone, Serialize)]
pub struct BridgeStatus {
    pub enabled: bool,
    pub port: u16,
    /// 是否真的在监听（端口被占用 / 未开库时为 false）。
    pub listening: bool,
    pub bound_port: u16,
    pub token: String,
    pub last_error: String,
    pub requests: u64,
    pub ingested: u64,
    pub recent: Vec<Ingested>,
    /// 当前活动图库（没有就是 None，此时扩展会提示「先打开一个图库」）。
    pub library: Option<BridgeLibrary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeLibrary {
    pub id: String,
    pub name: String,
    pub root_path: String,
}

pub struct Bridge {
    cfg: Mutex<BridgeConfig>,
    path: PathBuf,
    /// 实际绑上的端口（0 = 没在监听）。accept 循环退出重绑时会归零。
    bound: AtomicU16,
    last_error: Mutex<String>,
    requests: AtomicU64,
    ingested: AtomicU64,
    recent: Mutex<VecDeque<Ingested>>,
    /// 端口 / 开关改动后让 accept 循环重绑，而不是重启应用。
    dirty: AtomicBool,
}

impl Bridge {
    pub fn new() -> Arc<Self> {
        let dir = crate::engine::Engine::get_data_dir().unwrap_or_else(|| PathBuf::from("."));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("bridge.json");

        let mut cfg: BridgeConfig = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        // 老配置文件 / 手改坏了也要能用：token 永远是必需的
        if cfg.token.trim().is_empty() {
            cfg.token = new_token();
        }
        if cfg.port == 0 {
            cfg.port = DEFAULT_PORT;
        }

        let bridge = Arc::new(Bridge {
            cfg: Mutex::new(cfg),
            path,
            bound: AtomicU16::new(0),
            last_error: Mutex::new(String::new()),
            requests: AtomicU64::new(0),
            ingested: AtomicU64::new(0),
            recent: Mutex::new(VecDeque::new()),
            dirty: AtomicBool::new(false),
        });
        bridge.save();
        bridge
    }

    fn save(&self) {
        if let Ok(cfg) = self.cfg.lock() {
            if let Ok(json) = serde_json::to_vec_pretty(&*cfg) {
                let _ = std::fs::write(&self.path, json);
            }
        }
    }

    fn snapshot(&self) -> BridgeConfig {
        self.cfg.lock().map(|c| c.clone()).unwrap_or_default()
    }

    fn set_error(&self, msg: impl Into<String>) {
        if let Ok(mut e) = self.last_error.lock() {
            *e = msg.into();
        }
    }

    fn push_recent(&self, item: Ingested) {
        if let Ok(mut r) = self.recent.lock() {
            r.push_front(item);
            while r.len() > 30 {
                r.pop_back();
            }
        }
    }

    pub fn set_enabled(&self, on: bool) {
        if let Ok(mut cfg) = self.cfg.lock() {
            cfg.enabled = on;
        }
        self.dirty.store(true, Ordering::SeqCst);
        self.save();
    }

    /// 换端口。返回 false 表示端口非法（<1024 需要管理员权限，直接不给选）。
    pub fn set_port(&self, port: u16) -> bool {
        if port < 1024 {
            return false;
        }
        if let Ok(mut cfg) = self.cfg.lock() {
            cfg.port = port;
        }
        self.dirty.store(true, Ordering::SeqCst);
        self.save();
        true
    }

    pub fn regenerate_token(&self) -> String {
        let token = new_token();
        if let Ok(mut cfg) = self.cfg.lock() {
            cfg.token = token.clone();
        }
        self.save();
        token
    }

    pub fn status(&self, library: Option<BridgeLibrary>) -> BridgeStatus {
        let cfg = self.snapshot();
        BridgeStatus {
            enabled: cfg.enabled,
            port: cfg.port,
            listening: self.bound.load(Ordering::SeqCst) != 0,
            bound_port: self.bound.load(Ordering::SeqCst),
            token: cfg.token,
            last_error: self.last_error.lock().map(|e| e.clone()).unwrap_or_default(),
            requests: self.requests.load(Ordering::SeqCst),
            ingested: self.ingested.load(Ordering::SeqCst),
            recent: self
                .recent
                .lock()
                .map(|r| r.iter().cloned().collect())
                .unwrap_or_default(),
            library,
        }
    }

    /// 起 accept 循环。整个过程是「绑定 → 服务 → 发现配置变了就重绑」，
    /// 因此改端口不用重启应用。
    pub fn start(self: &Arc<Self>, app: AppHandle) {
        let bridge = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let cfg = bridge.snapshot();
                if !cfg.enabled {
                    bridge.bound.store(0, Ordering::SeqCst);
                    bridge.set_error("");
                    tokio::time::sleep(Duration::from_millis(400)).await;
                    continue;
                }

                let listener = match TcpListener::bind(("127.0.0.1", cfg.port)).await {
                    Ok(l) => l,
                    Err(e) => {
                        bridge.bound.store(0, Ordering::SeqCst);
                        bridge.set_error(format!(
                            "端口 {} 无法监听：{}（可能被其它程序占用）",
                            cfg.port, e
                        ));
                        tokio::time::sleep(Duration::from_millis(1500)).await;
                        continue;
                    }
                };

                bridge.bound.store(cfg.port, Ordering::SeqCst);
                bridge.dirty.store(false, Ordering::SeqCst);
                bridge.set_error("");

                loop {
                    tokio::select! {
                        // 600ms 看一眼配置是否被改过 —— 这点开销远比「换个端口要重启
                        // 整个软件」划算。
                        _ = tokio::time::sleep(Duration::from_millis(600)) => {
                            let now = bridge.snapshot();
                            if bridge.dirty.load(Ordering::SeqCst)
                                || now.port != cfg.port
                                || now.enabled != cfg.enabled
                            {
                                break;
                            }
                        }
                        accepted = listener.accept() => {
                            match accepted {
                                Ok((stream, _addr)) => {
                                    bridge.requests.fetch_add(1, Ordering::Relaxed);
                                    let app = app.clone();
                                    let b = bridge.clone();
                                    tauri::async_runtime::spawn(async move {
                                        let _ = handle_connection(stream, app, b).await;
                                    });
                                }
                                Err(_) => break,
                            }
                        }
                    }
                }
            }
        });
    }
}

// ───────────────────────── HTTP ─────────────────────────

struct Request {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        let n = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == n)
            .map(|(_, v)| v.as_str())
    }

    fn query_get(&self, key: &str) -> Option<String> {
        for pair in self.query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = match pair.split_once('=') {
                Some((k, v)) => (k, v),
                None => (pair, ""),
            };
            if url_decode(k) == key {
                return Some(url_decode(v));
            }
        }
        None
    }

    /// 同名参数可能出现多次（`?tag=a&tag=b`），这个版本把它们全收上来。
    fn query_all(&self, key: &str) -> Vec<String> {
        let mut out = Vec::new();
        for pair in self.query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = match pair.split_once('=') {
                Some((k, v)) => (k, v),
                None => (pair, ""),
            };
            if url_decode(k) == key {
                out.push(url_decode(v));
            }
        }
        out
    }
}

struct Response {
    status: u16,
    body: Vec<u8>,
    content_type: &'static str,
}

impl Response {
    fn json(status: u16, v: serde_json::Value) -> Self {
        Response {
            status,
            body: serde_json::to_vec(&v).unwrap_or_else(|_| b"{}".to_vec()),
            content_type: "application/json; charset=utf-8",
        }
    }

    fn text(status: u16, s: impl Into<String>) -> Self {
        Response {
            status,
            body: s.into().into_bytes(),
            content_type: "text/plain; charset=utf-8",
        }
    }

    fn empty(status: u16) -> Self {
        Response {
            status,
            body: Vec::new(),
            content_type: "text/plain; charset=utf-8",
        }
    }
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

async fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    let mut tmp = [0u8; 8192];
    let head_end = loop {
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > MAX_HEADER {
            return Err("请求头过大".to_string());
        }
        let n = stream.read(&mut tmp).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("连接已关闭".to_string());
        }
        buf.extend_from_slice(&tmp[..n]);
    };

    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut rest: Vec<u8> = buf[head_end + 4..].to_vec();

    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_ascii_uppercase();
    let full_path = parts.next().unwrap_or("/").to_string();
    let (path, query) = match full_path.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (full_path, String::new()),
    };

    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }

    let req = Request {
        method,
        path,
        query,
        headers,
        body: Vec::new(),
    };

    // 分块传输不支持：扩展侧永远用 Blob / ArrayBuffer，浏览器一定会带
    // Content-Length。真碰上别的客户端发分块，直接拒绝比半懂不懂地解析安全。
    if let Some(te) = req.header("transfer-encoding") {
        if te.to_ascii_lowercase().contains("chunked") {
            return Err("不支持 Transfer-Encoding: chunked".to_string());
        }
    }

    let want: usize = req
        .header("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    if want > MAX_BODY {
        return Err(format!("文件太大（{} MB），上限 512 MB", want / 1024 / 1024));
    }

    let mut body = std::mem::take(&mut rest);
    while body.len() < want {
        let n = stream.read(&mut tmp).await.map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(want);

    Ok(Request { body, ..req })
}

async fn write_response(
    stream: &mut TcpStream,
    resp: Response,
    origin: Option<&str>,
) -> Result<(), String> {
    let reason = match resp.status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        500 => "Internal Server Error",
        _ => "OK",
    };

    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        resp.status,
        reason,
        resp.content_type,
        resp.body.len()
    );
    // 只有扩展的来源才回 CORS 头：普通网页就算硬发请求，读到的响应也会被
    // 浏览器拦下 —— 少了这一条，任何网站都能往用户图库里塞文件。
    if let Some(o) = origin {
        head.push_str(&format!(
            "Access-Control-Allow-Origin: {o}\r\n\
             Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
             Access-Control-Allow-Headers: content-type, x-pixvault-token\r\n\
             Access-Control-Allow-Private-Network: true\r\n\
             Access-Control-Max-Age: 86400\r\n\
             Vary: Origin\r\n"
        ));
    }
    head.push_str("\r\n");

    stream
        .write_all(head.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    if !resp.body.is_empty() {
        stream
            .write_all(&resp.body)
            .await
            .map_err(|e| e.to_string())?;
    }
    let _ = stream.flush().await;
    Ok(())
}

fn origin_allowed(origin: &str) -> bool {
    let o = origin.to_ascii_lowercase();
    o.starts_with("chrome-extension://")
        || o.starts_with("moz-extension://")
        || o.starts_with("safari-web-extension://")
        || o.starts_with("extension://")
}

async fn handle_connection(
    mut stream: TcpStream,
    app: AppHandle,
    bridge: Arc<Bridge>,
) -> Result<(), String> {
    let req = match read_request(&mut stream).await {
        Ok(r) => r,
        Err(e) => {
            let _ = write_response(&mut stream, Response::text(400, e), None).await;
            return Ok(());
        }
    };

    let origin = req.header("origin").map(|s| s.to_string());
    // 非扩展来源：有 Origin 就拦，没 Origin（curl / 桌面程序）必须带对 token。
    if let Some(o) = origin.as_deref() {
        if !origin_allowed(o) {
            let _ = write_response(&mut stream, Response::text(403, "来源不被允许"), None).await;
            return Ok(());
        }
    }

    if req.method == "OPTIONS" {
        let _ = write_response(&mut stream, Response::empty(204), origin.as_deref()).await;
        return Ok(());
    }

    let cfg = bridge.snapshot();
    let supplied = req.header("x-pixvault-token").unwrap_or("");
    if supplied != cfg.token {
        let body = Response::json(
            401,
            serde_json::json!({ "ok": false, "error": "token 不匹配：请在扩展选项里填入设置面板中的访问令牌" }),
        );
        let _ = write_response(&mut stream, body, origin.as_deref()).await;
        return Ok(());
    }

    let resp = route(&req, &app, &bridge);
    let _ = write_response(&mut stream, resp, origin.as_deref()).await;
    Ok(())
}

fn route(req: &Request, app: &AppHandle, bridge: &Arc<Bridge>) -> Response {
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/ping") | ("GET", "/api/status") => {
            let (library, _) = current_library(app);
            Response::json(
                200,
                serde_json::json!({
                    "ok": true,
                    "app": "pixvault",
                    "version": env!("CARGO_PKG_VERSION"),
                    "library": library,
                }),
            )
        }
        // 采集面板的「存放」下拉：库里已有的文件夹 + 已经有过的标签，都从这里取。
        // 面板在网页上下文里，直接 fetch 127.0.0.1 会被页面 CSP 拦，所以只走后台代理。
        ("GET", "/api/meta") | ("GET", "/api/folders") => meta(app),
        ("POST", "/api/ingest") => ingest(req, app, bridge),
        _ => Response::json(
            404,
            serde_json::json!({ "ok": false, "error": format!("未知接口：{} {}", req.method, req.path) }),
        ),
    }
}

/// 取当前活动图库的 (id, 名称, 根目录)。
fn current_library(app: &AppHandle) -> (Option<serde_json::Value>, Option<String>) {
    let state = app.state::<crate::AppState>();
    let engine = match state.engine.lock() {
        Ok(e) => e,
        Err(_) => return (None, None),
    };
    let Some(arc) = engine.active_library() else {
        return (None, None);
    };
    let lib = match arc.lock() {
        Ok(l) => l,
        Err(_) => return (None, None),
    };
    (
        Some(serde_json::json!({
            "id": lib.id,
            "name": lib.name,
            "root_path": lib.root_path.to_string_lossy(),
        })),
        Some(lib.id.clone()),
    )
}

// ───────────────────────── 库内已有文件夹 / 标签 ─────────────────────────

/// 给采集面板用的「存放位置」清单。
///
/// 只列出相对库根的目录名，最多扫 [MAX_FOLDER_DEPTH] 层、[MAX_FOLDERS] 条 ——
/// 面板是个下拉框，不是文件管理器；几千条既滚不动也没人挑得完。
fn meta(app: &AppHandle) -> Response {
    let (lib_name, root, tags) = {
        let state = app.state::<crate::AppState>();
        let engine = match state.engine.lock() {
            Ok(e) => e,
            Err(_) => {
                return Response::json(
                    500,
                    serde_json::json!({ "ok": false, "error": "应用状态锁失效" }),
                )
            }
        };
        let Some(arc) = engine.active_library() else {
            return Response::json(
                409,
                serde_json::json!({ "ok": false, "error": "PixVault 里还没有打开图库" }),
            );
        };
        let lib = match arc.lock() {
            Ok(l) => l,
            Err(_) => {
                return Response::json(
                    500,
                    serde_json::json!({ "ok": false, "error": "图库锁失效" }),
                )
            }
        };
        let tags: Vec<String> = lib
            .db
            .get_all_tags()
            .map(|v| v.into_iter().map(|t| t.name).take(MAX_TAGS).collect())
            .unwrap_or_default();
        (lib.name.clone(), lib.root_path.clone(), tags)
    };
    // 锁在这里已经放掉：扫盘可能要几百毫秒，不能一边扫一边顶着图库锁。
    let folders = collect_folders(&root);

    Response::json(
        200,
        serde_json::json!({
            "ok": true,
            "library": { "name": lib_name, "root_path": root.to_string_lossy() },
            "folders": folders,
            "tags": tags,
        }),
    )
}

const MAX_FOLDER_DEPTH: usize = 3;
const MAX_FOLDERS: usize = 400;
const MAX_TAGS: usize = 300;

/// 扫出库根下的子目录（相对路径，`/` 分隔）。跳过点开头的目录与元数据目录。
fn collect_folders(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return out;
    }
    let meta_name = crate::utils::winattr::metadata_dir(root)
        .file_name()
        .map(|n| n.to_string_lossy().to_string());
    for entry in walkdir::WalkDir::new(root)
        .min_depth(1)
        .max_depth(MAX_FOLDER_DEPTH)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        // 点开头的目录（含元数据目录）整枝剪掉：`filter_entry` 会连子树一起跳过，
        // 否则光 `.pixvault/thumbnails` 就有上万条，白扫。
        .filter_entry(move |e| {
            let name = e.file_name().to_string_lossy();
            let name: &str = &name;
            !(name.starts_with('.') || Some(name) == meta_name.as_deref())
        })
        .filter_map(|e| e.ok())
    {
        if out.len() >= MAX_FOLDERS {
            break;
        }
        if !entry.file_type().is_dir() {
            continue;
        }
        let rel = match entry.path().strip_prefix(root) {
            Ok(p) => p.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        out.push(rel);
    }
    out
}

/// `?tags=猫,狗` / `?tag=猫&tag=狗` / `?tags=猫，狗` 都认。
/// 逐个清洗：去 `#`、去空白、截断到 40 字、按小写去重、最多 [MAX_TAG_INPUT] 个。
fn parse_tags(req: &Request) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for raw in req
        .query_all("tags")
        .into_iter()
        .chain(req.query_all("tag"))
    {
        for piece in raw.split([',', '，', '、', '|']) {
            let name = piece.trim().trim_start_matches('#').trim();
            if name.is_empty() {
                continue;
            }
            let name = truncate_chars(name, 40);
            let key = name.to_lowercase();
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            out.push(name);
            if out.len() >= MAX_TAG_INPUT {
                return out;
            }
        }
    }
    out
}

const MAX_TAG_INPUT: usize = 12;

// ───────────────────────── 落盘 + 入库 ─────────────────────────

fn ingest(req: &Request, app: &AppHandle, bridge: &Arc<Bridge>) -> Response {
    if req.body.is_empty() {
        return Response::json(
            400,
            serde_json::json!({ "ok": false, "error": "请求体是空的：扩展应该把文件字节放在 body 里" }),
        );
    }

    let page = req
        .query_get("page")
        .or_else(|| req.header("referer").map(|s| s.to_string()))
        .unwrap_or_default();
    let folder = req.query_get("folder").unwrap_or_default();
    let tags = parse_tags(req);
    let mime = req
        .header("content-type")
        .unwrap_or("application/octet-stream")
        .to_string();

    let raw_name = req
        .query_get("name")
        .or_else(|| {
            req.query_get("url").map(|u| {
                let tail = u.split('?').next().unwrap_or(&u);
                tail.rsplit('/').next().unwrap_or("").to_string()
            })
        })
        .unwrap_or_default();
    let fallback_ext = ext_from_mime(&mime);
    let filename = sanitize_file_name(&raw_name, &fallback_ext);

    // ① 先确认图库 —— 没有打开的库就什么都不写，免得文件落到不知道的地方
    let (lib_id, root) = {
        let state = app.state::<crate::AppState>();
        let engine = match state.engine.lock() {
            Ok(e) => e,
            Err(_) => {
                return Response::json(
                    500,
                    serde_json::json!({ "ok": false, "error": "应用状态锁失效" }),
                )
            }
        };
        let Some(arc) = engine.active_library() else {
            return Response::json(
                409,
                serde_json::json!({ "ok": false, "error": "PixVault 里还没有打开图库，先在应用里打开一个库再摘取" }),
            );
        };
        let lib = match arc.lock() {
            Ok(l) => l,
            Err(_) => {
                return Response::json(
                    500,
                    serde_json::json!({ "ok": false, "error": "图库锁失效" }),
                )
            }
        };
        (lib.id.clone(), lib.root_path.clone())
    };

    // ② 决定落点：库根 / 库根下的子文件夹（扩展按站点建目录）
    let mut dir = root.clone();
    for seg in folder.split(['/', '\\']) {
        let seg = sanitize_dir_segment(seg);
        if seg.is_empty() {
            continue;
        }
        dir = dir.join(seg);
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return Response::json(
            500,
            serde_json::json!({ "ok": false, "error": format!("创建目录失败：{e}") }),
        );
    }

    let target = unique_path(&dir, &filename);
    if let Err(e) = std::fs::write(&target, &req.body) {
        return Response::json(
            500,
            serde_json::json!({ "ok": false, "error": format!("写入失败：{e}") }),
        );
    }

    let size = req.body.len() as u64;
    let path_str = target.to_string_lossy().to_string();
    let final_name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| filename.clone());
    let ext = target
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let is_video = is_video_ext(&ext);

    // ③ 入库。写文件和拿锁分开，避免几十 MB 的写盘阻塞界面。
    {
        let state = app.state::<crate::AppState>();
        let engine = match state.engine.lock() {
            Ok(e) => e,
            Err(_) => {
                return Response::json(
                    500,
                    serde_json::json!({
                        "ok": false,
                        "path": path_str,
                        "error": format!("文件已保存到 {path_str}，但索引时状态锁失效"),
                    }),
                )
            }
        };
        let Some(arc) = engine.get_library(&lib_id) else {
            // 库在写盘期间被关掉了：文件已经安全落在硬盘上，只是这次没能入库
            return Response::json(
                200,
                serde_json::json!({
                    "ok": true, "indexed": false, "path": path_str, "filename": final_name,
                    "size": size, "warning": "图库已关闭，文件已保存但未入库"
                }),
            );
        };
        let lib = match arc.lock() {
            Ok(l) => l,
            Err(_) => {
                return Response::json(
                    500,
                    serde_json::json!({ "ok": false, "error": "图库锁失效（文件已保存）" }),
                )
            }
        };

        if let Some(entry) = crate::engine::indexer::Indexer::build_entry(&target) {
            let _ = lib.db.insert_file(&entry);
        }
        // 网页来源写进备注：库里没有专门的 source_url 列，写备注最省事，
        // 也不会覆盖用户自己写的（新文件备注本来就是空的）。
        if !page.is_empty() {
            let _ = lib
                .db
                .update_file_notes(&path_str, &format!("来源：{page}"));
        }
        // 采集时勾的标签：名字不存在就现建一个（`a/b` 会自动补齐父级）。
        if !tags.is_empty() {
            if let Ok(Some(row)) = lib.db.get_file_by_path(&path_str) {
                for name in &tags {
                    if let Ok(tag_id) = lib.db.get_or_create_tag(name) {
                        let _ = lib.db.add_file_tag(row.id, tag_id);
                    }
                }
            }
        }

        // 视频不进缩略图队列：解码器不认，硬排只会白占线程。
        if !is_video {
            let hash = crate::utils::hash::blake3_hash_bytes(path_str.as_bytes());
            let job = crate::engine::thumbs::ThumbJob {
                key: crate::engine::thumbs::job_key(&lib_id, &path_str),
                path: path_str.clone(),
                source: target.clone(),
                hash,
                engine: lib.thumbnail.clone(),
                kind: crate::engine::thumbs::ThumbKind::Grid,
            };
            state
                .thumbs
                .enqueue_with(vec![job], crate::engine::thumbs::Priority::Urgent);
        }
    }

    let item = Ingested {
        path: path_str.clone(),
        filename: final_name.clone(),
        size,
        page: page.clone(),
        at: chrono::Utc::now()
            .with_timezone(&chrono::Local)
            .format("%m-%d %H:%M")
            .to_string(),
        library_id: lib_id.clone(),
    };
    bridge.ingested.fetch_add(1, Ordering::Relaxed);
    bridge.push_recent(item.clone());
    let _ = app.emit("bridge-ingested", item.clone());

    Response::json(
        200,
        serde_json::json!({
            "ok": true,
            "indexed": true,
            "path": path_str,
            "filename": final_name,
            "size": size,
            "video": is_video,
            "folder": folder,
            "tags": tags,
        }),
    )
}

fn ext_from_mime(mime: &str) -> String {
    let m = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let ext = match m.as_str() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/avif" => "avif",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        "image/tiff" => "tiff",
        "image/heic" => "heic",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "video/x-matroska" => "mkv",
        _ => "",
    };
    ext.to_string()
}

/// Windows 保留设备名。叫 `CON.jpg` 的文件在资源管理器里根本建不出来。
fn is_reserved_name(stem: &str) -> bool {
    let up = stem.to_ascii_uppercase();
    matches!(
        up.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6"
            | "COM7" | "COM8" | "COM9" | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6"
            | "LPT7" | "LPT8" | "LPT9"
    )
}

/// 把网页上拿到的文件名洗成能落盘的。
///
/// 网页图片名里什么都有：URL 编码残留、`<>:"/\|?*`、结尾的点、超长串，还有
/// `?w=800` 这种查询串被一起带下来的情况（调用方已经切掉 query，这里再防一次）。
fn sanitize_file_name(raw: &str, fallback_ext: &str) -> String {
    let base = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(raw)
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim();

    let mut cleaned = String::with_capacity(base.len());
    for ch in base.chars() {
        if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            cleaned.push('_');
        } else {
            cleaned.push(ch);
        }
    }

    // Windows 不允许结尾的点和空格（`a.jpg.` 会变成 `a.jpg`，看上去像丢了扩展名）
    let mut name = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    if name.is_empty() {
        name = "untitled".to_string();
    }

    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.is_empty() => (s.to_string(), e.to_string()),
        _ => (name.clone(), String::new()),
    };

    let stem = if is_reserved_name(&stem) {
        format!("_{stem}")
    } else {
        stem
    };

    // 超长截断：Windows 单段 255 字节上限，中文一个字 3 字节，按字符数留余量
    let stem = truncate_chars(&stem, 100);
    let ext = truncate_chars(&ext, 12);

    let ext = if ext.is_empty() {
        fallback_ext.to_string()
    } else {
        ext
    };

    if ext.is_empty() {
        stem
    } else {
        format!("{stem}.{ext}")
    }
}

fn sanitize_dir_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.trim().chars() {
        if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    let out = out.trim_matches(['.', ' ']).to_string();
    if out.is_empty() || out == ".." || out == "." {
        return String::new();
    }
    if is_reserved_name(&out) {
        return format!("_{out}");
    }
    truncate_chars(&out, 60)
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}

/// 同名不覆盖：`a.jpg` 已存在就存成 `a (2).jpg`。网页上同名不同图太常见了。
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let p = Path::new(name);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    let ext = p
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    for i in 2..10_000 {
        let c = dir.join(format!("{stem} ({i}){ext}"));
        if !c.exists() {
            return c;
        }
    }
    dir.join(format!(
        "{stem}-{}{ext}",
        chrono::Utc::now().timestamp_millis()
    ))
}

// ───────────────────────── 扩展分发 ─────────────────────────

/// 内置扩展的全部文件（编译期嵌进二进制）。
///
/// 之所以不用 tauri 的 resources：那需要在打包配置里多一份清单，而且开发模式
/// 下的路径和安装后不一样。`include_str!` 只有一条规则 —— 文件在仓库里就一定
/// 在 exe 里，用户点「导出扩展」得到的永远是这一版。
const EXT_FILES: &[(&str, &str)] = &[
    ("manifest.json", include_str!("../../extension/manifest.json")),
    ("background.js", include_str!("../../extension/background.js")),
    ("content.js", include_str!("../../extension/content.js")),
    ("popup.html", include_str!("../../extension/popup.html")),
    ("popup.js", include_str!("../../extension/popup.js")),
    ("options.html", include_str!("../../extension/options.html")),
    ("options.js", include_str!("../../extension/options.js")),
    ("README.md", include_str!("../../extension/README.md")),
];

const EXT_ICONS: &[(&str, &[u8])] = &[
    ("icons/icon16.png", include_bytes!("../../extension/icons/icon16.png")),
    ("icons/icon32.png", include_bytes!("../../extension/icons/icon32.png")),
    ("icons/icon48.png", include_bytes!("../../extension/icons/icon48.png")),
    ("icons/icon128.png", include_bytes!("../../extension/icons/icon128.png")),
];

/// 把内置扩展到用户选的目录，返回写出的文件数。
pub fn export_extension(dest: &Path) -> Result<usize, String> {
    let root = dest.join("pixvault-extension");
    std::fs::create_dir_all(root.join("icons")).map_err(|e| format!("创建目录失败：{e}"))?;

    let mut n = 0;
    for (name, content) in EXT_FILES {
        std::fs::write(root.join(name), content).map_err(|e| format!("写入 {name} 失败：{e}"))?;
        n += 1;
    }
    for (name, bytes) in EXT_ICONS {
        std::fs::write(root.join(name), bytes).map_err(|e| format!("写入 {name} 失败：{e}"))?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_paths_and_illegal_chars() {
        assert_eq!(sanitize_file_name("../../etc/passwd", "jpg"), "passwd.jpg");
        // `?` 是 URL 查询串的起点，先截断再清洗 —— 所以 `.png` 落在查询串里被一起丢掉，
        // 最后回退到 mime 推出来的扩展名。
        assert_eq!(sanitize_file_name("a:b*c?.png", "jpg"), "a_b_c.jpg");
        assert_eq!(sanitize_file_name("photo.jpg?w=800", "png"), "photo.jpg");
        assert_eq!(sanitize_file_name("trailing.  ", "png"), "trailing.png");
    }

    #[test]
    fn sanitize_reserved_names() {
        assert_eq!(sanitize_file_name("CON.jpg", "jpg"), "_CON.jpg");
        assert_eq!(sanitize_file_name("lpt1", "png"), "_lpt1.png");
    }

    #[test]
    fn sanitize_falls_back_to_mime_ext() {
        assert_eq!(sanitize_file_name("image", "webp"), "image.webp");
        assert_eq!(sanitize_file_name("", "gif"), "untitled.gif");
    }

    #[test]
    fn dir_segment_rejects_traversal() {
        assert_eq!(sanitize_dir_segment(".."), "");
        assert_eq!(sanitize_dir_segment("a/b"), "a_b");
        assert_eq!(sanitize_dir_segment("example.com"), "example.com");
    }

    #[test]
    fn video_detect() {
        assert!(is_video_ext("MP4"));
        assert!(is_video_ext(".webm"));
        assert!(!is_video_ext("jpg"));
    }

    #[test]
    fn unique_path_adds_suffix() {
        let dir = std::env::temp_dir().join(format!("pv-bridge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.jpg"), b"x").unwrap();
        let p = unique_path(&dir, "a.jpg");
        assert_eq!(p.file_name().unwrap().to_string_lossy(), "a (2).jpg");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn req_with_query(query: &str) -> Request {
        Request {
            method: "POST".to_string(),
            path: "/api/ingest".to_string(),
            query: query.to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    #[test]
    fn tags_accept_comma_repeat_and_hash() {
        // 逗号分隔（URL 编码的 `,`）+ 重复参数 + `#` 前缀 + 全角逗号
        assert_eq!(
            parse_tags(&req_with_query("tags=%E7%8C%AB%2C%E7%8B%97&tag=%E7%8C%AB")),
            vec!["猫", "狗"]
        );
        assert_eq!(parse_tags(&req_with_query("tags=%23cat")), vec!["cat"]);
        assert_eq!(
            parse_tags(&req_with_query("tags=a%EF%BC%8Cb")),
            vec!["a", "b"]
        );
        assert!(parse_tags(&req_with_query("")).is_empty());
    }

    #[test]
    fn tags_are_deduped_and_capped() {
        let mut q = String::new();
        for i in 0..20 {
            q.push_str(&format!("tag=t{i}&"));
        }
        assert_eq!(parse_tags(&req_with_query(&q)).len(), MAX_TAG_INPUT);
        // 大小写不敏感去重：Cat 已经存在就不再收 cat
        assert_eq!(parse_tags(&req_with_query("tag=Cat&tag=cat")), vec!["Cat"]);
    }

    #[test]
    fn folders_skip_hidden_and_metadata_trees() {
        let root = std::env::temp_dir().join(format!("pv-folders-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("art").join("风景")).unwrap();
        std::fs::create_dir_all(root.join("photos")).unwrap();
        std::fs::create_dir_all(root.join(".pixvault").join("thumbnails")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let out = collect_folders(&root);
        assert_eq!(out, vec!["art", "art/风景", "photos"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
