//! Claude Desktop on this computer: find the installed app, install it
//! (the 兔啃 download site on Windows, Anthropic's signed disk image on
//! macOS), and open or restart it so a new gateway profile takes effect.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{json, Value};

use crate::claude_config;

#[cfg(target_os = "macos")]
const DOWNLOAD_PAGE: &str = "https://claude.ai/download";
/// Anthropic's MSIX publisher id: package family `Claude_pzs8sxrjxfjjc`.
#[cfg(windows)]
const MSIX_PUBLISHER_ID: &str = "pzs8sxrjxfjjc";

static INSTALLING: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
static INSTALL_STEP: std::sync::Mutex<Option<Value>> = std::sync::Mutex::new(None);

#[cfg(windows)]
struct Installing;
#[cfg(windows)]
impl Drop for Installing {
    fn drop(&mut self) {
        *INSTALL_STEP.lock().unwrap() = None;
        INSTALLING.store(false, Ordering::SeqCst);
    }
}

struct Install {
    version: String,
    exe: PathBuf,
    /// Set for the MSIX package, which has to be started through its AUMID.
    #[cfg(not(target_os = "macos"))]
    aumid: Option<String>,
    /// macOS: Claude.app, which Launch Services opens.
    #[cfg(target_os = "macos")]
    app: PathBuf,
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

/// Claude.app in /Applications or ~/Applications, known by its bundle id;
/// the newer one if both are there, as `open -b` would pick.
#[cfg(target_os = "macos")]
fn find_install() -> Option<Install> {
    let mut best: Option<(Vec<u32>, Install)> = None;
    for dir in crate::desktop_mac::applications() {
        let app = dir.join("Claude.app");
        let Some(bundle) = crate::desktop_mac::read_bundle(&app) else {
            continue;
        };
        if !bundle.exe.is_file() {
            continue;
        }
        let parsed = parse_version(&bundle.version);
        if best.as_ref().is_none_or(|(v, _)| parsed > *v) {
            best = Some((
                parsed,
                Install {
                    version: bundle.version,
                    exe: bundle.exe,
                    app,
                },
            ));
        }
    }
    best.map(|(_, install)| install)
}

#[cfg(not(any(windows, target_os = "macos")))]
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

#[cfg(target_os = "macos")]
fn running_pids(exe: &std::path::Path) -> Vec<u32> {
    crate::desktop_mac::running_pids(exe)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn running_pids(_: &std::path::Path) -> Vec<u32> {
    Vec::new()
}

/// The Claude Code engines Claude Desktop runs now: its own child processes
/// speaking stream-json. None when they can't be read (empty when it isn't
/// running). Only reads; see `conversations` for why.
pub fn engines() -> Option<Vec<crate::conversations::Engine>> {
    let Some(install) = find_install() else {
        return Some(Vec::new());
    };
    let desktop = running_pids(&install.exe);
    if desktop.is_empty() {
        return Some(Vec::new());
    }
    child_engines(&desktop)
}

#[cfg(windows)]
fn child_engines(desktop: &[u32]) -> Option<Vec<crate::conversations::Engine>> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let mut children = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snapshot, &mut entry);
        while ok != 0 {
            // Its helpers run its own program; the engines are claude.exe too.
            if desktop.contains(&entry.th32ParentProcessID)
                && !desktop.contains(&entry.th32ProcessID)
            {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                if name == "claude.exe" {
                    children.push(entry.th32ProcessID);
                }
            }
            ok = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
    }
    let mut engines = Vec::new();
    for pid in children {
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let line = command_line(process);
            let mut created: FILETIME = std::mem::zeroed();
            let mut unused: [FILETIME; 3] = std::mem::zeroed();
            let timed = GetProcessTimes(
                process,
                &mut created,
                &mut unused[0],
                &mut unused[1],
                &mut unused[2],
            ) != 0;
            CloseHandle(process);
            let line = line?;
            if !line.contains("stream-json") {
                continue;
            }
            // 100 ns ticks since 1601 → ms since 1970.
            let ticks =
                (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
            let started = if timed {
                (ticks / 10_000) as i64 - 11_644_473_600_000
            } else {
                0
            };
            engines.push(crate::conversations::Engine {
                started,
                resumed: crate::conversations::resumed_in(&line),
            });
        }
    }
    Some(engines)
}

