import { createSignal, createEffect, Show, onMount, onCleanup } from "solid-js";
import type { FileInfo } from "@/lib/tauri-api";
import { COLOR_LABELS } from "@/lib/tauri-api";
import { toDisplaySrc } from "@/lib/thumbs";
import { IconHeart, IconImage, IconStarFilled } from "@/components/icons";

/** 颜色名 -> hex（与后端色板一致）。 */
const colorHex = (name: string | null | undefined): string =>
  COLOR_LABELS.find((c) => c.name === name)?.hex ?? "var(--text-muted)";

interface Props {
  file: FileInfo;
  thumbnailUrl?: string;
  /** 32px 的模糊占位图（LQIP）。真图就绪前铺在卡片上，避免大片空白。 */
  placeholderUrl?: string;
  isSelected: boolean;
  size: number;
  onSelect: (multi: boolean) => void;
  onRatingChange: (rating: number) => void;
  /** 收藏 / 取消收藏。 */
  onFavoriteChange: (favorite: boolean) => void;
  onOpen: () => void;
  /** 进出视口时通知父组件（父组件据此维护「当前视口窗口」）。 */
  onVisibleChange?: (inView: boolean) => void;
  onDoubleClick: () => void;
  /** 图片加载失败时的兜底（父组件会换 data URL）。返回 true 表示已换新地址。 */
  onImgError?: (src: string) => Promise<boolean>;
  onContextMenu?: (e: MouseEvent, path: string) => void;
}

