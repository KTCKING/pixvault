import type { JSX } from "solid-js";

/**
 * 图标系统。三套风格：
 *
 * - 本文件：A 风格 Fluent 线性（界面小图标 + 侧栏导航）。
 *   全部 24×24 viewBox、1.5px 圆角描边（侧栏导航 1.7px，见 IconTab*），
 *   `currentColor` 上色 —— 黑白两套主题自动跟随，不需要为每套主题各存一份。
 * - hero.tsx：C 风格灰阶 3D（欢迎页 logo 与空状态），多层不透明度叠高光。
 *
 * 替代原先散落各处的 emoji（86 处 / 31 种）。emoji 的问题：不同系统字形不统一、
 * 颜色不可控、尺寸随字号跳动、深浅主题下观感随机。
 */

export interface IconProps {
  /** 像素尺寸，默认 16。 */
  size?: number;
  /** 描边宽度，默认 1.5。个别图标需要更粗/更细时按需覆盖。 */
  strokeWidth?: number;
  class?: string;
  style?: JSX.CSSProperties;
}

function Svg(props: IconProps & { children: JSX.Element }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width={props.size ?? 16}
      height={props.size ?? 16}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width={props.strokeWidth ?? 1.5}
      stroke-linecap="round"
      stroke-linejoin="round"
      class={props.class}
      style={props.style}
      aria-hidden="true"
    >
      {props.children}
    </svg>
  );
}

/* ── A 风格 · 线性 ───────────────────────────────── */

export function IconClose(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M18 6 6 18M6 6l12 12" />
    </Svg>
  );
}

export function IconCheck(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M20 6 9 17l-5-5" />
    </Svg>
  );
}

export function IconCheckCircle(p: IconProps) {
  return (
    <Svg {...p}>
      <circle cx="12" cy="12" r="9" />
      <path d="m8.5 12 2.5 2.5 5-5" />
    </Svg>
  );
}

export function IconCopy(p: IconProps) {
  return (
    <Svg {...p}>
      <rect x="8" y="8" width="12" height="12" rx="2" />
      <path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" />
    </Svg>
  );
}

export function IconTrash(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M3 6h18" />
      <path d="M8 6V4.5A1.5 1.5 0 0 1 9.5 3h5A1.5 1.5 0 0 1 16 4.5V6" />
      <path d="M19 6l-.8 13a2 2 0 0 1-2 1.9H7.8a2 2 0 0 1-2-1.9L5 6" />
      <path d="M10 10.5v6M14 10.5v6" />
    </Svg>
  );
}

/** 清空 / 彻底删除：垃圾桶内加叉，比纯垃圾桶更"危险"，和「移入垃圾篓」区分。 */
export function IconTrashX(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M3 6h18" />
      <path d="M8 6V4.5A1.5 1.5 0 0 1 9.5 3h5A1.5 1.5 0 0 1 16 4.5V6" />
      <path d="M19 6l-.8 13a2 2 0 0 1-2 1.9H7.8a2 2 0 0 1-2-1.9L5 6" />
      <path d="M9.8 10.2l4.4 4.4M14.2 10.2l-4.4 4.4" />
    </Svg>
  );
}

/** 恢复（从垃圾篓捞回）。 */
export function IconUndo(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M9 14 4 9l5-5" />
      <path d="M4 9h10a6 6 0 0 1 0 12h-3" />
    </Svg>
  );
}

/** 移除库（保留文件）：出门 + 向左，与「恢复」的纯回退箭头区分。 */
export function IconLogOut(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M9 21H6a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3" />
      <path d="m16 17 5-5-5-5" />
      <path d="M21 12H9" />
    </Svg>
  );
}

export function IconPlus(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M12 5v14M5 12h14" />
    </Svg>
  );
}

export function IconFolder(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.1a2 2 0 0 1-1.7-.9l-.8-1.2A2 2 0 0 0 8.7 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
    </Svg>
  );
}

export function IconFolderOpen(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="m6 14 1.4-2.9A2 2 0 0 1 9.2 10H20a2 2 0 0 1 1.9 2.5l-1.5 6A2 2 0 0 1 18.4 20H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.7.9l.8 1.2a2 2 0 0 0 1.7.9H18a2 2 0 0 1 2 2v2" />
    </Svg>
  );
}

