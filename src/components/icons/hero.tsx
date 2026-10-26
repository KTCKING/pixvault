import type { JSX } from "solid-js";

/**
 * C 风格 · 灰阶 3D。只用于欢迎页 logo、特性图标与空状态插画这类大面积留白处。
 *
 * 纯黑白：主体 `currentColor`（跟随主题），层次靠多层不透明度叠白色高光 ——
 * 不用渐变，避免同页面多实例的 SVG id 冲突，也让深浅两套主题各自动适配。
 */

export interface HeroProps {
  size?: number;
  class?: string;
  style?: JSX.CSSProperties;
}

function HeroSvg(props: HeroProps & { children: JSX.Element }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width={props.size ?? 24}
      height={props.size ?? 24}
      viewBox="0 0 24 24"
      class={props.class}
      style={props.style}
      aria-hidden="true"
    >
      {props.children}
    </svg>
  );
}

/**
 * 品牌标：深色玻璃底板 + 山 + 太阳。
 * 两层山（远淡近实）出纵深，顶部一条薄高光模拟玻璃受光。
 */
export function HeroLogo(p: HeroProps) {
  return (
    <HeroSvg {...p}>
      <rect x="1.5" y="1.5" width="21" height="21" rx="5.5" fill="currentColor" />
      <path d="M1.5 6.5A5 5 0 0 1 6.5 1.5h11a5 5 0 0 1 5 5V9h-17Z" fill="#ffffff" opacity="0.1" />
      <circle cx="15.6" cy="8.4" r="4.1" fill="#ffffff" opacity="0.22" />
      <circle cx="15.6" cy="8.4" r="2.5" fill="#ffffff" opacity="0.95" />
      <path d="M2 19.8 9 11.6l4 5.2 3-3.6 6 7.4v.4H2Z" fill="#ffffff" opacity="0.32" />
      <path d="M2 21.4 8.6 13.6l4.4 5.6L16 14.8l6 6.8v-.2H2Z" fill="#ffffff" opacity="0.88" />
    </HeroSvg>
  );
}

/** 极速浏览：立体闪电，左上半透高光。 */
export function HeroZap(p: HeroProps) {
  return (
    <HeroSvg {...p}>
      <path d="M13 2 4.5 13.2a.6.6 0 0 0 .5.8H10l-1 8 8.5-11.2a.6.6 0 0 0-.5-.8H12l1-8Z" fill="currentColor" />
      <path d="M12.7 3.4 7 11.5a.5.5 0 0 0 .4.8h2.2l-.6 5.1 5.2-7a.5.5 0 0 0-.4-.8h-2.1Z" fill="#ffffff" opacity="0.32" />
    </HeroSvg>
  );
}

/** 开放格式：三层数据库，顶淡底实。 */
export function HeroDatabase(p: HeroProps) {
  return (
    <HeroSvg {...p}>
      <path d="M3.2 5.4v13.2c0 1.8 3.9 3.2 8.8 3.2s8.8-1.4 8.8-3.2V5.4" fill="currentColor" />
      <ellipse cx="12" cy="5.4" rx="8.8" ry="3.2" fill="currentColor" opacity="0.55" />
      <ellipse cx="12" cy="12" rx="8.8" ry="3.2" fill="currentColor" opacity="0.75" />
      <ellipse cx="12" cy="18.6" rx="8.8" ry="3.2" fill="currentColor" />
      <path d="M6.4 3.9a8.8 3.2 0 0 1 11.2 0" fill="none" stroke="#ffffff" stroke-width="1" opacity="0.4" stroke-linecap="round" />
    </HeroSvg>
  );
}

/** NAS 就绪：球体 + 大陆块 + 左上受光高光。 */
export function HeroGlobe(p: HeroProps) {
  return (
    <HeroSvg {...p}>
      <circle cx="12" cy="12" r="9.5" fill="currentColor" />
      <path d="M12 2.5a9.5 9.5 0 0 1 5.6 17.3A15 15 0 0 0 12 12 15 15 0 0 0 6.4 19.8 9.5 9.5 0 0 1 12 2.5Z" fill="#ffffff" opacity="0.14" />
      <path d="M6.8 8.6c1.6-1 3.4-.6 4.6.6 1.3 1.2 3 1.6 4.7.8 1.2-.6 2-.5 2.5.2-1.5 1-3.3.6-4.6-.6-1.3-1.2-3-1.6-4.7-.8-1 .5-2 .5-2.5-.2Z" fill="#ffffff" opacity="0.3" />
      <path d="M7.4 15.4c1.3-.8 2.8-.5 3.8.5 1 1 2.4 1.3 3.8.6.9-.4 1.7-.4 2.2 0-1.3.8-2.8.5-3.8-.5-1-1-2.4-1.3-3.8-.6-.8.4-1.5.4-2.2 0Z" fill="#ffffff" opacity="0.3" />
      <path d="M8.6 6.4A6.8 6.8 0 0 1 11.8 4" fill="none" stroke="#ffffff" stroke-width="1.2" opacity="0.5" stroke-linecap="round" />
    </HeroSvg>
  );
}

/** 空状态大图：3D 图片框。 */
export function HeroImage(p: HeroProps) {
  return (
    <HeroSvg {...p}>
      <rect x="2" y="3" width="20" height="18" rx="3.5" fill="currentColor" />
      <circle cx="8.4" cy="8.6" r="2.1" fill="#ffffff" opacity="0.9" />
      <path d="M2.6 19.6 9.2 11.8l4.3 5 3.6-4.2 4.3 6.2v.8H2.6Z" fill="#ffffff" opacity="0.55" />
      <path d="M2 6.5A3.5 3.5 0 0 1 5.5 3h13a3.5 3.5 0 0 1 3.5 3.5v.9H2Z" fill="#ffffff" opacity="0.12" />
    </HeroSvg>
  );
}