export function ThumbnailCard(props: Props) {
  const [showRating, setShowRating] = createSignal(false);
  const [imgLoaded, setImgLoaded] = createSignal(false);
  const [imgError, setImgError] = createSignal(false);
  /** 当前这张已经试过 data URL 兜底了，避免失败循环。 */
  const [fallbackTried, setFallbackTried] = createSignal(false);

  let rootRef!: HTMLDivElement;

  /**
   * 要显示的图片地址。
   *
   * 缩略图还没就绪时返回 `null`（卡片显示骨架屏）——**刻意不回退原图**。
   * 一张 4000×3000 的原片解码要占 50 MB 内存，整屏几十张一起解就是几百 MB
   * 加肉眼可见的卡顿；后台缩略图任务几秒内就会推送结果，等一下远比解原图划算。
   * 只有后端明确说「这张生成不出来」时才会把原图路径塞进 thumbnailUrl。
   */
  const preferredSrc = () => {
    const url = props.thumbnailUrl;
    return url ? toDisplaySrc(url) : null;
  };

  /**
   * 模糊占位图。32px 的小图铺满整格，本身已经很糊；真图淡入盖在它上面。
   */
  const placeholderSrc = () => (props.placeholderUrl ? toDisplaySrc(props.placeholderUrl) : undefined);

  const [imgSrc, setImgSrc] = createSignal<string | null>(null);

  createEffect(() => {
    setImgSrc(preferredSrc());
    setImgError(false);
    setImgLoaded(false);
    setFallbackTried(false);
  });

  onMount(() => {
    // 父组件自己按渲染范围算窗口时（网格 / 列表模式的虚拟化）不会传这个回调，
    // 那就完全不需要观察器 —— 2400 个 IntersectionObserver 的开销不小。
    if (!props.onVisibleChange) return;

    // 只观察、不请求：进出视口的信号交给父组件，由它攒够一屏后**批量**提交
    // （并告诉后端这就是当前视口窗口）。以前每张卡片各自 invoke 一次，快速
    // 下滑时几百个请求同时抢后端那把引擎锁，越滑越慢。
    if (typeof IntersectionObserver === "undefined") {
      props.onVisibleChange(true);
      return;
    }
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) props.onVisibleChange?.(e.isIntersecting);
      },
      { rootMargin: "400px" },
    );
    io.observe(rootRef);
    onCleanup(() => io.disconnect());
  });

  /**
   * 图片加载失败。
   *
   * 第一次失败先请父组件换一条通道（data URL）—— 有些 WebView2 环境里
   * `http://asset.localhost/…` 会静默失败，那时卡片就只剩空白，只有把字节
   * 直接读回来才显示得出来。兜底也失败才认为这张图真的显示不了。
   */
  const handleImgError = async (e: Event) => {
    const el = e.currentTarget as HTMLImageElement;
    const src = el.src;
    if (!fallbackTried() && props.onImgError) {
      setFallbackTried(true);
      const fixed = await props.onImgError(src);
      if (fixed) return; // 新的 thumbnailUrl 会触发 effect，换掉 imgSrc
    }
    setImgError(true);
  };

  const handleClick = (e: MouseEvent) => {
    props.onSelect(e.ctrlKey || e.metaKey);
    // Single click opens info panel
    props.onOpen();
  };

  const formatSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes}B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
  };

  return (
    <div ref={rootRef} class="thumbnail-card" classList={{ selected: props.isSelected }}
      data-path={props.file.path}
      style={{ height: `${props.size + 40}px` }}
      onClick={handleClick}
      onContextMenu={(e) => props.onContextMenu?.(e, props.file.path)}
      onDblClick={(e) => { e.preventDefault(); props.onDoubleClick(); }}
      onMouseEnter={() => setShowRating(true)}
      onMouseLeave={() => setShowRating(false)}>

      <div class="relative overflow-hidden" style={{ background: "var(--bg-tertiary)", height: `${props.size}px` }}>
        {/* 模糊占位图在最底层：真图（下面那个 opacity 0→1 的 img）盖在它上面淡入。 */}
        <Show when={placeholderSrc()}>
          {(src) => (
            <img src={src()} alt="" aria-hidden="true"
              class="absolute inset-0 w-full h-full object-cover"
              draggable={false} decoding="async" />
          )}
        </Show>
        <Show when={imgSrc()}>
          {(src) => (
            <img src={src()} alt={props.file.filename}
              class="w-full h-full object-cover transition-opacity duration-200 relative"
              classList={{ "opacity-0": !imgLoaded(), "opacity-100": imgLoaded() }}
              draggable={false} decoding="async"
              onLoad={() => setImgLoaded(true)} onError={handleImgError} />
          )}
        </Show>
        <Show when={!imgLoaded() && !imgError() && !placeholderSrc()}>
          <div class="absolute inset-0 skeleton" />
        </Show>
        <Show when={imgError()}>
          <div class="absolute inset-0 flex items-center justify-center" style={{ background: "var(--bg-tertiary)" }}>
            <span style={{ color: "var(--text-muted)" }}>
              <IconImage size={26} />
            </span>
          </div>
        </Show>

        <div class="absolute top-1 left-1 px-1.5 py-0.5 rounded text-2xs uppercase"
          style={{ background: "rgba(0,0,0,0.5)", color: "#ccc" }}>
          {props.file.extension}
        </div>

        {/* 颜色标记：左上角常驻小色条 */}
        <Show when={props.file.color_label}>
          {(color) => (
            <div class="absolute left-1 w-1 rounded-full"
              style={{ background: colorHex(color()), top: "1.4rem", bottom: "0.25rem" }} />
          )}
        </Show>

        {/* 收藏：右上角常驻心形 */}
        <Show when={props.file.favorite}>
          <div class="absolute top-1 right-1 p-0.5 rounded" style={{ color: "#ff6b6b" }} title="已收藏">
            <IconHeart size={12} />
          </div>
        </Show>

        <Show when={showRating() || props.file.rating}>
          <div class="thumbnail-actions">
            <button class="flex items-center transition-colors"
              style={{ color: props.file.favorite ? "#ff6b6b" : "rgba(255,255,255,0.5)" }}
              title={props.file.favorite ? "取消收藏" : "收藏"}
              onClick={(e) => { e.stopPropagation(); props.onFavoriteChange(!props.file.favorite); }}>
              <IconHeart size={11} />
            </button>
            {[1, 2, 3, 4, 5].map((star) => (
              <button class="flex items-center transition-colors"
                style={{ color: (props.file.rating || 0) >= star ? "var(--star-color)" : "rgba(255,255,255,0.5)" }}
                onClick={(e) => { e.stopPropagation(); props.onRatingChange((props.file.rating || 0) >= star ? 0 : star); }}>
                <IconStarFilled size={11} />
              </button>
            ))}
          </div>
        </Show>
      </div>

      <div class="p-1.5 text-2xs">
        <div class="truncate font-medium" style={{ color: "var(--text-primary)" }} title={props.file.filename}>
          {props.file.filename}
        </div>
        <div class="flex items-center justify-between mt-0.5" style={{ color: "var(--text-muted)" }}>
          <span>{props.file.width && props.file.height ? `${props.file.width}×${props.file.height}` : formatSize(props.file.size_bytes)}</span>
          <Show when={props.file.tags.length > 0}>
            <div class="flex gap-0.5">
              {props.file.tags.slice(0, 3).map((tag) => (
                <span class="w-1.5 h-1.5 rounded-full" style={{ background: tag.color || "var(--text-muted)" }} title={tag.name} />
              ))}
            </div>
          </Show>
        </div>
      </div>
    </div>
  );
}
