//! eaglepack（Eagle 素材包）的解包与解析。
//!
//! `.eaglepack` 就是一个 zip —— 官方支持文档给的自救办法就是「把后缀改成 .zip
//! 打开」，所以这里不需要什么私有解析器，只需要把 zip 读对、把两种内部布局都认下来。
//!
//! **Pack 布局**（Eagle 自己导出的素材包、社区下载的免费包）
//! ```text
//! pack.json                  { "images": [ <条目元数据>… ] }
//! <ITEM_ID>.info/
//!   ├ metadata.json          同一份元数据（重复存一份，单独解包也能用）
//!   ├ photo.jpg              原始素材
//!   └ photo_thumbnail.png    可选缩略图
//! ```
//!
//! **Library 布局**（把整个 `.library` 目录压成 zip，外面有时还套一层文件夹）
//! ```text
//! metadata.json              库级：文件夹树
//! mtime.json
//! images/<ITEM_ID>.info/…
//! ```
//!
//! 所以「解包」和「认布局」是两件独立的事：先把 zip 流式解到目标目录（不落临时
//! 文件、逐条目回报进度、随时可取消），再按内容判断布局，交给 import 模块解析。
//! 解压出来的目录**直接就是新图库的根**（与现有 Eagle / Pixcall 导入语义一致，
//! 零复制），所以必须解到用户选的持久目录，不能解到 temp。

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::import::{
    find_original_file, folder_path_map, map_meta, EagleFolder, EagleItemMeta, EagleRoot,
    ImportItem,
};

/// 取消时返回的错误串。前端据此区分「用户点了取消」和「真的失败」。
pub const CANCELED: &str = "__canceled__";

/// 解包 / 解析进度，前端 `eaglepack-progress` 事件的载荷。
#[derive(Debug, Clone, Serialize)]
pub struct PackProgress {
    /// `unpack` 解压中 / `parse` 解析中
    pub phase: &'static str,
    pub done: u64,
    pub total: u64,
    /// 当前条目名，仅用于展示
    pub current: String,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct UnpackReport {
    /// 实际写出的文件数
    pub files: usize,
    /// 实际写出的字节数
    pub bytes: u64,
    /// 跳过的条目（目录项、macOS 垃圾、越界路径）
    pub skipped: usize,
}

/// 开包前的统计。用来在真正解压前告诉用户「这包多大、多少东西」，
/// 也用于磁盘空间预检 —— 不要解到一半才发现没空间。
#[derive(Debug, Default, Clone, Serialize)]
pub struct PackProbe {
    /// `pack` / `library` / `unknown`
    pub layout: String,
    pub item_count: usize,
    pub folder_count: usize,
    pub entry_count: usize,
    /// 解压后总大小（中央目录里记的未压缩大小之和）
    pub unpacked_bytes: u64,
}

/// 解析结果统计。
#[derive(Debug, Default, Clone, Serialize)]
pub struct ParseReport {
    pub items: usize,
    /// 回收站里被跳过的
    pub skipped_deleted: usize,
    /// 只有网页链接、没有原文件的条目（Eagle 的书签）
    pub skipped_bookmark: usize,
    /// 元数据在但找不到原图
    pub skipped_missing: usize,
}

// ───────────────────────── zip 层 ─────────────────────────

fn open_zip(pack: &Path) -> Result<zip::ZipArchive<File>, String> {
    let file = File::open(pack).map_err(|e| format!("打开素材包失败: {}", e))?;
    zip::ZipArchive::new(file).map_err(|e| format!("不是有效的 zip 素材包（{}）", e))
}

/// 判断某个 zip 条目该不该解出来。
fn should_skip_entry(name: &str) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name);
    name.starts_with("__MACOSX/")
        || base == ".DS_Store"
        || base == "Thumbs.db"
        || base == "desktop.ini"
        || name.is_empty()
}

