#!/usr/bin/env bash
#
# PixVault — Windows 构建脚本（GNU 工具链）
# 自动把便携版 MinGW-w64 加入 PATH（windres / gcc / ld 等资源编译器），
# 然后执行 tauri 打包，产出 exe + msi。
#
# 用法：bash scripts/build-windows.sh
#
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# 便携版 MinGW-w64 的位置（提供 windres，tauri-winres 打包图标/版本资源时必需）
MINGW_BIN="${MINGW_BIN:-$HOME/mingw64/mingw64/bin}"
if [[ -d "$MINGW_BIN" ]]; then
  export PATH="$MINGW_BIN:$PATH"
  echo "==> 已加入 MinGW 到 PATH: $MINGW_BIN"
else
  echo "⚠ 未找到 MinGW 目录（$MINGW_BIN）。若构建时报 windres 缺失，"
  echo "  请设置 MINGW_BIN 指向含 windres.exe 的目录。"
fi

if ! command -v windres >/dev/null 2>&1; then
  echo "✗ 仍然找不到 windres，构建会失败。请检查 MinGW 安装。" >&2
  exit 1
fi
echo "    windres: $(command -v windres)"

# ── 预检：平台门控 API ─────────────────────────────────────────────────
# 本地只编 Windows，用了「Windows 有、macOS 没有」的 Tauri API 时本地不会报错，
# 要等 CI 的 dmg 任务编译失败才发现（2026-10-13 踩过 transparent）。这里先挡一道。
# 只想跳过：SKIP_PLATFORM_CHECK=1 bash scripts/build-windows.sh
if [[ "${SKIP_PLATFORM_CHECK:-0}" == "1" ]]; then
  echo "==> 已跳过平台门控 API 预检（SKIP_PLATFORM_CHECK=1）"
elif [[ -f scripts/check-platform-api.sh ]]; then
  bash scripts/check-platform-api.sh || exit 1
fi

echo "==> 开始 Windows 构建 …"
npm run build:tauri

echo ""
echo "✅ 构建完成。产物："
ls -lh src-tauri/target/release/bundle/nsis/*.exe 2>/dev/null | awk '{print "   📦 " $NF "  (" $5 ")"}'
ls -lh src-tauri/target/release/bundle/msi/*.msi 2>/dev/null | awk '{print "   📦 " $NF "  (" $5 ")"}'

# ── 构建后自动安装到本机 ───────────────────────────────────────────────
# 桌面 / 开始菜单快捷方式指向 NSIS 的 per-user 安装目录
# （%LOCALAPPDATA%\PixVault\pixvault.exe），而不是 target/release 下的
# 构建产物。所以「只构建、不安装」的话，点图标启动的永远是上一次安装进去
# 的旧版本 —— 看起来就像修复没生效。
# 这里默认打包完就覆盖安装一次，保证本机跑的就是刚构建的代码。
# 想只出包不安装：NO_INSTALL=1 bash scripts/build-windows.sh
#
# 【约定】脚本只构建 + 安装，**不负责启动软件** —— 启动由用户手动完成。
if [[ "${NO_INSTALL:-0}" == "1" ]]; then
  echo ""
  echo "==> 已跳过本机安装（NO_INSTALL=1）"
else
  INSTALLER="$(ls -t src-tauri/target/release/bundle/nsis/*.exe 2>/dev/null | head -1 || true)"
  if [[ -z "$INSTALLER" ]]; then
    echo "⚠ 未找到 NSIS 安装包，跳过安装。" >&2
    exit 1
  fi

  echo ""
  echo "==> 安装到本机（覆盖旧版本）…"
  # 正在运行的实例会占用 exe，先结束掉；没跑则忽略错误。
  taskkill //F //IM pixvault.exe >/dev/null 2>&1 || true
  sleep 1
  # 用相对文件名 + cmd start /wait（bash 直接执行 NSIS /S 会被 SIGTERM）。
  (cd "$(dirname "$INSTALLER")" && cmd //c "start /wait $(basename "$INSTALLER") /S")
  sleep 2

  INSTALLED="${LOCALAPPDATA:-$USERPROFILE/AppData/Local}/PixVault/pixvault.exe"
  if [[ -f "$INSTALLED" ]]; then
    built_exe="src-tauri/target/release/pixvault.exe"
    if [[ -f "$built_exe" ]]; then
      b_size=$(stat -c %s "$built_exe")
      i_size=$(stat -c %s "$INSTALLED")
      if [[ "$b_size" == "$i_size" ]]; then
        echo "    安装目录: $INSTALLED ($i_size bytes) — 与构建产物一致 ✓"
      else
        echo "    ⚠ 大小不一致：构建产物 $b_size bytes / 已安装 $i_size bytes" >&2
        exit 1
      fi
    fi
    echo "✅ 本机已更新为最新版本，直接点桌面 / 开始菜单图标即为本次构建的代码。"
  else
    echo "⚠ 未找到安装后的 exe（$INSTALLED），请确认 NSIS 安装是否成功。" >&2
    exit 1
  fi
fi
