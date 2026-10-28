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
- 右键侧栏的库条目：重命名 / 在资源管理器中打开 / 从列表移除（保留文件）/ 删除库及所有文件
- 库顺序可拖拽调整，重启后自动恢复上次打开的库
- 网络库（SMB/NFS）元数据放在 AppData，离线也能浏览已索引内容

「重命名」只改图库里显示的名字，磁盘上的文件夹名不动（改了会连带影响 Eagle / Pixcall
的目录结构）。「从列表移除」是最轻的一档，文件原样留在硬盘上，下次还能重新打开。

「删除库及所有文件」是真的 `remove_dir_all`，**不进回收站、无法恢复**，所以设了三道闸：
弹窗里必须**逐字输入库名**才执行；后端复核同一个库名；路径闸再拦一遍磁盘根目录、用户主目录
（含桌面 / 文档 / 下载），以及库里没有 `.pixvault` 元数据的目录——那种多半是库根的上级目录，
不是库本身。原地读取的 Eagle / Pixcall 库也能被删，弹窗里会明确提示「删掉的就是你原本的
素材目录」，拿不准就用「从列表移除」。

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
- **真虚拟化网格**：只挂可见 ±2 行的卡片，10 万张也能即时滚动；32px LQIP 占位图先于真图到达，铺底淡入不白屏
- **后台预热 + 侧栏进度环**：导入 / 打开库后自动补齐缺失缩略图，资源库区实时显示本轮进度；可在设置里关闭预热
- **交互感知节流**：滚动 / 拖拽时后端暂停 bulk 队列只 urgent，界面更跟手；进程与解码线程均降优先级，不让风扇抢戏
- **渲染方式（GPU）开关**：设置页可选 自动 / 硬件加速 / 软件渲染，WebView2 崩溃后自动降级并持久化，不再白屏
- Fluent 玻璃拟态界面（侧栏 / 标题栏 / 弹层真高斯模糊，卡片用伪玻璃兼顾性能）、黑白双主题、全套自绘线性图标
- **浏览器扩展摘取**：网页上的图片 / 视频一键存进当前图库（面板多选、**顺手选好文件夹和标签**、悬停速存、右键保存、iframe 里的图也能扫、失败的能单独重试），字节由浏览器带着你的登录态去取 —— 防盗链、`blob:`、canvas 画出来的图都能存下来（见下节）
- **视频入库**：mp4 / webm / mov 等直接进库，卡片显示播放角标，双击在预览器里播放（WebView 解不了的容器会提示改用系统播放器）
- 套索框选、框选后拖拽到回收站、拖出窗口交给 OS（可丢进 QQ / 微信 / 资源管理器）
- 全文搜索（FTS5 + LIKE 兜底）、重复文件查找
- 导入 Eagle / Pixcall 库，以及 **Eagle 素材包 `.eaglepack`**（流式解压、按 Eagle 里的文件夹层级重组落盘、逐条目进度、可取消，见下节）
- 深浅双主题、网格 / 瀑布流 / 列表三种视图、缩略图尺寸滑杆

---

## 从网页摘取（浏览器扩展）

安装包内置一个 Chrome / Edge 扩展（源码在 `extension/`），把网页上的图片和视频直接
存进当前图库。思路和 Eagle / Pixcall 一致，但**字节交给浏览器去取**：

```
浏览器扩展（MV3）                        PixVault（Rust）
┌────────────────────────┐             ┌──────────────────────────────┐
│ content.js             │             │ bridge.rs                    │
│  扫描 img / video /    │  1. POST    │  127.0.0.1:41596（只看本机）   │
│  srcset / 背景图 /     │  ─────────▶ │  POST /api/ingest?name&page   │
│  懒加载属性            │  文件字节   │   ↓                          │
│  采集面板（多选/文件夹/│             │  Origin 白名单 + 令牌校验      │
│  标签）· 悬停速存      │  2. 落盘    │   ↓                          │
├────────────────────────┤  ◀───────── │  文件名清洗 → 写进当前库       │
│ background.js          │   JSON      │  → insert_file               │
│  fetch(带站点 cookie)  │             │  → 排队生成缩略图             │
│  串行队列 · 重试 · 徽标 │             │  → 打标签 → 排缩略图          │
└────────────────────────┘             └──────────────────────────────┘
```