/// 读中央目录做统计，不解任何数据。
pub fn probe(pack: &Path) -> Result<PackProbe, String> {
    let mut zip = open_zip(pack)?;
    let n = zip.len();

    let mut out = PackProbe::default();
    let mut pack_json_idx: Option<usize> = None;
    let mut library_header_idx: Option<usize> = None;
    let mut has_images_dir = false;
    // Library 布局的库头里没有条目清单（元数据分散在 `images/*.info/metadata.json`），
    // 但中央目录里有完整路径，按 `<…>/images/<ID>.info` 这个前缀去重计数就能拿到
    // 准确条目数 —— 零解压，选包时前端就能显示出真实数字。
    let mut item_dirs: HashSet<String> = HashSet::new();

    for i in 0..n {
        let Ok(entry) = zip.by_index(i) else { continue };
        let name = entry.name().to_string();
        let is_dir = entry.is_dir();
        let size = entry.size();
        let depth = name.matches('/').count();
        let base = name.rsplit('/').next().unwrap_or(&name);

        if !is_dir {
            out.entry_count += 1;
            out.unpacked_bytes += size;
        }
        // 只看根或「根下一层」，避免把某个人素材目录里的同名文件当库头
        if depth <= 1 && base == "pack.json" {
            pack_json_idx = Some(i);
        }
        if depth <= 1 && base == "metadata.json" {
            library_header_idx = Some(i);
        }
        // 注意别去找「有没有 images/ 这个目录条目」——打包工具（包括 python 的
        // zipfile）通常只写文件、不写目录条目，那样判断会整个失效，把明明是
        // Library 布局的包报成 unknown。只能从路径段里看。
        let segs: Vec<&str> = name.split('/').collect();
        if let Some(pos) = segs.iter().position(|s| *s == "images") {
            has_images_dir = true;
            if let Some(next) = segs.get(pos + 1) {
                if next.ends_with(".info") {
                    item_dirs.insert(segs[..=pos + 1].join("/"));
                }
            }
        }
    }

    if let Some(idx) = pack_json_idx {
        out.layout = "pack".to_string();
        if let Ok(mut entry) = zip.by_index(idx) {
            let mut s = String::new();
            if entry.read_to_string(&mut s).is_ok() {
                if let Ok(h) = serde_json::from_str::<PackHeader>(&s) {
                    out.item_count = h.images.len();
                    out.folder_count = count_folders(&h.folders);
                }
            }
        }
    } else if library_header_idx.is_some() && has_images_dir {
        out.layout = "library".to_string();
        out.item_count = item_dirs.len();
        if let Some(idx) = library_header_idx {
            if let Ok(mut entry) = zip.by_index(idx) {
                let mut s = String::new();
                if entry.read_to_string(&mut s).is_ok() {
                    if let Ok(h) = serde_json::from_str::<super::import::EagleRoot>(&s) {
                        out.folder_count = count_folders(&h.folders);
                    }
                }
            }
        }
    } else {
        out.layout = "unknown".to_string();
    }

    Ok(out)
}

fn count_folders(folders: &[EagleFolder]) -> usize {
    folders
        .iter()
        .map(|f| 1 + count_folders(&f.children))
        .sum()
}