/// Another process's command line (ProcessCommandLineInformation, Windows 8.1+).
#[cfg(windows)]
unsafe fn command_line(process: windows_sys::Win32::Foundation::HANDLE) -> Option<String> {
    use windows_sys::Wdk::System::Threading::{
        NtQueryInformationProcess, ProcessCommandLineInformation,
    };
    use windows_sys::Win32::Foundation::UNICODE_STRING;
    let mut len = 0u32;
    NtQueryInformationProcess(
        process,
        ProcessCommandLineInformation,
        std::ptr::null_mut(),
        0,
        &mut len,
    );
    if (len as usize) < std::mem::size_of::<UNICODE_STRING>() || len > 1 << 20 {
        return None;
    }
    // u64s, so the UNICODE_STRING at its start is aligned.
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    if NtQueryInformationProcess(
        process,
        ProcessCommandLineInformation,
        buf.as_mut_ptr().cast(),
        len,
        &mut len,
    ) < 0
    {
        return None;
    }
    let text = &*(buf.as_ptr() as *const UNICODE_STRING);
    // The characters follow it, inside the same buffer.
    let start = buf.as_ptr() as usize;
    let at = text.Buffer as usize;
    let bytes = usize::from(text.Length);
    if text.Buffer.is_null() || at < start || at + bytes > start + buf.len() * 8 {
        return None;
    }
    Some(String::from_utf16_lossy(std::slice::from_raw_parts(
        text.Buffer,
        bytes / 2,
    )))
}

#[cfg(target_os = "macos")]
fn child_engines(desktop: &[u32]) -> Option<Vec<crate::conversations::Engine>> {
    let out = std::process::Command::new("/bin/ps")
        .args(["-axww", "-o", "pid=,ppid=,etime=,command="])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    Some(engines_in_ps(
        &String::from_utf8_lossy(&out.stdout),
        desktop,
        now,
    ))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn child_engines(_: &[u32]) -> Option<Vec<crate::conversations::Engine>> {
    Some(Vec::new())
}

/// The engines in `ps -o pid=,ppid=,etime=,command=` output: children of
/// Claude Desktop speaking stream-json; etime is `[[dd-]hh:]mm:ss`.
#[cfg(any(target_os = "macos", test))]
fn engines_in_ps(text: &str, desktop: &[u32], now: i64) -> Vec<crate::conversations::Engine> {
    let elapsed = |s: &str| -> Option<i64> {
        let (days, rest) = match s.split_once('-') {
            Some((d, r)) => (d.parse::<i64>().ok()?, r),
            None => (0, s),
        };
        let mut secs = 0i64;
        for part in rest.split(':') {
            secs = secs * 60 + part.parse::<i64>().ok()?;
        }
        Some((days * 86_400 + secs) * 1000)
    };
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let pid: u32 = words.next()?.parse().ok()?;
            let ppid: u32 = words.next()?.parse().ok()?;
            let age = elapsed(words.next()?)?;
            let command = words.collect::<Vec<_>>().join(" ");
            (desktop.contains(&ppid) && !desktop.contains(&pid) && command.contains("stream-json"))
                .then(|| crate::conversations::Engine {
                    started: now - age,
                    resumed: crate::conversations::resumed_in(&command),
                })
        })
        .collect()
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

