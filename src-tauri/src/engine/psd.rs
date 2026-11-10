//! PSD（Photoshop 文档）缩略图解码。
//!
//! `image` crate 不认 PSD，所以以前这类文件在缩略图队列里必然失败，前端拿到
//! `fallback` 之后会把**原始 PSD 路径**塞给 `<img>` —— 一张 1.5 GB 的 PSD 会
//! 让 WebView 去走 asset 协议把整个文件读一遍再解码失败，界面直接卡死。这个
//! 模块就是来把这个洞补上的。
//!
//! 只做一件事：把「合成图」（merged composite，也就是 Photoshop 里所有可见
//! 图层拍平后的结果）解出来，边解边降采样到目标尺寸。刻意**不**碰图层树 ——
//! 图层要解析海量块结构，还会把整张图放大内存，对缩略图毫无意义。
//!
//! 关键设计是**流式**：
//!   * 头部 / 资源段 / 图层段都用 `seek` 跳过，不去读那几百 MB 的图层数据；
//!   * 合成数据按「通道 → 行」顺序 RLE 解码，每行解完立刻累加进目标小图；
//!   * 因此内存峰值只有「一行 + 目标图」，与源文件多大无关。1.5 GB 的 PSD
//!     也只占几 MB 内存。
//!
//! 顺带提供 [`embedded_thumbnail`]：PSD 的资源段里通常带一张 160px 的 JPEG
//! 预览（资源 1033 / 1036）。它只读文件前几百 KB，用来给网格垫模糊底 —— 也就
//! 是说 PSD 也能像 JPEG 那样「先看到轮廓，再变清晰」。

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use image::RgbImage;

/// 资源段最多读这么多字节去找内嵌缩略图。
///
/// 正常 PSD 的资源段是几十 KB 到几 MB（主要被 ICC 配置文件占掉）。设个上限是
/// 为了避开畸形文件：真碰上 2 GB 的资源段，宁可放弃快路径也不要一口气读进内存。
const RESOURCES_CAP: u64 = 64 * 1024 * 1024;

/// PSD 头部里我们关心的字段。
#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub width: u32,
    pub height: u32,
    pub channels: u16,
    /// 每通道位深（8 / 16 / 32）。
    pub depth: u16,
    /// 色彩模式：1 = 灰度，2 = 索引，3 = RGB，4 = CMYK。
    pub color_mode: u16,
}

// ── 基础读取 ────────────────────────────────────────

fn be_u16<R: Read>(r: &mut R) -> Result<u16, String> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b).map_err(|e| format!("读取 PSD 头部失败: {e}"))?;
    Ok(u16::from_be_bytes(b))
}

fn be_u32<R: Read>(r: &mut R) -> Result<u32, String> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b).map_err(|e| format!("读取 PSD 头部失败: {e}"))?;
    Ok(u32::from_be_bytes(b))
}

fn be_u16_at(buf: &[u8], p: usize) -> u16 {
    if p + 2 > buf.len() {
        return 0;
    }
    u16::from_be_bytes([buf[p], buf[p + 1]])
}

fn be_u32_at(buf: &[u8], p: usize) -> u32 {
    if p + 4 > buf.len() {
        return 0;
    }
    u32::from_be_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]])
}

/// 读 26 字节文件头并做基本校验。
fn read_header<R: Read>(r: &mut R) -> Result<Header, String> {
    let mut sig = [0u8; 4];
    r.read_exact(&mut sig).map_err(|e| format!("不是 PSD 文件: {e}"))?;
    if &sig != b"8BPS" {
        return Err("不是 PSD 文件（签名不是 8BPS）".into());
    }
    let version = be_u16(r)?;
    if version != 1 {
        // PSB（大文档格式）的段长度是 64 位，结构不同，暂时不支持。
        return Err(format!("暂不支持 PSD 版本 {version}"));
    }
    let mut reserved = [0u8; 6];
    r.read_exact(&mut reserved).map_err(|e| format!("读取 PSD 头部失败: {e}"))?;

    let channels = be_u16(r)?;
    let height = be_u32(r)?;
    let width = be_u32(r)?;
    let depth = be_u16(r)?;
    let color_mode = be_u16(r)?;

    if width == 0 || height == 0 {
        return Err("PSD 画布尺寸为 0".into());
    }
    // 尺寸上限纯粹是防呆：真按 10 万 × 10 万去算目标映射表会先把自己算死。
    if width > 200_000 || height > 200_000 {
        return Err(format!("PSD 画布尺寸异常：{width}×{height}"));
    }
    Ok(Header { width, height, channels, depth, color_mode })
}

