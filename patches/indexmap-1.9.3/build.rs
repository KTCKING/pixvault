fn main() {
    // If "std" is explicitly requested, don't bother probing the target for it.
    match std::env::var_os("CARGO_FEATURE_STD") {
        Some(_) => autocfg::emit("has_std"),
        None => {
            // Patch: force has_std on Windows targets where autocfg sysroot probing
            // fails under the GNU toolchain (this crate is always used in a std
            // context on desktop apps).
            #[cfg(windows)]
            autocfg::emit("has_std");
            #[cfg(not(windows))]
            autocfg::new().emit_sysroot_crate("std");
        }
    }
    autocfg::rerun_path("build.rs");
}
