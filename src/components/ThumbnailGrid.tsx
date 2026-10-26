import { For, Show, createSignal, createEffect, createMemo, on, onMount, onCleanup } from "solid-js";
import { useFilesStore } from "@/stores/files";
import { useUIStore } from "@/stores/ui";
import { useViewerStore } from "@/stores/viewer";
import { useLibraryStore } from "@/stores/library";
import { useTheme } from "@/stores/theme";
import { ThumbnailCard } from "./ThumbnailCard";
import { api } from "@/lib/tauri-api";
import type { FileInfo, ThumbReadyItem, ThumbProgress, ThumbPlaceholderItem } from "@/lib/tauri-api";
import {
  IconArrowDown,
  IconArrowUp,
  IconGrid,
  IconHeart,
  IconImage,
  IconMasonry,
  IconMenu,
  IconStarFilled,
} from "@/components/icons";
import {
  thumbUrl, setThumbUrl, placeholderUrl, setPlaceholderUrl, clearThumbUrls, toDisplaySrc,
} from "@/lib/thumbs";
import { activeLibraryId } from "@/stores/library";
import { listen } from "@tauri-apps/api/event";
import { startDrag } from "@crabnebula/tauri-plugin-drag";

/** Rectangle in viewport (client) coordinates. */
interface MarqueeRect { x: number; y: number; w: number; h: number; }

/** Movement (px) before a press becomes a real drag. */
const DRAG_THRESHOLD = 3;
/** Distance (px) from the container edge that triggers auto-scroll. */
const EDGE_ZONE = 48;
/** Max auto-scroll speed (px per frame). */
const SCROLL_SPEED = 22;

/**
 * 滚动去抖：视口变化后静默这么久才提交请求。
 *
 * 关键在于「静默」而不是「节流」—— 每来一个视口变化就重置计时器，所以只要还在
 * 滑，就一个请求都不发。以前是卡片一进预取边距就各自 invoke，快速下滑时几百个
 * 请求挤在同一个后端锁上，滑动越快、每张图等得越久。
 */
const FLUSH_DEBOUNCE_MS = 120;
/** 兜底重扫时每批提交多少条（队列跑空后用来补漏掉的事件）。 */
const RESYNC_CHUNK = 200;

/** 网格卡片之间的间距 —— 对应 Tailwind 的 `gap-2`。 */
const GRID_GAP = 8;
/** 滚动容器的内边距 —— 对应 `p-3`（左右各 12px）。 */
const CONTAINER_PAD = 12;
/** 列表行的固定高度：`py-2`（上下各 8px）+ 40px 缩略图。 */
const LIST_ROW_H = 58;
/** 可见范围上下各多渲染几行，避免快速滚动时露出空白。 */
const OVERSCAN_ROWS = 2;

/** 日志里只留路径尾部，避免整条路径刷屏。 */
const trail = (p: string, n = 42): string => (p.length > n ? `…${p.slice(-n)}` : p);

