#!/usr/bin/env bash
#
# 把工作区的源码同步到英文路径的构建目录（Windows 上构建用）。
# 中文路径下 Rust/GNU 链接器可能出错，因此构建统一在英文路径目录进行。
#
# 用法：
#   bash scripts/sync-build.sh            # 同步到 ../pixvault-build
#   bash scripts/sync-build.sh <目标目录>
#
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${1:-$HOME/pixvault-build}"

if [[ ! -d "$DEST" ]]; then
  echo "✗ 目标构建目录不存在: $DEST" >&2
  echo "  请先创建，或传入正确的目录路径。" >&2
  exit 1
fi

echo "==> 同步 $ROOT_DIR → $DEST"

mkdir -p "$DEST/src" "$DEST/src-tauri/src" "$DEST/scripts" "$DEST/patches"

# 前端源码
cp -r "$ROOT_DIR/src/." "$DEST/src/"

# 后端源码
cp -r "$ROOT_DIR/src-tauri/src/." "$DEST/src-tauri/src/"

# Cargo patch（[patch.crates-io] 依赖它，漏同步 → cargo 报
# "unable to update patches/indexmap-1.9.3 / 系统找不到指定的路径"）
cp -rf "$ROOT_DIR/patches/." "$DEST/patches/" 2>/dev/null || true

# 配置文件与图标
cp -f "$ROOT_DIR/src-tauri/tauri.conf.json" \
      "$ROOT_DIR/src-tauri/tauri.windows.conf.json" \
      "$ROOT_DIR/src-tauri/tauri.macos.conf.json" \
      "$ROOT_DIR/src-tauri/app-icon.svg" \
      "$ROOT_DIR/src-tauri/Cargo.toml" \
      "$ROOT_DIR/src-tauri/build.rs" \
      "$DEST/src-tauri/" 2>/dev/null || true

# Tauri 权限配置（capabilities 改了不同步 → 打包版用旧权限）
rm -rf "$DEST/src-tauri/capabilities"
cp -r "$ROOT_DIR/src-tauri/capabilities" "$DEST/src-tauri/"

rm -rf "$DEST/src-tauri/icons"
cp -r "$ROOT_DIR/src-tauri/icons" "$DEST/src-tauri/"

# 脚本、包配置、说明
# 注意：目录要带 -r —— 之前用 `cp -f`，cp 会以 "omitting directory" 失败，
# 错误又被 `|| true` 吞掉，结果 scripts/ 从来没同步过去，构建目录一直用旧脚本。
cp -rf "$ROOT_DIR/scripts/." "$DEST/scripts/" 2>/dev/null || true
cp -f "$ROOT_DIR/package.json" "$DEST/" 2>/dev/null || true

# 前端构建配置（漏掉这些会导致构建目录用旧配置，例如 Tailwind 新增动画不生效）
cp -f "$ROOT_DIR/tailwind.config.ts" \
      "$ROOT_DIR/postcss.config.ts" \
      "$ROOT_DIR/vite.config.ts" \
      "$ROOT_DIR/tsconfig.json" \
      "$ROOT_DIR/index.html" \
      "$DEST/" 2>/dev/null || true

echo "✅ 同步完成"