/// 读一段的 4 字节长度并直接跳过它。
///
/// 用 `seek` 而不是 `read`：图层段动辄几百 MB，读进来只会白烧内存和 IO。
fn skip_section<R: Read + Seek>(r: &mut R, what: &str) -> Result<u64, String> {
    let len = be_u32(r)? as u64;
    r.seek(SeekFrom::Current(len as i64))
        .map_err(|e| format!("跳过 PSD {what} 段失败: {e}"))?;
    Ok(len)
}

// ── 内嵌缩略图（快路径）──────────────────────────────

/// 取出 PSD 内嵌的预览图（JPEG 字节）。
///
/// 资源 1033 是「Photoshop 4.0 缩略图」，1036 是「Photoshop 5.0 缩略图」，
/// 两者结构一致（1036 末尾多 4 字节位深 / 平面数），内容一般是一张 160×120 的
/// JPEG。读它只需要扫过资源段（几十 KB 到几 MB），对 GB 级文件也是毫秒级。
///
/// 拿它当 **LQIP（32px 模糊底）** 用是极致划算的：几乎零成本，却让用户在等
/// 448px 合成图解码的那一两秒里先看见画面轮廓。
pub fn embedded_thumbnail(path: &Path) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    let mut r = BufReader::with_capacity(256 * 1024, file);

    read_header(&mut r).ok()?;
    // 色彩模式数据段
    skip_section(&mut r, "色彩模式").ok()?;
    // 资源段：这一段需要真正扫描，其余全部跳过。
    let res_len = be_u32(&mut r).ok()? as u64;
    let want = res_len.min(RESOURCES_CAP);
    let mut buf = vec![0u8; want as usize];
    r.read_exact(&mut buf).ok()?;
    find_thumbnail(&buf)
}

/// 在资源段字节里找 1033 / 1036 缩略图块，返回其中的图片字节。
fn find_thumbnail(buf: &[u8]) -> Option<Vec<u8>> {
    let mut p = 0usize;
    while p + 12 <= buf.len() {
        // 每个资源块：'8BIM' + id(2) + Pascal 名字(1+n，补齐到偶数) + size(4) + data
        if &buf[p..p + 4] != b"8BIM" {
            // 签名断了就别硬猜了 —— 继续往前扫只会读出垃圾。
            return None;
        }
        let id = be_u16_at(buf, p + 4);
        let name_len = buf[p + 6] as usize;
        // 名字字段总长 = 1 + name_len，需要是偶数
        let name_field = if (name_len + 1) % 2 == 0 { name_len + 1 } else { name_len + 2 };
        let size_pos = p + 6 + name_field;
        if size_pos + 4 > buf.len() {
            return None;
        }
        let size = be_u32_at(buf, size_pos) as usize;
        let data_pos = size_pos + 4;
        let data_end = data_pos.checked_add(size)?;
        if data_end > buf.len() {
            return None;
        }

        if id == 1033 || id == 1036 {
            if let Some(jpeg) = parse_thumb_resource(&buf[data_pos..data_end], id) {
                return Some(jpeg);
            }
        }

        // 块数据按偶数对齐
        p = data_end + (size % 2);
    }
    None
}