为什么不让应用自己去下载 URL？因为**只有浏览器知道那张图真正的字节**：带 cookie 的
防盗链、`blob:` 地址（canvas 画出来的图、流媒体）、要 Referer 的图床、登录后才可见的
原图 —— 应用直接发请求，拿回来的经常是 302 到一张占位图。

### 三种用法

| 方式 | 操作 | 适合 |
| --- | --- | --- |
| 整页挑选 | 点扩展图标 →「打开采集面板」，或网页里按 `Alt` + `S` | 一页里挑几张 |
| 单张速存 | 鼠标悬停在图片上，点左上角浮出的方块 | 逛图站顺手存 |
| 右键菜单 | 图片 / 视频上右键 →「保存到 PixVault」 | 最快的一步 |

面板里支持全选 / 清空 / 反选；视频会被标上播放角标，和图片一起勾选。

面板顶部还有一行**存放位置**：选文件夹（库里已有的目录、`＋` 新建多级目录、库根、
或「按站点自动」）+ 填标签（逗号分隔，也可以直接点库里已有的标签）。这两项会记在
扩展本地，下次打开面板沿用同一套 —— 一边挑图一边归档，省掉存完再回头整理的功夫。

- **iframe 里的图也能采**：画廊 / 图床常把内容塞在 iframe 里，扩展在每一帧都跑扫描，
  顶层面板按 URL 合并去重（同一个地址只出现一次）。
- **失败能单独重试**：一次存几十张时偶有超时 / 5xx，每张最多试 3 次；确实失败的会留在
  后台，面板底部出现「重试」按钮，只重跑那几张，不用重新勾选（4xx 的明确拒绝不重试）。

### 安装

1. 设置 → **浏览器扩展** →「导出扩展到文件夹…」（扩展是编译期嵌进 exe 的，导出的
   永远和当前版本一致，不用去 GitHub 找压缩包）
2. 浏览器打开 `chrome://extensions`（Edge 用 `edge://extensions`），打开右上角「开发者模式」
3. 「加载已解压的扩展程序」→ 选导出的 `pixvault-extension` 文件夹
4. 点扩展图标 →「扩展设置」→ 填设置面板里的**端口**和**访问令牌** →「测试连接」

装好之后，**已经打开的网页要刷新一次**才会出现悬停按钮和快捷键 —— 浏览器不会给已存在
的页面补注入脚本。

### 安全上按「本机半个写入接口」来对待

- 只监听 `127.0.0.1`，不碰 `0.0.0.0`；
- Origin 白名单：CORS 头只回给浏览器扩展，普通网页就算硬发请求也读不到响应；
- 访问令牌：16 位随机串，存在 `%APPDATA%\PixVault\bridge.json`，网页脚本拿不到，可随时重新生成；
- 文件名逐字符清洗（`<>:"/\|?*`、Windows 保留名 `CON`/`LPT1`、结尾的点和空格、超长截断），
  路径只允许落在当前图库之内，`..` 会被拍平成下划线；
- 没有打开图库时直接拒收，绝不把文件丢到不知道哪去。

### 会自动处理的情况

`srcset` 取最大一档（不会把缩略图当原图存）· 懒加载 `data-src` / `data-original` ·
CSS 背景图 · `<picture>` · 指向图片文件的链接 · `blob:` 地址 · 需要登录 / 防盗链的图 ·
同名文件自动加 `(2)` · 网页来源写进文件「备注」。

默认**按站点建子文件夹**（`库根/example.com/图片.jpg`），可在扩展设置里改成固定子文件夹
或直接存库根。单文件上限 512 MB。

---

## 导入 Eagle 素材包（`.eaglepack`）

`.eaglepack` 不是黑盒——**它就是一个 zip**。Eagle 官方帮助中心给的自救办法就是「把后缀改成 `.zip` 打开，
里面应该有 pack.json 和若干 info 文件夹」。所以难点不在压缩格式，在于**包里有两种布局**，都得认。

