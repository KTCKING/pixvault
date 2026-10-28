//! 浏览器扩展桥接相关命令。
//!
//! 真正的服务在 `crate::bridge`，这里只做「前端能调的几件事」：看状态、开关、
//! 换端口、换令牌、把扩展导到硬盘上。

use std::path::PathBuf;

use tauri::State;

use crate::bridge::{self, BridgeLibrary, BridgeStatus};
use crate::AppState;

fn current_library(state: &AppState) -> Option<BridgeLibrary> {
    let engine = state.engine.lock().ok()?;
    let arc = engine.active_library()?;
    let lib = arc.lock().ok()?;
    Some(BridgeLibrary {
        id: lib.id.clone(),
        name: lib.name.clone(),
        root_path: lib.root_path.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub fn get_bridge_status(state: State<'_, AppState>) -> BridgeStatus {
    let lib = current_library(&state);
    state.bridge.status(lib)
}

#[tauri::command]
pub fn set_bridge_enabled(enabled: bool, state: State<'_, AppState>) -> BridgeStatus {
    state.bridge.set_enabled(enabled);
    let lib = current_library(&state);
    state.bridge.status(lib)
}

#[tauri::command]
pub fn set_bridge_port(port: u16, state: State<'_, AppState>) -> Result<BridgeStatus, String> {
    if !state.bridge.set_port(port) {
        return Err("端口需要大于 1023（1024 以下要管理员权限，普通启动绑不上）".to_string());
    }
    let lib = current_library(&state);
    Ok(state.bridge.status(lib))
}

#[tauri::command]
pub fn regenerate_bridge_token(state: State<'_, AppState>) -> BridgeStatus {
    state.bridge.regenerate_token();
    let lib = current_library(&state);
    state.bridge.status(lib)
}

/// 把内置扩展到用户选的目录，返回文件数。
///
/// 用户拿到的是一个 `pixvault-extension` 文件夹，直接在浏览器「加载已解压的
/// 扩展程序」里指过去就行 —— 不用去 GitHub 找压缩包，也保证和当前版本一致。
#[tauri::command]
pub fn export_extension(dest_dir: String) -> Result<usize, String> {
    let dest = PathBuf::from(&dest_dir);
    if !dest.is_dir() {
        return Err(format!("目标不是文件夹：{dest_dir}"));
    }
    bridge::export_extension(&dest)
}

/// 打开扩展所在目录（导出后引导用户去「加载已解压的扩展程序」）。
#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    let target = if p.is_dir() { p } else { p.parent().map(|x| x.to_path_buf()).unwrap_or(p) };
    opener::open(target).map_err(|e| format!("打开失败：{e}"))
}