/// 解析缩略图资源块，返回 JPEG 字节。
///
/// 结构：format(4) width(4) height(4) widthbytes(4) totalSize(4) compressedSize(4)
/// [bits(2) planes(2) —— 仅 1036]，随后是图像数据。format = 1 表示 JPEG。
fn parse_thumb_resource(data: &[u8], id: u16) -> Option<Vec<u8>> {
    if data.len() < 28 {
        return None;
    }
    let format = be_u32_at(data, 0);
    let w = be_u32_at(data, 4);
    let h = be_u32_at(data, 8);
    let compressed = be_u32_at(data, 20) as usize;
    if w == 0 || h == 0 {
        return None;
    }
    // 只要 JPEG。format = 0 是裸 RGB，虽然也能读，但极少见，交给合成图解码兜底。
    if format != 1 {
        return None;
    }
    let header_len: usize = if id == 1036 { 28 } else { 24 };
    let start: usize = header_len;
    let end = start.checked_add(compressed)?;
    if compressed == 0 || end > data.len() {
        return None;
    }
    Some(data[start..end].to_vec())
}

// ── 合成图解码 ──────────────────────────────────────

/// 把 PSD 的合成图解出来，长边不超过 `max_edge`。
///
/// 边解边降采样：源图的每一行解完就累加进目标小图，所以中间不存在「整张原图」
/// 这个对象 —— 这正是能处理 GB 级文件的原因。
///
/// 支持范围（覆盖了实际图库里 100% 的 PSD）：
///   * 位深 8 / 16
///   * 色彩模式 灰度 / RGB / CMYK
///   * 压缩方式 0（原始）/ 1（RLE）
///
/// 其它情况（32 位浮点、ZIP 压缩、索引色）返回 `Err`，由调用方退回内嵌缩略图。
pub fn render_composite(path: &Path, max_edge: u32) -> Result<RgbImage, String> {
    let file = File::open(path).map_err(|e| format!("打开 PSD 失败: {e}"))?;
    let mut r = BufReader::with_capacity(1 << 20, file);

    let head = read_header(&mut r)?;
    if head.depth != 8 && head.depth != 16 {
        return Err(format!("不支持的位深 {}", head.depth));
    }
    let base_planes: usize = match head.color_mode {
        1 => 1,  // 灰度
        3 => 3,  // RGB
        4 => 4,  // CMYK
        m => return Err(format!("不支持的色彩模式 {m}")),
    };

    // 色彩模式数据、资源段、图层段全部跳过，直接落到合成图数据。
    skip_section(&mut r, "色彩模式")?;
    skip_section(&mut r, "资源")?;
    skip_section(&mut r, "图层")?;

    let compression = be_u16(&mut r)?;
    if compression != 0 && compression != 1 {
        return Err(format!("不支持的压缩方式 {compression}"));
    }

    let (sw, sh) = (head.width as usize, head.height as usize);
    let total_channels = head.channels.max(1) as usize;
    // 我们最多要「基色通道 + 1 个 alpha」。多出来的（专色通道）直接不读。
    let want_planes = if total_channels > base_planes { base_planes + 1 } else { base_planes };
    if total_channels < base_planes {
        return Err(format!("通道数 {total_channels} 与色彩模式不符"));
    }

    let (tw, th) = target_size(sw, sh, max_edge);
    let (xmap, cx) = column_map(sw, tw);
    let cy = row_counts(sh, th);

    let bytes_per_sample = if head.depth == 16 { 2 } else { 1 };
    let row_bytes = sw * bytes_per_sample;
    let mut src_row = vec![0u8; row_bytes];
    let mut row = vec![0u8; sw];
    let mut acc = vec![0u32; tw * th];
    let mut planes: Vec<Vec<u8>> = Vec::with_capacity(want_planes);

    // RLE 的行长度表：通道数 × 行数，每项 u16。
    let rle_counts: Vec<u16> = if compression == 1 {
        let n = total_channels * sh;
        // 表本身就有 2n 字节；2 GB 级的表只可能来自畸形文件。
        if n > 64 * 1024 * 1024 {
            return Err("PSD RLE 长度表异常".into());
        }
        let mut raw = vec![0u8; n * 2];
        r.read_exact(&mut raw).map_err(|e| format!("读取 PSD 行长度表失败: {e}"))?;
        (0..n).map(|i| u16::from_be_bytes([raw[i * 2], raw[i * 2 + 1]])).collect()
    } else {
        Vec::new()
    };

    let mut packed = vec![0u8; row_bytes];
    for plane in 0..want_planes {
        acc.iter_mut().for_each(|v| *v = 0);

        for y in 0..sh {
            if compression == 1 {
                let len = *rle_counts
                    .get(plane * sh + y)
                    .ok_or("PSD 行长度表越界")? as usize;
                if len > packed.len() {
                    packed.resize(len, 0);
                }
                let dst = &mut packed[..len];
                r.read_exact(dst).map_err(|e| format!("读取 PSD 行数据失败: {e}"))?;
                unpack_bits(dst, &mut src_row, row_bytes);
            } else {
                r.read_exact(&mut src_row).map_err(|e| format!("读取 PSD 像素失败: {e}"))?;
            }

            // 16 位取样只取高字节 —— 8 位缩略图看不出区别，还能少一半搬运。
            if bytes_per_sample == 2 {
                for (i, v) in src_row.chunks_exact(2).enumerate() {
                    row[i] = v[0];
                }
            } else {
                row.copy_from_slice(&src_row[..sw]);
            }

            let ty = y * th / sh;
            let dst_row = &mut acc[ty * tw..(ty + 1) * tw];
            for (x, &v) in row.iter().enumerate() {
                dst_row[xmap[x] as usize] += v as u32;
            }
        }

        // 归一化：每个目标像素覆盖 cx × cy 个源像素。
        let mut out = Vec::with_capacity(tw * th);
        for ty in 0..th {
            for tx in 0..tw {
                let n = (cx[tx] * cy[ty]).max(1);
                let v = acc[ty * tw + tx] / n;
                out.push(v.min(255) as u8);
            }
        }
        planes.push(out);
    }

    Ok(compose(&planes, head.color_mode, tw, th, want_planes > base_planes))
}

