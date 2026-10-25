//! Claude Desktop on this PC: find the installed app, install it through
//! winget, and open or restart it so a new gateway profile takes effect.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{json, Value};

use crate::claude_config;

const DOWNLOAD_PAGE: &str = "https://claude.ai/download";
const WINGET_ID: &str = "Anthropic.Claude";
/// Anthropic's MSIX publisher id: package family `Claude_pzs8sxrjxfjjc`.
const MSIX_PUBLISHER_ID: &str = "pzs8sxrjxfjjc";

static INSTALLING: AtomicBool = AtomicBool::new(false);

struct Install {
    version: String,
    exe: PathBuf,
    /// Set for the MSIX package, which has to be started through its AUMID.
    aumid: Option<String>,
}

fn parse_version(v: &str) -> Vec<u32> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

#[cfg(windows)]
fn find_install() -> Option<Install> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    const PACKAGES: &str = r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages";

    // MSIX: one key per installed package, e.g. Claude_2.42304.0.0_x64__pzs8sxrjxfjjc.
    let suffix = format!("__{MSIX_PUBLISHER_ID}");
    let mut best: Option<(Vec<u32>, Install)> = None;
    if let Ok(packages) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(PACKAGES) {
        for name in packages.enum_keys().flatten() {
            let Some(rest) = name
                .strip_prefix("Claude_")
                .and_then(|r| r.strip_suffix(suffix.as_str()))
            else {
                continue;
            };
            let version = rest.split('_').next().unwrap_or_default().to_string();
            let Ok(root) = packages
                .open_subkey(&name)
                .and_then(|k| k.get_value::<String, _>("PackageRootFolder"))
            else {
                continue;
            };
            let exe = PathBuf::from(root).join("app").join("Claude.exe");
            if !exe.exists() {
                continue;
            }
            let parsed = parse_version(&version);
            if best.as_ref().is_none_or(|(v, _)| parsed > *v) {
                best = Some((
                    parsed,
                    Install {
                        version,
                        exe,
                        aumid: Some(format!("Claude_{MSIX_PUBLISHER_ID}!Claude")),
                    },
                ));
            }
        }
    }
    if let Some((_, install)) = best {
        return Some(install);
    }

    // Older Squirrel builds live in %LOCALAPPDATA%\AnthropicClaude.
    let root = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("AnthropicClaude");
    let exe = root.join("claude.exe");
    if !exe.exists() {
        return None;
    }
    let version = std::fs::read_dir(&root)
        .ok()?
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_str()?
                .strip_prefix("app-")
                .map(str::to_string)
        })
        .max_by_key(|v| parse_version(v))
        .unwrap_or_default();
    Some(Install {
        version,
        exe,
        aumid: None,
    })
}

#[cfg(not(windows))]
fn find_install() -> Option<Install> {
    None
}

/// Processes started from `exe`. Matching the full path keeps Claude Code CLI
/// (`claude.exe` anywhere else) out of it.
#[cfg(windows)]
fn running_pids(exe: &std::path::Path) -> Vec<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let wanted = exe.to_string_lossy().to_lowercase();
    let file_name = exe
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mut pids = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return pids;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snapshot, &mut entry);
        while ok != 0 {
            let len = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
            if name == file_name {
                let process =
                    OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
                if !process.is_null() {
                    let mut buf = [0u16; 1024];
                    let mut size = buf.len() as u32;
                    if QueryFullProcessImageNameW(
                        process,
                        PROCESS_NAME_WIN32,
                        buf.as_mut_ptr(),
                        &mut size,
                    ) != 0
                        && String::from_utf16_lossy(&buf[..size as usize]).to_lowercase() == wanted
                    {
                        pids.push(entry.th32ProcessID);
                    }
                    CloseHandle(process);
                }
            }
            ok = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
    }
    pids
}

#[cfg(not(windows))]
fn running_pids(_: &std::path::Path) -> Vec<u32> {
    Vec::new()
}

#[cfg(windows)]
fn terminate(pids: &[u32]) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    for pid in pids {
        unsafe {
            let process = OpenProcess(PROCESS_TERMINATE, 0, *pid);
            if !process.is_null() {
                TerminateProcess(process, 0);
                CloseHandle(process);
            }
        }
    }
}

#[cfg(not(windows))]
fn terminate(_: &[u32]) {}

fn launch(install: &Install) -> Result<(), String> {
    let mut command = match &install.aumid {
        Some(aumid) => {
            let mut c = std::process::Command::new("explorer.exe");
            c.arg(format!("shell:AppsFolder\\{aumid}"));
            c
        }
        None => std::process::Command::new(&install.exe),
    };
    command
        .spawn()
        .map(|_| ())
        .map_err(|_| "无法启动 Claude Desktop".to_string())
}