```
# ① Pack 布局：Eagle 自己导出的素材包、社区下载的免费包
pack.json                       { "images": [ <全部条目元数据> ] }
<ITEM_ID>.info/
  ├ metadata.json               同一份元数据（副本，单独解包也能用）
  ├ photo.jpg                   原始素材
  └ photo_thumbnail.png         可选缩略图

# ② Library 布局：把整个 .library 目录压成 zip（外面有时还套一层文件夹）
metadata.json                   库级：文件夹树
mtime.json
images/<ITEM_ID>.info/…
```

两者都按**内容**识别（`pack.json` 有 `images` 数组 / `metadata.json` 有 `folders` 且旁边有 `images/`），
不靠文件名猜，也不靠解压出来的目录名。

### 导入流程

1. 选 `.eaglepack`（或 `.zip`）→ 只读 zip 中央目录给出统计：多少条目、多少文件夹、解压后多大。
   这一步不解压，几百 MB 的包也是毫秒级返回，你可以先看清再决定。
2. 选一个**空文件夹**作为目标——它之后就是新图库的根。
3. 流式解压：边读边写，不落临时文件（否则 1 GB 的包要占 2 GB 盘）。每 16 个条目回报一次进度，
   随时可以取消。取消或失败会把半成品**整棵删掉**，不会给你留一个半截图库。
4. **按 Eagle 里的文件夹层级重组落盘**，然后把这个文件夹当新图库打开——零复制，和
   Eagle / Pixcall 目录导入的语义一致。

新库的名字取**你刚选的那个目标文件夹名**，不是素材包文件名——网上下来的包经常就叫
`Untitled.eaglepack`，拿它当库名会让人找不到自己刚导入的东西。

### 为什么导入后硬盘上不是一堆 `<ID>.info`

Eagle 的文件夹是**虚拟**的：磁盘上所有素材扁平地躺在 `<ID>.info/` 里，归属关系只记在条目
自己的 `folders: [id]`。所以在 Eagle 里看到的是 `设计素材/图标/a.png`，硬盘上却是
`MVQ55RU1NL8GV.info/photo.png`。直接解压出来就是几百个壳目录。

素材包是我们自己解出来的副本（源包不动），所以这里**真的重组**：

```
解压后（Eagle 的样子）              重组后（你看到的样子）
out/                                out/
├ pack.json                         ├ pack.json
├ IMG001.info/                      └ 设计素材/
│ ├ metadata.json                     ├ hero.png
│ ├ hero.png                          └ 图标/
│ └ hero_thumbnail.png                    └ icon.png
└ IMG002.info/…
```

- `<ID>.info` 壳连同里面的缩略图、元数据副本一起删掉（内容已经进库了）
- `images/`、包外多套的那层 `旧库.library/` 文件夹一并清掉，Eagle 的记账文件
  （`metadata.json` / `mtime.json` / `pack.json`）提到库根，它们不是图片，不会出现在网格里
- 同名文件自动加序号：`封面.png` → `封面 (2).png`。Eagle 里重名很常见，直接搬会覆盖掉一张
- 文件名要清洗：`< > : " / \ | ? *`、控制字符、结尾的点与空格、`CON`/`COM1` 这类保留名，
  在 Windows 上都建不出来，不清洗就整个搬运失败
- 超长名截断但**保住扩展名**，否则图片变成没后缀的文件，索引器认不出来
- 搬运失败的文件（被杀软占用、跨卷复制中断）按原路径入库，**图不会丢**，只是位置没变漂亮

**只有素材包这么做**。「导入 Eagle 资源库」是原地读取用户已有的库，重组之后 Eagle 自己
就打不开了，那种情况只在展示层折叠：`db.rs::get_folder_tree` 遇到 `.info` 层就往上折叠，
文件归属到壳外面那层，磁盘一个字节都不动。

元数据映射与目录导入共用同一套规则（`import.rs::map_meta`）：Eagle 的文件夹归属 → 层级标签
（`设计素材/图标`），`annotation` 优先于 `note`，`star` 1–5 直接当评分，`url` 存成来源链接。
一条素材同时属于多个文件夹时，磁盘上落在**第一个**文件夹，其余归属关系仍然全部进标签。

