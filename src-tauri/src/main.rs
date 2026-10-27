// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The elevated installer never starts Tauri, reads accounts, or opens a window.
    #[cfg(windows)]
    if let Some(code) = tuken_lib::desktop_windows::helper_entry() {
        std::process::exit(code);
    }
    tuken_lib::run();
}
