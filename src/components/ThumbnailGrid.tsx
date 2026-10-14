import { For, Show, createSignal, createEffect, on, onMount, onCleanup } from "solid-js";
import { useFilesStore } from "@/stores/files";
import { useUIStore } from "@/stores/ui";
import { useViewerStore } from "@/stores/viewer";
import { useLibraryStore } from "@/stores/library";
import { useTheme } from "@/stores/theme";
import { ThumbnailCard } from "./ThumbnailCard";
import { api } from "@/lib/tauri-api";
import type { ThumbReadyItem, ThumbProgress } from "@/lib/tauri-api";
import { thumbUrl, setThumbUrl, clearThumbUrls, toDisplaySrc } from "@/lib/thumbs";
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

/** 日志里只留路径尾部，避免整条路径刷屏。 */
const trail = (p: string, n = 42): string => (p.length > n ? `…${p.slice(-n)}` : p);

export function ThumbnailGrid() {
  const files = useFilesStore();
  const ui = useUIStore();
  const viewer = useViewerStore();
  const library = useLibraryStore();
  const theme = useTheme();

  let containerRef!: HTMLDivElement;
  /** 已经发过缩略图请求的文件，避免同一张图重复 invoke。 */
  const requestedThumbs = new Set<string>();
  /** 诊断用：前若干个请求的结果会写进后端日志，方便定位「为什么不出图」。 */
  let loggedRequests = 0;
  const LOG_REQUESTS = 25;

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

  /**
   * 请求一张图的缩略图。后端只做「文件在不在」的判断：命中就立刻返回路径，
   * 未命中则把解码任务丢进后台队列并返回 `pending`，结果稍后由
   * `thumb-ready` 事件推回来。这里刻意**不回退原图** —— 那会让每张卡片都去
   * 解码几千像素的原片，几张还好，几百张一起就是几百 MB 内存和长时间卡顿。
   */
  const loadThumbnail = async (filePath: string) => {
    const libId = activeLibraryId();
    if (!libId || requestedThumbs.has(filePath)) return;
    requestedThumbs.add(filePath);
    try {
      const res = await api.getThumbnailPath(libId, filePath);
      if (loggedRequests < LOG_REQUESTS) {
        loggedRequests += 1;
        void api.diagLog("thumb", `${res.status} ${trail(filePath)} -> ${trail(res.path || "-")}`);
      }
      if (res.status === "ready") {
        setThumbUrl(filePath, res.path);
      } else if (res.status === "fallback") {
        // 生成不出来（RAW / PSD 等），交给 WebView 直接解原图。
        setThumbUrl(filePath, filePath);
      }
      // pending：什么都不做，等后台任务推送。
    } catch (err) {
      // 查询失败：允许下次可见时重试，但保持骨架屏而不是去解原图。
      requestedThumbs.delete(filePath);
      void api.diagLog("thumb", `ERROR ${String(err)} ${trail(filePath)}`);
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
    listen<ThumbReadyItem[]>("thumb-ready", (event) => {
      const payload = event.payload;
      if (!payload?.length) return;
      for (const item of payload) {
        setThumbUrl(item.path, item.thumb ?? item.path);
      }
    })
      .then((fn) => { unlisten = fn; })
      .catch((err) => console.error("订阅缩略图事件失败:", err));

    // 兜底：后台队列跑空后，把仍然没有结果的图重新问一遍。这能补上「卡片在
    // 事件订阅生效之前就发了请求」这类竞态 —— 此时结果已经缓存在后端，重新
    // 查询会立刻从内存返回 ready / fallback，不会再解一次码。
    listen<ThumbProgress>("thumb-progress", (event) => {
      if (event.payload?.running) return;
      const missing = files.files().filter((f) => !thumbUrl(f.path)).map((f) => f.path);
      if (missing.length === 0) return;
      for (const p of missing) requestedThumbs.delete(p);
      void (async () => {
        const CHUNK = 48;
        for (let i = 0; i < missing.length; i += CHUNK) {
          await Promise.all(missing.slice(i, i + CHUNK).map((p) => loadThumbnail(p)));
          if (i + CHUNK < missing.length) await new Promise((r) => setTimeout(r, 0));
        }
      })();
    })
      .then((fn) => { unlistenProgress = fn; })
      .catch(() => { /* 忽略 */ });

    onCleanup(() => { unlisten?.(); unlistenProgress?.(); });
  });

  // 切换图库时清空缓存，让新库的图片重新走一遍请求。
  createEffect(on(activeLibraryId, () => {
    requestedThumbs.clear();
    clearThumbUrls();
  }));

  const handleScroll = () => {
    if (!containerRef) return;
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
    onCleanup(() => {
      containerRef.removeEventListener("scroll", handleScroll);
      containerRef.removeEventListener("mousedown", onContainerMouseDown);
      window.removeEventListener("click", onClickCapture, true);
      stopAutoScroll();
      drag = null;
      endCardDrag();
      detachPointerListeners();
    });
  });

  const size = () => ui.thumbnailSize();
  const mode = () => ui.viewMode();

  /** 当前视图的显示名（工具栏左侧）。 */
  const viewTitle = () => {
    const v = files.currentView();
    switch (v.kind) {
      case "recent": return "最近新增";
      case "favorite": return "收藏";
      case "rated": return "已评分";
      case "color": return `颜色 · ${v.color}`;
      case "folder": return `文件夹 · ${v.folder?.split(/[\\/]/).pop() ?? ""}`;
      case "tag": return "标签";
      case "trash": return "回收站";
      default: return "所有图片";
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
      <div class="flex items-center gap-3 px-3 py-1.5 flex-shrink-0" style={{ background: s.secondary, borderBottom: `1px solid ${s.border}` }}>
        {/* 当前视图 */}
        <span class="text-xs font-medium flex-shrink-0" style={{ color: s.text }} title={viewTitle()}>
          {viewTitle()}
        </span>

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
          class="btn-ghost text-xs px-2 py-1"
          onClick={() => {
            const newOrder = files.currentSort().order === "DESC" ? "ASC" : "DESC";
            files.setCurrentSort({ ...files.currentSort(), order: newOrder });
            files.loadFiles(0);
          }}
          title={files.currentSort().order === "DESC" ? "降序" : "升序"}
        >
          {files.currentSort().order === "DESC" ? "↓" : "↑"}
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
            class="px-2 py-1 rounded text-xs transition-colors"
            style={{ background: mode() === "grid" ? s.accent : "transparent", color: mode() === "grid" ? "#fff" : s.muted }}
            onClick={() => ui.setViewMode("grid")}
            title="网格视图"
          >▦</button>
          <button
            class="px-2 py-1 rounded text-xs transition-colors"
            style={{ background: mode() === "masonry" ? s.accent : "transparent", color: mode() === "masonry" ? "#fff" : s.muted }}
            onClick={() => ui.setViewMode("masonry")}
            title="瀑布流视图"
          >▥</button>
          <button
            class="px-2 py-1 rounded text-xs transition-colors"
            style={{ background: mode() === "list" ? s.accent : "transparent", color: mode() === "list" ? "#fff" : s.muted }}
            onClick={() => ui.setViewMode("list")}
            title="列表视图"
          >☰</button>
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
              <div class="text-4xl mb-3">🖼️</div>
              <p class="text-sm">暂无图片</p>
            </div>
          </div>
        }>
          {/* Grid Mode */}
          <Show when={mode() === "grid"}>
            <div class="grid gap-2" style={{
              "grid-template-columns": `repeat(auto-fill, minmax(${size()}px, 1fr))`,
              "grid-auto-rows": `${size() + 40}px`,
            }}>
              <For each={files.files()}>
                {(file, index) => (
                  <ThumbnailCard file={file} thumbnailUrl={thumbUrl(file.path)}
                    isSelected={files.selectedFiles().has(file.path)} size={size()}
                    onSelect={(multi) => files.toggleSelect(file.path, multi)}
                    onRatingChange={(r) => files.updateRating(file.path, r)}
                    onFavoriteChange={(fav) => files.setFavorite(file.path, fav)}
                    onOpen={() => { ui.setInfoPanelFile(file.path); ui.setInfoPanelOpen(true); }}
                    onDoubleClick={() => viewer.open(files.files(), index())}
                    onVisible={() => loadThumbnail(file.path)}
                    onImgError={(src) => handleImgError(file.path, src)}
                    onContextMenu={openFileContextMenu} />
                )}
              </For>
            </div>
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
                      isSelected={files.selectedFiles().has(file.path)} size={size()}
                      onSelect={(multi) => files.toggleSelect(file.path, multi)}
                      onRatingChange={(r) => files.updateRating(file.path, r)}
                      onOpen={() => { ui.setInfoPanelFile(file.path); ui.setInfoPanelOpen(true); }}
                      onDoubleClick={() => viewer.open(files.files(), index())}
                      onVisible={() => loadThumbnail(file.path)}
                      onImgError={(src) => handleImgError(file.path, src)}
                      onContextMenu={openFileContextMenu} />
                  </div>
                )}
              </For>
            </div>
          </Show>

          {/* List Mode */}
          <Show when={mode() === "list"}>
            <div class="flex flex-col gap-0.5">
              <For each={files.files()}>
                {(file, index) => (
                  <div
                    class="flex items-center gap-3 px-3 py-2 rounded cursor-pointer transition-colors"
                    data-path={file.path}
                    style={{ background: files.selectedFiles().has(file.path) ? "var(--accent-bg)" : "transparent" }}
                    onClick={(e) => { files.toggleSelect(file.path, e.ctrlKey || e.metaKey); ui.setInfoPanelFile(file.path); ui.setInfoPanelOpen(true); }}
                    onContextMenu={(e) => openFileContextMenu(e, file.path)}
                    onDblClick={() => viewer.open(files.files(), index())}
                  >
                    <div ref={() => loadThumbnail(file.path)} class="w-10 h-10 flex-shrink-0 rounded overflow-hidden" style={{ background: "var(--bg-tertiary)" }}>
                      <Show when={thumbUrl(file.path)} fallback={<div class="w-full h-full skeleton" />}>
                        {(url) => (
                          <img
                            src={toDisplaySrc(url())}
                            alt={file.filename} class="w-full h-full object-cover" draggable={false}
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
                    <Show when={file.rating}>
                      <div class="text-xs" style={{ color: "var(--star-color)" }}>{"★".repeat(file.rating || 0)}</div>
                    </Show>
                  </div>
                )}
              </For>
            </div>
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
                style={{ background: "var(--accent)", color: "#fff" }}>{p().count}</div>
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
