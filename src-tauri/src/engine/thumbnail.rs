use std::path::{Path, PathBuf};
use std::fs;
use std::io::Read;

/// 网格缩略图长边。
///
/// 前端尺寸滑块的范围是 120–400px，也就是说 512 比真正会被显示的最大尺寸还
/// 大 28% —— 多出来的像素既占磁盘，又要在 WebView 里解码、占内存（512² 的位图
/// 约 1 MB，400 张就是 400 MB）。448 覆盖 1x / 1.25x 缩放绰绰有余。
pub const GRID_SIZE: u32 = 448;
/// 右侧预览栏用的中间尺寸。
///
/// 以前这个字段是 `2048` 但**从来没有被调用过** —— 预览栏直接解码原图，中位
/// 2.6 MB / 约 200ms / 展开瞬间 30 MB 内存。1600px 对应 13rem 的显示高度已经
/// 是 4 倍余量，单张约 250 KB。
pub const PREVIEW_SIZE: u32 = 1600;
/// JPEG 质量。70 与 75 在缩略图这个尺寸下肉眼无差，体积小约 12%。
const JPEG_QUALITY: u8 = 70;
/// 重采样滤镜。
///
/// `CatmullRom` 比默认的 `Triangle`（双线性）锐利，正好补偿 512 → 448 的
/// 分辨率下降 —— 看起来反而更清楚，而体积是实打实小了一圈。
const FILTER: image::imageops::FilterType = image::imageops::FilterType::CatmullRom;

/// 缓存格式版本。任何影响输出字节的参数改动都要跟着改这里，
/// 否则旧尺寸的缩略图会永远留在磁盘上（用户看到的是两份缓存）。
const CACHE_VERSION: &str = "grid448-preview1600-q70-catmull";

#[derive(Clone)]
pub struct ThumbnailEngine {
    cache_dir: PathBuf,
    grid_size: u32,
    preview_size: u32,
}

impl ThumbnailEngine {
    pub fn new(cache_dir: &Path) -> Self {
        fs::create_dir_all(cache_dir).ok();
        ThumbnailEngine {
            cache_dir: cache_dir.to_path_buf(),
            grid_size: GRID_SIZE,
            preview_size: PREVIEW_SIZE,
        }
    }

    pub fn thumbnail_path(&self, content_hash: &str, size: u32) -> PathBuf {
        let prefix = &content_hash[..2.min(content_hash.len())];
        let dir = self.cache_dir.join(prefix);
        dir.join(format!("{}_{}.jpg", content_hash, size))
    }

    /// 预览图路径（`{hash}_1600.jpg`，与缩略图同一个目录树）。
    pub fn preview_path(&self, content_hash: &str) -> PathBuf {
        self.thumbnail_path(content_hash, self.preview_size)
    }

    /// Generate a thumbnail — tries EXIF first, then full decode
    pub fn generate(&self, source_path: &Path, content_hash: &str) -> Result<PathBuf, String> {
        let thumb_path = self.thumbnail_path(content_hash, self.grid_size);
        
        if let Some(parent) = thumb_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        if thumb_path.exists() {
            return Ok(thumb_path);
        }

        // Try EXIF thumbnail first (near-instant for JPEGs)
        if self.try_exif_thumbnail(source_path, &thumb_path).is_ok() {
            return Ok(thumb_path);
        }

        self.generate_with_image_crate(source_path, &thumb_path, self.grid_size)?;
        Ok(thumb_path)
    }

    /// 生成右侧预览图（`{hash}_1600.jpg`）。
    ///
    /// 刻意**不**走 EXIF 快路径：内嵌缩略图通常只有 160×120，最多 320px，撑不起
    /// 1600px 的预览。代价是每次都整图解码 —— 但它只在用户真正打开某张图的
    /// 预览时才触发，生成过一次之后长期复用。
    pub fn generate_preview(&self, source_path: &Path, content_hash: &str) -> Result<PathBuf, String> {
        let dest = self.preview_path(content_hash);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        if dest.exists() {
            return Ok(dest);
        }
        self.generate_with_image_crate(source_path, &dest, self.preview_size)?;
        Ok(dest)
    }

