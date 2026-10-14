#!/usr/bin/env node
/**
 * Cross-platform helper: open the bundle output folder in the OS file manager
 * after a `tauri build`. Failures are non-fatal (the build already succeeded).
 *
 *   Windows : opens the NSIS folder in Explorer
 *   macOS   : opens the DMG folder in Finder
 *   Linux   : opens the bundle folder with xdg-open (best effort)
 */
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const bundleDir = path.join(__dirname, "..", "src-tauri", "target", "release", "bundle");

function firstExisting(...candidates) {
  for (const c of candidates) {
    if (fs.existsSync(c)) return c;
  }
  return bundleDir;
}

function run(cmd, args) {
  try {
    spawn(cmd, args, { stdio: "ignore", detached: true }).unref();
  } catch {
    /* ignore — opening the folder is a convenience, not a requirement */
  }
}

let target;
if (process.platform === "win32") {
  target = firstExisting(path.join(bundleDir, "nsis"), path.join(bundleDir, "msi"));
  run("explorer", [target]);
} else if (process.platform === "darwin") {
  target = firstExisting(path.join(bundleDir, "dmg"), path.join(bundleDir, "macos"));
  run("open", [target]);
} else {
  target = bundleDir;
  run("xdg-open", [target]);
}

console.log(`📂 构建产物目录: ${target}`);
