//! Full, pinned Claude MSIX from 兔啃. The elevated helper only verifies and
//! deploys this package and prepares Windows' Virtual Machine Platform;
//! Windows verifies the package's publisher signature at deployment.
//! Progress uses a local pipe, never a writable elevated status file or a shell.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::windows::named_pipe::ServerOptions;
use windows::Foundation::Uri;
use windows::Management::Deployment::{
    DeploymentOptions, DeploymentProgress, DeploymentResult, PackageManager,
};
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};
use windows_future::AsyncOperationProgressHandler;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, SECURITY_IDENTIFICATION};
use windows_sys::Win32::System::Pipes::GetNamedPipeClientProcessId;
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, GetProcessId};
use windows_sys::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
    SHELLEXECUTEINFOW,
};

const MSIX_URL: &str = "https://download.tuken.site/claude-code-desktop/v3.6556.2/Claude-Code-Desktop-Windows-x64.msix";
const MSIX_SIZE: u64 = 284_230_330;
const MSIX_SHA256: &str = "c0ac245c9ad458abd550bcbf2a0b34c75c51173a27f3c870602e5452cf3484f1";
const HELPER: &str = "--tuken-install-claude-msix";
const VM_HELPER: &str = "--tuken-prepare-claude-vm";
const PIPE_PREFIX: &str = r"\\.\pipe\tuken-claude-";

fn disk_error(e: std::io::Error) -> String {
    match e.raw_os_error() {
        Some(112) => "磁盘空间不够，请清理至少 2 GB 空间后重试".into(),
        _ => format!("无法保存安装包，请检查磁盘空间和文件权限后重试：{e}"),
    }
}

fn check_digest(size: u64, hash: &str) -> Result<(), String> {
    if size != MSIX_SIZE || hash != MSIX_SHA256 {
        Err("安装包校验没有通过，请重新点击一键安装；如果仍然失败，请联系客服".into())
    } else {
        Ok(())
    }
}

async fn download(path: &Path, progress: &impl Fn(Value)) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .https_only(true)
        // A mirror failure must never send the user back to Anthropic's hosts.
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(30 * 60))
        .build()
        .map_err(|e| e.to_string())?;
    progress(json!({"step": "download", "percent": 0}));
    let mut response = client
        .get(MSIX_URL)
        .send()
        .await
        .map_err(|_| "连接兔啃下载站失败，请检查网络后重试".to_string())?;
    if response.status() != reqwest::StatusCode::OK || response.content_length() != Some(MSIX_SIZE)
    {
        return Err("兔啃下载站暂时没有返回完整安装包，请稍后重试或联系客服".into());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(disk_error)?;
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut previous = 0;
    loop {
        let chunk = tokio::time::timeout(Duration::from_secs(60), response.chunk())
            .await
            .map_err(|_| "下载中断超过一分钟，请检查网络后重试".to_string())?
            .map_err(|_| "下载中断，请检查网络后重试".to_string())?;
        let Some(chunk) = chunk else { break };
        size += chunk.len() as u64;
        if size > MSIX_SIZE {
            return Err("安装包大小不正确，请稍后重试".into());
        }
        file.write_all(&chunk).map_err(disk_error)?;
        hash.update(&chunk);
        let percent = (size * 100 / MSIX_SIZE) as u8;
        if percent != previous {
            previous = percent;
            progress(json!({"step": "download", "percent": percent}));
        }
    }
    file.sync_all().map_err(disk_error)?;
    progress(json!({"step": "verify"}));
    check_digest(size, &format!("{:x}", hash.finalize()))
}

struct Process(OwnedHandle);
impl Process {
    fn finished(&self) -> bool {
        let mut code = 259;
        unsafe { GetExitCodeProcess(self.0.as_raw_handle(), &mut code) == 0 || code != 259 }
    }
}

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(Some(0)).collect()
}

fn elevate(exe: &Path, package: Option<&Path>, pipe: &str) -> Result<Process, String> {
    // The only argument with user-controlled characters is a file path. Windows
    // file names cannot contain quotes; the path ends in .msix (no trailing slash).
    let parameters = match package {
        Some(package) => format!("{HELPER} \"{}\" {pipe}", package.display()),
        None => format!("{VM_HELPER} {pipe}"),
    };
    let file = wide(exe.as_os_str());
    let args = wide(parameters.as_ref());
    let verb = wide(std::ffi::OsStr::new("runas"));
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = args.as_ptr();
    info.nShow = 0; // no installer/console window; only Windows' UAC prompt
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        return Err(match unsafe { GetLastError() } {
            1223 => "你取消了 Windows 安装授权。点击一键安装重试，并在系统提示中选择「是」".into(),
            code => format!("无法启动安装，请重试并允许 Windows 管理员授权（错误 {code}）"),
        });
    }
    if info.hProcess.is_null() {
        return Err("无法读取安装进程，请重试".into());
    }
    Ok(Process(unsafe {
        OwnedHandle::from_raw_handle(info.hProcess)
    }))
}

