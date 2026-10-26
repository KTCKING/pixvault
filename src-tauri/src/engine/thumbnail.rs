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
/// 降采样倍率低于这个值时不用 `thumbnail()`（见 `downscale`）。
const THUMBNAIL_MIN_RATIO: f32 = 2.0;

/// 等比缩到 `bounds` 以内（长边上限），并按倍率挑重采样算法。
///
/// 实测同一张 4000×3000 JPEG 缩到 448px（4K 库，16 张取平均）：
/// `resize(CatmullRom)` 单张 **105ms**，`thumbnail()` 只要 **10.6ms**。
/// 而重采样在整条链路上占 49%（比解码的 45% 还高），所以这里是最值钱的一刀。
///
/// `imageops::thumbnail` 是「整数块面积平均」：每个源像素只贡献给一个目标像素。
/// 8~10 倍降采样时每个目标像素平均掉几十个源像素，抗混叠反而比卷积核更彻底，
/// 又因为不用逐像素加权求和，快到接近 memcpy。
///
/// 但它有个明确的失效区间：倍率接近 1 时会**整个跳过**一部分源像素，出现锯齿
/// 和摩尔纹（官方文档也写着 "may give aliasing artifacts if new size is close
/// to old size"）。所以只在倍率 ≥ 2 时用它，否则退回 `resize(CatmullRom)`。
/// 实际命中情况：网格图（448）对绝大多数片子都是 8 倍以上，全走快路；预览图
/// （1600）对 4000px 原片是 2.5 倍，也能走；只有 1900px 这种「略大于 1600」
/// 的源图会落回慢路，而这类图数量少、又是按需生成，不影响体感。
fn downscale(img: &image::DynamicImage, bounds: u32) -> image::DynamicImage {
    let (w, h) = (img.width(), img.height());
    if w <= bounds && h <= bounds {
        return img.clone();
    }
    let ratio = w.max(h) as f32 / bounds as f32;
    if ratio >= THUMBNAIL_MIN_RATIO {
        img.thumbnail(bounds, bounds)
    } else {
        img.resize(bounds, bounds, image::imageops::FilterType::CatmullRom)
    }
}

/// 占位图（LQIP）边长。
///
/// 32px 的 JPEG 约 400–700 字节，base64 后 1 KB 上下，可以塞进事件里直接推给
/// 前端；`object-cover` 拉满整格后已经足够看出「这张图长什么样」，用户在等待
/// 真图时看到的是一片有内容的轮廓，而不是空白骨架。
///
/// 注意前端**不要**再给这张小图叠 `filter: blur()` —— 32px 放大 14 倍本身已经
/// 糊到看不出像素块，再挂一层高斯模糊只是给每张卡片白烧一次 CPU。
pub const PLACEHOLDER_SIZE: u32 = 32;
/// 占位图质量 —— 反正会被模糊，压狠一点无所谓。
const LQIP_QUALITY: u8 = 60;

