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

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::import::{
    find_original_file, folder_path_map, import_eagle, map_meta, EagleFolder, EagleItemMeta,
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

/// 在一个解开的包里收集所有可导入条目。
///
/// Library 布局直接复用 `import_eagle`（磁盘结构完全一样）；Pack 布局以
/// `pack.json` 的 `images` 数组为准 —— 它是包内唯一的完整清单，而 `<ID>.info/`
/// 目录里那份 metadata.json 是它的副本（缺目录时可以用副本兜底，见 `load_item_meta`）。
pub fn collect_items(root: &Path) -> Result<(Vec<ImportItem>, ParseReport), String> {
    let Some((layout, real)) = detect_layout(root) else {
        return Err(
            "这个包里既没有 pack.json，也没有 Eagle 库的 metadata.json + images 目录，无法识别".to_string(),
        );
    };

    match layout {
        Layout::Library => {
            let items = import_eagle(&real)?;
            let n = items.len();
            Ok((
                items,
                ParseReport { items: n, ..Default::default() },
            ))
        }
        Layout::Pack => {
            let raw = std::fs::read_to_string(real.join("pack.json"))
                .map_err(|e| format!("读取 pack.json 失败: {}", e))?;
            let header: PackHeader = serde_json::from_str(&raw)
                .map_err(|e| format!("解析 pack.json 失败: {}", e))?;

            let folder_paths = folder_path_map(&header.folders);
            let mut report = ParseReport::default();
            let mut items = Vec::new();

            for meta in &header.images {
                if meta.is_deleted {
                    report.skipped_deleted += 1;
                    continue;
                }
                let Some(id) = meta.id.as_deref().filter(|s| !s.is_empty()) else {
                    report.skipped_missing += 1;
                    continue;
                };
                let Some(dir) = item_dir(&real, id) else {
                    report.skipped_missing += 1;
                    continue;
                };
                let Some(original) =
                    find_original_file(&dir, meta.name.as_deref(), meta.ext.as_deref())
                else {
                    // 没有原文件的条目是 Eagle 的网页书签，只有缩略图，跳过
                    report.skipped_bookmark += 1;
                    continue;
                };
                let (tags, notes, rating, source) = map_meta(meta, &folder_paths);
                items.push(ImportItem { path: original, tags, notes, rating, source });
            }

            report.items = items.len();
            Ok((items, report))
        }
    }
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
