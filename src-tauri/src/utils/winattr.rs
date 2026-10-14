//! 文件属性的平台适配。
//!
//! 图库的元数据目录固定叫 `.pixvault`。点前缀在 macOS / Linux 上是「隐藏」的
//! 约定，系统与文件管理器都会默认不显示；但 **Windows 不认识点前缀** ——
//! 资源管理器会老老实实把 `.pixvault` 当成一个普通文件夹，摆在用户的照片堆里。
//! 结果就是同一个软件在不同平台行为不一致，而且用户整理照片时很容易顺手
//! 把整个 `.pixvault` 删掉（里面装着标签、评分、备注和整库缩略图缓存）。
//!
//! 所以 Windows 上需要显式补一个「隐藏」属性位。这里不引入任何新依赖，
//! 直接声明 kernel32 的两个函数即可。

use std::path::Path;

/// `FILE_ATTRIBUTE_HIDDEN` —— 资源管理器默认不显示。
///
/// 三个常量都只给 Windows 实现用；加 `cfg` 是为了在 macOS / Linux 上不产生
/// `never used` 告警（CI 的 macOS 任务会把告警刷满）。
#[cfg(windows)]
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x0000_0002;
/// `FILE_ATTRIBUTE_NOT_CONTENT_INDEXED` —— 不参与 Windows 搜索的内容索引。
/// 库里装了上万张缩略图和几个 SQLite 文件，它们既不该被索引，也不该出现在
/// 用户「在这台电脑上搜索图片」的结果里。
#[cfg(windows)]
const FILE_ATTRIBUTE_NOT_CONTENT_INDEXED: u32 = 0x0000_2000;
/// `GetFileAttributesW` 的失败返回值。
#[cfg(windows)]
const INVALID_FILE_ATTRIBUTES: u32 = 0xFFFF_FFFF;

#[cfg(windows)]
mod imp {
    use super::*;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileAttributesW(lp_file_name: *const u16) -> u32;
        fn SetFileAttributesW(lp_file_name: *const u16, dw_file_attributes: u32) -> i32;
    }

    /// 转成以 NUL 结尾的宽字符（UTF-16）路径。
    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }

    /// 给路径补上「隐藏 + 不参与索引」两个属性位，其余属性原样保留。
    ///
    /// 幂等：已经是期望状态就什么都不做（省掉一次写属性，也避免在用户的
    /// 同步盘上制造无意义的时间戳变动）。返回是否真的写入了属性。
    ///
    /// 失败（路径不存在、无权限、只读介质）一律静默返回 `false` —— 属性只是
    /// 观感与防误删，绝不能因为它失败就影响图库打开。
    pub fn set_hidden(path: &Path) -> bool {
        let w = wide(path);
        unsafe {
            let current = GetFileAttributesW(w.as_ptr());
            if current == INVALID_FILE_ATTRIBUTES {
                return false;
            }
            let wanted = current | FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED;
            if wanted == current {
                return false;
            }
            SetFileAttributesW(w.as_ptr(), wanted) != 0
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    /// macOS / Linux：点前缀本身就是隐藏约定，无需额外处理。
    pub fn set_hidden(_path: &Path) -> bool {
        false
    }
}

pub use imp::set_hidden;

/// 图库元数据目录（`<库根>/.pixvault`）的标准位置。
///
/// 抽出来是为了让「构造路径」和「打隐藏属性」永远指向同一个地方 ——
/// 之前 `.pixvault` 这个字面量散落在好几个文件里，容易改漏。
pub fn metadata_dir(library_root: &Path) -> std::path::PathBuf {
    library_root.join(".pixvault")
}

/// 隐藏图库元数据目录（幂等，可重复调用）。
///
/// 新建图库、恢复图库、重新打开已加载的库都会走到这里，所以老库第一次被
/// 打开时会自动补齐属性，不需要用户手动作任何事。
pub fn hide_library_metadata(library_root: &Path) -> bool {
    set_hidden(&metadata_dir(library_root))
}
