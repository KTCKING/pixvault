#!/usr/bin/env bash
#
# PixVault — 打包「可移植源码包」，用于同步到 Mac 上构建 dmg。
# 排除 node_modules / target / dist 等本地产物，只保留源码。
#
# 用法：
#   bash scripts/package-source.sh              # 输出到 ../dist-src/
#   bash scripts/package-source.sh /path/out    # 指定输出目录
#
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

OUT_DIR="${1:-$ROOT_DIR/../dist-src}"
mkdir -p "$OUT_DIR"

VERSION="$(node -p "require('./package.json').version" 2>/dev/null || echo 0.0.0)"
STAMP="$(date +%Y%m%d)"
TARBALL="$OUT_DIR/PixVault-src-${VERSION}-${STAMP}.tar.gz"
ZIPFILE="$OUT_DIR/PixVault-src-${VERSION}-${STAMP}.zip"

echo "==> 打包源码 → $OUT_DIR"

# 用 tar 打包（跨平台，Windows/macOS/Linux 都自带）
tar \
  --exclude='./node_modules' \
  --exclude='./src-tauri/target' \
  --exclude='./dist' \
  --exclude='./.git' \
  --exclude='./patches/indexmap-1.9.3/target' \
  --exclude='*.log' \
  -czf "$TARBALL" .

MADE=("$TARBALL")

# 若系统有 zip，额外产出一份 zip（macOS 双击即可解压）
if command -v zip >/dev/null 2>&1; then
  rm -f "$ZIPFILE"
  zip -qr "$ZIPFILE" . \
    -x "node_modules/*" "src-tauri/target/*" "dist/*" ".git/*" \
    "patches/indexmap-1.9.3/target/*" "*.log"
  MADE+=("$ZIPFILE")
fi

for f in "${MADE[@]}"; do
  echo "✅ 已生成: $f"
done

echo ""
echo "在 Mac 上：解压后进入目录，运行  bash scripts/build-macos.sh"
