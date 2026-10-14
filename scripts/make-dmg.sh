#!/usr/bin/env bash
#
# PixVault — macOS dmg 打包（CI 友好 / 带兜底）
#
# 为什么需要这个脚本：
#   Tauri 的 dmg 打包内部内嵌了 create-dmg 1.2.1。它在 CI 上会走 `--skip-jenkins`
#   （CI=true 时 tauri-cli 自动加上，跳过 AppleScript 排版），但 hdiutil 的
#   attach / detach / convert 阶段仍会偶发卡死，整条 `tauri build --bundles dmg`
#   直接失败，而 tauri-cli 只回一句被吞掉详情的 "failed to run bundle_dmg.sh"。
#
# 策略（层层兜底，保证有包可发）：
#   [1] 先单独打 `.app` —— 必须单独打，因为 `--bundles dmg` 成功后会**清理掉**
#       bundle/macos 下的 .app，兜底时就没得封装了。打完立刻复制到暂存目录。
#   [2] 再试 tauri 原生 dmg（保留自定义窗口/图标位置）；失败则清掉残留挂载点与
#       半成品 rw 镜像后重试一次。
#   [3] 仍失败 → 用暂存目录里的 .app 走 hdiutil 直接封装，不依赖 AppleScript。
#
# 踩过的坑（别改回去）：
#   - Tauri 的 dmg 文件名叫 `<App>_<ver>_x64.dmg`（x86_64 也是 x64，不是 x86_64），
#     不要按 rust triple 去猜文件名；本脚本一律用 glob 抓实际产物。
#   - `set -u` 下，变量后面**紧跟全角标点**会被 bash 当成变量名的一部分
#     （`${FOO}）` 安全，`$FOO）` 会报 unbound variable）。本脚本一律写 `${VAR}`。
#
# 用法：
#   bash scripts/make-dmg.sh <rust-target-triple> [out_dir]
#
# 示例：
#   bash scripts/make-dmg.sh aarch64-apple-darwin out      # CI：产物同时拷到 out/
#   bash scripts/make-dmg.sh x86_64-apple-darwin           # 只留在 bundle 目录
#   bash scripts/make-dmg.sh "$(rustc -vV | awk '/^host:/{print $2}')"
#
set -uo pipefail

TARGET="${1:-}"
OUT_DIR="${2:-}"

if [[ -z "${TARGET}" ]]; then
  echo "用法: bash scripts/make-dmg.sh <rust-target-triple> [out_dir]" >&2
  echo "  例: bash scripts/make-dmg.sh aarch64-apple-darwin out" >&2
  exit 2
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

APP_NAME="PixVault"
BUNDLE_DIR="src-tauri/target/${TARGET}/release/bundle"
DMG_DIR="${BUNDLE_DIR}/dmg"
APP_DIR="${BUNDLE_DIR}/macos"
APP_BUNDLE="${APP_DIR}/${APP_NAME}.app"

# 与 Tauri 的命名保持一致：x86_64 -> x64，aarch64 -> aarch64
case "${TARGET}" in
  aarch64-apple-darwin) TAURI_ARCH="aarch64" ;;
  x86_64-apple-darwin)  TAURI_ARCH="x64" ;;
  *)                    TAURI_ARCH="${TARGET%%-*}" ;;
esac

VERSION="$(node -p "require('./src-tauri/tauri.conf.json').version" 2>/dev/null || echo '0.0.0')"
DMG_NAME="${APP_NAME}_${VERSION}_${TAURI_ARCH}.dmg"
FALLBACK_DMG="${DMG_DIR}/${DMG_NAME}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "✗ dmg 只能在 macOS 上构建，当前系统：$(uname -s)" >&2
  exit 1
fi

echo "==> PixVault dmg 打包"
echo "    目标: ${TARGET}    版本: ${VERSION}    产物名: ${DMG_NAME}"

if command -v pnpm >/dev/null 2>&1; then PM="pnpm"; else PM="npx"; fi

STAGE="$(mktemp -d)"
trap 'rm -rf "${STAGE}"' EXIT

# ── 清场：上一次失败的挂载点 / 半成品读写镜像会让下一次尝试同样失败 ──
cleanup_leftovers() {
  local v
  shopt -s nullglob
  for v in /Volumes/"${APP_NAME}"* /Volumes/dmg.*; do
    if [[ -d "${v}" ]]; then
      echo "    · 卸载残留挂载点 ${v}"
      hdiutil detach "${v}" -force >/dev/null 2>&1 || true
    fi
  done
  shopt -u nullglob
  rm -f "${APP_DIR}"/rw.*.dmg "${DMG_DIR}"/rw.*.dmg 2>/dev/null || true
}

# ── [1] 单独打 .app，并复制到暂存目录 ───────────────────────
echo "==> [1/3] 构建 ${APP_NAME}.app"
if ! "${PM}" tauri build --target "${TARGET}" --bundles app --verbose; then
  echo "✗ .app 构建失败，往上看 cargo 的报错" >&2
  exit 1
