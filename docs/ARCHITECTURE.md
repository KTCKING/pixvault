# PixVault 架构与功能全清单

> 对应版本 **v0.4.0**。这份文档是「结构设计参考」：上层是功能清单（用户看到什么），
> 下层是模块地图（代码在哪），中间用一张分层图串起来，改功能前先在这里定位归属。

---

## 一、整体架构

```
┌───────────────────────────────────────────────────────────────────┐
│ ① 浏览器扩展 · Chrome / Edge（MV3，extension/）                    │
│    content.js      扫描 img / srcset / 懒加载 / 背景图 / video      │
│                    Shadow DOM 采集面板（多选 + 文件夹 + 标签）      │
│    background.js   用站点 cookie fetch 字节 · 串行队列 · 重试 · 徽标│
│    popup/options   连接状态 · 端口令牌 · 采集规则                   │
└──────────────────────────┬────────────────────────────────────────┘
                           │ POST /api/ingest（原始字节 + ?name&page&folder&tags）
                           │ 仅 127.0.0.1，Origin 白名单 + x-pixvault-token
┌──────────────────────────▼────────────────────────────────────────┐
│ ② 本地桥接 bridge.rs（手写 HTTP/1.1，零框架）                       │
│    文件名清洗 → 唯一命名 (2) → 写入当前库 → insert_file → 打标签    │
│    → 排缩略图 · GET /api/meta 供面板取文件夹/标签                  │
│    emit bridge-ingested  ────────────────────────────────┐         │
└──────────────────────────┬───────────────────────────────┼─────────┘
                           │                               │
┌──────────────────────────▼───────────────────────────────▼─────────┐
│ ③ 应用主体（Tauri 2）                                              │
│  ┌──────────────────────────┐  IPC  ┌───────────────────────────┐  │
│  │ 前端 SolidJS（src/）      │◀────▶│ Rust 命令层 + engine       │  │
│  │  App / Sidebar / Grid    │       │  db  indexer  thumbs       │  │
│  │  Card / InfoPanel /      │       │  thumbnail  import         │  │
│  │  Viewer / ContextMenu /  │       │  eaglepack  watcher        │  │
│  │  Settings / Extension    │       │  gpu  priority  winattr    │  │
│  │  stores · tauri-api.ts   │       │  commands/*（57 个命令）    │  │
│  └──────────────────────────┘       └───────────────────────────┘  │
└──────────────────────────┬─────────────────────────────────────────┘
                           │
┌──────────────────────────▼─────────────────────────────────────────┐
│ ④ 存储（每个库自包含，拷走即迁移）                                  │
│    <库根>/.pixvault/db.sqlite        schema v3，WAL                 │
│    <库根>/.pixvault/thumbnails/      blake3 键：448 / 1600 / LQIP   │
│    %APPDATA%/PixVault/libraries.json 库清单与顺序                   │
│    %APPDATA%/PixVault/bridge.json    扩展端口 / 令牌 / 开关          │
└────────────────────────────────────────────────────────────────────┘
```

**关键设计取舍**

| 决策 | 原因 |
| --- | --- |
| 扩展只负责「抓」，落盘入库全在 Rust | 浏览器拿得到字节，但拿不到文件系统；职责一刀切最省心 |
| 让浏览器去 fetch 而不是应用自己下 URL | 防盗链 / `blob:` / canvas / 需登录 —— 应用直连常拿到 302 占位图 |
| 桥接手写 HTTP/1.1 不引 axum | 只有 3 条路由，引框架要多带约 1.5 MB 依赖 |
| 元数据放库内 `.pixvault` | 库可整目录搬走、可放网盘，离线仍能浏览；Windows 上打隐藏属性 |
| 缩略图不落库表、按 blake3 键存文件 | 换尺寸只加文件不改 schema；`prune_stale` 负责清理 |

---

## 二、功能全清单