/// 缓存格式版本。任何影响输出字节的参数改动都要跟着改这里，
/// 否则旧尺寸的缩略图会永远留在磁盘上（用户看到的是两份缓存）。
///
/// 注意：**新增 `_lqip` 这类旁路文件不要动这个版本号** —— 改它会让整库缩略图
/// 被判为过期后全删重算，等于把用户已经攒下的缓存清零，反而更慢。
///
/// 同理，重采样算法从 `CatmullRom` 换成 `thumbnail()`（面积平均）也**刻意不升版**：
/// 输出字节确实变了，但 448px 下两者肉眼分不出来（都是 8 倍以上降采样，抗混叠
/// 都足够），而升版要付出「整库重算一遍」的代价 —— 拿 1 分钟的确定开销去换
/// 一个看不出来的差异不划算。混着用没有副作用，新生成的图自然就是新算法。
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

    /// 占位图（LQIP）路径 —— `{hash}_lqip.jpg`，与缩略图共用目录树。
    pub fn placeholder_path(&self, content_hash: &str) -> PathBuf {
        let prefix = &content_hash[..2.min(content_hash.len())];
        let dir = self.cache_dir.join(prefix);
        dir.join(format!("{}_lqip.jpg", content_hash))
    }

    /// 读一张**已经缓存好**的占位图，返回可直接塞进 `<img src>` 的 data URL。
    ///
    /// 纯磁盘读（约 0.5 KB），不涉及任何解码 —— 所以批量查询里可以放心地对
    /// 每一张图调用，用来给"还没轮到生成真图"的卡片先垫一个模糊底。
    pub fn read_placeholder(&self, content_hash: &str) -> Option<String> {
        let bytes = fs::read(self.placeholder_path(content_hash)).ok()?;
        if bytes.is_empty() {
            return None;
        }
        Some(jpeg_data_url(&bytes))
    }

    /// 生成占位图：**优先复用 EXIF 内嵌缩略图**（只读文件头，毫秒级）。
    ///
    /// 这是整套占位方案能"立刻见效"的关键 —— 相机/手机拍出来的 JPEG 绝大多数
    /// 自带一张 160×120 的内嵌缩略图，正好适合压成 32px。注意它和网格缩略图
    /// 的取舍完全相反：网格嫌 160px 太糊会退回整图解码，而占位图 32px 反而
    /// 刚好，于是这条近乎免费的快路径在这里终于派上用场。
    ///
    /// 没有 EXIF 内嵌图的（PNG、截图、编辑器导出的 JPEG）返回 `Err`，交给整图
    /// 解码完成后用 [`placeholder_from_thumb`] 补。
    pub fn make_placeholder(&self, source: &Path, content_hash: &str) -> Result<String, String> {
        let dest = self.placeholder_path(content_hash);
        if let Some(url) = read_lqip_data_url(&dest) {
            return Ok(url);
        }

        let bytes = Self::read_exif_thumbnail_bytes(source)?;
        let img = image::load_from_memory(&bytes)
            .map_err(|e| format!("内嵌缩略图无法解码: {}", e))?;
        self.write_lqip(&img, &dest)
    }

    /// 从**已经生成好的网格缩略图**反推占位图。
    ///
    /// 解码一张 448px 的 JPEG 只要几毫秒（相比原片的几百毫秒），所以给那些
    /// 没有 EXIF 内嵌图的文件补占位图，走这条比再解一次原图划算得多。
    pub fn placeholder_from_thumb(&self, thumb: &Path, content_hash: &str) -> Result<String, String> {
        let dest = self.placeholder_path(content_hash);
        if let Some(url) = read_lqip_data_url(&dest) {
            return Ok(url);
        }
        let img = image::ImageReader::open(thumb)
            .map_err(|e| format!("Failed to open thumb: {}", e))?
            .decode()
            .map_err(|e| format!("Failed to decode thumb: {}", e))?;
        self.write_lqip(&img, &dest)
    }

    /// 缩到 32px、编成 JPEG、落盘，并返回 data URL。
    fn write_lqip(&self, img: &image::DynamicImage, dest: &Path) -> Result<String, String> {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let small = downscale(img, PLACEHOLDER_SIZE);
        let mut buf = Vec::new();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, LQIP_QUALITY);
        enc.encode(&small.to_rgb8(), small.width(), small.height(), image::ColorType::Rgb8.into())
            .map_err(|e| format!("JPEG encode: {}", e))?;
        fs::write(dest, &buf).map_err(|e| e.to_string())?;
        Ok(jpeg_data_url(&buf))
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

    /// 从 JPEG 头部取出 EXIF 内嵌缩略图的**原始 JPEG 字节**。
    ///
    /// 只读文件前 1 MiB（EXIF 紧跟在 SOI 之后），所以对 30 MB 的原片也是毫秒级
    /// —— 早期版本把整个文件读进来再切，是整条链路里最贵的一步。
    ///
    /// 两个调用方共用这段解析：网格缩略图（在乎清晰度，太小的内嵌图会放弃）和
    /// LQIP 占位图（32px 反而刚好）。历史上这里踩过「按文件偏移切片」和「取
    /// count 当长度」两个坑，所以刻意只留一份实现。
    fn read_exif_thumbnail_bytes(source: &Path) -> Result<Vec<u8>, String> {
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
                        return Ok(data[start..end].to_vec());
                    }
                }
                break;
            }
            pos += 2 + len;
        }
        Err("No EXIF thumbnail".into())
    }

    /// 网格缩略图的 EXIF 快路径：内嵌缩略图够大就直接用它，省掉整图解码。
    fn try_exif_thumbnail(&self, source: &Path, dest: &Path) -> Result<(), String> {
        let bytes = Self::read_exif_thumbnail_bytes(source)?;

        // 解码失败必须返回 Err（而不是假装成功），否则调用方会拿到一个并不存在
        // 的缩略图路径，卡片就是空白。
        let img = image::load_from_memory(&bytes)
            .map_err(|e| format!("Embedded EXIF thumbnail undecodable: {e}"))?;

        // 很多老图 / 扫描件的内嵌缩略图只有 160×120，放大到 448px 只会更糊 ——
        // 这种情况宁可整图解码一次保证画质。只有这里设门槛：LQIP 占位图正好
        // 需要小图，不受此限。
        if img.width().max(img.height()) < 256 {
            return Err("EXIF thumbnail too small".into());
        }

        // 已经比目标小就别放大了。
        let resized = downscale(&img, self.grid_size);

        let jpeg_path = dest.with_extension("jpg");
        let mut jpeg = Vec::new();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY);
        enc.encode(&resized.to_rgb8(), resized.width(), resized.height(), image::ColorType::Rgb8.into())
            .map_err(|e| format!("JPEG encode: {}", e))?;
        fs::write(&jpeg_path, &jpeg).map_err(|e| e.to_string())?;

        if !jpeg_path.exists() {
            return Err("EXIF thumbnail not written".into());
        }
        Ok(())
    }

    /// 整图解码生成缩略图 / 预览图。`target_size` 是长边上限。
    fn generate_with_image_crate(&self, source: &Path, dest: &Path, target_size: u32) -> Result<(), String> {
        let img = image::ImageReader::open(source)
            .map_err(|e| format!("Failed to open: {}", e))?
            .decode()
            .map_err(|e| format!("Failed to decode: {}", e))?;

        let thumb = downscale(&img, target_size);

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
            // `_lqip` 是旁路文件（占位图），不属于「尺寸变体」，任何版本都要保留。
            let is_lqip = stem.ends_with("_lqip");
            let size_ok = stem
                .rsplit_once('_')
                .and_then(|(_, n)| n.parse::<u32>().ok())
                .map(|n| keep.contains(&n))
                .unwrap_or(false);
            if !is_lqip && !size_ok && fs::remove_file(entry.path()).is_ok() {
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

/// 把一段 JPEG 字节包成 data URL。
///
/// 占位图只有 0.5 KB 上下，base64 后 1 KB 出头，直接跟着事件推给前端比让
/// WebView 再走一次 asset 协议更快、也更不容易踩到协议层的坑。
fn jpeg_data_url(bytes: &[u8]) -> String {
    use base64::Engine as _;
    format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// 读一张已落盘的占位图并转成 data URL（文件不存在 / 空文件都返回 `None`）。
fn read_lqip_data_url(dest: &Path) -> Option<String> {
    let bytes = fs::read(dest).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(jpeg_data_url(&bytes))
}