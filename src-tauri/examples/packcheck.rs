//! 用真实样本包验证 eaglepack 的解包、重组与解析，不用开 GUI。
//!
//! ```bash
//! cargo run --release --example packcheck -- <包路径> <目标目录>
//! ```
//!
//! 它会打印 probe 统计、解包结果、重组结果与最终落盘路径，并做三条断言：
//! ① zip slip 的越界文件没写到目标目录外 ② 目标目录里不再有 `.info` 壳
//! ③ 每个条目的路径都落在目标目录内。

use std::path::PathBuf;

use pixvault_lib::engine::eaglepack;

fn main() {
    let mut args = std::env::args().skip(1);
    let pack = PathBuf::from(args.next().expect("用法: packcheck <包路径> <目标目录>"));
    let dest = PathBuf::from(args.next().expect("用法: packcheck <包路径> <目标目录>"));

    let probe = eaglepack::probe(&pack).expect("probe 失败");
    println!(
        "probe: layout={} 条目={} 文件夹={} zip条目={} 解压后={} 字节",
        probe.layout, probe.item_count, probe.folder_count, probe.entry_count, probe.unpacked_bytes
    );

    let report = eaglepack::unpack(&pack, &dest, |_p| true).expect("unpack 失败");
    println!(
        "unpack: 写出 {} 个文件 / {} 字节，跳过 {} 个条目",
        report.files, report.bytes, report.skipped
    );

    let (items, rep, flat) = eaglepack::import_pack(&dest, |_p| {}).expect("重组/解析失败");
    println!(
        "import_pack: {} 个可导入条目（跳过 回收站 {} / 书签 {} / 缺文件 {}）",
        items.len(),
        rep.skipped_deleted,
        rep.skipped_bookmark,
        rep.skipped_missing
    );
    println!(
        "  重组: 搬 {} 个 / 重名改名 {} 个 / 失败 {} 个 / 清掉 {} 个目录",
        flat.moved, flat.renamed, flat.failed, flat.removed_dirs
    );

    for it in &items {
        let rel = it.path.strip_prefix(&dest).unwrap_or(&it.path);
        println!(
            "  {}\n    标签={:?} 评分={:?} 备注={:?} 来源={:?}",
            rel.display(),
            it.tags,
            it.rating,
            it.notes,
            it.source
        );
    }

    // ① zip slip：`../evil.txt` 绝不能出现在目标目录的上级
    let evil = dest
        .parent()
        .map(|p| p.join("evil.txt"))
        .unwrap_or_else(|| PathBuf::from("evil.txt"));
    println!(
        "① zip slip 越界文件落盘了没有: {}",
        if evil.exists() { "有！出事了" } else { "没有（正确）" }
    );

    // ② 壳目录必须清干净，否则「软件里怎么显示、硬盘上就怎么放」这条没做到
    let leftovers = find_item_dirs(&dest);
    println!(
        "② 残留的 .info 壳目录: {}",
        if leftovers.is_empty() {
            "没有（正确）".to_string()
        } else {
            format!("{} 个 —— {:?}", leftovers.len(), leftovers)
        }
    );

    // ③ 每条记录都得指向目标目录里面
    let outside = items.iter().filter(|it| !it.path.starts_with(&dest)).count();
    println!(
        "③ 落在目标目录外的条目: {}",
        if outside == 0 { "0（正确）".to_string() } else { format!("{} 个！", outside) }
    );
}

/// 列出目录树里所有以 `.info` 结尾的子目录。
fn find_item_dirs(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name.ends_with(".info") {
                out.push(e.path());
            } else {
                stack.push(e.path());
            }
        }
    }
    out
}