export function IconPencil(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M17 3a2.8 2.8 0 0 1 4 4L7.5 20.5 2 22l1.5-5.5Z" />
      <path d="m15 5 4 4" />
    </Svg>
  );
}

export function IconArrowLeft(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M19 12H5" />
      <path d="m12 19-7-7 7-7" />
    </Svg>
  );
}

export function IconArrowRight(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M5 12h14" />
      <path d="m12 5 7 7-7 7" />
    </Svg>
  );
}

export function IconArrowUp(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M12 19V5" />
      <path d="m5 12 7-7 7 7" />
    </Svg>
  );
}

export function IconArrowDown(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M12 5v14" />
      <path d="m19 12-7 7-7-7" />
    </Svg>
  );
}

/** 排序按钮：一上一下双箭头。 */
export function IconSort(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="m17 4 4 4-4 4" />
      <path d="M21 8H8" />
      <path d="m7 20-4-4 4-4" />
      <path d="M3 16h13" />
    </Svg>
  );
}

export function IconStar(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M12 2.6l2.9 5.9 6.5.9-4.7 4.6 1.1 6.5L12 17.4l-5.8 3.1 1.1-6.5L2.6 9.4l6.5-.9Z" />
    </Svg>
  );
}

/** 实心星（评分已点亮的那几颗）。 */
export function IconStarFilled(p: IconProps) {
  return (
    <Svg {...p}>
      <path
        d="M12 2.6l2.9 5.9 6.5.9-4.7 4.6 1.1 6.5L12 17.4l-5.8 3.1 1.1-6.5L2.6 9.4l6.5-.9Z"
        fill="currentColor"
      />
    </Svg>
  );
}

export function IconMenu(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M4 6h16M4 12h16M4 18h16" />
    </Svg>
  );
}

export function IconImage(p: IconProps) {
  return (
    <Svg {...p}>
      <rect x="3" y="3" width="18" height="18" rx="2" />
      <circle cx="8.8" cy="8.8" r="1.8" />
      <path d="m21 15.5-3.6-3.6a2 2 0 0 0-2.8 0L5.5 21" />
    </Svg>
  );
}

export function IconSettings(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M20 7h-9" />
      <path d="M14 17H5" />
      <circle cx="17" cy="7" r="3" />
      <circle cx="7" cy="17" r="3" />
    </Svg>
  );
}

export function IconMoon(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M12 3a6.4 6.4 0 0 0 9 9 9 9 0 1 1-9-9Z" />
    </Svg>
  );
}

export function IconSun(p: IconProps) {
  return (
    <Svg {...p}>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
    </Svg>
  );
}

export function IconMonitor(p: IconProps) {
  return (
    <Svg {...p}>
      <rect x="2" y="3" width="20" height="14" rx="2" />
      <path d="M8 21h8M12 17v4" />
    </Svg>
  );
}

export function IconGlobe(p: IconProps) {
  return (
    <Svg {...p}>
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18" />
      <path d="M12 3a15 15 0 0 1 4 9 15 15 0 0 1-4 9 15 15 0 0 1-4-9 15 15 0 0 1 4-9Z" />
    </Svg>
  );
}

export function IconSearch(p: IconProps) {
  return (
    <Svg {...p}>
      <circle cx="11" cy="11" r="7" />
      <path d="m20.5 20.5-4.2-4.2" />
    </Svg>
  );
}

export function IconTag(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M12.6 2.6A2 2 0 0 0 11.2 2H4a2 2 0 0 0-2 2v7.2a2 2 0 0 0 .6 1.4l8.7 8.7a2.4 2.4 0 0 0 3.4 0l6.6-6.6a2.4 2.4 0 0 0 0-3.4Z" />
      <circle cx="7.2" cy="7.2" r="1.4" fill="currentColor" />
    </Svg>
  );
}

export function IconZap(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M13 2 4.5 13.2a.6.6 0 0 0 .5.8H10l-1 8 8.5-11.2a.6.6 0 0 0-.5-.8H12l1-8Z" />
    </Svg>
  );
}

export function IconDatabase(p: IconProps) {
  return (
    <Svg {...p}>
      <ellipse cx="12" cy="5.5" rx="8" ry="3" />
      <path d="M4 5.5v13c0 1.7 3.6 3 8 3s8-1.3 8-3v-13" />
      <path d="M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3" />
    </Svg>
  );
}