/// 流式解压到 `dest`。
///
/// `on_progress` 每处理一批条目调一次，返回 `false` 表示用户取消 —— 此时立刻
/// 中止并返回 `CANCELED`，调用方负责把半成品目录删掉。
///
/// 三条安全线：
/// 1. **zip slip**：只信 `enclosed_name()` 的返回值（它已经清掉 `..`、绝对路径
///    和盘符），拿不到就直接跳过这个条目，绝不用原始 name 拼路径；
/// 2. **不落临时文件**：边读边写目标文件，几百 MB 的包不占双份磁盘；
/// 3. **跳过垃圾**：`__MACOSX/`、`.DS_Store`、`Thumbs.db` 不写。
pub fn unpack<F>(pack: &Path, dest: &Path, mut on_progress: F) -> Result<UnpackReport, String>
where
    F: FnMut(&PackProgress) -> bool,
{
    let mut zip = open_zip(pack)?;
    let total = zip.len() as u64;
    std::fs::create_dir_all(dest).map_err(|e| format!("创建目标目录失败: {}", e))?;

    let mut report = UnpackReport::default();
    let mut done: u64 = 0;

    for i in 0..total as usize {
        let mut entry = match zip.by_index(i) {
            Ok(e) => e,
            Err(e) => {
                report.skipped += 1;
                done += 1;
                tracing::warn!("eaglepack 第 {} 个条目读取失败: {}", i + 1, e);
                continue;
            }
        };

        let name = entry.name().to_string();

        let Some(rel) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            // 越界路径（zip slip），直接丢弃
            report.skipped += 1;
            done += 1;
            continue;
        };

        if should_skip_entry(&name) {
            report.skipped += 1;
            done += 1;
            continue;
        }

        let out_path: PathBuf = dest.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)
                .map_err(|e| format!("创建目录 {} 失败: {}", out_path.display(), e))?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("创建目录 {} 失败: {}", parent.display(), e))?;
            }
            let mut f = File::create(&out_path)
                .map_err(|e| format!("写入 {} 失败: {}", out_path.display(), e))?;
            let written = io::copy(&mut entry, &mut f)
                .map_err(|e| format!("写入 {} 失败: {}", out_path.display(), e))?;
            report.files += 1;
            report.bytes += written;
        }

        done += 1;
        // 每 16 个回报一次（几万个条目也不会把事件队列刷爆），最后一个必报
        if done % 16 == 0 || done == total {
            if !on_progress(&PackProgress {
                phase: "unpack",
                done,
                total,
                current: name,
            }) {
                return Err(CANCELED.to_string());
            }
        }
    }

    on_progress(&PackProgress {
        phase: "unpack",
        done: total,
        total,
        current: String::new(),
    });
    Ok(report)
}

// ───────────────────────── 布局识别与解析 ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// `pack.json` + 根下的 `<ID>.info/`
    Pack,
    /// `metadata.json` + `images/<ID>.info/`
    Library,
}

#[derive(Debug, Deserialize)]
struct PackHeader {
    #[serde(default)]
    images: Vec<EagleItemMeta>,
    #[serde(default)]
    folders: Vec<EagleFolder>,
    /// 包名。当前用目录名当库名，这个字段留给「以后让用户改库名」用。
    #[serde(default)]
    #[allow(dead_code)]
    name: Option<String>,
}

fn has_marker(dir: &Path) -> bool {
    dir.join("pack.json").is_file()
        || (dir.join("metadata.json").is_file() && dir.join("images").is_dir())
}

/// 找真正的根：包外面可能套了一层文件夹（「新建文件夹/xxx.eaglepack」解压出来
/// 就是这样）。只在「根下恰好一个子目录」时才下潜一层，避免误进素材目录。
fn locate_root(root: &Path) -> PathBuf {
    if has_marker(root) {
        return root.to_path_buf();
    }
    if let Ok(rd) = std::fs::read_dir(root) {
        let dirs: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .filter(|p| {
                // 我们自己的元数据目录不算
                p.file_name().map(|n| n != ".pixvault").unwrap_or(true)
            })
            .collect();
        if dirs.len() == 1 && has_marker(&dirs[0]) {
            return dirs[0].clone();
        }
    }
    root.to_path_buf()
}

pub fn detect_layout(root: &Path) -> Option<(Layout, PathBuf)> {
    let real = locate_root(root);
    if real.join("pack.json").is_file() {
        return Some((Layout::Pack, real));
    }
    if real.join("metadata.json").is_file() && real.join("images").is_dir() {
        return Some((Layout::Library, real));
    }
    None
}

// ───────────────────────── 落盘重组 ─────────────────────────
//
// Eagle 的文件夹是**虚拟**的：磁盘上所有素材扁平地躺在 `<ID>.info/` 里，归属关系
// 只记在条目自己的 `folders: [id]`。所以在 Eagle 里看起来是 `素材/图标/a.png`，
// 硬盘上却是 `MVQ55RU1NL8GV.info/photo.png`。
//
// 素材包是我们自己解出来的副本（源包不动），所以这里可以真的重组：把文件按
// Eagle 里的文件夹层级搬到 `素材/图标/a.png`，然后把 `<ID>.info/` 壳整个删掉。
// 结果是「软件里怎么显示，硬盘上就怎么放」。
//
// 注意**只有素材包这么做**。原地读取的 Eagle 库（Import Eagle Library）不能动 ——
// 那是用户已有的库，重组之后 Eagle 自己就打不开了。

