use std::path::{Path, PathBuf};

/// Normalize a path for consistent storage and comparison
pub fn normalize_path(path: &Path) -> PathBuf {
    // Canonicalize resolves symlinks and normalizes separators
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Get the relative path from a base directory
pub fn relative_path(path: &Path, base: &Path) -> Option<PathBuf> {
    path.strip_prefix(base).ok().map(|p| p.to_path_buf())
}

/// Check if a path is a network mount (SMB, NFS, etc.)
pub fn is_network_path(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    path_str.starts_with("\\\\") || path_str.starts_with("//")
}

/// Get the file extension in lowercase
pub fn extension_lower(path: &Path) -> String {
    path.extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase()
}

/// Format file size for display
pub fn format_file_size(bytes: i64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[unit_idx])
    } else {
        format!("{:.1} {}", size, UNITS[unit_idx])
    }
}

/// Truncate a path for display
pub fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        return path.to_string();
    }

    let parts: Vec<&str> = path.split(&['\\', '/'][..]).collect();
    if parts.len() <= 2 {
        // Can't truncate meaningfully
        let half = (max_len - 3) / 2;
        format!("{}...{}", &path[..half], &path[path.len() - half..])
    } else {
        // Keep first and last parts, truncate middle
        let first = parts[0];
        let last = parts[parts.len() - 1];
        format!("{}\\...\\{}", first, last)
    }
}