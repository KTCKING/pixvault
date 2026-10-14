import { Show, onMount, onCleanup, createSignal } from "solid-js";
import { useViewerStore } from "@/stores/viewer";
import { convertFileSrc } from "@tauri-apps/api/core";

export function ImageViewer() {
  const viewer = useViewerStore();
  const [zoom, setZoom] = createSignal(1);
  const [pan, setPan] = createSignal({ x: 0, y: 0 });
  const [dragging, setDragging] = createSignal(false);
  const [dragStart, setDragStart] = createSignal({ x: 0, y: 0 });

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
        <button class="absolute top-4 right-4 z-10 w-10 h-10 flex items-center justify-center rounded-full text-2xl transition-colors"
          style={{ background: "rgba(255,255,255,0.1)", color: "#fff" }} onClick={viewer.close}>✕</button>

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

        {/* Image */}
        <img
          src={convertFileSrc(file()?.path || "")}
          alt={file()?.filename}
          class="max-w-[95vw] max-h-[95vh] object-contain select-none transition-transform duration-75"
          draggable={false}
          style={{
            transform: `scale(${zoom()}) translate(${pan().x / zoom()}px, ${pan().y / zoom()}px)`,
          }}
          onError={(e) => { (e.currentTarget as HTMLImageElement).style.display = "none"; }}
        />

        {/* Bottom hint */}
        <div class="absolute bottom-4 left-1/2 -translate-x-1/2 text-xs" style={{ color: "rgba(255,255,255,0.4)" }}>
          ← → 方向键切换 · 滚轮缩放 · 拖拽平移 · Esc 关闭
        </div>
      </div>
    </Show>
  );
}