/// Stops the running Claude Desktop before it is opened again. On macOS it is
/// asked to quit as ⌘Q does (see `desktop_mac::signal`); what still runs
/// after 10 seconds is stopped, as Windows stops it right away.
async fn quit_running(install: &Install) {
    let pids = running_pids(&install.exe);
    if pids.is_empty() {
        return;
    }
    #[cfg(target_os = "macos")]
    crate::desktop_mac::signal(&pids, libc::SIGTERM);
    #[cfg(not(target_os = "macos"))]
    terminate(&pids);
    for _ in 0..40 {
        if running_pids(&install.exe).is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    #[cfg(target_os = "macos")]
    {
        let left = running_pids(&install.exe);
        if !left.is_empty() {
            crate::desktop_mac::signal(&left, libc::SIGKILL);
            for _ in 0..20 {
                if running_pids(&install.exe).is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
        // Launch Services takes a moment to see the app gone; opening it
        // before then only brings back the one that is quitting.
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn terminate(_: &[u32]) {}

/// Opens Claude.app (the one found, then by bundle id).
#[cfg(target_os = "macos")]
fn launch(install: &Install) -> Result<(), String> {
    let open = |args: &[&std::ffi::OsStr]| {
        std::process::Command::new("/usr/bin/open")
            .args(args)
            .stdin(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    if open(&[install.app.as_os_str()])
        || open(&["-b".as_ref(), crate::desktop_mac::BUNDLE_ID.as_ref()])
    {
        Ok(())
    } else {
        Err("无法启动 Claude Desktop".to_string())
    }
}

#[cfg(not(target_os = "macos"))]
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

/// Whether Claude Desktop is open: its own program, not Claude Code.
pub fn running() -> bool {
    find_install().is_some_and(|i| !running_pids(&i.exe).is_empty())
}

/// The Code engine of the installed Claude Desktop, for 手机遥控. A dev build
/// may point `TUKEN_CLAUDE_CODE` at one, as its own data folder has none.
pub fn code_engine(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if cfg!(debug_assertions) {
        if let Some(p) = std::env::var_os("TUKEN_CLAUDE_CODE").map(PathBuf::from) {
            return p
                .is_file()
                .then_some(p)
                .ok_or_else(|| "TUKEN_CLAUDE_CODE 指向的文件不存在".into());
        }
    }
    let missing = "Claude Desktop 里的 Claude Code 组件还没装好：请先在电脑上的兔啃首页修复";
    let install = find_install().ok_or("这台电脑还没装 Claude Desktop：请先在兔啃首页安装")?;
    #[cfg(windows)]
    {
        let spec = crate::desktop_code::installed_spec(&install.exe)?;
        let root = crate::config_paths(app)?.desktop_code_cache(false)?;
        if !crate::desktop_code::ready(&root, &spec) {
            return Err(missing.into());
        }
        Ok(spec.binary(&root))
    }
    #[cfg(target_os = "macos")]
    {
        let _ = (app, &install);
        // Claude Desktop keeps it at claude-code/<version>/<checksum>/claude.app.
        let root = dirs::data_dir()
            .ok_or(missing)?
            .join("Claude-3p")
            .join("claude-code");
        let mut found: Vec<(Vec<u32>, PathBuf)> = Vec::new();
        for version in std::fs::read_dir(&root).map_err(|_| missing)?.flatten() {
            for build in std::fs::read_dir(version.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                let exe = build.path().join("claude.app/Contents/MacOS/claude");
                if exe.is_file() {
                    found.push((parse_version(&version.file_name().to_string_lossy()), exe));
                }
            }
        }
        found.sort();
        found
            .pop()
            .map(|(_, exe)| exe)
            .ok_or_else(|| missing.into())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (app, install, missing);
        Err("这个系统暂不支持手机遥控".into())
    }
}

#[tauri::command]
pub async fn desktop_status(app: tauri::AppHandle) -> Result<Value, String> {
    #[cfg(windows)]
    let install_step = INSTALL_STEP.lock().unwrap().clone();
    #[cfg(not(windows))]
    let install_step: Option<Value> = None;
    let install = find_install();
    let running = install
        .as_ref()
        .is_some_and(|i| !running_pids(&i.exe).is_empty());
    let paths = crate::config_paths(&app)?;
    #[cfg(windows)]
    let code_ready = install.as_ref().is_some_and(|i| {
        crate::desktop_code::installed_spec(&i.exe).is_ok_and(|spec| {
            paths
                .desktop_code_cache(false)
                .is_ok_and(|root| crate::desktop_code::ready(&root, &spec))
        })
    });
    #[cfg(not(windows))]
    let code_ready: Option<bool> = None;
    Ok(json!({
        "supported": cfg!(any(windows, target_os = "macos")),
        "installed": install.is_some(),
        "version": install.as_ref().map(|i| i.version.clone()),
        "running": running,
        "installing": INSTALLING.load(Ordering::SeqCst),
        "install_step": install_step,
        "code_ready": code_ready,
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

/// Installs Claude Desktop from Anthropic's disk image once its signature and
/// notarization check out, reporting progress as `tuken://desktop-install`
/// events. Something on this Mac in the way (no room, no permission) is said
/// as it is; any other failure opens the official download page instead.
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn desktop_install(app: tauri::AppHandle) -> Result<Value, String> {
    use tauri::Emitter;
    use tauri_plugin_opener::OpenerExt;
    let paths = crate::config_paths(&app)?;
    if find_install().is_some() {
        claude_config::prepare_desktop_first_launch(&paths)?;
        return Ok(json!({"method": "none"}));
    }
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err("Claude Desktop 正在安装，请稍候".into());
    }
    let events = app.clone();
    let result = crate::desktop_mac::install(move |step| {
        let _ = events.emit("tuken://desktop-install", step);
    })
    .await;
    INSTALLING.store(false, Ordering::SeqCst);
    match result {
        Ok(_) if find_install().is_some() => {
            claude_config::prepare_desktop_first_launch(&paths)?;
            Ok(json!({"method": "dmg"}))
        }
        Err(crate::desktop_mac::Failure::Local(why)) => Err(why),
        outcome => {
            if let Err(crate::desktop_mac::Failure::Elsewhere(why)) = &outcome {
                log::warn!("Claude Desktop install failed: {why}");
            }
            app.opener()
                .open_url(DOWNLOAD_PAGE, None::<&str>)
                .map_err(|_| "无法打开下载页面".to_string())?;
            Ok(json!({"method": "browser"}))
        }
    }
}

/// Downloads the full MSIX and deploys it locally. No browser or bootstrapper.
#[cfg(windows)]
#[tauri::command]
pub async fn desktop_install(app: tauri::AppHandle) -> Result<Value, String> {
    use tauri::Emitter;
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err("Claude Desktop 正在安装，请稍候".into());
    }
    let _guard = Installing;
    let paths = crate::config_paths(&app)?;
    let root = paths.desktop_code_cache(true)?;
    let progress = move |step: Value| {
        *INSTALL_STEP.lock().unwrap() = Some(step.clone());
        let _ = app.emit("tuken://desktop-install", step);
    };
    let mut install = find_install();
    let fresh = install.is_none();
    if fresh {
        crate::desktop_windows::install(progress.clone()).await?;
    }
    // Registration must be visible to the original account, even if UAC used
    // another administrator's credentials. Never claim that download = installed.
    for _ in 0..20 {
        if install.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        install = find_install();
    }
    let install = install.ok_or("安装后未能在当前账户检测到 Claude Desktop。请用当前 Windows 账户授权安装，或重启电脑后重试")?;
    let spec = crate::desktop_code::installed_spec(&install.exe)?;
    crate::desktop_code::ensure(root, spec, progress).await?;
    claude_config::prepare_desktop_first_launch(&paths)?;
    Ok(json!({"method": if fresh { "msix" } else { "runtime" }}))
}

#[cfg(not(any(windows, target_os = "macos")))]
#[tauri::command]
pub async fn desktop_install(_app: tauri::AppHandle) -> Result<Value, String> {
    Err("目前只支持 Windows 和 macOS".into())
}

/// Opens Claude Desktop. With `restart`, quits the running app first so a new
/// gateway profile takes effect; unsaved input in Claude Desktop is lost.
#[tauri::command]
pub async fn desktop_open(app: tauri::AppHandle, restart: bool) -> Result<(), String> {
    // Opening through Tuken also repairs older installs before the first prompt.
    #[cfg(windows)]
    desktop_install(app.clone()).await?;
    let install = find_install().ok_or("未检测到 Claude Desktop")?;
    let paths = crate::config_paths(&app)?;
    claude_config::prepare_desktop_first_launch(&paths)?;
    if restart {
        if cfg!(debug_assertions) {
            return Err("开发版不会重启 Claude Desktop，请手动重启".into());
        }
        quit_running(&install).await;
    }
    // Closed now (it reads its preferences only when it starts): new users
    // get bypass permissions offered, and new conversations start in it.
    if running_pids(&install.exe).is_empty() {
        if let Err(why) = claude_config::seed_bypass(&paths) {
            log::warn!("bypass permissions not offered: {why}");
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
        // Reads only: when each engine started and what it resumed.
        println!("engines={:?}", engines());
    }

    #[test]
    fn engines_are_claude_desktops_children_speaking_stream_json() {
        let ps = "\
  100     1    1-02:03:04 /Applications/Claude.app/Contents/MacOS/Claude
  101   100       10:00 /Applications/Claude.app/Contents/Frameworks/Claude Helper --type=gpu
  102   100    02:00:05 /x/claude-code/2.1/claude.app/Contents/MacOS/claude --output-format stream-json --resume=11111111-1111-4111-8111-111111111111
  103   100       00:30 /x/claude-code/2.1/claude.app/Contents/MacOS/claude --output-format stream-json
  104   555       00:30 claude --output-format stream-json --resume=22222222-2222-4222-8222-222222222222
";
        let now = 10_000_000_000;
        let found = engines_in_ps(ps, &[100, 101], now);
        assert_eq!(
            found,
            vec![
                crate::conversations::Engine {
                    started: now - (2 * 3600 + 5) * 1000,
                    resumed: Some("11111111-1111-4111-8111-111111111111".into()),
                },
                crate::conversations::Engine {
                    started: now - 30_000,
                    resumed: None,
                },
            ]
        );
    }

    #[cfg(target_os = "macos")]
    async fn wait(seconds: u64, done: impl Fn() -> bool) -> bool {
        for _ in 0..seconds * 4 {
            if done() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        done()
    }

    /// All of it on a Mac without Claude Desktop (GitHub's macOS runner):
    /// install it from Anthropic's disk image, write 兔啃's profile, open it
    /// and see it take the profile, then restart it the way 兔啃 does. It
    /// installs and quits Claude Desktop, so it only runs with TUKEN_E2E=1:
    /// `TUKEN_E2E=1 cargo test macos_e2e -- --ignored --nocapture`.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    #[ignore]
    async fn macos_e2e_install_connect_open_restart() {
        use crate::desktop_mac::Step;
        assert_eq!(
            std::env::var("TUKEN_E2E").as_deref(),
            Ok("1"),
            "this installs and quits Claude Desktop; set TUKEN_E2E=1"
        );
        if find_install().is_none() {
            let steps = std::sync::Mutex::new(Vec::new());
            let app = crate::desktop_mac::install(|s| steps.lock().unwrap().push(s))
                .await
                .expect("Claude Desktop installs from its disk image");
            let steps = steps.into_inner().unwrap();
            println!("installed {app:?}; steps {:?}", steps.last());
            assert!(steps.iter().any(|s| matches!(s, Step::Download { .. })));
            assert!(steps.contains(&Step::Verify) && steps.contains(&Step::Copy));
        }
        let install = find_install().expect("the installed Claude Desktop is found");
        println!("Claude Desktop {} at {:?}", install.version, install.app);
        assert!(install.exe.ends_with("Contents/MacOS/Claude"));

        // 兔啃's profile, written as connecting writes it, at a gateway that
        // doesn't exist, so nothing reaches 兔啃's server.
        let backups = tempfile::tempdir().unwrap();
        let paths = claude_config::Paths::system(backups.path().into()).unwrap();
        claude_config::connect_desktop(&paths, "sk-tuken-e2e").unwrap();
        claude_config::set_desktop_models(&paths, &["claude-opus-4-1".into()], "max").unwrap();
        let home = dirs::home_dir().unwrap();
        let threep = home.join("Library/Application Support/Claude-3p");
        let profile = threep.join(format!("configLibrary/{}.json", claude_config::PROFILE_ID));
        let text = std::fs::read_to_string(&profile).unwrap();
        std::fs::write(
            &profile,
            text.replace(crate::api::SITE, "https://gateway.invalid"),
        )
        .unwrap();

        // Claude Desktop opens on the profile: it moves its data to Claude-3p,
        // where Chromium then keeps its "Local State".
        launch(&install).unwrap();
        assert!(
            wait(60, || !running_pids(&install.exe).is_empty()).await,
            "Claude Desktop runs"
        );
        assert!(
            wait(120, || threep.join("Local State").exists()).await,
            "Claude Desktop runs from its third-party data folder"
        );
        let meta = std::fs::read_to_string(threep.join("configLibrary/_meta.json")).unwrap();
        assert!(meta.contains(claude_config::PROFILE_ID), "{meta}");
        // Its log is in Logs/Claude-3p too; what it says of the profile.
        let log = std::fs::read_to_string(home.join("Library/Logs/Claude-3p/main.log"))
            .expect("Claude Desktop logs as a third-party install");
        let about: Vec<&str> = log
            .lines()
            .filter(|l| {
                let l = l.to_lowercase();
                [
                    "3p",
                    "inference",
                    "gateway",
                    "configlibrary",
                    "managed config",
                ]
                .iter()
                .any(|w| l.contains(w))
            })
            .take(40)
            .collect();
        println!("main.log about the profile:\n{}", about.join("\n"));

        // Restarted the way 兔啃 does: asked to quit, it quits within the
        // 10 seconds, and opens again.
        let before = running_pids(&install.exe);
        let started = std::time::Instant::now();
        quit_running(&install).await;
        let took = started.elapsed();
        println!("quit in {took:?}");
        assert!(running_pids(&install.exe).is_empty(), "Claude Desktop quit");
        assert!(
            took < Duration::from_secs(10),
            "it quit when asked: {took:?}"
        );
        launch(&install).unwrap();
        assert!(
            wait(60, || !running_pids(&install.exe).is_empty()).await,
            "Claude Desktop opens again"
        );
        assert_ne!(running_pids(&install.exe), before);
        quit_running(&install).await;
    }
}