/// 重组结果。
#[derive(Debug, Default, Clone, Serialize)]
pub struct FlattenReport {
    /// 成功按文件夹层级落盘的文件数
    pub moved: usize,
    /// 因重名被自动加上 `(2)`/`(3)` 的文件数
    pub renamed: usize,
    /// 搬运失败的文件数（仍按原路径入库，图不会丢）
    pub failed: usize,
    /// 清掉的目录数：`<ID>.info` 壳、`images/`、包外多套的那层文件夹
    pub removed_dirs: usize,
}

/// 一条还没落盘的素材。
struct RawItem {
    /// 解包后它现在躺在哪
    file: PathBuf,
    /// Eagle 声明的文件夹路径（`父/子`），`None` = 库根
    folder: Option<String>,
    /// 期望的文件名（含扩展名，尚未清洗）
    name: String,
    tags: Vec<String>,
    notes: Option<String>,
    rating: Option<i32>,
    source: Option<String>,
}

enum Skip {
    /// 回收站里的条目，包内仍保留
    Deleted,
    /// 只有网页链接和缩略图，没有原文件（Eagle 的书签）
    Bookmark,
    /// 元数据在但找不到原图，或者解析不了
    Missing,
}

/// 解析 + 重组 + 清理，一步到位。
///
/// 返回的 `ImportItem.path` 已经是**重组之后**的位置，直接拿去写库即可；
/// 调用方不需要再扫一遍目录。
pub fn import_pack<F>(
    root: &Path,
    mut on_progress: F,
) -> Result<(Vec<ImportItem>, ParseReport, FlattenReport), String>
where
    F: FnMut(&PackProgress),
{
    let (real, raw, parse_report) = parse_raw(root)?;
    let total = raw.len() as u64;

    // 包外常套一层文件夹（`旧库.library/…`）。先把 Eagle 自己的记账文件提到库根，
    // 那层壳才能跟着一起清掉，侧栏不会剩一个只装着 metadata.json 的目录。
    lift_metadata_files(&real, root);

    let mut out: Vec<ImportItem> = Vec::with_capacity(raw.len());
    let mut report = FlattenReport::default();
    let mut used: HashSet<String> = HashSet::new();
    // 我们自己建的文件夹目录，清理阶段不能当成素材壳删掉
    let mut our_dirs: HashSet<PathBuf> = HashSet::new();

    for (i, item) in raw.into_iter().enumerate() {
        let dir = folder_segments(item.folder.as_deref())
            .iter()
            .fold(root.to_path_buf(), |acc, seg| acc.join(seg));

        // Windows 单条路径上限 260 字符，留出库根与文件名的余量后按剩余预算截断
        let budget = 240usize.saturating_sub(dir.to_string_lossy().chars().count() + 1);
        let name = clip_name(&item.name, budget.clamp(24, 120));

        let (target, renamed) = unique_target(&dir, &name, &mut used);
        if renamed {
            report.renamed += 1;
        }

        let ok = std::fs::create_dir_all(&dir).is_ok() && move_file(&item.file, &target).is_ok();
        our_dirs.insert(dir);

        // 搬运失败就按原路径入库：图还在，只是位置没变漂亮
        let (path, failed) = if ok {
            report.moved += 1;
            (target, false)
        } else {
            report.failed += 1;
            (item.file, true)
        };
        if failed {
            tracing::warn!("eaglepack 重组失败，保留原路径: {}", path.display());
        }

        out.push(ImportItem {
            path,
            tags: item.tags,
            notes: item.notes,
            rating: item.rating,
            source: item.source,
        });

        let done = i as u64 + 1;
        if done % 64 == 0 || done == total {
            on_progress(&PackProgress {
                phase: "organize",
                done,
                total,
                current: String::new(),
            });
        }
    }

    report.removed_dirs = purge_item_dirs(root, &our_dirs) + remove_empty_dirs(root);
    Ok((out, parse_report, report))
}