export function IconPackage(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M21 8a2 2 0 0 0-1-1.7l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.7l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z" />
      <path d="m3.3 7 8.7 5 8.7-5" />
      <path d="M12 22V12" />
    </Svg>
  );
}

/** Eagle 导入：简化展翅鹰（品牌意象，替代原 🦅 emoji）。 */
export function IconEagle(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M12 8.5a1.6 1.6 0 1 0 0-3.2 1.6 1.6 0 0 0 0 3.2Z" fill="currentColor" />
      <path d="M12 7.5V3.8" />
      <path d="M12 7.5c.8 2.6 2.9 4 5.6 4.6 2.3.5 4.4.2 6.4-1.1-1.9.3-3.5 0-4.8-.9-1.3.5-2.7.6-4.1.2" />
      <path d="M12 7.5c-.8 2.6-2.9 4-5.6 4.6-2.3.5-4.4.2-6.4-1.1 1.9.3 3.5 0 4.8-.9 1.3.5 2.7.6 4.1.2" />
      <path d="M12 7.8c.5 2.7.4 5.4-.3 8.2l-.9 4.5-1.5-4.3-1.4 3.2-1-4.7c-.6-2.6-.7-5.1-.4-7.6" />
    </Svg>
  );
}

/** 资源库（本机目录）：照片堆叠 —— 后一张只露右上两条边，前一张里画山与太阳，
 *  和桌面图标 / 欢迎页 logo 同一套意象，一眼能认出是"图片库"而不是一台显示器。 */
export function IconLibrary(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M7.8 2.4h11.4a2 2 0 0 1 2 2v10.4" />
      <rect x="2.8" y="7.8" width="13.4" height="12.4" rx="2.4" />
      <circle cx="6.9" cy="12.1" r="1.6" />
      <path d="m3.6 19.9 3.2-3.4a1.3 1.3 0 0 1 1.9 0l2.6 2.8" />
    </Svg>
  );
}

/* ── 侧栏导航 tab（统一 1.7px 线性）─────────────
   这排图标原先是「三个实心 + 一个加粗描边」的混搭：实心块和细线条并排时
   视觉重量差一倍，看起来参差；激活态又靠贴底的 2px 下划线表达，离图标太远。
   现在四个统一到同一条描边，激活态交给外层容器的"浮起卡片"去表达。 */

export function IconTabFolder(p: IconProps) {
  return (
    <Svg {...p} strokeWidth={1.7}>
      <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.1a2 2 0 0 1-1.7-.9l-.8-1.2A2 2 0 0 0 8.7 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
    </Svg>
  );
}

export function IconTabTag(p: IconProps) {
  return (
    <Svg {...p} strokeWidth={1.7}>
      <path d="M12.6 2.6A2 2 0 0 0 11.2 2H4a2 2 0 0 0-2 2v7.2a2 2 0 0 0 .6 1.4l8.7 8.7a2.4 2.4 0 0 0 3.4 0l6.6-6.6a2.4 2.4 0 0 0 0-3.4Z" />
      <circle cx="7.2" cy="7.2" r="1.5" />
    </Svg>
  );
}

export function IconTabZap(p: IconProps) {
  return (
    <Svg {...p} strokeWidth={1.7}>
      <path d="M13 2 4.5 13.2a.6.6 0 0 0 .5.8H10l-1 8 8.5-11.2a.6.6 0 0 0-.5-.8H12l1-8Z" />
    </Svg>
  );
}

export function IconTabSearch(p: IconProps) {
  return (
    <Svg {...p} strokeWidth={1.7}>
      <circle cx="11" cy="11" r="7" />
      <path d="m20.5 20.5-4.2-4.2" />
    </Svg>
  );
}

/** 信息提示（Toast info）。 */
export function IconInfo(p: IconProps) {
  return (
    <Svg {...p}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 8h.01" />
      <path d="M12 11.5V16" />
    </Svg>
  );
}

/** 外部链接 / 打开方式。 */
export function IconLink(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7" />
      <path d="M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7" />
    </Svg>
  );
}