pub async fn install(progress: impl Fn(Value)) -> Result<crate::desktop_vm::Status, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    install_with_exe(&exe, progress).await
}

async fn install_with_exe(
    exe: &Path,
    progress: impl Fn(Value),
) -> Result<crate::desktop_vm::Status, String> {
    let folder = tempfile::Builder::new()
        .prefix("tuken-claude-")
        .tempdir()
        .map_err(disk_error)?;
    let package = folder.path().join("Claude.msix");
    download(&package, &progress).await?;
    run_helper(exe, Some(&package), progress).await
}

pub async fn prepare_vm(progress: impl Fn(Value)) -> Result<crate::desktop_vm::Status, String> {
    crate::desktop_vm::allow_prepare()?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    run_helper(&exe, None, progress).await
}

async fn run_helper(
    exe: &Path,
    package: Option<&Path>,
    progress: impl Fn(Value),
) -> Result<crate::desktop_vm::Status, String> {
    let pipe = format!("{PIPE_PREFIX}{}", uuid::Uuid::new_v4());
    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(&pipe)
        .map_err(|e| format!("无法读取安装进度，请重试：{e}"))?;
    progress(json!({"step": "permission"}));
    let helper_exe = exe.to_path_buf();
    let helper_package = package.map(Path::to_path_buf);
    let helper_pipe = pipe.clone();
    let process = tokio::task::spawn_blocking(move || {
        elevate(&helper_exe, helper_package.as_deref(), &helper_pipe)
    })
    .await
    .map_err(|_| "无法启动安装，请重试".to_string())??;
    let pid = unsafe { GetProcessId(process.0.as_raw_handle()) };
    let connected = async {
        loop {
            tokio::select! {
                result = server.connect() => {
                    result.map_err(|e| format!("安装进度连接失败，请重试：{e}"))?;
                    let mut client = 0;
                    if unsafe { GetNamedPipeClientProcessId(server.as_raw_handle(), &mut client) } != 0 && client == pid {
                        return Ok::<(), String>(());
                    }
                    server.disconnect().map_err(|e| e.to_string())?;
                }
                _ = tokio::time::sleep(Duration::from_millis(250)) => {
                    if process.finished() { return Err("安装程序提前结束，请重新点击一键安装".into()); }
                }
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(60), connected)
        .await
        .map_err(|_| "安装程序没有响应，请重启兔啃后重试".to_string())??;
    let mut lines = BufReader::new(server).lines();
    // No invented installation percentage: these are PackageManager's actual
    // DeploymentProgress callbacks. Success arrives only after GetResults.
    while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
        let value: Value =
            serde_json::from_str(&line).map_err(|_| "无法读取安装进度，请重试".to_string())?;
        match value["step"].as_str() {
            Some("complete") => {
                return serde_json::from_value(value["vm"].clone())
                    .map_err(|_| "无法读取虚拟机平台准备结果，请重试".into())
            }
            Some("error") => {
                return Err(value["message"]
                    .as_str()
                    .unwrap_or("安装失败，请重试")
                    .to_string())
            }
            Some("install" | "verify" | "vm_prepare") => progress(value),
            _ => {}
        }
    }
    Err("安装程序提前结束，请重新点击一键安装".into())
}

fn deployment_error(code: u32) -> String {
    let why = match code {
        0x80070070 => "磁盘空间不够，请清理至少 2 GB 空间后重试",
        0x80073D02 => "Claude Desktop 正在使用中，请先保存工作并退出它，再重试",
        0x80073D06 => "电脑上已有更新版本，请重启兔啃以重新检测",
        0x80073D28 | 0x80070005 => "Windows 没有允许安装，请使用当前账户的管理员权限重试",
        0x80073CFF => "Windows 的应用安装策略阻止了安装，请联系电脑管理员允许安装已签名的应用",
        0x80073CFD => "Windows 版本不符合要求，请升级到 Windows 10 2004 或更新版本后重试",
        _ => "Windows 没有完成安装，请重启电脑后重试；仍然失败时请把错误码发给客服",
    };
    format!("{why}（0x{code:08X}）")
}

fn deploy(package: &Path, send: impl Fn(Value) + Send + Sync + 'static) -> Result<(), String> {
    // Keep a read-only lock until deployment ends, preventing replacement after
    // checksum verification. The elevated helper accepts this one pinned package.
    let mut locked = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(package)
        .map_err(disk_error)?;
    let size = locked.metadata().map_err(disk_error)?.len();
    if size != MSIX_SIZE {
        return check_digest(size, "");
    }
    send(json!({"step": "verify"}));
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = locked.read(&mut buffer).map_err(disk_error)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    check_digest(size, &format!("{:x}", hash.finalize()))?;
    let uri =
        url::Url::from_file_path(package).map_err(|_| "安装包路径无效，请重试".to_string())?;
    let operation = (|| -> windows::core::Result<_> {
        let manager = PackageManager::new()?;
        let uri = Uri::CreateUri(&windows::core::HSTRING::from(uri.as_str()))?;
        manager.AddPackageAsync(&uri, None, DeploymentOptions::None)
    })()
    .map_err(|e| deployment_error(e.code().0 as u32))?;
    let progress =
        AsyncOperationProgressHandler::<DeploymentResult, DeploymentProgress>::new(move |_, p| {
            send(json!({"step": "install", "percent": p.percentage.min(100)}));
            Ok(())
        });
    operation
        .SetProgress(&progress)
        .map_err(|e| deployment_error(e.code().0 as u32))?;
    let result = operation
        .get()
        .map_err(|e| deployment_error(e.code().0 as u32))?;
    let error = result
        .ExtendedErrorCode()
        .map_err(|e| deployment_error(e.code().0 as u32))?;
    if error.is_err() {
        return Err(deployment_error(error.0 as u32));
    }
    drop(locked);
    Ok(())
}

/// Called before Tauri's single-instance handling. Invalid helper requests exit
/// without opening an app or accepting caller-provided commands or packages.
pub fn helper_entry() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().collect();
    let installing = match args.get(1).and_then(|a| a.to_str()) {
        Some(HELPER) => true,
        Some(VM_HELPER) => false,
        _ => return None,
    };
    if args.len() != if installing { 4 } else { 3 } {
        return Some(1);
    }
    let pipe = args[if installing { 3 } else { 2 }]
        .to_str()
        .unwrap_or_default();
    if pipe
        .strip_prefix(PIPE_PREFIX)
        .is_none_or(|s| uuid::Uuid::parse_str(s).is_err())
    {
        return Some(1);
    }
    let Ok(file) = OpenOptions::new()
        .write(true)
        // Never let the unelevated pipe server impersonate this elevated helper.
        .security_qos_flags(SECURITY_IDENTIFICATION)
        .open(pipe)
    else {
        return Some(1);
    };
    let pipe = Arc::new(Mutex::new(file));
    let send = move |value: Value| {
        let _ = writeln!(pipe.lock().unwrap(), "{value}");
    };
    let hr = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    let result = if !installing {
        Ok(())
    } else if let Err(e) = &hr {
        Err(deployment_error(e.code().0 as u32))
    } else {
        deploy(Path::new(&args[2]), send.clone())
    };
    if hr.is_ok() {
        unsafe {
            RoUninitialize();
        }
    }
    match result {
        Ok(()) => {
            send(json!({"step": "vm_prepare"}));
            // Feature preparation failure doesn't erase a successful MSIX
            // install or prevent the host Code engine from being used.
            let vm = crate::desktop_vm::prepare();
            send(json!({"step": "complete", "vm": vm}));
            Some(0)
        }
        Err(message) => {
            send(json!({"step": "error", "message": message}));
            Some(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_wrong_or_incomplete_packages() {
        assert!(check_digest(MSIX_SIZE - 1, MSIX_SHA256).is_err());
        assert!(check_digest(MSIX_SIZE, &"0".repeat(64)).is_err());
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("fake.msix");
        std::fs::write(&path, b"not a package").unwrap();
        assert!(deploy(&path, |_| {}).unwrap_err().contains("校验"));
    }

    /// Only on a disposable Windows runner, never on the user's computer.
    #[tokio::test]
    #[ignore]
    async fn windows_e2e_mirror_install() {
        assert_eq!(std::env::var("TUKEN_E2E").as_deref(), Ok("1"));
        let exe = std::env::var_os("TUKEN_INSTALLER_EXE").expect("build tuken.exe first");
        let vm = install_with_exe(Path::new(&exe), |p| println!("{p}"))
            .await
            .unwrap();
        println!(
            "Windows virtualization preparation: {}",
            serde_json::to_string(&vm).unwrap()
        );
        assert_ne!(vm.state, "error", "{}", vm.message);
    }

    #[tokio::test]
    #[ignore]
    async fn windows_e2e_vm_helper() {
        assert_eq!(std::env::var("TUKEN_E2E").as_deref(), Ok("1"));
        assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
        let exe = std::env::var_os("TUKEN_INSTALLER_EXE").expect("build tuken.exe first");
        let vm = run_helper(Path::new(&exe), None, |p| println!("{p}"))
            .await
            .unwrap();
        println!("{}", serde_json::to_string(&vm).unwrap());
        assert_ne!(vm.state, "error", "{}", vm.message);
    }
}