/// 按布局解析出「待落盘清单」。两条布局的差别只在元数据从哪来。
fn parse_raw(root: &Path) -> Result<(PathBuf, Vec<RawItem>, ParseReport), String> {
    let Some((layout, real)) = detect_layout(root) else {
        return Err(
            "这个包里既没有 pack.json，也没有 Eagle 库的 metadata.json + images 目录，无法识别"
                .to_string(),
        );
    };

    match layout {
        // 包内唯一的完整清单是 pack.json 的 images 数组；`<ID>.info/metadata.json`
        // 是它的副本，只在缺目录时兜底
        Layout::Pack => {
            let raw = std::fs::read_to_string(real.join("pack.json"))
                .map_err(|e| format!("读取 pack.json 失败: {}", e))?;
            let header: PackHeader =
                serde_json::from_str(&raw).map_err(|e| format!("解析 pack.json 失败: {}", e))?;

            let folder_paths = folder_path_map(&header.folders);
            let mut report = ParseReport::default();
            let mut items = Vec::new();

            for meta in &header.images {
                let dir = meta
                    .id
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .and_then(|id| item_dir(&real, id));
                let Some(dir) = dir else {
                    report.skipped_missing += 1;
                    continue;
                };
                match raw_from_meta(meta, &dir, &folder_paths) {
                    Ok(item) => items.push(item),
                    Err(Skip::Deleted) => report.skipped_deleted += 1,
                    Err(Skip::Bookmark) => report.skipped_bookmark += 1,
                    Err(Skip::Missing) => report.skipped_missing += 1,
                }
            }

            report.items = items.len();
            Ok((real, items, report))
        }
        // 库布局：元数据分散在 `images/<ID>.info/metadata.json`，库头只有文件夹树
        Layout::Library => {
            let raw = std::fs::read_to_string(real.join("metadata.json"))
                .map_err(|e| format!("读取 metadata.json 失败: {}", e))?;
            let header: EagleRoot = serde_json::from_str(&raw)
                .map_err(|e| format!("解析 Eagle metadata.json 失败: {}", e))?;

            let folder_paths = folder_path_map(&header.folders);
            let mut report = ParseReport::default();
            let mut items = Vec::new();

            let images = real.join("images");
            for entry in std::fs::read_dir(&images)
                .map_err(|e| format!("读取 images 目录失败: {}", e))?
                .flatten()
            {
                if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let dir = entry.path();
                let Ok(json) = std::fs::read_to_string(dir.join("metadata.json")) else {
                    report.skipped_missing += 1;
                    continue;
                };
                let Ok(meta) = serde_json::from_str::<EagleItemMeta>(&json) else {
                    report.skipped_missing += 1;
                    continue;
                };
                match raw_from_meta(&meta, &dir, &folder_paths) {
                    Ok(item) => items.push(item),
                    Err(Skip::Deleted) => report.skipped_deleted += 1,
                    Err(Skip::Bookmark) => report.skipped_bookmark += 1,
                    Err(Skip::Missing) => report.skipped_missing += 1,
                }
            }

            report.items = items.len();
            Ok((real, items, report))
        }
    }
}

fn raw_from_meta(
    meta: &EagleItemMeta,
    dir: &Path,
    folder_paths: &HashMap<String, String>,
) -> Result<RawItem, Skip> {
    if meta.is_deleted {
        return Err(Skip::Deleted);
    }
    let Some(file) = find_original_file(dir, meta.name.as_deref(), meta.ext.as_deref()) else {
        // 带链接没原文件 = Eagle 的书签条目；连链接都没有就是包本身不完整
        let has_url = meta.url.as_deref().map(|u| !u.trim().is_empty()).unwrap_or(false);
        return Err(if has_url { Skip::Bookmark } else { Skip::Missing });
    };
    let (tags, notes, rating, source) = map_meta(meta, folder_paths);
    Ok(RawItem {
        name: target_file_name(meta, &file),
        folder: primary_folder(meta, folder_paths),
        file,
        tags,
        notes,
        rating,
        source,
    })
}

