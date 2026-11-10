use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use walkdir::WalkDir;
use crate::engine::db::FileEntry;
use crate::utils::hash;

/// Supported formats for indexing.
///
/// 视频也收：浏览器扩展摘下来的 mp4/webm 得能在库里看得见。它们不参与缩略图
/// 生成（没有解码器），卡片上显示的是文件类型图标，双击用系统的播放器打开。
const SUPPORTED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "avif", "heic", "heif",
    "bmp", "tiff", "tif", "ico", "svg", "psd", "ai", "eps",
    "raw", "cr2", "cr3", "nef", "arw", "dng", "orf", "rw2", "raf",
    "3fr", "ari", "srf", "sr2", "bay", "cri", "cap", "iiq", "eip",
    "dcs", "dcr", "drf", "k25", "kdc", "mef", "mos", "mrw", "nrw",
    "obm", "orf", "pef", "ptx", "pxn", "r3d", "raw", "rwl", "srw",
    "x3f",
    "mp4", "m4v", "webm", "mov", "mkv", "avi", "wmv", "flv", "ogv", "mpg", "mpeg", "3gp",
];

pub struct Indexer {
    progress_tx: Option<mpsc::UnboundedSender<IndexProgress>>,
}

#[derive(Debug, Clone)]
pub struct IndexProgress {
    pub total: u64,
    pub processed: u64,
    pub current_file: String,
    pub stage: IndexStage,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IndexStage {
    Scanning,
    Hashing,
    GeneratingThumbnails,
    Complete,
}

/// 判断一个文件是不是 Eagle 条目目录（`<ID>.info/`）里的缩略图。
///
/// 两种命名都见过：`thumbnail.png` 与 `<原名>_thumbnail.png`。只在这个特定目录
/// 结构下生效，否则用户真有一张叫 `cover_thumbnail.jpg` 的图就会被吞掉。
fn is_eagle_sidecar_thumb(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let in_info_dir = parent.components().any(|c| match c {
        std::path::Component::Normal(s) => {
            s.to_string_lossy().to_lowercase().ends_with(".info")
        }
        _ => false,
    });
    if !in_info_dir {
        return false;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let stem = match name.rsplit_once('.') {
        Some((s, _)) => s.to_string(),
        None => name.clone(),
    };
    stem == "thumbnail" || stem.ends_with("_thumbnail")
}

impl Indexer {
    pub fn new() -> Self {
        Indexer { progress_tx: None }
    }

    pub fn set_progress_sender(&mut self, tx: mpsc::UnboundedSender<IndexProgress>) {
        self.progress_tx = Some(tx);
    }

    /// Scan directory and return all supported image files
    pub fn scan_directory(&self, root: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            
            // Skip any file inside a .pixvault directory (metadata/thumbnails)
            let mut skip = false;
            for component in path.components() {
                if let std::path::Component::Normal(c) = component {
                    if c == ".pixvault" {
                        skip = true;
                        break;
                    }
                }
            }
            if skip {
                continue;
            }

            // Eagle 的条目目录 `<ID>.info` 里，原图旁边还躺着一张缩略图
            // （`thumbnail.png` 或 `<原名>_thumbnail.png`）。它不是素材，收进图库
            // 只会让每个条目多出一条「长得一样但很糊」的记录。限定在 `.info` 目录
            // 内判断，用户自己起名叫 xxx_thumbnail 的文件不受影响。
            if is_eagle_sidecar_thumb(&path) {
                continue;
            }

            if let Some(ext) = path.extension() {
                let ext_lower = ext.to_string_lossy().to_lowercase();
                if SUPPORTED_EXTENSIONS.contains(&ext_lower.as_str()) {
                    files.push(path.to_path_buf());
                }
            }
        }

        files.sort();
        files
    }

    /// Build a FileEntry from a file path (without hashing yet)
    pub fn build_entry(path: &Path) -> Option<FileEntry> {
        let metadata = path.metadata().ok()?;
        
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        
        let extension = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();

        let mtime = metadata
            .modified()
            .ok()
            .map(|t| {
                chrono::DateTime::<chrono::Utc>::from(t)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_default();

        let ctime = metadata
            .created()
            .ok()
            .map(|t| {
                chrono::DateTime::<chrono::Utc>::from(t)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_default();

        // Try to get image dimensions
        let (width, height) = Self::get_image_dimensions(path);

        let now_ts = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        Some(FileEntry {
            id: 0, // Will be assigned by DB
            path: path.to_string_lossy().to_string(),
            filename,
            extension,
            size_bytes: metadata.len() as i64,
            width,
            height,
            blake3_hash: None,
            phash: None,
            mtime,
            ctime,
            rating: None,
            color_label: None,
            notes: None,
            is_deleted: false,
            favorite: false,
            // Only honoured on first insert; a re-index preserves the
            // original import time (see `Database::insert_file`).
            created_at: now_ts.clone(),
            updated_at: now_ts,
        })
    }

    /// Get image dimensions quickly by parsing only headers (no full decode)
    fn get_image_dimensions(path: &Path) -> (Option<i32>, Option<i32>) {
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        
        let dims = match ext.as_str() {
            "jpg" | "jpeg" => Self::read_jpeg_dimensions(path),
            "png" => Self::read_png_dimensions(path),
            "gif" => Self::read_gif_dimensions(path),
            "webp" => Self::read_webp_dimensions(path),
            "bmp" => Self::read_bmp_dimensions(path),
            // PSD / PSB 的尺寸在固定偏移的 26 字节头里，读一下就有 —— 以前没接
            // 这两条，导致库里所有 PSD 的 width/height 都是 NULL，卡片只能显示
            // 文件大小，按尺寸排序时也排不进去。
            "psd" | "psb" => crate::engine::psd::dimensions(path)
                .and_then(|(w, h)| Some((i32::try_from(w).ok()?, i32::try_from(h).ok()?))),
            // 视频只认 ISO-BMFF 家族（mp4 / m4v / mov）；mkv、avi 这些容器结构
            // 完全不同，暂时留空 —— 它们本来也没有缩略图。
            "mp4" | "m4v" | "mov" => Self::read_mp4_dimensions(path),
            _ => None,
        };
        dims.map(|(w, h)| (Some(w), Some(h))).unwrap_or((None, None))
    }

    /// 单独探一次尺寸（索引之外的回填用：只读文件头，不碰图像数据）。
    pub fn probe_dimensions(path: &Path) -> Option<(i32, i32)> {
        let (w, h) = Self::get_image_dimensions(path);
        Some((w?, h?))
    }

    fn read_jpeg_dimensions(path: &Path) -> Option<(i32, i32)> {
        let mut file = std::fs::File::open(path).ok()?;
        let mut buf = [0u8; 2];
        use std::io::Read;
        file.read_exact(&mut buf).ok()?;
        if buf != [0xFF, 0xD8] { return None; }
        loop {
            if file.read_exact(&mut buf).is_err() { return None; }
            if buf[0] != 0xFF { return None; }
            if buf[1] == 0xFF { continue; }
            if (0xC0..=0xC3).contains(&buf[1]) || (0xC5..=0xC7).contains(&buf[1]) 
                || (0xC9..=0xCB).contains(&buf[1]) || (0xCD..=0xCF).contains(&buf[1]) {
                let mut sof = [0u8; 7];
                file.read_exact(&mut sof).ok()?;
                // SOF 段的字节排布是 `Length(2) Precision(1) Height(2) Width(2)`。
                // 这里原来整体错了一位：`h` 取的是「段长低字节 + 精度」，而常见
                // JPEG 的 SOF 段长就是 0x0011、精度恒为 8，于是**每一张 JPEG 的
                // height 都被写成 4360**；`w` 取到的其实是 height。后果是全库
                // JPEG 的宽高全错（实测 2747 张），瀑布流按错的比例排版，横图被
                // 裁成竖条。
                let h = sof[3] as i32 * 256 + sof[4] as i32;
                let w = sof[5] as i32 * 256 + sof[6] as i32;
                return Some((w, h));
            }
            let mut len_buf = [0u8; 2];
            file.read_exact(&mut len_buf).ok()?;
            let len = len_buf[0] as i64 * 256 + len_buf[1] as i64;
            if len < 2 { return None; }
            use std::io::Seek;
            file.seek(std::io::SeekFrom::Current(len - 2)).ok()?;
        }
    }

    fn read_png_dimensions(path: &Path) -> Option<(i32, i32)> {
        let mut file = std::fs::File::open(path).ok()?;
        let mut buf = [0u8; 24];
        use std::io::Read;
        file.read_exact(&mut buf).ok()?;
        let w = buf[16] as i32 * 16777216 + buf[17] as i32 * 65536 + buf[18] as i32 * 256 + buf[19] as i32;
        let h = buf[20] as i32 * 16777216 + buf[21] as i32 * 65536 + buf[22] as i32 * 256 + buf[23] as i32;
        Some((w, h))
    }

    fn read_gif_dimensions(path: &Path) -> Option<(i32, i32)> {
        let mut file = std::fs::File::open(path).ok()?;
        let mut buf = [0u8; 10];
        use std::io::Read;
        file.read_exact(&mut buf).ok()?;
        let w = buf[6] as i32 + buf[7] as i32 * 256;
        let h = buf[8] as i32 + buf[9] as i32 * 256;
        Some((w, h))
    }

    fn read_webp_dimensions(path: &Path) -> Option<(i32, i32)> {
        let mut file = std::fs::File::open(path).ok()?;
        let mut buf = [0u8; 30];
        use std::io::Read;
        file.read_exact(&mut buf).ok()?;
        if &buf[0..4] != b"RIFF" || &buf[8..12] != b"WEBP" { return None; }
        match &buf[12..16] {
            b"VP8 " => {
                let w = (buf[23] as i32 & 0x3F) * 256 + buf[22] as i32;
                let h = (buf[25] as i32 & 0x3F) * 256 + buf[24] as i32;
                Some((w, h))
            }
            b"VP8L" => {
                let w = (buf[17] as i32 & 0x3F) * 256 + buf[16] as i32 + 1;
                let h = ((buf[19] as i32 & 0x0F) * 4096 + buf[18] as i32 * 16 + ((buf[17] as i32 >> 6) & 0x03)) + 1;
                Some((w, h))
            }
            // VP8X（扩展格式：动图 / 带 alpha / 带 ICC 的 webp）。画布尺寸是
            // 24..27 与 27..30 两个 24 位小端值，存的是「尺寸 - 1」。
            b"VP8X" => {
                let w = (buf[24] as i32 | (buf[25] as i32) << 8 | (buf[26] as i32) << 16) + 1;
                let h = (buf[27] as i32 | (buf[28] as i32) << 8 | (buf[29] as i32) << 16) + 1;
                Some((w, h))
            }
            _ => None,
        }
    }

    fn read_bmp_dimensions(path: &Path) -> Option<(i32, i32)> {
        let mut file = std::fs::File::open(path).ok()?;
        let mut buf = [0u8; 26];
        use std::io::Read;
        file.read_exact(&mut buf).ok()?;
        let w = buf[18] as i32 + buf[19] as i32 * 256 + buf[20] as i32 * 65536 + buf[21] as i32 * 16777216;
        let h = buf[22] as i32 + buf[23] as i32 * 256 + buf[24] as i32 * 65536 + buf[25] as i32 * 16777216;
        Some((w, h.abs()))
    }

    /// 读 mp4 / m4v / mov 的视频显示尺寸。
    ///
    /// 尺寸在 `moov → trak → tkhd` 里：末尾两个 16.16 定点数就是宽高。`moov`
    /// 一般在文件尾部（可能是几十 GB 视频的末尾），所以顶层用 box 长度**跳着
    /// 走**，跳过 `mdat` 只要一次 seek；找到 `moov` 后只把这一个 box 读进内存
    /// （上限 [`MOOV_CAP`]），再在里面找 `tkhd`。
    ///
    /// 音轨的 `tkhd` 宽高是 0，会被跳过；带旋转矩阵的视频这里返回的是**编码
    /// 尺寸**（没做矩阵换算），缩略图和「图片尺寸」显示用足够了。
    fn read_mp4_dimensions(path: &Path) -> Option<(i32, i32)> {
        use std::io::{Read, Seek, SeekFrom};

        let mut file = std::fs::File::open(path).ok()?;
        let file_len = file.metadata().ok()?.len();
        let mut pos = 0u64;

        while pos + 8 <= file_len {
            file.seek(SeekFrom::Start(pos)).ok()?;
            let mut hdr = [0u8; 16];
            file.read_exact(&mut hdr[..8]).ok()?;

            let mut box_len = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
            let header_len = match box_len {
                // 1 = 后面跟 64 位长度
                1 => {
                    file.read_exact(&mut hdr[8..16]).ok()?;
                    box_len = u64::from_be_bytes([
                        hdr[8], hdr[9], hdr[10], hdr[11],
                        hdr[12], hdr[13], hdr[14], hdr[15],
                    ]);
                    16
                }
                // 0 = 一直到文件尾
                0 => {
                    box_len = file_len - pos;
                    8
                }
                _ => 8,
            };
            if box_len < header_len || pos + box_len > file_len {
                return None;
            }

            if &hdr[4..8] == b"moov" {
                let payload = (box_len - header_len).min(MOOV_CAP) as usize;
                file.seek(SeekFrom::Start(pos + header_len)).ok()?;
                let mut buf = vec![0u8; payload];
                file.read_exact(&mut buf).ok()?;
                return find_tkhd_dimensions(&buf);
            }

            pos += box_len;
        }

        None
    }

    /// Check if a file needs re-indexing (changed since last index)
    pub fn needs_reindex(path: &Path, known_mtime: &str) -> bool {
        if let Ok(metadata) = path.metadata() {
            if let Ok(mtime) = metadata.modified() {
                let current_mtime = chrono::DateTime::<chrono::Utc>::from(mtime)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string();
                return current_mtime != known_mtime;
            }
        }
        // If we can't read metadata, assume it needs reindexing
        true
    }
}
/// 读进内存的 `moov` box 上限。
///
/// 正常视频的 moov 是几百 KB 到几 MB（长片、多音轨会更大些）。设个上限是为了
/// 避开畸形文件：宁可放弃尺寸，也不要为一个索引项吃掉几十 MB 内存。
const MOOV_CAP: u64 = 32 * 1024 * 1024;

/// 在 `moov` 内容里找第一段带非零宽高的 `tkhd`。
///
/// 直接按 4 字节对齐扫 `tkhd` 标记，而不是完整解析 box 树 —— tkhd 的字段位置
/// 只跟 version 有关，扫到就能算。音轨的 tkhd 宽高是 0，跳过继续找视频轨。
fn find_tkhd_dimensions(buf: &[u8]) -> Option<(i32, i32)> {
    let be_u32_at = |p: usize| -> u32 {
        u32::from_be_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]])
    };

    let mut i = 0usize;
    while i + 96 <= buf.len() {
        if &buf[i..i + 4] == b"tkhd" {
            // box 头是 size(4) + "tkhd"(4)，字段从 payload 起算。
            let payload = i + 4;
            let base = match buf[payload] {
                // v0: version/flags(4) + 时间与轨道 id(16) + 时长(4) + 保留(8)
                //     + layer/alt/volume/保留(8) + matrix(36) = 76
                0 => payload + 76,
                // v1: 两处时间戳各多 4 字节，其余相同
                1 => payload + 88,
                _ => {
                    i += 4;
                    continue;
                }
            };
            if base + 8 <= buf.len() {
                // 16.16 定点数，取整数部分。
                let w = (be_u32_at(base) >> 16) as i32;
                let h = (be_u32_at(base + 4) >> 16) as i32;
                if (1..=100_000).contains(&w) && (1..=100_000).contains(&h) {
                    return Some((w, h));
                }
            }
        }
        i += 1;
    }
    None
}
