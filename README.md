# PixVault

> 资源库式图片管理软件 —— 以 **Tauri 2 + Rust + SolidJS** 构建，采用 Pixcall 式的资源库（Library）逻辑：
> 多库管理、智能视图、文件夹树、标签树、评分 / 颜色标记 / 收藏，元数据全部存在本地标准 SQLite 里，无锁定、可迁移。

[![Build Installers](https://github.com/KTCKING/pixvault/actions/workflows/build-installers.yml/badge.svg)](https://github.com/KTCKING/pixvault/actions/workflows/build-installers.yml)
[![Latest Release](https://img.shields.io/github/v/release/KTCKING/pixvault?color=blue)](https://github.com/KTCKING/pixvault/releases/latest)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](#许可证)

---

## 下载安装

前往 [**Releases**](https://github.com/KTCKING/pixvault/releases/latest) 下载对应平台的安装包：

| 平台 | 文件 | 适用机型 |
| --- | --- | --- |
| Windows | `PixVault_<版本>_x64-setup.exe`（NSIS，推荐）/ `.msi` | Windows 10/11 x64 |
| macOS | `PixVault_<版本>_aarch64.dmg` | Apple Silicon（M 系列） |
| macOS | `PixVault_<版本>_x64.dmg` | Intel Mac |

安装包由 GitHub Actions 在对应平台原生构建（桌面安装包无法交叉编译）。

---

## 功能特性

### 资源库（Library）是一等实体
- 同时打开多个库，侧边栏「资源库」区一键切换，顶部栏也有「当前库」下拉切换器
- 新建库 / 打开已有文件夹作为库 / 重命名 / 移除（保留文件）/ 删除（含文件）
- 库顺序可拖拽调整，重启后自动恢复上次打开的库
- 网络库（SMB/NFS）元数据放在 AppData，离线也能浏览已索引内容

### 智能视图（Smart Views）
侧边栏顶部即得，全部带实时计数徽标：

| 视图 | 逻辑 |
| --- | --- |
| 所有图片 | 整个库，按修改时间倒序 |
| 最近新增 | 按**导入图库的时间**倒序（新入库的一眼可见） |
| 收藏 | `favorite = 1` |
| 已评分 | `rating ≥ 1`，按评分倒序 |
| 回收站 | 软删除的文件，可恢复 / 彻底删除 / 清空 |

### 颜色标记（Pixcall 式 7 色）
红 / 橙 / 黄 / 绿 / 蓝 / 紫 / 灰。右键菜单、信息面板均可打色；
侧边栏「颜色标记」区点色即筛选，每色显示该色图片数。

### 文件夹树 + 标签树
- 「文件夹」：库内目录层级（含直接图片计数），点文件夹看该目录及其子目录的全部图片
- 「标签」：层级标签树（父子嵌套），选中图片后右键即可打标；侧边栏点标签筛选

### 评分 / 收藏 / 描述
- 卡片悬浮条：♥ 收藏 + ★ 五星评分
- 信息面板（右侧，Pixcall 式可折叠分区）：标签、描述、评分、收藏、颜色、尺寸、时间、完整路径

### 其他
- 缩略图双档：网格 448px（整库后台预热）+ 预览 1600px（按需生成），解码绝不持锁
- 套索框选、框选后拖拽到回收站、拖出窗口交给 OS（可丢进 QQ / 微信 / 资源管理器）
- 全文搜索（FTS5 + LIKE 兜底）、重复文件查找
- 导入 Eagle / Pixcall 库（设置面板）
- 深浅双主题、网格 / 瀑布流 / 列表三种视图、缩略图尺寸滑杆

---

## 技术架构

```
┌──────────────────────────────────────────────────────────────┐
│ 前端 SolidJS（src/）                                          │
│  App.tsx            布局 + 当前库切换 + 切库重载 effect        │
│  components/        Sidebar(分区侧边栏) ThumbnailGrid         │
│                    ThumbnailCard InfoPanel ContextMenu …     │
│  stores/            files(视图状态机) library tags ui viewer  │
│  lib/tauri-api.ts   invoke 封装 + 类型 + mock                 │
├──────────────────────────────────────────────────────────────┤
│ Tauri 2 IPC（tauri::generate_handler）                        │
├──────────────────────────────────────────────────────────────┤
│ 后端 Rust（src-tauri/src/）                                   │
│  engine/db.rs        SQLite：files/tags/file_tags/…           │
│                      schema v3（favorite 列 + 迁移）           │
│                      FileFilter 组合过滤（文件夹/收藏/颜色/评分）│
│                      get_folder_tree 内存聚合目录树            │
│  engine/indexer.rs  目录扫描（跳过 .pixvault）+ 头部解析尺寸    │
│  engine/thumbs.rs   缩略图 worker 池（rayon， urgent/bulk）    │
│  engine/thumbnail.rs 448/1600 双档 + 缓存版本哨兵              │
│  engine/import.rs   Eagle / Pixcall 库导入                    │
│  commands/          library files tags search import          │
│  utils/winattr.rs   Windows 上给 .pixvault 打隐藏属性          │
├──────────────────────────────────────────────────────────────┤
│ 存储                                                            │
│  <库根>/.pixvault/db.sqlite     每库一个 SQLite（WAL）         │
│  <库根>/.pixvault/thumbnails/   缩略图缓存（blake3 键）         │
│  %APPDATA%/PixVault/libraries.json  库清单 + 顺序              │
└──────────────────────────────────────────────────────────────┘
```

### 数据模型（schema v3）

```
files        id, path(UNIQUE), filename, extension, size_bytes, width, height,
             blake3_hash, phash, mtime, ctime, rating, color_label, notes,
             is_deleted, created_at(导入时间), updated_at, favorite
tags         id, name(UNIQUE), parent_id, color, icon
file_tags    file_id × tag_id
smart_folders / snapshots / library_meta(schema_version)
files_fts    FTS5 镜像（filename/path/notes）
```

- `created_at` = 导入图库时间（UPSERT 时不回退），`updated_at` = 记录刷新时间
- 重新索引是 UPSERT：只刷新文件派生字段，**不碰**用户的评分 / 颜色 / 收藏 / 备注
- 文件夹视图用前缀范围匹配（`folder+sep ≤ path < folder+sep+1`），不用 LIKE，免转义

---

## 开发

```bash
pnpm install
pnpm dev            # 浏览器开发（mock 数据）
pnpm dev:tauri      # Tauri 开发模式
```

## 构建

```bash
# Windows（exe + msi，自动覆盖安装到本机；NO_INSTALL=1 跳过安装）
bash scripts/build-windows.sh

# macOS（dmg）
bash scripts/build-macos.sh
```

- Windows 构建需在**英文路径**下进行（GNU 工具链在中文路径下链接会出错），
  可用 `scripts/sync-build.sh` 把源码同步到英文构建目录
- 平台配置拆分：`tauri.conf.json`（公共）/ `tauri.windows.conf.json` / `tauri.macos.conf.json`
- CI：`.github/workflows/build-installers.yml` 三平台 runner 原生构建，
  重跑工作流即刷新 Release 全部安装包

## 目录结构

```
pixvault/
├── src/                     # SolidJS 前端
├── src-tauri/               # Tauri + Rust 后端
│   ├── src/engine/          # db / indexer / thumbs / thumbnail / import …
│   ├── src/commands/        # Tauri 命令层
│   └── tauri.*.conf.json    # 公共 + 平台配置
├── scripts/                 # 构建 / 同步 / dmg 打包脚本
└── .github/workflows/       # CI
```

## 许可证

MIT