/// 目标尺寸：等比缩到长边 ≤ `max_edge`，绝不放大。
fn target_size(w: usize, h: usize, max_edge: u32) -> (usize, usize) {
    let max_edge = max_edge.max(1) as usize;
    if w <= max_edge && h <= max_edge {
        return (w, h);
    }
    let scale = max_edge as f64 / w.max(h) as f64;
    (
        ((w as f64 * scale).round() as usize).max(1),
        ((h as f64 * scale).round() as usize).max(1),
    )
}

/// 源列 → 目标列映射，以及每个目标列占了多少源列。
fn column_map(sw: usize, tw: usize) -> (Vec<u32>, Vec<u32>) {
    let mut map = vec![0u32; sw];
    let mut counts = vec![0u32; tw];
    for (x, slot) in map.iter_mut().enumerate() {
        let tx = x * tw / sw;
        *slot = tx as u32;
        counts[tx] += 1;
    }
    (map, counts)
}

/// 每个目标行占了多少源行。
fn row_counts(sh: usize, th: usize) -> Vec<u32> {
    let mut counts = vec![0u32; th];
    for y in 0..sh {
        counts[y * th / sh] += 1;
    }
    counts
}

/// PackBits 解包（PSD 的 RLE 就是这套）。
///
/// 控制字节按有符号解释：
///   * `0..=127`  → 后面 `n+1` 个字节是字面量
///   * `-127..=-1` → 后面 1 个字节重复 `1-n` 次
///   * `-128`      → 空操作
fn unpack_bits(src: &[u8], dst: &mut [u8], expect: usize) {
    let mut i = 0usize;
    let mut o = 0usize;
    while i < src.len() && o < expect {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let count = n as usize + 1;
            let take = count.min(src.len() - i).min(expect - o);
            dst[o..o + take].copy_from_slice(&src[i..i + take]);
            i += take;
            o += take;
        } else if n != -128 {
            if i >= src.len() {
                break;
            }
            let count = (1 - n as i32) as usize;
            let v = src[i];
            i += 1;
            let take = count.min(expect - o);
            dst[o..o + take].fill(v);
            o += take;
        }
    }
    // 解包不足时补黑：宁可出一张偏暗的图，也别让整张缩略图失败。
    if o < expect {
        dst[o..expect].fill(0);
    }
}

