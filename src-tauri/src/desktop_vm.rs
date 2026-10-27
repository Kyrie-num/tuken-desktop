//! Windows prerequisites for Claude's workspace, separate from the host Code
//! engine. Enabling VMP needs UAC and may need a restart; never force a reboot.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::{Deserialize, Serialize};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

const PENDING_KEY: &str = r"Software\Tuken\ClaudePrerequisites";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Status {
    pub state: String,
    pub message: String,
    pub can_prepare: bool,
    pub restart_required: bool,
    pub boot_id: String,
}

impl Status {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            state: "error".into(),
            message: message.into(),
            can_prepare: true,
            restart_required: false,
            boot_id: String::new(),
        }
    }
}

#[derive(Default, Deserialize)]
struct Probe {
    feature: u32,
    firmware: bool,
    hypervisor: bool,
    services: bool,
    virtual_machine: bool,
    boot_id: String,
    reboot: bool,
    error: Option<String>,
}

fn classify(probe: Probe, pending_boot: Option<&str>) -> Status {
    let pending =
        probe.reboot || (!probe.boot_id.is_empty() && pending_boot == Some(probe.boot_id.as_str()));
    let (state, message, can_prepare) = if let Some(error) = probe.error {
        (
            "error",
            if pending {
                format!(
                    "Windows 已提交部分系统更改，请先保存工作并重启电脑，兔啃会重新检查。{error}"
                )
            } else {
                format!(
                    "Windows 虚拟机平台尚未准备好，请重试；如果是单位电脑，请联系管理员。{error}"
                )
            },
            !pending,
        )
    } else if probe.feature == 0 {
        (
            "unsupported",
            "当前 Windows 没有提供虚拟机平台，请更新系统后重试。Claude Code 的本机模式仍可使用。"
                .into(),
            false,
        )
    } else if pending {
        ("restart_required", "虚拟机平台已提交准备，请保存工作后从 Windows 开始菜单选择「重启」。重启后兔啃会自动检查结果。".into(), false)
    } else if probe.feature != 1 {
        ("disabled", "Claude 的工作区需要 Windows 虚拟机平台。兔啃可以帮你开启，系统会申请管理员授权，完成后可能需要重启电脑。".into(), true)
    } else if probe.services && probe.hypervisor {
        ("ready", "Windows 虚拟机平台已准备好".into(), false)
    } else if !probe.firmware && !probe.hypervisor {
        let message = if probe.virtual_machine {
            "Windows 功能已开启，但虚拟机没有提供硬件虚拟化。请在宿主机开启嵌套虚拟化，再重启这台虚拟机。"
        } else {
            "Windows 功能已开启，但硬件虚拟化尚未可用。请在电脑 BIOS/UEFI 中开启 Intel VT-x 或 AMD SVM，再重启电脑。"
        };
        ("firmware_required", message.into(), false)
    } else {
        (
            "needs_repair",
            "虚拟机平台已开启，但相关服务尚未准备好。可以先点修复，再保存工作并重启 Windows。"
                .into(),
            true,
        )
    };
    Status {
        state: state.into(),
        message,
        can_prepare,
        restart_required: pending,
        boot_id: probe.boot_id,
    }
}

fn system_directory() -> Result<PathBuf, String> {
    let mut buffer = [0u16; 32768];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err("找不到 Windows 系统目录".into());
    }
    Ok(PathBuf::from(String::from_utf16_lossy(&buffer[..length])))
}

fn probe(prepare: bool) -> Result<Probe, String> {
    let system = system_directory()?;
    let powershell = system.join("WindowsPowerShell/v1.0/powershell.exe");
    let script = format!(
        "$prepare = ${}\n{}",
        if prepare { "true" } else { "false" },
        include_str!("desktop_vm.ps1")
    );
    let bytes: Vec<_> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let mut child = Command::new(&powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-EncodedCommand",
            &encoded,
        ])
        .env("TUKEN_SYSTEM_DIRECTORY", &system)
        .env(
            "PSModulePath",
            system.join("WindowsPowerShell/v1.0/Modules"),
        )
        .current_dir(&system)
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "无法运行 Windows 组件检查，请检查系统 PowerShell 是否可用")?;
    let start = Instant::now();
    let limit = Duration::from_secs(if prepare { 15 * 60 } else { 20 });
    while child.try_wait().map_err(|e| e.to_string())?.is_none() {
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(
                "Windows 组件检查或准备超时，请稍后重试；如系统正在更新，请等更新完成".into(),
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("Windows 组件检查失败，请检查系统 PowerShell 或联系电脑管理员".into());
    }
    serde_json::from_slice(&output.stdout).map_err(|_| "无法读取 Windows 虚拟机平台状态".into())
}