export function ThumbnailGrid() {
  const files = useFilesStore();
  const ui = useUIStore();
  const viewer = useViewerStore();
  const library = useLibraryStore();
  const theme = useTheme();

  let containerRef!: HTMLDivElement;
  /**
   * 当前在视口内（含 400px 预取边距）的卡片路径。
   *
   * 这个集合就是提交给后端的「视口窗口」—— 后端会据此把窗口外那些还没做完的
   * 排队任务降级回整库队列。没有它，快速下滑会把滑过去的每一屏都堆在队列前面，
   * 用户真正盯着的一屏反而排最后。
   */
  const visiblePaths = new Set<string>();
  /**
   * 当前列表里实际存在的路径。
   *
   * 用来剔除窗口里的「残留」——切视图模式、搜索过滤、分页替换都会让一批卡片
   * 卸载，但卸载不会回调告诉我们。不清理的话这些旧路径会一直挂在窗口定义里，
   * 每次提交都要为它们白查一次数据库。
   */
  const livePaths = new Set<string>();
  /** 去抖计时器。 */
  let flushTimer = 0;
  /** 组件已销毁后不再提交（去抖回调可能晚于卸载）。 */
  let disposed = false;
  /** 诊断用：前若干个窗口提交会写进后端日志，方便定位「为什么不出图」。 */
  let loggedWindows = 0;
  const LOG_WINDOWS = 12;

  // ── Marquee (drag) selection state ─────────────
  const [marquee, setMarquee] = createSignal<MarqueeRect | null>(null);
  let drag: {
    startX: number; startY: number;
    clientX: number; clientY: number;
    additive: boolean;
    base: Set<string>;
    active: boolean;
    onEmpty: boolean;
  } | null = null;
  let rafId = 0;
  let suppressClickUntil = 0;

  // Press on a card arms a custom pointer-driven drag. Dragging within the
  // window moves images into the trash; dragging the cursor OUT of the window
  // hands the drag over to the OS (tauri-plugin-drag) so images can be dropped
  // into QQ / WeChat / Explorer.
  //
  // HTML5 drag-and-drop is NOT used here: Tauri's OS-level drop target
  // (`dragDropEnabled: true`) suppresses the webview's HTML5 drag events on
  // Windows, which is why `draggable` cards never fired `drop` on the trash.
  let cardDrag: {
    pointerId: number;
    startX: number;
    startY: number;
    paths: string[];
    active: boolean;
    handedOff: boolean;
  } | null = null;
  const [dragPreview, setDragPreview] = createSignal<{ x: number; y: number; path: string; count: number } | null>(null);

  /** Paths to carry when starting a drag on `path` (honours multi-selection). */
  const pathsFor = (path: string): string[] => {
    const selected = files.selectedFiles();
    return selected.has(path) && selected.size > 1 ? [...selected] : [path];
  };

  const trashDropEl = () => document.querySelector<HTMLElement>("[data-trash-drop]");

  const isOverTrash = (x: number, y: number): boolean => {
    const el = trashDropEl();
    if (!el) return false;
    const r = el.getBoundingClientRect();
    return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  };

  const endCardDrag = () => {
    if (cardDrag) {
      try { containerRef.releasePointerCapture(cardDrag.pointerId); } catch { /* ignore */ }
    }
    cardDrag = null;
    setDragPreview(null);
    ui.setTrashHover(false);
  };

  const onCardPointerDown = (e: PointerEvent) => {
    if (e.button !== 0) return;
    const t = e.target as HTMLElement | null;
    if (!t || t.closest("input, select, textarea, button, .thumbnail-actions")) return;
    const cardEl = t.closest("[data-path]") as HTMLElement | null;
    const cardPath = cardEl?.dataset.path;
    if (!cardPath) return; // blank space -> marquee selection handles it
    cardDrag = {
      pointerId: e.pointerId,
      startX: e.clientX,
      startY: e.clientY,
      paths: pathsFor(cardPath),
      active: false,
      handedOff: false,
    };
  };

  const onCardPointerMove = (e: PointerEvent) => {
    const d = cardDrag;
    if (!d || e.pointerId !== d.pointerId) return;

    if (!d.active) {
      if (Math.hypot(e.clientX - d.startX, e.clientY - d.startY) < DRAG_THRESHOLD) return;
      d.active = true;
      // Capture the pointer so we keep receiving moves (and can detect the
      // cursor leaving the window) even outside the container.
      try { containerRef.setPointerCapture(d.pointerId); } catch { /* ignore */ }
    }

    // Hand the drag over to the OS as soon as the cursor leaves the window.
    const outsideWindow =
      e.clientX < 0 || e.clientY < 0 || e.clientX > window.innerWidth || e.clientY > window.innerHeight;
    if (outsideWindow && !d.handedOff) {
      d.handedOff = true;
      const paths = d.paths;
      const thumb = thumbUrl(paths[0]);
      // 拖拽图标必须是一个真实文件路径，data URL 交给原生拖拽只会失败。
      const icon = thumb && !thumb.startsWith("data:") ? thumb : paths[0];
      endCardDrag();
      suppressClickUntil = Date.now() + 400;
      void startDrag({ item: paths, icon }).catch((err) => console.error("拖出文件失败:", err));
      return;
    }

    ui.setTrashHover(isOverTrash(e.clientX, e.clientY));
    setDragPreview({ x: e.clientX, y: e.clientY, path: d.paths[0], count: d.paths.length });
  };

  const onCardPointerUp = (e: PointerEvent) => {
    const d = cardDrag;
    if (!d || e.pointerId !== d.pointerId) return;
    const wasActive = d.active;
    const handedOff = d.handedOff;
    const paths = d.paths;
    const droppedOnTrash = isOverTrash(e.clientX, e.clientY);
    endCardDrag();
    if (!wasActive || handedOff) return;
    suppressClickUntil = Date.now() + 300;
    if (!droppedOnTrash) return;

    const n = paths.length;
    ui.requestConfirm({
      title: "移入回收站",
      message: n > 1 ? `确定要将选中的 ${n} 张图片移入回收站吗？` : "确定要将这张图片移入回收站吗？",
      confirmText: "移入回收站",
      danger: true,
      onConfirm: async () => {
        await files.trashFiles(paths);
        await files.loadFiles(0);
        ui.showToast(n > 1 ? `已移入回收站 ${n} 张` : "已移入回收站");
      },
    });
  };

  const onCardPointerCancel = (e: PointerEvent) => {
    const d = cardDrag;
    if (!d || e.pointerId !== d.pointerId) return;
    endCardDrag();
  };

  /** 卡片进出视口 → 维护窗口，并安排一次去抖提交。 */
  const onCardVisibility = (path: string, inView: boolean) => {
    if (inView) visiblePaths.add(path);
    else visiblePaths.delete(path);
    scheduleFlush();
  };

  /**
   * 安排一次窗口提交。
   *
   * 每来一个视口变化就**重置**计时器（去抖，不是节流）—— 只要还在滑，就一个
   * 请求都不发；停稳 120ms 后把整屏一次性提交。
   */
  const scheduleFlush = () => {
    if (disposed) return;
    if (flushTimer) window.clearTimeout(flushTimer);
    flushTimer = window.setTimeout(() => {
      flushTimer = 0;
      void flushWindow();
    }, FLUSH_DEBOUNCE_MS);
  };

  /**
   * 提交当前视口窗口。
   *
   * `window` 参数同时承担两个职责：告诉后端「要这几张图的地址」，以及「这就是
   * 用户此刻在看的东西」—— 后端据此把窗口外那些还没做完的排队任务降级回整库
   * 队列。所以**已经拿到图的路径也要带上**：它们必须出现在窗口定义里，否则那些
   * 还在排队的邻居会被误判成「已经滑过去」而降级。
   *
   * 这里刻意**不回退原图**：一张 4000×3000 的原片解码要占 50 MB，整屏几十张
   * 一起解就是几百 MB 加肉眼可见的卡顿。后台几秒内就会推送结果，等一等远比解
   * 原图划算；而且现在有 32px 模糊占位图垫着，等待期间也不是一片空白。
   */
  const flushWindow = async () => {
    const libId = activeLibraryId();
    if (!libId || disposed) return;

    // 顺手剔除已经不在列表里的残留（过滤 / 搜索 / 换页都会产生）。
    const win: string[] = [];
    for (const p of visiblePaths) {
      if (livePaths.has(p)) win.push(p);
      else visiblePaths.delete(p);
    }
    if (win.length === 0) return;

    try {
      const items = await api.getThumbnails(libId, win, win);
      if (loggedWindows < LOG_WINDOWS) {
        loggedWindows += 1;
        const pending = items.filter((i) => i.status === "pending").length;
        const withPh = items.filter((i) => i.placeholder).length;
        void api.diagLog(
          "window",
          `${items.length} 张 · 命中 ${items.length - pending} · 待生成 ${pending} · 带占位 ${withPh}`,
        );
      }
      for (const it of items) {
        if (it.placeholder) setPlaceholderUrl(it.path, it.placeholder);
        if (it.status === "ready") setThumbUrl(it.path, it.thumb);
        else if (it.status === "fallback") setThumbUrl(it.path, it.path);
        // pending：什么都不做，等后台的 thumb-placeholder / thumb-ready 推回来。
      }
    } catch (err) {
      void api.diagLog("window", `ERROR ${String(err)} ${trail(win[0] ?? "")}`);
    }
  };

  // ── 虚拟化（网格 / 列表） ───────────────────────────
  //
  // 以前 <For each={files.files()}> 会把整库（实测 2400 张）全部挂进 DOM，只靠
  // CSS `content-visibility` 顶着。在软件渲染（GPU 被禁）的机器上，几百个骨架屏
  // 同时绘制本身就在抢解码线程的 CPU；而且每个卡片各自挂一个
  // IntersectionObserver，窗口集合还要靠回调慢慢收敛。
  //
  // 现在只挂「可见 ± 2 行」的卡片，并把渲染范围**直接**当作后端要的视口窗口 ——
  // 不再依赖观察器：范围由 scrollTop 同步算出来，滑得再快也不会漏报。
  const [viewport, setViewport] = createSignal({ top: 0, width: 0, height: 0 });

  interface VirtualLayout {
    /** 本次要渲染的卡片切片。 */
    slice: FileInfo[];
    /** 切片在完整列表里的起始下标（双击进查看器要用全局下标）。 */
    start: number;
    /** 整个列表撑起的滚动高度。 */
    totalHeight: number;
    /** 切片相对列表顶部的偏移。 */
    offsetY: number;
    /** 网格列数；列表模式恒为 1。 */
    columns: number;
    /** 列模板，直接写进 `grid-template-columns`。 */
    template: string;
  }

  /** 把容器的滚动位置与尺寸同步进信号（同一帧内的多次调用合并成一次）。 */
  let viewportRaf = 0;
  const syncViewport = () => {
    viewportRaf = 0;
    if (!containerRef) return;
    const next = {
      top: containerRef.scrollTop,
      width: containerRef.clientWidth,
      height: containerRef.clientHeight,
    };
    const prev = viewport();
    if (prev.top !== next.top || prev.width !== next.width || prev.height !== next.height) {
      setViewport(next);
    }
  };
  const scheduleViewport = () => {
    if (viewportRaf) return;
    viewportRaf = requestAnimationFrame(syncViewport);
  };

  const virtual = createMemo<VirtualLayout | null>(() => {
    const m = ui.viewMode();
    if (m === "masonry") return null; // 瀑布流列高不均，不做虚拟化

    const list = files.files();
    const total = list.length;
    const v = viewport();
    const card = ui.thumbnailSize();
    if (total === 0 || v.width <= 0 || v.height <= 0) return null;

    const inner = Math.max(card, v.width - CONTAINER_PAD * 2);
    const columns =
      m === "list" ? 1 : Math.max(1, Math.floor((inner + GRID_GAP) / (card + GRID_GAP)));
    const rowH = m === "list" ? LIST_ROW_H : card + 40 + GRID_GAP;
    const rows = Math.ceil(total / columns);

    const firstRow = Math.max(0, Math.floor(v.top / rowH) - OVERSCAN_ROWS);
    const lastRow = Math.min(rows, Math.ceil((v.top + v.height) / rowH) + OVERSCAN_ROWS);
    const start = Math.min(total, firstRow * columns);
    const end = Math.min(total, lastRow * columns);

    return {
      slice: list.slice(start, end),
      start,
      totalHeight: Math.max(0, rows * rowH),
      offsetY: firstRow * rowH,
      columns,
      template: `repeat(${columns}, minmax(0, 1fr))`,
    };
  });

  /**
   * 视口窗口 = 当前渲染范围。
   *
   * 范围变化（滚动跨过一行）就重算窗口并安排一次去抖提交 —— 去抖本身保证了
   * 「还在滑就一个请求都不发」，所以这里可以放心地跟着滚动同步更新。
   *
   * 它同时承担「切视图模式时清空旧窗口」的职责：切模式会让整批卡片卸载，旧
   * 路径若不清掉就会一直挂在窗口定义里，把真正可见的那批挤出去。两件事写在
   * 同一个 effect 里，是为了避免两个 effect 争抢 `visiblePaths` 的执行顺序问题。
   */
  createEffect(on([ui.viewMode, virtual], ([m, v]) => {
    visiblePaths.clear();
    if (m !== "masonry" && v) {
      const list = files.files();
      for (let i = v.start; i < v.start + v.slice.length; i++) {
        const p = list[i]?.path;
        if (p) visiblePaths.add(p);
      }
    }
    scheduleFlush();
  }));

  /**
   * 信息面板开 / 关会改变网格宽度 → 列数变化 → 同一张图换了行。
   *
   * 点击时它明明在眼前，面板一开网格变窄、列数变少，它就被推到更靠后的行，
   * 于是滑出可视区 —— 用户感觉"点了就跳走了"，还得自己找回来。
   *
   * 列数变化只改行号、不改行高，所以把 scrollTop 平移「行差 × 行高」就能让它
   * 留在原来的纵向位置（不是"滚到它可见"，而是逐像素不动）。
   *
   * 时机选 rAF 而不是微任务：rAF 一定在 DOM 更新之后、本帧绘制之前。此时
   * `containerRef.clientWidth` 已经是新宽度，而 `virtual()` 里的列数还是旧的
   * （ResizeObserver 要等本帧的 rAF 回调之后才派发）—— 正好一旧一新可以对比；
   * 而且用户不会看到"先跳到别处再跳回来"的闪烁。
   */
  let anchorPath: string | null = null;
  let anchorRaf = 0;

  const findCardEl = (root: HTMLElement, path: string): HTMLElement | null => {
    const esc = typeof CSS !== "undefined" && CSS.escape
      ? CSS.escape(path)
      : path.replace(/["\\]/g, "\\$&");
    return root.querySelector<HTMLElement>(`[data-path="${esc}"]`);
  };

  const keepFocusedInPlace = () => {
    const c = containerRef;
    // 关面板时 onClose 会先把 infoPanelFile 清空，所以退回"最后打开过的那张"
    const path = ui.infoPanelFile() ?? anchorPath;
    if (!c || !path) return;
    anchorPath = path;

    const v = virtual();
    if (!v) {
      // 瀑布流没有虚拟化（CSS columns 列高不均算不出行位置）→ 用元素实测位置兜底
      const el = findCardEl(c, path);
      if (!el) return;
      const cr = c.getBoundingClientRect();
      const er = el.getBoundingClientRect();
      if (er.top < cr.top) c.scrollTop += er.top - cr.top - GRID_GAP;
      else if (er.bottom > cr.bottom) c.scrollTop += er.bottom - cr.bottom + GRID_GAP;
      return;
    }

    const list = files.files();
    const index = list.findIndex((f) => f.path === path);
    if (index < 0) return;

    const card = size();
    const isList = mode() === "list";
    const inner = Math.max(card, c.clientWidth - CONTAINER_PAD * 2);
    const cols = isList ? 1
      : Math.max(1, Math.floor((inner + GRID_GAP) / (card + GRID_GAP)));
    if (cols === v.columns) return;   // 宽度变了但列数没变 → 位置本来就不动

    const rowH = isList ? LIST_ROW_H : card + 40 + GRID_GAP;
    const delta = (Math.floor(index / cols) - Math.floor(index / v.columns)) * rowH;
    if (!delta) return;

    // 必须先记下旧位置再让新宽度进信号：列数一变，撑起滚动高度的行数也跟着变，
    // 浏览器可能趁机把 scrollTop 夹一下；用绝对目标值（旧位置 + 行差）算才稳。
    const from = c.scrollTop;
    syncViewport();                                        // 宽度进信号 → 列数 / 总高度重建
    c.scrollTop = Math.max(0, from + delta);               // 此刻上界已是新布局的
    syncViewport();                                        // 让切片立刻跟上新位置
  };

  // 记住最后打开过信息面板的那张图（关面板时 infoPanelFile 会被置空）
  createEffect(on(() => ui.infoPanelFile(), (p) => { if (p) anchorPath = p; }));

  createEffect(on(() => ui.infoPanelOpen(), () => {
    if (anchorRaf) cancelAnimationFrame(anchorRaf);
    anchorRaf = requestAnimationFrame(() => { anchorRaf = 0; keepFocusedInPlace(); });
  }, { defer: true }));

  /**
   * 兜底重扫：后台队列跑空后，把仍然没有结果的图重新问一遍。
   *
   * 用来补上「请求发出去时事件订阅还没生效」「渲染恰好错过一帧」这类竞态 ——
   * 后端有内存结果表，重问是零成本命中，不会再解一次码。
   *
   * 注意这里**不传窗口**：整库重扫跟「用户在看哪一屏」无关，若当成窗口提交，
   * 会把当前视口里正在排队的任务错误地降级掉。
   */
  const resyncMissing = async (missing: string[]) => {
    const libId = activeLibraryId();
    if (!libId) return;
    for (let i = 0; i < missing.length; i += RESYNC_CHUNK) {
      if (disposed) return;
      try {
        const items = await api.getThumbnails(libId, missing.slice(i, i + RESYNC_CHUNK));
        for (const it of items) {
          if (it.placeholder) setPlaceholderUrl(it.path, it.placeholder);
          if (it.status === "ready") setThumbUrl(it.path, it.thumb);
          else if (it.status === "fallback") setThumbUrl(it.path, it.path);
        }
      } catch (err) {
        void api.diagLog("resync", `ERROR ${String(err)}`);
        return;
      }
      await new Promise((r) => setTimeout(r, 0));
    }
  };

  /**
   * 最后的兜底：asset 协议（`http://asset.localhost/…`）加载失败时，直接向后端
   * 要一份 data URL。这样即使某台机器上 asset 协议不可用，缩略图也能显示出来，
   * 不至于全是空白。
   */
  const fallbackToDataUrl = async (filePath: string): Promise<boolean> => {
    const libId = activeLibraryId();
    if (!libId) return false;
    try {
      const dataUrl = await api.readThumbDataUrl(libId, filePath);
      if (dataUrl) {
        setThumbUrl(filePath, dataUrl);
        void api.diagLog("img", `data-url-ok ${trail(filePath)}`);
        return true;
      }
      void api.diagLog("img", `data-url-miss ${trail(filePath)}`);
      return false;
    } catch (err) {
      void api.diagLog("img", `data-url-error ${String(err)} ${trail(filePath)}`);
      return false;
    }
  };

  /** 图片加载失败 → 记日志 + 尝试 data URL 兜底。返回 true 表示已换新地址。 */
  const handleImgError = async (filePath: string, src: string): Promise<boolean> => {
    void api.diagLog("img", `load-error ${trail(filePath)} src=${src.slice(0, 90)}`);
    return fallbackToDataUrl(filePath);
  };

  onMount(() => {
    files.loadFiles(0);

    // 后台缩略图任务完成一批就推送一次结果。
    let unlisten: (() => void) | undefined;
    let unlistenProgress: (() => void) | undefined;
    let unlistenPlaceholder: (() => void) | undefined;

    listen<ThumbReadyItem[]>("thumb-ready", (event) => {
      const payload = event.payload;
      if (!payload?.length) return;
      for (const item of payload) {
        setThumbUrl(item.path, item.thumb ?? item.path);
      }
    })
      .then((fn) => { unlisten = fn; })
      .catch((err) => console.error("订阅缩略图事件失败:", err));

    // 32px 模糊占位图 —— 比真图早得多到达（只读文件头取 EXIF 内嵌图）。有了它，
    // 卡片从"一片空白"变成"模糊的轮廓"，真图随后淡入覆盖。
    listen<ThumbPlaceholderItem[]>("thumb-placeholder", (event) => {
      const payload = event.payload;
      if (!payload?.length) return;
      for (const item of payload) {
        setPlaceholderUrl(item.path, item.data);
      }
    })
      .then((fn) => { unlistenPlaceholder = fn; })
      .catch(() => { /* 忽略 */ });

    // 兜底：后台队列跑空后，把仍然没有结果的图重新问一遍。这能补上「卡片在
    // 事件订阅生效之前就发了请求」这类竞态 —— 此时结果已经缓存在后端，重新
    // 查询会立刻从内存返回 ready / fallback，不会再解一次码。
    listen<ThumbProgress>("thumb-progress", (event) => {
      if (event.payload?.running) return;
      const missing = files.files().filter((f) => !thumbUrl(f.path)).map((f) => f.path);
      if (missing.length === 0) return;
      void resyncMissing(missing);
    })
      .then((fn) => { unlistenProgress = fn; })
      .catch(() => { /* 忽略 */ });

    onCleanup(() => {
      disposed = true;
      if (flushTimer) window.clearTimeout(flushTimer);
      unlisten?.();
      unlistenProgress?.();
      unlistenPlaceholder?.();
    });
  });

  // 切换图库时清空缓存与窗口，让新库的图片重新走一遍请求。
  createEffect(on(activeLibraryId, () => {
    visiblePaths.clear();
    clearThumbUrls();
  }));

  // 列表内容变了（分页追加 / 过滤 / 搜索）就刷新「现存路径」表。
  createEffect(on(files.files, (list) => {
    livePaths.clear();
    for (const f of list) livePaths.add(f.path);
  }));

  const handleScroll = () => {
    if (!containerRef) return;
    // 交互感知节流：告诉后台「用户正在滚动」，接下来约 0.5 秒它会暂停整库
    // 预热、只做当前视口的任务，把 CPU 让给渲染。函数内部已限流（120ms）+
    // fire-and-forget，所以这里可以每帧无脑调。
    api.notifyInteraction();
    // 先把滚动位置同步给虚拟化，再判断要不要续拉下一页 —— 两者互不依赖，
    // 但先更新视口能让渲染范围在同一帧内跟上。
    scheduleViewport();
    const { scrollTop, scrollHeight, clientHeight } = containerRef;
    if (scrollHeight - scrollTop - clientHeight < 600) {
      // 上一次分页还没回来就不要再发，否则同一页会被拉好几次。
      if (files.isLoading()) return;
      const currentCount = files.files().length;
      if (currentCount < files.totalFiles()) files.loadFiles(currentCount);
    }
  };

  // ── Marquee helpers ────────────────────────────

  /** Return the paths of every rendered card intersecting `r` (client coords). */
  const collectHits = (r: MarqueeRect): string[] => {
    const hits: string[] = [];
    const nodes = containerRef.querySelectorAll<HTMLElement>("[data-path]");
    for (let i = 0; i < nodes.length; i++) {
      const b = nodes[i].getBoundingClientRect();
      if (b.right < r.x || b.left > r.x + r.w || b.bottom < r.y || b.top > r.y + r.h) continue;
      const p = nodes[i].dataset.path;
      if (p) hits.push(p);
    }
    return hits;
  };

  /** Recompute the rectangle + selection for the current pointer position. */
  const refreshMarquee = (clientX: number, clientY: number) => {
    const d = drag;
    if (!d) return;
    const r: MarqueeRect = {
      x: Math.min(d.startX, clientX),
      y: Math.min(d.startY, clientY),
      w: Math.abs(clientX - d.startX),
      h: Math.abs(clientY - d.startY),
    };
    setMarquee(r);
    files.setSelection(collectHits(r), d.additive, d.base);
  };

  const stopAutoScroll = () => {
    if (rafId) { cancelAnimationFrame(rafId); rafId = 0; }
  };

  /** Scroll the container while the pointer hovers near its top/bottom edge. */
  const startAutoScroll = () => {
    if (rafId) return;
    const step = () => {
      rafId = 0;
      const d = drag;
      if (!d || !d.active) return;
      const box = containerRef.getBoundingClientRect();
      const y = d.clientY;
      let delta = 0;
      if (y < box.top + EDGE_ZONE) {
        delta = -SCROLL_SPEED * Math.min(1, (box.top + EDGE_ZONE - y) / EDGE_ZONE);
      } else if (y > box.bottom - EDGE_ZONE) {
        delta = SCROLL_SPEED * Math.min(1, (y - (box.bottom - EDGE_ZONE)) / EDGE_ZONE);
      }
      if (delta) {
        const before = containerRef.scrollTop;
        containerRef.scrollTop = before + delta;
        if (containerRef.scrollTop !== before) refreshMarquee(d.clientX, d.clientY);
      }
      rafId = requestAnimationFrame(step);
    };
    rafId = requestAnimationFrame(step);
  };

  const detachPointerListeners = () => {
    window.removeEventListener("mousemove", onDocMouseMove);
    window.removeEventListener("mouseup", onDocMouseUp);
    window.removeEventListener("scroll", onDocScroll, true);
  };

  const onDocMouseMove = (e: MouseEvent) => {
    const d = drag;
    if (!d) return;
    d.clientX = e.clientX;
    d.clientY = e.clientY;
    if (!d.active) {
      if (Math.hypot(e.clientX - d.startX, e.clientY - d.startY) < DRAG_THRESHOLD) return;
      d.active = true;
      startAutoScroll();
    }
    // 拖拽框选同样会让后台让路（内部限流，见 api.notifyInteraction）。
    api.notifyInteraction();
    refreshMarquee(e.clientX, e.clientY);
  };

  const onDocScroll = () => {
    const d = drag;
    if (!d || !d.active) return;
    refreshMarquee(d.clientX, d.clientY);
  };

  const onDocMouseUp = () => {
    const d = drag;
    drag = null;
    stopAutoScroll();
    detachPointerListeners();
    setMarquee(null);
    if (d?.active) {
      // Swallow the click that the browser emits after a drag.
      suppressClickUntil = Date.now() + 200;
    } else if (d && d.onEmpty && !d.additive) {
      // Plain click on blank space clears the selection.
      files.clearSelection();
    }
  };

  const onContainerMouseDown = (e: MouseEvent) => {
    if (e.button !== 0) return;
    const t = e.target as HTMLElement | null;
    if (!t || t.closest("input, select, textarea, button, .thumbnail-actions")) return;

    const cardEl = t.closest("[data-path]") as HTMLElement | null;
    const cardPath = cardEl?.dataset.path;

    stopAutoScroll();
    detachPointerListeners();
    drag = null;

    if (cardPath) {
      // Card presses are handled by the pointer-driven drag above.
      return;
    }

    // Pressed on blank space -> marquee selection.
    drag = {
      startX: e.clientX, startY: e.clientY,
      clientX: e.clientX, clientY: e.clientY,
      additive: e.ctrlKey || e.metaKey,
      base: new Set(files.selectedFiles()),
      active: false,
      onEmpty: true,
    };
    window.addEventListener("mousemove", onDocMouseMove);
    window.addEventListener("mouseup", onDocMouseUp);
    window.addEventListener("scroll", onDocScroll, true);
  };

  const onClickCapture = (e: MouseEvent) => {
    if (Date.now() < suppressClickUntil) {
      e.stopPropagation();
      e.preventDefault();
    }
  };

  onMount(() => {
    containerRef.addEventListener("scroll", handleScroll, { passive: true });
    containerRef.addEventListener("mousedown", onContainerMouseDown);
    // Window-level capture so a click emitted right after a drag is swallowed
    // everywhere (e.g. it must not also "open" the trash it was dropped on).
    window.addEventListener("click", onClickCapture, true);

    // 虚拟化需要知道容器有多大：先量一次，之后交给 ResizeObserver
    // （窗口缩放、侧栏折叠、缩略图尺寸滑块都会改变它）。
    syncViewport();
    let ro: ResizeObserver | undefined;
    if (typeof ResizeObserver !== "undefined") {
      ro = new ResizeObserver(scheduleViewport);
      ro.observe(containerRef);
    }

    onCleanup(() => {
      containerRef.removeEventListener("scroll", handleScroll);
      containerRef.removeEventListener("mousedown", onContainerMouseDown);
      window.removeEventListener("click", onClickCapture, true);
      if (viewportRaf) cancelAnimationFrame(viewportRaf);
      ro?.disconnect();
      stopAutoScroll();
      drag = null;
      endCardDrag();
      detachPointerListeners();
    });
  });

  const size = () => ui.thumbnailSize();
  const mode = () => ui.viewMode();

  /** 当前视图的人类可读名称（Pixcall 式：工具栏显示「你在看哪个视图」）。 */
  const viewLabel = () => {
    const v = files.currentView();
    switch (v.kind) {
      case "all": return "所有图片";
      case "recent": return "最近新增";
      case "favorite": return "收藏";
      case "rated": return "已评分";
      case "color": return `颜色 · ${v.color ?? ""}`;
      case "folder": return `文件夹 · ${(v.folder ?? "").split(/[\\/]/).filter(Boolean).pop() ?? ""}`;
      case "tag": return "标签";
      case "trash": return "回收站";
      default: return "";
    }
  };

  /** Show the context menu for one or more files (right-click). */
  const openFileContextMenu = (e: MouseEvent, path: string) => {
    e.preventDefault();
    const selected = files.selectedFiles();
    // If right-clicking an unselected file, select it; if part of a
    // multi-selection, act on the whole selection.
    let paths: string[];
    if (selected.has(path)) {
      paths = [...selected];
    } else {
      files.setSelection([path]);
      paths = [path];
    }
    ui.setContextMenu({
      x: e.clientX,
      y: e.clientY,
      target: { kind: "files", paths },
    });
  };

  const formatSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes}B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
  };

  const s = {
    bg: "var(--bg-primary)",
    border: "var(--border-color)",
    text: "var(--text-primary)",
    muted: "var(--text-muted)",
    accent: "var(--accent)",
    hover: "var(--bg-hover)",
    secondary: "var(--bg-secondary)",
  };

  return (
    <div class="flex flex-col flex-1 overflow-hidden" style={{ background: s.bg }}>
      {/* Toolbar */}
      <div class="flex items-center gap-3 px-3 py-1.5 flex-shrink-0" style={{ background: s.secondary, "border-bottom": `1px solid ${s.border}` }}>
        {/* Sort */}
        <select
          class="input text-xs w-20 py-1"
          value={files.currentSort().by}
          onChange={(e) => { files.setCurrentSort({ by: e.currentTarget.value, order: files.currentSort().order }); files.loadFiles(0); }}
        >
          <option value="mtime">日期</option>
          <option value="name">名称</option>
          <option value="size">大小</option>
          <option value="rating">评分</option>
        </select>
        <button
          class="btn-ghost text-xs px-2 py-1 flex"
          onClick={() => {
            const newOrder = files.currentSort().order === "DESC" ? "ASC" : "DESC";
            files.setCurrentSort({ ...files.currentSort(), order: newOrder });
            files.loadFiles(0);
          }}
          title={files.currentSort().order === "DESC" ? "降序" : "升序"}
        >
          {files.currentSort().order === "DESC" ? <IconArrowDown size={14} /> : <IconArrowUp size={14} />}
        </button>

        {/* Filter by type */}
        <select
          class="input text-xs w-24 py-1"
          value={files.filterExt() || ""}
          onChange={(e) => { files.setFilterExt(e.currentTarget.value || undefined); files.loadFiles(0); }}
        >
          <option value="">全部</option>
          <option value="jpg">JPG</option>
          <option value="png">PNG</option>
          <option value="webp">WebP</option>
          <option value="gif">GIF</option>
          <option value="svg">SVG</option>
          <option value="psd">PSD</option>
          <option value="raw">RAW</option>
        </select>

        <div class="flex-1" />

        {/* 当前视图（智能视图 / 文件夹 / 颜色 / 标签 / 回收站） */}
        <span class="text-xs font-medium" style={{ color: s.text }} title={viewLabel()}>
          {viewLabel()}
        </span>

        {/* File count */}
        <span class="text-xs" style={{ color: s.muted }}>
          {files.files().length} / {files.totalFiles()} 个文件
        </span>

        <Show when={files.selectedFiles().size > 0}>
          <span class="text-xs" style={{ color: s.accent }}>
            已选 {files.selectedFiles().size} 个
          </span>
        </Show>

        {/* View mode toggle */}
        <div class="flex items-center gap-0.5 rounded p-0.5" style={{ background: "var(--bg-tertiary)" }}>
          <button
            class="flex px-2 py-1 rounded transition-colors"
            style={{ background: mode() === "grid" ? s.accent : "transparent", color: mode() === "grid" ? "var(--accent-contrast)" : s.muted }}
            onClick={() => ui.setViewMode("grid")}
            title="网格视图"
          ><IconGrid size={14} /></button>
          <button
            class="flex px-2 py-1 rounded transition-colors"
            style={{ background: mode() === "masonry" ? s.accent : "transparent", color: mode() === "masonry" ? "var(--accent-contrast)" : s.muted }}
            onClick={() => ui.setViewMode("masonry")}
            title="瀑布流视图"
          ><IconMasonry size={14} /></button>
          <button
            class="flex px-2 py-1 rounded transition-colors"
            style={{ background: mode() === "list" ? s.accent : "transparent", color: mode() === "list" ? "var(--accent-contrast)" : s.muted }}
            onClick={() => ui.setViewMode("list")}
            title="列表视图"
          ><IconMenu size={14} /></button>
        </div>

        {/* Thumbnail size */}
        <input
          type="range" min="120" max="400" value={ui.thumbnailSize()}
          onInput={(e) => ui.setThumbnailSize(parseInt(e.currentTarget.value))}
          class="w-20"
          title={`${ui.thumbnailSize()}px`}
        />
      </div>

      {/* Content */}
      <div ref={containerRef} class="virtual-scroll-container p-3"
        onPointerDown={onCardPointerDown}
        onPointerMove={onCardPointerMove}
        onPointerUp={onCardPointerUp}
        onPointerCancel={onCardPointerCancel}>
        <Show when={files.files().length > 0} fallback={
          <div class="flex items-center justify-center h-full" style={{ color: s.muted }}>
            <div class="text-center">
              <div class="mb-3 flex justify-center opacity-50">
                <IconImage size={40} />
              </div>
              <p class="text-sm">暂无图片</p>
            </div>
          </div>
        }>
          {/* Grid Mode —— 虚拟化：只挂「可见 ± 2 行」的卡片 */}
          <Show when={mode() === "grid"}>
            <Show when={virtual()}>
              {(v) => (
                <div style={{ position: "relative", height: `${v().totalHeight}px` }}>
                  <div class="grid gap-2" style={{
                    position: "absolute",
                    top: "0",
                    left: "0",
                    right: "0",
                    "grid-template-columns": v().template,
                    "grid-auto-rows": `${size() + 40}px`,
                    transform: `translateY(${v().offsetY}px)`,
                  }}>
                    <For each={v().slice}>
                      {(file, index) => (
                        <ThumbnailCard file={file} thumbnailUrl={thumbUrl(file.path)}
                          placeholderUrl={placeholderUrl(file.path)}
                          isSelected={files.selectedFiles().has(file.path)} size={size()}
                          onSelect={(multi) => files.toggleSelect(file.path, multi)}
                          onRatingChange={(r) => files.updateRating(file.path, r)}
                          onFavoriteChange={(fav) => files.setFavorite(file.path, fav)}
                          onOpen={() => { ui.setInfoPanelFile(file.path); ui.setInfoPanelOpen(true); }}
                          onDoubleClick={() => viewer.open(files.files(), v().start + index())}
                          onImgError={(src) => handleImgError(file.path, src)}
                          onContextMenu={openFileContextMenu} />
                      )}
                    </For>
                  </div>
                </div>
              )}
            </Show>
          </Show>

          {/* Masonry Mode (CSS columns) */}
          <Show when={mode() === "masonry"}>
            <div style={{
              "column-count": Math.max(2, Math.floor(window.innerWidth / (size() + 16))),
              "column-gap": "8px",
            }}>
              <For each={files.files()}>
                {(file, index) => (
                  <div style={{ "break-inside": "avoid", "margin-bottom": "8px" }}>
                    <ThumbnailCard file={file} thumbnailUrl={thumbUrl(file.path)}
                      placeholderUrl={placeholderUrl(file.path)}
                      isSelected={files.selectedFiles().has(file.path)} size={size()}
                      onSelect={(multi) => files.toggleSelect(file.path, multi)}
                      onRatingChange={(r) => files.updateRating(file.path, r)}
                      onFavoriteChange={(fav) => files.setFavorite(file.path, fav)}
                      onOpen={() => { ui.setInfoPanelFile(file.path); ui.setInfoPanelOpen(true); }}
                      onDoubleClick={() => viewer.open(files.files(), index())}
                      onVisibleChange={(inView) => onCardVisibility(file.path, inView)}
                      onImgError={(src) => handleImgError(file.path, src)}
                      onContextMenu={openFileContextMenu} />
                  </div>
                )}
              </For>
            </div>
          </Show>

          {/* List Mode —— 同样虚拟化：行高固定，按行切片 */}
          <Show when={mode() === "list"}>
            <Show when={virtual()}>
              {(v) => (
                <div style={{ position: "relative", height: `${v().totalHeight}px` }}>
                  <div class="flex flex-col" style={{
                    position: "absolute", top: "0", left: "0", right: "0",
                    transform: `translateY(${v().offsetY}px)`,
                  }}>
                    <For each={v().slice}>
                      {(file, index) => (
                        <div
                          class="flex items-center gap-3 px-3 py-2 rounded cursor-pointer transition-colors"
                          data-path={file.path}
                          style={{
                            height: "56px",
                            "margin-bottom": "2px",
                            background: files.selectedFiles().has(file.path) ? "var(--accent-bg)" : "transparent",
                          }}
                          onClick={(e) => { files.toggleSelect(file.path, e.ctrlKey || e.metaKey); ui.setInfoPanelFile(file.path); ui.setInfoPanelOpen(true); }}
                          onContextMenu={(e) => openFileContextMenu(e, file.path)}
                          onDblClick={() => viewer.open(files.files(), v().start + index())}
                        >
                          <div class="relative w-10 h-10 flex-shrink-0 rounded overflow-hidden" style={{ background: "var(--bg-tertiary)" }}>
                            <Show when={placeholderUrl(file.path)}>
                              {(ph) => (
                                <img src={toDisplaySrc(ph())} alt="" aria-hidden="true"
                                  class="absolute inset-0 w-full h-full object-cover"
                                  draggable={false} decoding="async" />
                              )}
                            </Show>
                            <Show when={thumbUrl(file.path)} fallback={
                              <Show when={!placeholderUrl(file.path)}>
                                <div class="w-full h-full skeleton" />
                              </Show>
                            }>
                              {(url) => (
                                <img
                                  src={toDisplaySrc(url())}
                                  alt={file.filename} class="relative w-full h-full object-cover" draggable={false}
                                  decoding="async"
                                  onError={async (e) => {
                                    const el = e.currentTarget as HTMLImageElement;
                                    const fixed = await handleImgError(file.path, el.src);
                                    if (!fixed) el.style.visibility = "hidden";
                                  }}
                                />
                              )}
                            </Show>
                          </div>
                          <div class="flex-1 min-w-0">
                            <div class="text-sm font-medium truncate" style={{ color: s.text }}>{file.filename}</div>
                            <div class="text-xs" style={{ color: s.muted }}>
                              {file.width && file.height ? `${file.width}×${file.height}` : formatSize(file.size_bytes)} · {file.extension.toUpperCase()}
                            </div>
                          </div>
                          <div class="text-xs" style={{ color: s.muted }}>{new Date(file.mtime).toLocaleDateString()}</div>
                          <button class="flex p-0.5 rounded transition-colors flex-shrink-0"
                            style={{ color: file.favorite ? "#ff6b6b" : "var(--text-muted)" }}
                            title={file.favorite ? "取消收藏" : "收藏"}
                            onClick={(e) => { e.stopPropagation(); files.setFavorite(file.path, !file.favorite); }}>
                            <IconHeart size={12} />
                          </button>
                          <Show when={file.rating}>
                            <div class="flex gap-0.5" style={{ color: "var(--star-color)" }}>
                              {Array.from({ length: file.rating || 0 }, (_, i) => (
                                <IconStarFilled size={11} />
                              ))}
                            </div>
                          </Show>
                        </div>
                      )}
                    </For>
                  </div>
                </div>
              )}
            </Show>
          </Show>
        </Show>
      </div>

      {/* Floating drag preview that follows the cursor while dragging a card */}
      <Show when={dragPreview()}>
        {(p) => (
          <div
            class="fixed z-[60] pointer-events-none rounded-lg overflow-hidden shadow-2xl"
            style={{
              left: `${p().x + 14}px`,
              top: `${p().y + 14}px`,
              width: "84px",
              border: "2px solid var(--accent)",
              background: "var(--bg-secondary)",
            }}
          >
            <img
              src={thumbUrl(p().path) ? toDisplaySrc(thumbUrl(p().path) as string) : "/placeholder.svg"}
              class="w-full h-20 object-cover" alt="" draggable={false} />
            <Show when={p().count > 1}>
              <div class="absolute bottom-0 right-0 px-1.5 py-0.5 text-2xs rounded-tl"
                style={{ background: "var(--accent)", color: "var(--accent-contrast)" }}>{p().count}</div>
            </Show>
          </div>
        )}
      </Show>

      {/* Marquee selection box (viewport-fixed; kept outside the scroll container) */}
      <Show when={marquee()}>
        {(r) => (
          <div class="marquee-box" style={{
            left: `${r().x}px`,
            top: `${r().y}px`,
            width: `${r().w}px`,
            height: `${r().h}px`,
          }} />
        )}
      </Show>
    </div>
  );
}