/// 把各通道平面拼成 RGB。
fn compose(planes: &[Vec<u8>], color_mode: u16, tw: usize, th: usize, has_alpha: bool) -> RgbImage {
    let mut img = RgbImage::new(tw as u32, th as u32);
    let n = tw * th;
    let alpha = if has_alpha && planes.len() > 1 { planes.last() } else { None };
    // 有些导出工具把 alpha 全写成 0（本意是"不透明"），照字面合成会得到一张纯白图。
    let alpha_degenerate = alpha.map(|a| a.iter().all(|&v| v == 0)).unwrap_or(false);

    for i in 0..n {
        let a = match alpha {
            Some(pl) if !alpha_degenerate => pl[i] as u32,
            _ => 255,
        };
        let (r, g, b) = match color_mode {
            1 => {
                let v = planes[0][i];
                (v, v, v)
            }
            4 => {
                // 简易 CMYK → RGB：不做色彩管理，缩略图够用。
                let c = planes[0][i] as u32;
                let m = planes[1][i] as u32;
                let y = planes[2][i] as u32;
                let k = planes[3][i] as u32;
                let inv_k = 255 - k;
                (
                    (((255 - c) * inv_k) / 255) as u8,
                    (((255 - m) * inv_k) / 255) as u8,
                    (((255 - y) * inv_k) / 255) as u8,
                )
            }
            _ => (planes[0][i], planes[1][i], planes[2][i]),
        };

        // 透明区域合成到白底：网格底色是浅色，比透出黑色自然得多。
        let px = if a >= 255 {
            image::Rgb([r, g, b])
        } else {
            let blend = |c: u8| -> u8 {
                ((c as u32 * a + 255 * (255 - a)) / 255) as u8
            };
            image::Rgb([blend(r), blend(g), blend(b)])
        };
        img.put_pixel((i % tw) as u32, (i / tw) as u32, px);
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packbits_handles_literals_and_runs() {
        // 3 个字节的字面量，然后 4 个 0x7F
        let src = [2u8, 0xAA, 0xBB, 0xCC, 0xFD, 0x7F];
        let mut dst = [0u8; 7];
        unpack_bits(&src, &mut dst, 7);
        assert_eq!(dst, [0xAA, 0xBB, 0xCC, 0x7F, 0x7F, 0x7F, 0x7F]);
    }

    #[test]
    fn target_size_never_upscales() {
        assert_eq!(target_size(100, 80, 448), (100, 80));
        assert_eq!(target_size(896, 448, 448), (448, 224));
    }

    #[test]
    fn column_map_covers_every_source_column() {
        let (map, counts) = column_map(1000, 448);
        assert_eq!(map.len(), 1000);
        assert_eq!(counts.iter().sum::<u32>(), 1000);
        assert!(map.iter().all(|&c| (c as usize) < 448));
    }

    /// 手工验证用：`PIXVAULT_PSD_SAMPLE=<路径> cargo test renders_sample_psd -- --nocapture`
    ///
    /// 把真实 PSD 解出来写到临时目录，方便和 Photoshop / Pillow 的合成图逐像素比对。
    /// 没设环境变量时直接跳过，不会在 CI 上误跑。
    #[test]
    fn renders_sample_psd() {
        let Ok(sample) = std::env::var("PIXVAULT_PSD_SAMPLE") else { return };
        let started = std::time::Instant::now();
        let img = render_composite(Path::new(&sample), 448).expect("解码 PSD 失败");
        let out = std::env::temp_dir().join("pixvault-psd-check.png");
        img.save(&out).unwrap();
        println!(
            "{}×{} 用时 {:?} → {}",
            img.width(),
            img.height(),
            started.elapsed(),
            out.display()
        );
    }
}
