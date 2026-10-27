//! 免 GUI 检查侧栏「文件夹」树的折叠规则。
//!
//! 拿一份真实的库跑一遍，确认 Eagle 的 `<条目ID>.info` 壳目录没有出现在树里。
//!
//! ```text
//! cargo run --release --example foldertree -- <库根> <db 路径>
//! ```
//!
//! 建议传 db 的**副本**，别直接开正在用的库。

use pixvault_lib::engine::db::{Database, FolderNode};
use std::path::Path;

fn print(node: &FolderNode, depth: usize) {
    println!("{}{}  [{}]", "  ".repeat(depth), node.name, node.file_count);
    for c in &node.children {
        print(c, depth + 1);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = args.get(1).expect("缺少库根参数");
    let db_path = args.get(2).expect("缺少 db 路径参数");

    let db = Database::open(Path::new(db_path)).expect("打开库失败");
    let tree = db.get_folder_tree(root).expect("取文件夹树失败");

    println!("库根: {}", root);
    print(&tree, 0);

    let mut ghosts = Vec::new();
    fn walk(n: &FolderNode, ghosts: &mut Vec<String>) {
        if n.name.to_ascii_lowercase().ends_with(".info") {
            ghosts.push(n.name.clone());
        }
        for c in &n.children {
            walk(c, ghosts);
        }
    }
    walk(&tree, &mut ghosts);

    if ghosts.is_empty() {
        println!("\n✅ 没有 .info 壳目录漏进树里");
    } else {
        println!("\n❌ 仍有 {} 个壳目录: {:?}", ghosts.len(), ghosts);
        std::process::exit(1);
    }
}