### 1. 资源库（Library）
- 多库并存，侧栏「资源库」区与顶部下拉双向切换
- 建库：选目录建新库 / 打开已有库（自动识别 PrismVault·PixVault·Eagle·Pixcall 四种）
- 库右键菜单：**重命名**、在资源管理器中打开、从列表移除（保留文件）、**删除库及所有文件**
- 删除三道闸：弹窗逐字输入库名 → 后端复核库名 → 路径闸（磁盘根 / 用户主目录 / 桌面文档下载 / 缺 `.pixvault`）
- 库顺序可拖拽，重启恢复上次打开的库
- 网络库（SMB/NFS）：元数据落 AppData，离线可浏览已索引内容

### 2. 浏览与视图
- 三种布局：网格 / 瀑布流 / 列表；缩略图尺寸滑杆
- **真虚拟化**：只挂可见 ±2 行（瀑布流例外，靠卡片观察器）
- 智能视图：所有图片、最近新增（7 / 30 天窗口）、收藏、已评分；回收站独立成跨库「逻辑库」，不在智能视图里
- 颜色标记 7 色、五星评分、♥ 收藏
- 文件夹树 + 标签树（侧栏分区，可折叠）
- 全文搜索（FTS5，LIKE 兜底）
- 多选：单击 / Ctrl / Shift / 套索框选

### 3. 信息与编辑
- 右侧信息面板（可折叠分区）：标签、描述、评分、收藏、颜色、尺寸、时间、完整路径
- 单张 / 批量打标签（可新建标签）
- 编辑描述、评分、颜色、收藏
- 打开方式：预览器 / 系统默认程序 / 资源管理器定位

### 4. 预览器
- 全屏预览，← → 切换，滚轮缩放，拖拽平移，Esc 关闭
- **视频**：`video` 标签直接播放，WebView 不支持的容器提示改用系统播放器
- 1600px 预览图按需生成（不预热）

### 5. 缩略图管线
- 双档：网格 448px（整库预热）+ 预览 1600px（按需）
- 32px LQIP 占位图先到，铺底淡入，绝不回退原图
- Worker 池：urgent（LIFO，当前视口）/ bulk（FIFO，预热）
- **视口窗口调度**：滚动去抖 120 ms 整屏提交，窗口外 urgent 降回 bulk
- **交互感知节流**：滚动时只跑 urgent，bulk 停发
- 进程 / 解码线程降优先级，后台不让风扇抢戏
- 侧栏进度环显示预热进度，可在设置里关闭预热
- 「补齐缺失缩略图」手动入口

### 6. 导入
- **Eagle 素材包 `.eaglepack`**：流式解压、两种布局（Pack / Library）自动识别、
  按 Eagle 内文件夹层级**真搬盘重组**、清掉 `<ID>.info` 壳、重名加 `(2)`、
  逐条目进度、可取消
- Eagle 资源库原地导入（不搬盘，展示层折叠 `.info`）
- Pixcall 资源库导入
- 单目录扫描导入（`index_library`）

### 7. 文件操作
- 删除 → 回收站（软删，`is_deleted`）
- **回收站是跨库的「逻辑库」**：侧栏「资源库」置底常驻，没有自己的目录、不搬文件；
  `get_trash` 遍历 Engine 里已打开的库（启动时全部打开、连接常驻）聚合，
  每库一句走 `idx_files_deleted` 索引；条目自带 `library_id`，
  恢复 / 彻底删除**一律回原库执行**，不存在跨库写入
- 回收站还原 / 彻底删除 / 清空（跨库）；缩略图与信息面板按条目的来源库请求
- 「彻底删除」默认**只移除索引，磁盘原文件保留**；设置里可开启「一并删除磁盘文件」，
  开启后先校验路径在库根内（`canonicalize` 比较，挡掉库外软链）再 `remove_file`，
  删失败则保留记录
- 拖出窗口交给 OS（可丢进 QQ / 微信 / 资源管理器）
- 复制文件到剪贴板
- 重复文件查找（blake3）

### 8. 从网页摘取（v0.4.0 新增，v0.5.0 / v0.6.0 增强）
- 三种入口：整页采集面板（`Alt+S`）、悬停速存按钮、右键「保存到 PixVault」
- 自动处理：`srcset` 取最大档、懒加载 `data-src`、CSS 背景图、`<picture>`、
  指向图片的链接、`blob:`、需登录 / 防盗链
