import { Show, onMount, onCleanup, createSignal, createEffect, on } from "solid-js";
import { useViewerStore } from "@/stores/viewer";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { api } from "@/lib/tauri-api";
import { activeLibraryId } from "@/stores/library";
import { isVideoPath, thumbUrl, toDisplaySrc } from "@/lib/thumbs";
import { IconClose } from "@/components/icons";

/**
 * WebView 自己能渲染的位图 / 矢量格式。
 *
 * 不在这个名单里的（PSD、RAW、AI、EPS…）**不能**直接喂原图：一张 1.5 GB 的
 * PSD 会让 asset 协议把整个文件读一遍再解码失败，界面直接冻住。这类文件改用
 * 后端生成的 1600px 预览图。
 */
const NATIVE_RE = /\.(jpe?g|png|gif|webp|bmp|ico|svg|avif|tiff?|tif)$/i;

export function ImageViewer() {
  const viewer = useViewerStore();
  const [zoom, setZoom] = createSignal(1);
  const [pan, setPan] = createSignal({ x: 0, y: 0 });
  const [dragging, setDragging] = createSignal(false);
  const [dragStart, setDragStart] = createSignal({ x: 0, y: 0 });
  /** WebView 播不了的容器（mkv / avi 之类）—— 提示用户改用系统播放器 */
  const [videoError, setVideoError] = createSignal(false);

  const handleKeyDown = (e: KeyboardEvent) => {
    if (!viewer.viewerOpen()) return;
    switch (e.key) {
      case "Escape":
        viewer.close();
        break;
      case "ArrowLeft":
        e.preventDefault();
        viewer.prev();
        break;
      case "ArrowRight":
        e.preventDefault();
        viewer.next();
        break;
    }
  };

  // Reset zoom when image changes
  const currentFile = () => viewer.currentFile();
  const resetView = () => {
    setZoom(1);
    setPan({ x: 0, y: 0 });
  };

  // Watch for file changes to reset zoom
  const filePath = () => currentFile()?.path;
  let lastPath: string | undefined;
  onMount(() => {
    window.addEventListener("keydown", handleKeyDown);
    onCleanup(() => window.removeEventListener("keydown", handleKeyDown));
  });

  const handleWheel = (e: WheelEvent) => {
    // 视频有自己的原生控件，缩放 / 平移一律不接管（否则滚轮调不动音量）
    if (isVideo()) return;
    e.preventDefault();
    if (e.ctrlKey) {
      // Touchpad pinch-to-zoom (Windows precision touchpad sends Ctrl+Wheel)
      // deltaY < 0 = zoom in, deltaY > 0 = zoom out
      const delta = -e.deltaY * 0.005;
      setZoom((z) => Math.max(0.1, Math.min(10, z + delta)));
    } else {
      // Regular mouse wheel zoom
      const delta = e.deltaY > 0 ? -0.15 : 0.15;
      setZoom((z) => Math.max(0.1, Math.min(10, z + delta)));
    }
  };

  const handleMouseDown = (e: MouseEvent) => {
    if (zoom() > 1) {
      setDragging(true);
      setDragStart({ x: e.clientX - pan().x, y: e.clientY - pan().y });
    }
  };

  const handleMouseMove = (e: MouseEvent) => {
    if (dragging()) {
      setPan({ x: e.clientX - dragStart().x, y: e.clientY - dragStart().y });
    }
  };

  const handleMouseUp = () => {
    setDragging(false);
  };

  const handleBackdrop = (e: MouseEvent) => {
    if (e.target === e.currentTarget) viewer.close();
  };

  const file = () => viewer.currentFile();
  const index = () => viewer.viewerIndex();
  const total = () => viewer.viewerFiles().length;
  const isVideo = () => isVideoPath(file()?.path || "");

  // 换文件时把「播不了」的判定清掉，否则下一段视频会直接显示错误提示
  createEffect(() => {
    void file()?.path;
    setVideoError(false);
  });

  /**
   * 查看器真正要显示的图片地址。
   *
   * 原生格式直接用原图（最清晰）；PSD / RAW 这类改用后端生成的 1600px 预览图
   * —— 它们的「原图」WebView 根本解不了，硬加载只会把界面读死。
   */
  const [bigSrc, setBigSrc] = createSignal<string | null>(null);
  const [previewOnly, setPreviewOnly] = createSignal(false);

  const loadBigSrc = async (path: string) => {
    if (NATIVE_RE.test(path)) {
      setPreviewOnly(false);
      setBigSrc(convertFileSrc(path));
      return;
    }
    setPreviewOnly(true);
    // 先垫一张已有的缩略图（秒回），再去后台要 1600px 预览。
    const cached = thumbUrl(path);
    if (cached) setBigSrc(toDisplaySrc(cached));
    const libId = activeLibraryId();
    if (!libId) return;
    try {
      const res = await api.getPreviewPath(libId, path);
      if (file()?.path !== path) return;   // 已经翻到别的图了
      if (res.status === "ready") setBigSrc(toDisplaySrc(res.path));
    } catch {
      /* 预览生成失败就保持缩略图 */
    }
  };

  createEffect(on(() => file()?.path, (path) => {
    setBigSrc(null);
    if (path) void loadBigSrc(path);
  }, { defer: true }));

  onMount(() => {
    // 后台把 1600px 预览生成好后推回来 —— 正好是当前这张就换上。
    let unlisten: (() => void) | undefined;
    listen<{ path: string; thumb: string | null }[]>("preview-ready", (event) => {
      const cur = file()?.path;
      if (!cur) return;
      const hit = event.payload?.find((it) => it.path === cur);
      if (hit?.thumb) setBigSrc(toDisplaySrc(hit.thumb));
    })
      .then((fn) => { unlisten = fn; })
      .catch(() => { /* 忽略 */ });
    onCleanup(() => unlisten?.());
  });

  const formatSize = (bytes: number) => {
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  const handlePrev = (e: MouseEvent) => { e.stopPropagation(); viewer.prev(); resetView(); };
  const handleNext = (e: MouseEvent) => { e.stopPropagation(); viewer.next(); resetView(); };

  return (
    <Show when={viewer.viewerOpen() && file()}>
      <div
        class="fixed inset-0 z-50 flex items-center justify-center overflow-hidden"
        style={{ background: "rgba(0,0,0,0.92)", cursor: zoom() > 1 ? (dragging() ? "grabbing" : "grab") : "default" }}
        onClick={handleBackdrop}
        onWheel={handleWheel}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseUp}
      >
        {/* Close button */}
        <button class="absolute top-4 right-4 z-10 w-10 h-10 flex items-center justify-center rounded-full transition-colors"
          style={{ background: "rgba(255,255,255,0.1)", color: "#fff" }} onClick={viewer.close} title="关闭 (Esc)">
          <IconClose size={18} />
        </button>

        {/* Info bar */}
        <div class="absolute top-4 left-4 z-10 text-sm" style={{ color: "rgba(255,255,255,0.7)" }}>
          <span class="font-medium" style={{ color: "#fff" }}>{file()?.filename}</span>
          <span class="mx-2">·</span>
          <span>{index() + 1} / {total()}</span>
          <Show when={file()?.width && file()?.height}>
            <span class="mx-2">·</span>
            <span>{file()?.width}×{file()?.height}</span>
          </Show>
          <span class="mx-2">·</span>
          <span>{formatSize(file()?.size_bytes || 0)}</span>
          <span class="mx-2">·</span>
          <span>{Math.round(zoom() * 100)}%</span>
        </div>

        {/* Previous button */}
        <Show when={index() > 0}>
          <button class="absolute left-4 top-1/2 -translate-y-1/2 z-10 w-12 h-12 flex items-center justify-center rounded-full text-2xl transition-colors"
            style={{ background: "rgba(255,255,255,0.1)", color: "#fff" }} onClick={handlePrev}>‹</button>
        </Show>

        {/* Next button */}
        <Show when={index() < total() - 1}>
          <button class="absolute right-4 top-1/2 -translate-y-1/2 z-10 w-12 h-12 flex items-center justify-center rounded-full text-2xl transition-colors"
            style={{ background: "rgba(255,255,255,0.1)", color: "#fff" }} onClick={handleNext}>›</button>
        </Show>

        {/* Image / Video */}
        <Show
          when={isVideo()}
          fallback={
            <Show when={bigSrc()} fallback={
              <div class="text-sm" style={{ color: "rgba(255,255,255,0.7)" }}>
                正在生成预览…
              </div>
            }>
              {(src) => (
                <img
                  src={src()}
                  alt={file()?.filename}
                  class="max-w-[95vw] max-h-[95vh] object-contain select-none transition-transform duration-75"
                  draggable={false}
                  style={{
                    transform: `scale(${zoom()}) translate(${pan().x / zoom()}px, ${pan().y / zoom()}px)`,
                  }}
                  onError={(e) => { (e.currentTarget as HTMLImageElement).style.display = "none"; }}
                />
              )}
            </Show>
          }
        >
          <Show
            when={!videoError()}
            fallback={
              <div
                class="px-5 py-4 rounded-xl text-center text-sm leading-relaxed"
                style={{ background: "rgba(255,255,255,0.08)", color: "rgba(255,255,255,0.82)", "max-width": "26rem" }}
              >
                这个视频容器浏览器内核解不了（{file()?.extension}）。
                <br />
                在卡片上右键「用系统播放器打开」，或换成 mp4 / webm 再摘取。
              </div>
            }
          >
            <video
              src={convertFileSrc(file()?.path || "")}
              class="max-w-[95vw] max-h-[95vh] outline-none"
              controls
              autoplay
              onClick={(e) => e.stopPropagation()}
              onError={() => setVideoError(true)}
            />
          </Show>
        </Show>

        {/* Bottom hint */}
        <div class="absolute bottom-4 left-1/2 -translate-x-1/2 text-xs" style={{ color: "rgba(255,255,255,0.4)" }}>
          {isVideo()
            ? "← → 方向键切换 · 空格播放/暂停 · Esc 关闭"
            : previewOnly()
              ? "预览图（原始文件无法直接解码）· ← → 方向键切换 · Esc 关闭"
              : "← → 方向键切换 · 滚轮缩放 · 拖拽平移 · Esc 关闭"}
        </div>
      </div>
    </Show>
  );
}