/// Eagle 允许一条素材同时属于多个文件夹，但磁盘上一个文件只能落在一个位置：
/// 取第一个还在树里的文件夹。其余归属关系照样全部进标签（`map_meta` 是全量追加的），
/// 所以「这张图也属于另一个文件夹」这条信息没丢。
fn primary_folder(
    meta: &EagleItemMeta,
    folder_paths: &HashMap<String, String>,
) -> Option<String> {
    meta.folders
        .iter()
        .filter_map(|id| folder_paths.get(id))
        .find(|p| !p.trim().is_empty())
        .cloned()
}

/// 目标文件名：优先用 Eagle 记的 `name` + `ext`，退回磁盘上的文件名。
///
/// **名字必须清洗**：素材名来自用户的原始文件，`设计稿：终稿.png` 里那个全角冒号
/// 没问题，但 ASCII 的 `:` `*` `?` 在 Windows 上根本建不出来，不清洗就整个搬运失败。
fn target_file_name(meta: &EagleItemMeta, file: &Path) -> String {
    let disk_ext = file.extension().map(|e| e.to_string_lossy().to_string());
    let ext = meta
        .ext
        .clone()
        .filter(|e| !e.trim().is_empty())
        .or(disk_ext)
        .map(|e| e.trim_start_matches('.').to_string())
        .filter(|e| !e.is_empty());
    let stem = meta
        .name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| file.file_stem().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_else(|| "未命名".to_string());

    match ext {
        // name 里偶尔已经带了扩展名（第三方工具导出的包会这样），别拼成 a.jpg.jpg
        Some(e) if stem.to_lowercase().ends_with(&format!(".{}", e.to_lowercase())) => {
            let (s, _) = split_name(&stem);
            format!("{}.{}", sanitize_segment(&s), e)
        }
        Some(e) => format!("{}.{}", sanitize_segment(&stem), e),
        None => sanitize_segment(&stem),
    }
}

/// `父/子` 拆成清洗过的目录名。空段丢掉，别造出空目录。
fn folder_segments(folder: Option<&str>) -> Vec<String> {
    match folder {
        None => Vec::new(),
        Some(p) => p
            .split('/')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(sanitize_segment)
            .collect(),
    }
}

/// 清洗成 Windows / macOS / Linux 都能落盘的名字。
///
/// Windows 的规矩最多：`< > : " / \ | ? *` 与 0x00-0x1F 非法，名字不能以点或
/// 空格结尾，`CON`/`PRN`/`COM1` 这类保留名建不出来。素材名字来自用户文件，
/// 中文素材包里有 `设计稿：终稿.png` 这种很常见，不清洗就会整个导入失败。
fn sanitize_segment(raw: &str) -> String {
    const BAD: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    let s: String = raw
        .chars()
        .map(|c| {
            if BAD.contains(&c) || (c as u32) < 0x20 {
                '_'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim_matches(|c: char| c.is_whitespace() || c == '.');
    if s.is_empty() {
        return "未命名".to_string();
    }
    let mut out = s.to_string();
    if is_reserved_name(&out) {
        out.insert(0, '_');
    }
    out
}

fn is_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim().to_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit())
}

/// `name.ext` → (stem, ".ext")。没有扩展名时后半段是空串。
fn split_name(name: &str) -> (String, String) {
    match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.is_empty() && e.chars().count() <= 12 => {
            (s.to_string(), format!(".{}", e))
        }
        _ => (name.to_string(), String::new()),
    }
}