#[tauri::command]
pub async fn desktop_status(app: tauri::AppHandle) -> Result<Value, String> {
    let install = find_install();
    let running = install
        .as_ref()
        .is_some_and(|i| !running_pids(&i.exe).is_empty());
    let paths = crate::config_paths(&app)?;
    Ok(json!({
        "supported": cfg!(windows),
        "installed": install.is_some(),
        "version": install.as_ref().map(|i| i.version.clone()),
        "running": running,
        "installing": INSTALLING.load(Ordering::SeqCst),
        // Dev builds find the developer's own Claude Desktop through the
        // registry, which a sandbox can't redirect, so they never quit it.
        "can_restart": !cfg!(debug_assertions),
        "connected": claude_config::desktop_key(&paths).is_some(),
        "cli_connected": claude_config::cli_key(&paths).is_some(),
        // The default model (first) and effort 兔啃 set, while connected.
        "models": claude_config::desktop_models(&paths),
    }))
}

/// Chooses what Claude Desktop starts new chats with on 兔啃: `models` are
/// offered in its model menu, the first being the default; `effort` is the
/// default reasoning effort. Takes effect the next time it starts.
#[tauri::command]
pub async fn desktop_models(
    app: tauri::AppHandle,
    models: Vec<String>,
    effort: String,
) -> Result<(), String> {
    let paths = crate::config_paths(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        claude_config::set_desktop_models(&paths, &models, &effort)
    })
    .await
    .map_err(|_| "写入配置失败".to_string())?
}

/// Installs Claude Desktop with winget (which verifies the installer hash), or
/// opens the official download page when winget is missing.
#[tauri::command]
pub async fn desktop_install(app: tauri::AppHandle) -> Result<Value, String> {
    use tauri_plugin_opener::OpenerExt;
    if find_install().is_some() {
        return Ok(json!({"method": "none"}));
    }
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err("Claude Desktop 正在安装，请稍候".into());
    }
    let result = async {
        let mut command = tokio::process::Command::new("winget");
        command.args([
            "install",
            "--id",
            WINGET_ID,
            "--exact",
            "--source",
            "winget",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ]);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        command.kill_on_drop(true);
        match tokio::time::timeout(Duration::from_secs(20 * 60), command.output()).await {
            Err(_) => Err("安装超时，请稍后重试或到官网下载".to_string()),
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                app.opener()
                    .open_url(DOWNLOAD_PAGE, None::<&str>)
                    .map_err(|_| "无法打开下载页面".to_string())?;
                Ok(json!({"method": "browser"}))
            }
            Ok(Err(_)) => Err("无法启动 winget".to_string()),
            Ok(Ok(output)) => {
                // winget exits non-zero for "already installed" too; the install itself is the proof.
                if find_install().is_some() {
                    Ok(json!({"method": "winget"}))
                } else {
                    log::warn!(
                        "winget install {WINGET_ID} failed: {:?} {}",
                        output.status.code(),
                        String::from_utf8_lossy(&output.stdout)
                            .lines()
                            .last()
                            .unwrap_or_default()
                    );
                    Err("安装没有完成，请稍后重试或到官网下载".to_string())
                }
            }
        }
    }
    .await;
    INSTALLING.store(false, Ordering::SeqCst);
    result
}

/// Opens Claude Desktop. With `restart`, quits the running app first so a new
/// gateway profile takes effect; unsaved input in Claude Desktop is lost.
#[tauri::command]
pub async fn desktop_open(restart: bool) -> Result<(), String> {
    let install = find_install().ok_or("未检测到 Claude Desktop")?;
    if restart {
        if cfg!(debug_assertions) {
            return Err("开发版不会重启 Claude Desktop，请手动重启".into());
        }
        let pids = running_pids(&install.exe);
        if !pids.is_empty() {
            terminate(&pids);
            for _ in 0..40 {
                if running_pids(&install.exe).is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }
    launch(&install)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert!(parse_version("2.42304.0.0") > parse_version("2.9999.0.0"));
        assert!(parse_version("1.10.0") > parse_version("1.9.9"));
    }

    /// Reads this PC's real registry and process list; run by hand.
    #[test]
    #[ignore]
    fn detects_local_install() {
        let install = find_install().expect("Claude Desktop is installed");
        println!(
            "{} {:?} running={:?}",
            install.version,
            install.exe,
            running_pids(&install.exe)
        );
    }
}
