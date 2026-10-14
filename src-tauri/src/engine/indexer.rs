use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use walkdir::WalkDir;
use crate::engine::db::FileEntry;
use crate::utils::hash;

/// Supported image formats for indexing
const SUPPORTED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "avif", "heic", "heif",
    "bmp", "tiff", "tif", "ico", "svg", "psd", "ai", "eps",
    "raw", "cr2", "cr3", "nef", "arw", "dng", "orf", "rw2", "raf",
    "3fr", "ari", "srf", "sr2", "bay", "cri", "cap", "iiq", "eip",
    "dcs", "dcr", "drf", "k25", "kdc", "mef", "mos", "mrw", "nrw",
    "obm", "orf", "pef", "ptx", "pxn", "r3d", "raw", "rwl", "srw",
    "x3f",
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
            _ => None,
        };
        dims.map(|(w, h)| (Some(w), Some(h))).unwrap_or((None, None))
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
                let h = sof[1] as i32 * 256 + sof[2] as i32;
                let w = sof[3] as i32 * 256 + sof[4] as i32;
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