static CACHE: tokio::sync::Mutex<Option<(Instant, Status)>> = tokio::sync::Mutex::const_new(None);

fn pending_boot() -> Option<String> {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(PENDING_KEY)
        .ok()?
        .get_value("PendingBoot")
        .ok()
}

pub async fn status(force: bool) -> Status {
    let mut cache = CACHE.lock().await;
    if !force {
        if let Some((at, status)) = cache.as_ref() {
            if at.elapsed() < Duration::from_secs(30) {
                return status.clone();
            }
        }
    }
    let status = tokio::task::spawn_blocking(|| {
        probe(false)
            .map(|p| classify(p, pending_boot().as_deref()))
            .unwrap_or_else(Status::error)
    })
    .await
    .unwrap_or_else(|_| Status::error("检查虚拟机平台失败，请重试"));
    *cache = Some((Instant::now(), status.clone()));
    status
}

pub async fn remember(status: Status) {
    // The original account remembers the restart, even when UAC used a different
    // administrator. A new Windows boot invalidates this marker automatically.
    if status.restart_required && !status.boot_id.is_empty() {
        if let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(PENDING_KEY) {
            let _ = key.set_value("PendingBoot", &status.boot_id);
        }
    }
    *CACHE.lock().await = Some((Instant::now(), status));
}

fn mutation_allowed(debug: bool, e2e: bool, github: bool) -> bool {
    !debug || (e2e && github)
}

pub fn allow_prepare() -> Result<(), String> {
    if mutation_allowed(
        cfg!(debug_assertions),
        std::env::var("TUKEN_E2E").as_deref() == Ok("1"),
        std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true"),
    ) {
        Ok(())
    } else {
        Err("开发版不会修改本机 Windows 功能，请在隔离的 Windows 测试机验证".into())
    }
}

/// Only called by the narrowly scoped elevated helper. The script and Windows
/// feature name are compiled in; there are no caller-controlled shell commands.
pub fn prepare() -> Status {
    if let Err(why) = allow_prepare() {
        return Status::error(why);
    }
    probe(true)
        .map(|p| classify(p, None))
        .unwrap_or_else(Status::error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled() -> Probe {
        Probe {
            feature: 1,
            firmware: true,
            hypervisor: true,
            services: true,
            boot_id: "boot-a".into(),
            ..Default::default()
        }
    }

    #[test]
    fn pending_feature_is_not_reported_ready_until_a_new_boot() {
        assert_eq!(
            classify(enabled(), Some("boot-a")).state,
            "restart_required"
        );
        assert_eq!(classify(enabled(), Some("old-boot")).state, "ready");
        let mut p = enabled();
        p.reboot = true;
        assert!(classify(p, None).restart_required);
    }

    #[test]
    fn distinguishes_disabled_feature_hardware_and_service_failures() {
        let mut p = enabled();
        p.feature = 2;
        assert!(classify(p, None).can_prepare);
        let mut p = enabled();
        p.firmware = false;
        assert_eq!(classify(p, None).state, "ready"); // Hyper-V masks CPU flags.
        let mut p = enabled();
        p.firmware = false;
        p.services = false;
        assert_eq!(classify(p, None).state, "needs_repair");
        let mut p = enabled();
        p.firmware = false;
        p.services = false;
        p.hypervisor = false;
        assert!(classify(p, None).message.contains("BIOS"));
        let mut p = enabled();
        p.firmware = false;
        p.services = false;
        p.hypervisor = false;
        p.virtual_machine = true;
        assert!(classify(p, None).message.contains("嵌套"));
        let mut p = enabled();
        p.services = false;
        assert_eq!(classify(p, None).state, "needs_repair");
        assert_eq!(classify(Probe::default(), None).state, "unsupported");
    }

    #[test]
    fn partial_preparation_waits_for_restart_before_retrying() {
        let mut p = enabled();
        p.reboot = true;
        p.error = Some("Boot setting was not changed".into());
        let status = classify(p, None);
        assert!(status.restart_required);
        assert!(!status.can_prepare);
        assert!(status.message.contains("重启电脑"));
    }

    #[test]
    fn dev_builds_never_enable_features_on_the_developers_pc() {
        assert!(!mutation_allowed(true, false, false));
        assert!(!mutation_allowed(true, true, false));
        assert!(mutation_allowed(true, true, true));
        assert!(mutation_allowed(false, false, false));
    }

    #[tokio::test]
    #[ignore]
    async fn windows_vm_read_only_probe() {
        let status = status(true).await;
        println!("{}", serde_json::to_string(&status).unwrap());
        assert_ne!(status.state, "error");
    }
}