### 三条安全线

- **zip slip**：只信 `enclosed_name()` 的返回值（已清掉 `..`、绝对路径、盘符），拿不到就跳过该条目。
  绝不会拿原始文件名去拼路径——那是「解压一个包，文件跑到 `C://Windows` 去」的经典事故。
- **跳过垃圾**：`__MACOSX/`、`.DS_Store`、`Thumbs.db`、`desktop.ini`。
- **跳过不该导入的条目**：回收站里的（`isDeleted`）、Eagle 的网页书签（只有 url 和缩略图、没有原文件）。

### 顺手修掉的既有 bug：缩略图有两种命名

Eagle 的缩略图叫 `thumbnail.png`，叫 `<原名>_thumbnail.png` 的也真实存在。旧代码只跳过前缀式，
后缀式的会被当成原图导进图库——表现是「每张图都多出一条很糊的记录」，而且哪张中招取决于
`read_dir` 的返回顺序（文件系统给的，没保证），所以还是「我这边正常、你那边糊」的玄学 bug。

现在两处都修了：`find_original_file` 两种命名都跳过，并按「`name.ext` 精确匹配 → `name.*` → 体积最大」
挑原图，不再依赖遍历顺序；索引器也加了规则，跳过 `.info` 目录里的缩略图，且限定在这个目录结构内判断，
不会误吞用户自己起名叫 `cover_thumbnail.jpg` 的图。

---

## 几个容易被忽略的交互细节

这些都是「用着别扭但说不清哪不对」的那类问题，改法与原因一并记下来。

### 点开信息面板后，那张图为什么还钉在原位

信息面板是固定宽的兄弟节点（`w-[19rem]`），一开一关网格宽度就变，列数跟着变 —— 同一张图换了行。点击时它明明在眼前，面板一开列数变少，它被推到更靠后的行，于是滑出可视区，用户感觉「点了就跳走了」，还得自己找回来。

列数变化只改行号、不改行高，所以把 `scrollTop` 平移「行差 × 行高」就能让它留在原来的纵向位置（不是「滚到它可见」，而是逐像素不动）。三个坑：

1. **用 `requestAnimationFrame` 而不是微任务** —— rAF 一定在 DOM 更新之后、本帧绘制之前。此刻 `clientWidth` 已经是新宽度，而 `virtual()` 里的列数还是旧的（ResizeObserver 要等本帧 rAF 回调之后才派发），正好一旧一新可以对比；用户也不会看到「先跳到别处再跳回来」的闪烁。
2. **先记旧 `scrollTop` 再让新宽度进信号** —— 列数一变，撑起滚动高度的行数也跟着变，浏览器可能趁机把 `scrollTop` 夹一下；用绝对目标值（旧位置 + 行差）算才稳，不能累加。
3. **平移后立刻 `syncViewport()`** —— 否则切片还按旧位置算，会闪一帧空白。

关面板时 `onClose` 会先把 `infoPanelFile` 清空，所以另存一个 `anchorPath` 当锚点（Esc 那条路径不清空，只有 X 按钮清）。瀑布流没有虚拟化（CSS columns 列高不均算不出行位置），那里退回用元素实测矩形兜底。

### 一个 Esc 为什么关了三样东西

`window` 上有多个 `keydown` 监听器：`App.tsx`（全局快捷键）、`ImageViewer.tsx`（Esc / ←→）、`ContextMenu.tsx`（Esc 收菜单）、`Dialog.tsx`（Esc 关弹窗）。**同一事件里它们按注册顺序执行，后面的会读到被前面改过的状态。**

原写法 `if (viewer.viewerOpen()) return;` —— 预览器先把自己关掉（`viewerOpen` → false），App 的守卫就不成立了，于是 Esc 一次同时关掉预览 + 右侧信息栏 + 搜索框。

修法是**捕获阶段的快照**：捕获阶段监听器一定早于所有冒泡监听器（与注册顺序无关），用它把「事件开始派发时」的浮层状态存成 `overlayWasOpen`，后续判断全用快照。语义统一为「**Esc 只关最上面那一层**」，浮层集合 = 预览 / 右键菜单 / 确认弹窗；搜索框不算浮层（Esc 关它是 App 那层的职责）。