/** 瀑布流视图（错落竖条）。 */
export function IconMasonry(p: IconProps) {
  return (
    <Svg {...p}>
      <rect x="3" y="3" width="5" height="8" rx="1" />
      <rect x="3" y="13" width="5" height="8" rx="1" />
      <rect x="9.5" y="3" width="5" height="12" rx="1" />
      <rect x="16" y="3" width="5" height="6" rx="1" />
      <rect x="16" y="11" width="5" height="10" rx="1" />
    </Svg>
  );
}

/* ── 字符符号替代（原 ◀ ▶ ▥ ▦ ⏳）──────────────── */

export function IconChevronLeft(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="m15 18-6-6 6-6" />
    </Svg>
  );
}

export function IconChevronRight(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="m9 18 6-6-6-6" />
    </Svg>
  );
}

export function IconChevronDown(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="m6 9 6 6 6-6" />
    </Svg>
  );
}

/** 网格视图（四宫格）。 */
export function IconGrid(p: IconProps) {
  return (
    <Svg {...p}>
      <rect x="3" y="3" width="7.5" height="7.5" rx="1.5" />
      <rect x="13.5" y="3" width="7.5" height="7.5" rx="1.5" />
      <rect x="3" y="13.5" width="7.5" height="7.5" rx="1.5" />
      <rect x="13.5" y="13.5" width="7.5" height="7.5" rx="1.5" />
    </Svg>
  );
}

/** 右侧信息面板。 */
export function IconPanelRight(p: IconProps) {
  return (
    <Svg {...p}>
      <rect x="3" y="3" width="18" height="18" rx="2" />
      <path d="M15 3v18" />
    </Svg>
  );
}

/** 加载中（旋转弧，配合 animate-spin）。 */
export function IconLoader(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M21 12a9 9 0 1 1-6.2-8.55" />
    </Svg>
  );
}

/* ── 状态灯（原 🟢🟡🔴）────────────────────────── */
const DOT_TONE: Record<string, string> = {
  local: "var(--text-muted)",
  online: "#3ba55d",
  degraded: "#d29922",
  offline: "#e5534b",
};

/** NAS/本地库状态圆点。语义色保留（状态需要红黄绿可辨），尺寸随字号。 */
export function StatusDot(props: { tone: string; class?: string; style?: JSX.CSSProperties }) {
  return (
    <span
      class={props.class}
      style={{
        display: "inline-block",
        width: "0.5em",
        height: "0.5em",
        "border-radius": "50%",
        background: DOT_TONE[props.tone] ?? "var(--text-muted)",
        "vertical-align": "0.05em",
        "flex-shrink": "0",
        ...props.style,
      }}
    />
  );
}

/* ── 收藏（PixVault 资源库新增，替代文字 ♥）──────── */
/** 心形：实心表「已收藏」，描边形态由 props.strokeWidth 控制。 */
export function IconHeart(props: IconProps) {
  return (
    <Svg {...props}>
      <path
        d="M12 20.5s-6.4-4.1-8.7-7.5C1.5 10.3 2.5 7 5.6 6.3c1.8-.45 3.4.35 4.4 1.6 1-1.25 2.6-2.05 4.4-1.6 3.1.7 4.1 4 2.3 6.7-2.3 3.4-8.7 7.5-8.7 7.5z"
        fill="currentColor"
      />
    </Svg>
  );
}

/* ── 颜色标记（PixVault 资源库新增）──────────────── */
/** 调色板：颜色标记菜单 / 色板入口。 */
export function IconPalette(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M12 3a9 9 0 1 0 0 18c1.1 0 1.7-.9 1.7-1.8 0-.8-.7-1.5-.7-2.2 0-.9.7-1.6 1.6-1.6h1.6A4.8 4.8 0 0 0 21 10.6C21 6.4 17 3 12 3z" />
      <circle cx="7.6" cy="10.6" r="1.15" fill="currentColor" stroke="none" />
      <circle cx="12" cy="7.6" r="1.15" fill="currentColor" stroke="none" />
      <circle cx="16.4" cy="10.6" r="1.15" fill="currentColor" stroke="none" />
    </Svg>
  );
}

/* ── 时钟（最近新增视图）──────────────────────── */
export function IconClock(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7.5V12l3 2" />
    </Svg>
  );
}