fi
if [[ ! -d "${APP_BUNDLE}" ]]; then
  echo "✗ 没找到 ${APP_BUNDLE}" >&2
  exit 1
fi
# dmg 打包成功后 tauri 会删掉 bundle/macos 下的 .app，先留一份保命
cp -R "${APP_BUNDLE}" "${STAGE}/" || { echo "✗ 暂存 .app 失败" >&2; exit 1; }
echo "    · 已暂存 .app（兜底用）"

# ── [2] tauri 原生 dmg 打包，失败清场重试一次 ───────────────
try_tauri_dmg() {
  local n="${1}" log="/tmp/pixvault-dmg-attempt-${n}.log"
  echo "==> [2/3] 第 ${n} 次尝试：tauri 原生打包 dmg"
  # --verbose 会把内嵌 bundle_dmg.sh 的输出透出来，失败时才查得到真正原因
  "${PM}" tauri build --target "${TARGET}" --bundles dmg --verbose 2>&1 | tee "${log}"
  local rc=${PIPESTATUS[0]}
  if [[ ${rc} -ne 0 ]]; then
    echo "    ! 第 ${n} 次打包失败（退出码 ${rc}），关键行："
    grep -nE "AppleEvent|timed out|hdiutil|Resource busy|not detached|Failed running AppleScript|diskimages|No such file" "${log}" 2>/dev/null | tail -n 20 || true
  fi
  return ${rc}
}

for n in 1 2; do
  cleanup_leftovers
  # 清掉可能残留的旧 dmg，避免把上一轮的文件误当成本轮产物
  rm -f "${DMG_DIR}"/*.dmg 2>/dev/null || true
  if try_tauri_dmg "${n}"; then break; fi
  echo "    · 清场后准备重试 …"
  sleep 5
done

# 用 glob 抓实际产物 —— 不要按 triple 猜文件名
newest_dmg() {
  local f
  shopt -s nullglob
  for f in "${DMG_DIR}"/*.dmg; do
    shopt -u nullglob
    printf '%s\n' "${f}"
    return 0
  done
  shopt -u nullglob
  return 1
}

NATIVE_DMG="$(newest_dmg || true)"
FINAL_DMG=""

if [[ -n "${NATIVE_DMG}" && -f "${NATIVE_DMG}" ]]; then
  echo "==> 原生打包产出：${NATIVE_DMG}"
  FINAL_DMG="${NATIVE_DMG}"
else
  echo "    · 原生打包没留下 dmg，转兜底"
  # ── [3] hdiutil 兜底 ─────────────────────────────────────
  if [[ ! -d "${STAGE}/${APP_NAME}.app" ]]; then
    echo "✗ 兜底失败：暂存目录里没有 .app（${STAGE}/${APP_NAME}.app）" >&2
    exit 1
  fi
  echo "==> [3/3] 兜底：hdiutil 直接封装 .app"
  mkdir -p "${DMG_DIR}" "${STAGE}/out"
  # -R 保留符号链接与权限；.app 内部有 symlink，必须原样带过去
  cp -R "${STAGE}/${APP_NAME}.app" "${STAGE}/out/" || { echo "✗ 复制 .app 失败" >&2; exit 1; }
  ln -s /Applications "${STAGE}/out/Applications"
  rm -f "${FALLBACK_DMG}"
  if ! hdiutil create -volname "${APP_NAME}" -srcfolder "${STAGE}/out" -ov -format UDZO "${FALLBACK_DMG}"; then
    echo "✗ hdiutil 兜底也失败了" >&2
    exit 1
  fi
  FINAL_DMG="${FALLBACK_DMG}"
fi

# ── 校验 ─────────────────────────────────────────────────────
if [[ ! -f "${FINAL_DMG}" ]]; then
  echo "✗ 未产出 dmg：${FINAL_DMG}" >&2
  exit 1
fi

SIZE="$(wc -c < "${FINAL_DMG}" | tr -d ' ')"
if [[ "${SIZE}" -lt 500000 ]]; then
  echo "✗ dmg 体积异常（${SIZE} 字节），大概率是打包中途失败留下的残片" >&2
  exit 1
fi

# UDZO/UDIF 镜像最后 512 字节是 koly 尾部块
if [[ "$(tail -c 512 "${FINAL_DMG}" | head -c 4)" == "koly" ]]; then
  echo "==> koly 尾部块校验通过"
else
  echo "✗ dmg 缺少 koly 尾部块，镜像可能不完整" >&2
  exit 1
fi

SIZE_MB="$(awk -v s="${SIZE}" 'BEGIN { printf "%.2f", s / 1048576 }')"
echo "==> 产物：${FINAL_DMG}  (${SIZE_MB} MB)"

# ── 拷到指定输出目录（CI 用） ────────────────────────────────
if [[ -n "${OUT_DIR}" ]]; then
  mkdir -p "${OUT_DIR}"
  cp -f "${FINAL_DMG}" "${OUT_DIR}/"
  echo "==> 已拷贝到 ${OUT_DIR}/ ："
  ls -lh "${OUT_DIR}/"
fi

exit 0