    /// Extract EXIF thumbnail from JPEG by parsing APP1 marker.
    ///
    /// Only the head of the file is read (EXIF lives right after SOI), which
    /// keeps this near-instant even for 30 MB originals — reading the whole
    /// file was the single most expensive part of the old code path.
    fn try_exif_thumbnail(&self, source: &Path, dest: &Path) -> Result<(), String> {
        let ext = source.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if ext != "jpg" && ext != "jpeg" { return Err("Not JPEG".into()); }

        const HEAD_CAP: u64 = 1024 * 1024; // 1 MiB is plenty for EXIF + embedded thumbnail
        let file = fs::File::open(source).map_err(|e| e.to_string())?;
        let mut data = Vec::new();
        file.take(HEAD_CAP).read_to_end(&mut data).map_err(|e| e.to_string())?;
        if data.len() < 20 { return Err("Too small".into()); }

        // Find APP1 marker (0xFFE1) containing "Exif\0\0"
        let mut pos = 2; // skip SOI
        while pos < data.len() - 4 {
            if data[pos] != 0xFF { return Err("Bad marker".into()); }
            let marker = data[pos + 1];
            if marker == 0xDA || marker == 0xD9 { break; } // SOS or EOI
            
            let len = ((data[pos + 2] as usize) << 8) | (data[pos + 3] as usize);
            if marker == 0xE1 && pos + 10 < data.len() && &data[pos+4..pos+10] == b"Exif\0\0" {
                // Found EXIF APP1, parse TIFF header for thumbnail
                let tiff_start = pos + 10;
                if tiff_start + 8 > data.len() { return Err("TIFF too short".into()); }
                
                let little_endian = &data[tiff_start..tiff_start+2] == b"II";
                let read_u16 = |p: usize| -> u16 {
                    if little_endian { data[p] as u16 | ((data[p+1] as u16) << 8) }
                    else { ((data[p] as u16) << 8) | data[p+1] as u16 }
                };
                let read_u32 = |p: usize| -> u32 {
                    if little_endian {
                        data[p] as u32 | ((data[p+1] as u32) << 8) | ((data[p+2] as u32) << 16) | ((data[p+3] as u32) << 24)
                    } else {
                        ((data[p] as u32) << 24) | ((data[p+1] as u32) << 16) | ((data[p+2] as u32) << 8) | data[p+3] as u32
                    }
                };

                let ifd0_offset = read_u32(tiff_start + 4) as usize;
                if ifd0_offset == 0 || tiff_start + ifd0_offset + 2 > data.len() { return Err("IFD0 out of bounds".into()); }

                // Skip IFD0 to find IFD1
                let ifd0_pos = tiff_start + ifd0_offset;
                let num_entries = read_u16(ifd0_pos) as usize;
                let ifd1_pos = ifd0_pos + 2 + num_entries * 12 + 4; // +4 for next IFD offset
                if ifd1_pos + 2 > data.len() { return Err("IFD1 out of bounds".into()); }

                let ifd1_num = read_u16(ifd1_pos) as usize;
                let mut thumb_offset: Option<usize> = None;
                let mut thumb_len: Option<usize> = None;

                for i in 0..ifd1_num {
                    let entry_pos = ifd1_pos + 2 + i * 12;
                    if entry_pos + 12 > data.len() { break; }
                    let tag = read_u16(entry_pos);
                    // IFD 条目结构：[tag u16][type u16][count u32][value u32]。
                    // JPEGInterchangeFormat / ...Length 都是 LONG(4) 且 count=1，
                    // 所以偏移和长度都在 `value` 字段里 —— 以前这里取的是 count
                    // （恒等于 1），`len > 1024` 永远不成立，EXIF 快路径形同虚设，
                    // 每张图都退化成整图解码。
                    let value = read_u32(entry_pos + 8) as usize;
                    if tag == 0x0201 { thumb_offset = Some(value); }
                    if tag == 0x0202 { thumb_len = Some(value); }
                }

                if let (Some(off), Some(len)) = (thumb_offset, thumb_len) {
                    // JPEGInterchangeFormat / ...Length 里的偏移量是**相对 TIFF 头**
                    // 的，不是相对文件开头。以前直接拿 `off` 去切 `data`（文件头
                    // 缓冲区），切出来的是错位的垃圾字节，解码必然失败 ——
                    // 于是每张 JPEG 都退化成一整次全图解码，EXIF 快路径形同虚设。
                    let start = tiff_start + off;
                    let end = start + len;
                    if len > 1024 && end <= data.len() {
                        let thumb_data = &data[start..end];
                        let jpeg_path = dest.with_extension("jpg");

                        // 解码失败必须返回 Err（而不是假装成功），否则调用方会
                        // 拿到一个并不存在的缩略图路径，卡片就是空白。
                        let img = image::load_from_memory(thumb_data)
                            .map_err(|e| format!("Embedded EXIF thumbnail undecodable: {e}"))?;

                        // 很多老图 / 扫描件的内嵌缩略图只有 160×120，放大到网格
                        // 尺寸只会更糊 —— 这种情况宁可整图解码一次保证画质。
                        if img.width().max(img.height()) < 256 {
                            return Err("EXIF thumbnail too small".into());
                        }

                        // 已经比目标小就别放大了。
                        let resized = if img.width() > self.grid_size || img.height() > self.grid_size {
                            img.resize(self.grid_size, self.grid_size, FILTER)
                        } else {
                            img
                        };

                        let mut jpeg = Vec::new();
                        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY);
                        enc.encode(&resized.to_rgb8(), resized.width(), resized.height(), image::ColorType::Rgb8.into())
                            .map_err(|e| format!("JPEG encode: {}", e))?;
                        fs::write(&jpeg_path, &jpeg).map_err(|e| e.to_string())?;

                        if !jpeg_path.exists() {
                            return Err("EXIF thumbnail not written".into());
                        }
                        return Ok(());
                    }
                }
                break;
            }
            pos += 2 + len;
        }
        Err("No EXIF thumbnail".into())
    }

    /// 整图解码生成缩略图 / 预览图。`target_size` 是长边上限。
    fn generate_with_image_crate(&self, source: &Path, dest: &Path, target_size: u32) -> Result<(), String> {
        let img = image::ImageReader::open(source)
            .map_err(|e| format!("Failed to open: {}", e))?
            .decode()
            .map_err(|e| format!("Failed to decode: {}", e))?;

        let (w, h) = (img.width(), img.height());

        let thumb = if w > target_size || h > target_size {
            img.resize(target_size, target_size, FILTER)
        } else {
            img
        };

        let mut jpeg_data = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_data, JPEG_QUALITY);
        encoder
            .encode(&thumb.to_rgb8(), thumb.width(), thumb.height(), image::ColorType::Rgb8.into())
            .map_err(|e| format!("JPEG encode error: {}", e))?;

        let jpeg_path = dest.with_extension("jpg");
        fs::write(&jpeg_path, &jpeg_data).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 缓存格式升级：参数变了就把上一版的缩略图全部删掉。
    ///
    /// 用 `.pixvault/thumb-version` 当哨兵，只在版本号不匹配时扫一次目录，
    /// 正常情况下就是读一个小文件，可以放心地跟着「打开图库」跑。
    /// 不做这件事的话，磁盘上会同时留着 `_512.jpg` 和 `_448.jpg` 两套。
    pub fn ensure_cache_version(&self) -> u64 {
        let Some(parent) = self.cache_dir.parent() else { return 0 };
        let stamp = parent.join("thumb-version");
        if let Ok(cur) = fs::read_to_string(&stamp) {
            if cur.trim() == CACHE_VERSION {
                return 0;
            }
        }
        let removed = self.prune_stale();
        fs::write(&stamp, CACHE_VERSION).ok();
        removed
    }

    /// 删掉尺寸后缀不属于当前配置的缓存文件。
    fn prune_stale(&self) -> u64 {
        let keep = [self.grid_size, self.preview_size];
        let mut removed = 0u64;
        for entry in walkdir::WalkDir::new(&self.cache_dir)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let name = entry.file_name().to_string_lossy().to_string();
            let stem = name.split('.').next().unwrap_or(&name).to_string();
            let size_ok = stem
                .rsplit_once('_')
                .and_then(|(_, n)| n.parse::<u32>().ok())
                .map(|n| keep.contains(&n))
                .unwrap_or(false);
            if !size_ok && fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
        removed
    }

    /// Batch generate thumbnails in parallel using rayon
    pub fn generate_batch(&self, items: &[(PathBuf, String)]) -> Vec<(String, Result<PathBuf, String>)> {
        use rayon::prelude::*;
        items.par_iter().map(|(path, hash)| {
            (hash.clone(), self.generate(path, hash))
        }).collect()
    }

    pub fn cleanup_orphans(&self, active_hashes: &[String]) -> Result<u64, String> {
        let mut removed = 0u64;
        let active_set: std::collections::HashSet<&str> = active_hashes.iter().map(|s| s.as_str()).collect();

        for entry in walkdir::WalkDir::new(&self.cache_dir)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let name = entry.file_name().to_string_lossy();
            if let Some(hash_part) = name.split('_').next() {
                if !active_set.contains(hash_part) {
                    if fs::remove_file(entry.path()).is_ok() {
                        removed += 1;
                    }
                }
            }
        }
        Ok(removed)
    }

    pub fn cache_size(&self) -> u64 {
        walkdir::WalkDir::new(&self.cache_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
            .sum()
    }
}