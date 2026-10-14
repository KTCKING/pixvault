#!/usr/bin/env bash
#
# PixVault — 平台门控 API 预检
#
# 为什么需要这个脚本：
#   本地只在 Windows 上构建，而 CI 还会构建 macOS 的两个 dmg。Tauri 里有相当
#   一部分 API 是按平台门控的 —— 一旦用了「Windows 有、macOS 没有」的方法，
#   本地 `cargo check` 一切正常，推上去之后 macOS 任务才编译失败，白烧一轮 CI。
#
#   2026-10-13 就踩过一次：`WebviewWindowBuilder::transparent` 挂掉两个 dmg 任务。
#   在 Windows 本地无法通过 `cargo check --target aarch64-apple-darwin` 预检
#   （依赖链里的 objc2-exception-helper 需要 macOS 的 C 工具链，跨不过去），
#   所以退而求其次，用静态扫描把这一类问题挡住。
#
# 用法：bash scripts/check-platform-api.sh [src 目录，默认 src-tauri/src]
#
# 退出码：0 = 通过；1 = 发现「macOS 上不存在」的 API 调用。
#
set -uo pipefail

SRC_DIR="${1:-src-tauri/src}"

if [[ ! -d "$SRC_DIR" ]]; then
  echo "✗ 找不到源码目录：$SRC_DIR" >&2
  exit 1
fi

# ── 类别一：macOS 上「不存在」的方法 ────────────────────────────────────
# 门控形式 #[cfg(any(not(target_os = "macos"), feature = "macos-private-api"))]
# —— Windows / Linux 都有，macOS 默认没有。**这一类是致命的**：本地能编过，
# CI 的 mac 任务必挂。
# 参考：tauri-2.11.5/src/window/mod.rs:909、src/webview/mod.rs:1016、
#       src/webview/webview_window.rs:1075
FATAL_APIS=(
  "transparent"   # WindowBuilder / WebviewBuilder 的透明度开关，默认都是 false，别调
)

# ── 类别二：只有 macOS 才有的方法 ──────────────────────────────────────
# 反方向的问题：用了会在**本地** Windows 构建时报 E0599，自己能立刻发现，
# 所以只做提示，不算失败。
MACOS_ONLY_APIS=(
  "set_activation_policy"
  "set_dock_visibility"
  "on_web_content_process_terminate"
  "set_badge_label"
  "ns_window"
  "ns_view"
  "parent_raw"
  "title_bar_style"
  "hidden_title"
  "tabbing_identifier"
  "set_traffic_lights_inset"
)

# 把「行首是注释」的行排除掉 —— 我们的注释里会写出这些方法名来解释它们
# 为什么不能用，不该被算成调用。
hits_for() {
  local api="$1"
  grep -rn --include='*.rs' "\.${api}(" "$SRC_DIR" 2>/dev/null \
    | awk -F: '{ line=$0; sub(/^[^:]*:[0-9]+:/, "", line); gsub(/^[ \t]+/, "", line);
                 if (line !~ /^\/\//) print }'
}

fail=0

echo "==> 平台门控 API 预检：$SRC_DIR"

for api in "${FATAL_APIS[@]}"; do
  hits="$(hits_for "$api")"
  if [[ -n "$hits" ]]; then
    fail=1
    echo ""
    echo "✗ 发现 .${api}(...) —— 该方法在 macOS 上不存在，CI 的 dmg 任务会编译失败：" >&2
    echo "$hits" | sed 's/^/    /' >&2
    echo "    说明：门控为 #[cfg(any(not(target_os = \"macos\"), feature = \"macos-private-api\"))]" >&2
    echo "    处理：删掉这次调用（默认值通常就是你要的），或加 #[cfg(not(target_os = \"macos\"))] 包起来。" >&2
  fi
done

for api in "${MACOS_ONLY_APIS[@]}"; do
  hits="$(hits_for "$api")"
  if [[ -n "$hits" ]]; then
    echo ""
    echo "⚠ 发现 .${api}(...)（macOS 专属）—— 请确认已用 #[cfg(target_os = \"macos\")] 包好："
    echo "$hits" | sed 's/^/    /'
  fi
done

if [[ "$fail" -eq 0 ]]; then
  echo "✅ 通过：未发现 macOS 上不存在的 API 调用。"
fi

exit "$fail"