- 视频与图片可一起勾选，视频带播放角标
- **采集时就能归档**（v0.5.0）：面板顶部可选文件夹（库里已有的目录 / 新建多级目录 /
  库根 / 按站点自动）与打标签（逗号分隔，也可点库里已有标签），选择记在本地，
  下次沿用；标签名不存在时由后端现建（`a/b` 自动补父级）
- **iframe 内也能采**（v0.5.0）：`all_frames` + 后台「活着的帧」注册表，顶层帧逐帧收集
  再按 URL 去重；`blob:` 按 frameId 回到产出它的那一帧去读；面板只出现在顶层文档
- **失败重试**（v0.5.0）：每张最多试 3 次（400/900 ms 退避），4xx 直接判死；
  失败项留在后台，面板出现「重试」按钮，只能重跑失败的那几张
- **整页截图**（v0.6.0）：面板「截取整页」/ 页面右键 / `Alt+Shift+S` 三入口；
  `captureVisibleTab` 逐屏拍（Chrome 限流 2 次/秒 → 每屏等 520ms），
  后台用 `OffscreenCanvas` + `createImageBitmap` 拼长图（页面里拼会被站点 CSP 的
  `img-src` 拦 `data:`）。第一屏保留页头、第二屏起藏 `fixed`/`sticky`；截图期间
  临时关掉 `scroll-behavior:smooth` 与滚动条，结束后恢复原位。
  上限 16 屏 / 画布单边 16384px（超出等比缩放）；内部滚动容器的 SPA 退化为视口截图
- **落点规则**：按站点建子文件夹（默认）/ 固定子文件夹 / 面板里手选 / 直接存库根
- 同名自动 `(2)`；网页来源写进文件备注；单文件上限 512 MB
- 设置页「浏览器扩展」面板：开关、端口、令牌（可重新生成）、一键导出扩展、安装指引
- 存入后主窗口实时刷新并 toast 提示

### 9. 设置与系统
- 外观：深 / 浅 / 跟随系统
- 渲染方式：自动 / 硬件加速 / 软件渲染（重启生效，崩溃自动降级并持久化）
- 缩略图预热开关
- 导入统计（条目数 / 解包体积 / 按文件夹重组 / 重名改名）

### 10. 交互细节（踩过坑的地方）
- 开 / 关信息面板时网格列数变化，目标图用 rAF 平移补锚点留在原位
- Esc 只关最上面一层（捕获阶段快照浮层状态，不靠注册顺序）
- 侧栏缩进基线 18 px（`px-2` 容器 + `px-2.5` 条目）

---

## 三、模块地图

```
src/                      前端（SolidJS + Tailwind）
├── App.tsx               布局 · 切库重载 · 全局快捷键 · bridge-ingested 监听
├── components/
│   ├── Sidebar.tsx       分区侧栏 + 库右键 + 预热进度环
│   ├── ThumbnailGrid.tsx 三种布局 + 虚拟化 + 视口窗口提交
│   ├── ThumbnailCard.tsx LQIP 占位 · 视频角标 · 悬浮操作条
│   ├── ImageViewer.tsx   图片 / 视频预览
│   ├── InfoPanel.tsx     右侧信息面板
│   ├── ContextMenu.tsx   文件 / 库 / 分组三类菜单
│   ├── SettingsModal.tsx 设置（外观 / GPU / 预热 / 导入 / 扩展入口）
│   └── ExtensionModal.tsx 浏览器扩展面板
├── stores/               files  library  tags  ui  viewer  theme
└── lib/                  tauri-api.ts（invoke 封装）  thumbs.ts（URL 缓存）

src-tauri/src/
├── main.rs               窗口创建（GPU 门控在此）· 命令注册
├── lib.rs                AppState · 事件 · 启动时恢复库
├── bridge.rs             本地 HTTP 服务 + 扩展分发
├── gpu.rs                GPU 哨兵 / 模式
├── engine/
│   ├── db.rs             SQLite schema v3 · FileFilter · 文件夹树聚合
│   ├── indexer.rs        目录扫描（图片 + 视频扩展名）
│   ├── thumbs.rs         缩略图 worker 池 + 预热批次
│   ├── thumbnail.rs      448/1600 双档 · LQIP · downscale 统一重采样
│   ├── import.rs         Eagle / Pixcall 库导入
│   ├── eaglepack.rs      .eaglepack 解包 + 重组
│   └── watcher.rs        目录变更监听
├── commands/             library files tags search import bridge
└── utils/                priority.rs（降优先级）  winattr.rs（隐藏属性）

extension/                浏览器扩展（编译期 include_str! 进 exe）
```

