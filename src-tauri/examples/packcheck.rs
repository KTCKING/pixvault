//! 用真实样本包验证 eaglepack 的解包与解析，不用开 GUI。
//!
//! ```bash
//! cargo run --release --example packcheck -- <包路径> <目标目录>
//! ```
//!
//! 它会打印 probe 统计、解包结果、解析出的条目，并检查 zip slip 的越界文件
//! 有没有被写到目标目录外面 —— 这条最要紧，写出去就是安全事故。

use std::path::PathBuf;

use pixvault_lib::engine::eaglepack;

fn main() {
    let mut args = std::env::args().skip(1);
    let pack = PathBuf::from(args.next().expect("用法: packcheck <包路径> <目标目录>"));
    let dest = PathBuf::from(args.next().expect("用法: packcheck <包路径> <目标目录>"));

    let probe = eaglepack::probe(&pack).expect("probe 失败");
    println!(
        "probe: layout={} 条目={} 文件夹={} zip条目={} 解压后={} 字节",
        probe.layout,
        probe.item_count,
        probe.folder_count,
        probe.entry_count,
        probe.unpacked_bytes
    );

    let report = eaglepack::unpack(&pack, &dest, |_p| true).expect("unpack 失败");
    println!(
        "unpack: 写出 {} 个文件 / {} 字节，跳过 {} 个条目",
        report.files, report.bytes, report.skipped
    );

    let (items, rep) = eaglepack::collect_items(&dest).expect("collect 失败");
    println!(
        "collect: {} 个可导入条目（跳过 回收站 {} / 书签 {} / 缺文件 {}）",
        items.len(),
        rep.skipped_deleted,
        rep.skipped_bookmark,
        rep.skipped_missing
    );
    for it in &items {
        println!(
            "  {}\n    标签={:?} 评分={:?} 备注={:?} 来源={:?}",
            it.path.display(),
            it.tags,
            it.rating,
            it.notes,
            it.source
        );
    }

    // zip slip 断言：`../evil.txt` 绝不能出现在目标目录的上级
    let evil = dest
        .parent()
        .map(|p| p.join("evil.txt"))
        .unwrap_or_else(|| PathBuf::from("evil.txt"));
    println!("zip slip 越界文件落盘了没有: {}", if evil.exists() { "有！出事了" } else { "没有（正确）" });
}
