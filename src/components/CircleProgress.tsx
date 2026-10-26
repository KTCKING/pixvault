import { Show } from "solid-js";

interface Props {
  /** 已完成数量。 */
  value: number;
  /** 总数。 */
  max: number;
  /** 外径（px）。 */
  size?: number;
  /** 环宽（px）。 */
  stroke?: number;
  /** 环中心是否显示百分比文字（环太小时会自动省略）。 */
  showPercent?: boolean;
}

/**
 * 圆形进度环（SVG）。
 *
 * 用 `stroke-dasharray` 画弧：周长 C，已完成的弧长 = C × 百分比，剩下留空。
 * 整环旋转 -90°，让进度从 12 点方向顺时针增长 —— 这是进度环最自然的阅读方向。
 */
export function CircleProgress(props: Props) {
  const size = () => props.size ?? 16;
  const stroke = () => props.stroke ?? 2;
  const radius = () => (size() - stroke()) / 2;
  const circumference = () => 2 * Math.PI * radius();

  const pct = () => {
    const max = props.max;
    if (!max || max <= 0) return 0;
    return Math.min(1, Math.max(0, props.value / max));
  };

  const label = () => `${Math.round(pct() * 100)}%`;
  // 直径小于 20px 时塞不下文字，只画环。
  const withText = () => (props.showPercent ?? false) && size() >= 20;

  return (
    <span
      class="relative inline-flex items-center justify-center flex-shrink-0"
      style={{ width: `${size()}px`, height: `${size()}px` }}
      role="progressbar"
      aria-valuenow={Math.round(pct() * 100)}
      aria-valuemin={0}
      aria-valuemax={100}
    >
      <svg
        width={size()}
        height={size()}
        viewBox={`0 0 ${size()} ${size()}`}
        style={{ transform: "rotate(-90deg)" }}
        aria-hidden="true"
      >
        <circle
          cx={size() / 2}
          cy={size() / 2}
          r={radius()}
          fill="none"
          stroke="var(--bg-tertiary)"
          stroke-width={stroke()}
        />
        <circle
          cx={size() / 2}
          cy={size() / 2}
          r={radius()}
          fill="none"
          stroke="var(--accent)"
          stroke-width={stroke()}
          stroke-linecap="round"
          stroke-dasharray={`${circumference() * pct()} ${circumference()}`}
          style={{ transition: "stroke-dasharray 200ms linear" }}
        />
      </svg>
      <Show when={withText()}>
        <span
          class="absolute inset-0 flex items-center justify-center tabular-nums"
          style={{ "font-size": `${Math.max(7, size() * 0.3)}px`, color: "var(--accent)" }}
        >
          {label()}
        </span>
      </Show>
    </span>
  );
}