---

## 四、桥接协议（供扩展 / 脚本调用）

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| `GET` | `/api/ping` | 连通性 + 版本 + 当前库名 |
| `GET` | `/api/meta` | 采集面板的「存放」清单：库内子目录（相对库根，≤3 层 / 400 条）+ 已有标签名 |
| `POST` | `/api/ingest?name=&page=&folder=&tags=` | 请求体为原始字节；`folder` 相对库根、不存在会自动建，`tags` 逗号分隔（`tag=` 重复亦可）；返回 `{ok,path,filename,size,video,folder,tags}` |
| `OPTIONS` | 任意 | CORS 预检，只对扩展来源回 `Access-Control-Allow-Origin` |

`/api/meta` 里的目录扫描会整枝跳过点开头的目录（含库元数据目录），所以
`.pixvault/thumbnails` 里上万条缓存不会被当成「文件夹」列出来。

标签名在做清洗：去 `#`、去空白、截断 40 字、按小写去重、单次最多 12 个；
库里没有的名字调用 `get_or_create_tag` 现建（`a/b` 会自动补齐父级）。

请求头必带 `x-pixvault-token`（除扩展来源的预检）。错误码：
`401` 令牌不符 · `403` Origin 不被接受 · `409` 没有打开库 · `413` 超过 512 MB · `500` 落盘失败。

---

## 五、数据模型（schema v3）

```
files        id, path(UNIQUE), filename, extension, size_bytes, width, height,
             blake3_hash, phash, mtime, ctime, rating, color_label, notes,
             is_deleted, created_at(导入时间), updated_at, favorite
tags         id, name(UNIQUE), parent_id, color, icon
file_tags    file_id × tag_id
smart_folders / snapshots / library_meta(schema_version)
files_fts    FTS5 镜像（filename / path / notes）
```

- `created_at` = 导入时间（UPSERT 不回退），`updated_at` = 记录刷新时间
- `ctime` / `mtime` 是文件系统时间（UTC 无标记，前端按 UTC 解析）
- 重新索引是 UPSERT：只刷派生字段，**不碰**评分 / 颜色 / 收藏 / 备注
- 文件夹视图用前缀范围匹配，不用 LIKE

---

## 六、版本演进

| 版本 | 主题 |
| --- | --- |
| v0.1.x | 库 + 网格 + 缩略图管线 |
| v0.2.0 | 智能视图 / 颜色标记 / 真虚拟化 / GPU 开关 |
| v0.3.0–0.3.2 | `.eaglepack` 导入：流式解包 → 展示层折叠 → 真搬盘重组 |
| v0.3.3 | 库右键：重命名 / 删除（三道闸） |
| v0.4.0 | 浏览器扩展摘取 + 视频入库 |
| v0.5.0 | 采集面板可选文件夹 / 打标签 · iframe 内采集 · 失败重试 |
| v0.6.0 | 整页截图（逐屏拼长图） |
| **v0.6.1** | **修复智能视图点击失效（切库 effect 误追踪当前视图）** |
| **v0.6.2** | **「最近新增」可设时间窗口（最近一周 / 一个月，设置里切换）** |
| **v0.6.3** | **修复「当前库」下拉被顶栏 backdrop-filter 的层叠上下文压住（改用 Portal）** |
| **v0.6.4** | **去掉重复入口：顶栏只显示当前库名，切换统一由侧栏「资源库」承担** |
| **v0.7.0** | **回收站变跨库的「逻辑库」：侧栏置底 · 按来源库筛选 · 彻底删除默认只移除索引** |