不要试图用 `stopPropagation` 拦 —— 同为目标 `window` 的两个监听器互相拦不住（只有 `stopImmediatePropagation` 能拦，且依赖注册顺序）。

弹窗的 Esc 挂在 `window` 上而不是弹层 `div` 上：只有带输入框的弹窗才有焦点可落，纯确认弹窗打开时焦点还在 `body`，`div` 收不到 `keydown`。

### 侧栏的缩进基线是 18px

`.sidebar-section` 用 `px-2`（8px），`.sidebar-item` / `.sidebar-title` 再叠 `px-2.5`（10px），内容统一落在 **18px**。别再给 section 和 item 各写一层 `px-3` —— 两层叠加跑到 24px，而分组标题 12px、回收站 20px，三条缩进线各走各的，看着就是「没对齐但说不上哪歪」。颜色色板、空状态提示、「新建标签」输入框都跟着这条基线走。

资源库条目的图标是 `IconLibrary`（照片堆叠 + 山与太阳），不要退回 `IconMonitor` —— 显示器会被读成「屏幕 / 远程」，和「一个放图片的文件夹」没关系。

---

## 技术架构

> 完整的功能清单、模块地图、桥接协议和数据模型见 [**docs/ARCHITECTURE.md**](docs/ARCHITECTURE.md)。

```
┌──────────────────────────────────────────────────────────────┐
│ 前端 SolidJS（src/）                                          │
│  App.tsx            布局 + 当前库切换 + 切库重载 effect        │
│  components/        Sidebar(分区侧边栏+进度环) ThumbnailGrid   │
│                    (真虚拟化) ThumbnailCard(LQIP 占位)        │
│                    InfoPanel ContextMenu CircleProgress …     │
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
│  engine/thumbs.rs   缩略图 worker 池（rayon， urgent/bulk + 视口窗口降级）│
│  engine/thumbnail.rs 448/1600 双档 + 32px LQIP + 缓存版本哨兵        │
│  engine/import.rs   Eagle / Pixcall 库导入 + 条目元数据映射    │
│  engine/eaglepack.rs .eaglepack 流式解包 + 两种布局识别        │
│  commands/          library files tags search import bridge    │
│  bridge.rs          浏览器扩展桥接：手写 HTTP/1.1（零框架）      │
│                     127.0.0.1 + Origin 白名单 + 令牌 + 文件名清洗 │
│  gpu.rs             GPU 崩溃哨兵 + 渲染模式开关（窗口在此创建）    │
│  utils/winattr.rs   Windows 上给 .pixvault 打隐藏属性          │
│  utils/priority.rs  进程 / 解码线程降优先级（后台任务让路）      │
├──────────────────────────────────────────────────────────────┤
│ 浏览器扩展（extension/，MV3，编译期嵌进二进制）                 │
│  content.js         页面扫描 + 采集面板(Shadow DOM) + 悬停按钮  │
│  background.js      fetch 取字节 + 串行上传 + 右键菜单 + 徽标   │
│  popup / options    连接状态 / 端口令牌 / 采集规则              │
├──────────────────────────────────────────────────────────────┤
│ 存储                                                            │
│  <库根>/.pixvault/db.sqlite     每库一个 SQLite（WAL）         │
│  <库根>/.pixvault/thumbnails/   缩略图缓存（blake3 键）         │
│  %APPDATA%/PixVault/libraries.json  库清单 + 顺序              │
│  %APPDATA%/PixVault/bridge.json     扩展端口 + 访问令牌        │
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
npm install
npm run dev            # 浏览器开发（mock 数据）
npm run dev:tauri      # Tauri 开发模式
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
│   ├── src/bridge.rs        # 浏览器扩展的本地 HTTP 服务
│   └── tauri.*.conf.json    # 公共 + 平台配置
├── extension/               # Chrome / Edge 扩展（MV3，include_str! 编进 exe）
├── scripts/                 # 构建 / 同步 / dmg 打包脚本
└── .github/workflows/       # CI
```

## 许可证

MIT
