#!/usr/bin/env bash
#
# PixVault — macOS 一键构建脚本
# 产出：src-tauri/target/release/bundle/dmg/PixVault_<version>_<arch>.dmg
#      以及 .app 应用包
#
# 用法：
#   bash scripts/build-macos.sh            # 构建 dmg + app
#   bash scripts/build-macos.sh --app-only # 只构建 .app（调试用）
#
set -euo pipefail

APP_NAME="PixVault"
# 切到项目根目录（脚本所在目录的上一级）
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> PixVault macOS 构建"
echo "    项目目录: $ROOT_DIR"

# ── 1. 平台检查 ─────────────────────────────────────────────
if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "✗ 错误：dmg 只能在 macOS 上构建。当前系统：$(uname -s)" >&2
  exit 1
fi

# ── 2. 依赖检查 ─────────────────────────────────────────────
missing=0
check() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "✗ 缺少 $1 —— $2"
    missing=1
  fi
}
check cargo "请安装 Rust: https://rustup.rs  (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh)"
check node  "请安装 Node.js 18+: https://nodejs.org"
check npm   "随 Node.js 一起安装"

if ! xcode-select -p >/dev/null 2>&1; then
  echo "✗ 缺少 Xcode 命令行工具 —— 请运行: xcode-select --install"
  missing=1
fi

if [[ "$missing" -ne 0 ]]; then
  echo "请先补齐上面的依赖，再重新运行本脚本。" >&2
  exit 1
fi

echo "==> 环境检查通过"
echo "    rustc: $(rustc --version)"
echo "    node : $(node --version)"

# ── 3. 确定包管理器 ─────────────────────────────────────────
if [[ -f pnpm-lock.yaml ]] && command -v pnpm >/dev/null 2>&1; then
  PM="pnpm"
elif command -v pnpm >/dev/null 2>&1; then
  PM="pnpm"
else
  PM="npm"
fi
echo "==> 使用包管理器: $PM"

# ── 4. 安装依赖 ─────────────────────────────────────────────
if [[ ! -d node_modules ]]; then
  echo "==> 安装前端依赖 …"
  "$PM" install
else
  echo "==> node_modules 已存在，跳过安装（如需更新请手动执行 $PM install）"
fi

# ── 5. 清理旧的构建产物（可选） ─────────────────────────────
# 如需彻底重来，取消下一行注释
# rm -rf src-tauri/target/release/bundle

# ── 6. 构建 ─────────────────────────────────────────────────
HOST_TARGET="$(rustc -vV | awk '/^host:/{print $2}')"

if [[ "${1:-}" == "--app-only" ]]; then
  TARGET_DIR="src-tauri/target/release/bundle"
  echo "==> 只构建 .app（调试用），首次构建会编译全部 Rust 依赖，耗时较长 …"
  npx tauri build --bundles app
else
  TARGET_DIR="src-tauri/target/$HOST_TARGET/release/bundle"
  echo "==> 开始构建 dmg，首次构建会编译全部 Rust 依赖，耗时较长 …"
  # 直接跑 `tauri build --bundles dmg` 在无 GUI 会话（SSH / CI）里常常失败：
  # 内嵌 create-dmg 用 AppleScript 遥控 Finder 排版会报 -1712。
  # make-dmg.sh 会先试原生打包（失败清场重试），再用 hdiutil 兜底，详见其文件头注释。
  bash scripts/make-dmg.sh "$HOST_TARGET"
fi

# ── 7. 输出结果 ─────────────────────────────────────────────
echo ""
echo "✅ 构建完成！产物位置："
if [[ -d "$TARGET_DIR/dmg" ]]; then
  ls -lh "$TARGET_DIR"/dmg/*.dmg 2>/dev/null | awk '{print "   📦 " $NF "  (" $5 ")"}'
fi
if [[ -d "$TARGET_DIR/macos" ]]; then
  ls -d "$TARGET_DIR"/macos/*.app 2>/dev/null | awk '{print "   📱 " $NF}'
fi
echo ""
echo "提示：未签名的 dmg 首次打开会被 Gatekeeper 拦截，"
echo "      可在「系统设置 → 隐私与安全性」中点「仍要打开」，"
echo "      或执行： xattr -dr com.apple.quarantine <dmg文件>"