/// 超长名字截断，但**保住扩展名** —— 否则图片会变成没后缀的文件，索引器认不出来。
fn clip_name(name: &str, max_chars: usize) -> String {
    if name.chars().count() <= max_chars {
        return name.to_string();
    }
    let (stem, ext) = split_name(name);
    let keep = max_chars.saturating_sub(ext.chars().count());
    let clipped: String = stem.chars().take(keep).collect();
    let clipped = clipped.trim_end_matches(['.', ' ']);
    format!("{}{}", clipped, ext)
}

/// 同目录同名时加序号：`a.png` → `a (2).png`。Eagle 里重名很常见
/// （不同素材可以叫同一个名字），直接搬过去会覆盖掉一张。
fn unique_target(dir: &Path, name: &str, used: &mut HashSet<String>) -> (PathBuf, bool) {
    let (stem, ext) = split_name(name);
    for n in 1..=9999usize {
        let candidate = if n == 1 {
            format!("{}{}", stem, ext)
        } else {
            format!("{} ({}){}", stem, n, ext)
        };
        let path = dir.join(&candidate);
        // 大小写不敏感地判重：Windows 上 a.png 和 A.png 是同一个文件
        let key = path.to_string_lossy().to_lowercase();
        if used.contains(&key) || path.exists() {
            continue;
        }
        used.insert(key);
        return (path, n > 1);
    }
    let fallback = dir.join(format!("{}-{}{}", stem, used.len(), ext));
    used.insert(fallback.to_string_lossy().to_lowercase());
    (fallback, true)
}

/// 搬文件。同一卷上 `rename` 是原子的、不复制数据；跨卷（目标在别的盘）或被杀软
/// 短暂占用时退回「复制 + 删除」。
fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    std::fs::remove_file(from)
}

/// 把 Eagle 的记账文件提到库根，这样包外那层包装目录才能清掉。
/// 这些文件不是图片，不会被索引，也不会出现在网格里。
fn lift_metadata_files(real: &Path, dest: &Path) {
    if real == dest {
        return;
    }
    for name in ["metadata.json", "mtime.json", "pack.json", "tags.json"] {
        let src = real.join(name);
        if !src.is_file() {
            continue;
        }
        let target = dest.join(name);
        if target.exists() {
            continue;
        }
        let _ = std::fs::rename(&src, &target);
    }
}

/// 删掉 `<ID>.info` 素材壳（搬完只剩缩略图和元数据副本，内容已进库）。
///
/// 只删名字以 `.info` 结尾的目录，且跳过我们自己建的文件夹目录 —— 万一真有人
/// 把 Eagle 文件夹起名叫 `设计稿.info`，不至于连图和目录一起删掉。
fn purge_item_dirs(root: &Path, our_dirs: &HashSet<PathBuf>) -> usize {
    fn walk(dir: &Path, ours: &HashSet<PathBuf>, count: &mut usize) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let children: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        for p in children {
            if !p.is_dir() {
                continue;
            }
            if ours.contains(&p) {
                // 我们建的目录：继续往里走，但自己不删
                walk(&p, ours, count);
                continue;
            }
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if name.ends_with(".info") {
                if std::fs::remove_dir_all(&p).is_ok() {
                    *count += 1;
                }
            } else {
                walk(&p, ours, count);
            }
        }
    }
    let mut count = 0;
    walk(root, our_dirs, &mut count);
    count
}

/// 自底向上删空目录（`images/`、包外包装目录都会在这一步消失）。
fn remove_empty_dirs(root: &Path) -> usize {
    fn walk(dir: &Path, count: &mut usize) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let children: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        for p in children {
            if !p.is_dir() {
                continue;
            }
            walk(&p, count);
            let empty = std::fs::read_dir(&p)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false);
            if empty && std::fs::remove_dir(&p).is_ok() {
                *count += 1;
            }
        }
    }
    let mut count = 0;
    walk(root, &mut count);
    count
}

/// 条目目录可能落在三个位置，都见过：zip 根、`info/` 下、`images/` 下。
fn item_dir(root: &Path, id: &str) -> Option<PathBuf> {
    let name = format!("{}.info", id);
    for candidate in [
        root.join(&name),
        root.join("info").join(&name),
        root.join("images").join(&name),
    ] {
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}
