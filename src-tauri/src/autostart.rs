//! Starting 兔啃 when the user signs in to the computer, in the tray only.
//! Off until the user turns it on in the account menu. Windows keeps a `Run`
//! value in the user's registry, macOS a LaunchAgent in
//! ~/Library/LaunchAgents.

use std::path::Path;

use tauri::{AppHandle, Manager};

use crate::State;

/// What a start at sign-in passes, so the window stays hidden.
pub const ARG: &str = "--autostart";

/// Whether this run began at sign-in.
pub fn launched() -> bool {
    std::env::args_os().skip(1).any(|a| a == ARG)
}

/// Whether this run began at sign-in: the page then leaves the window hidden.
#[tauri::command]
pub fn autostarted() -> bool {
    launched()
}

/// Whether 兔啃 starts at sign-in; with `on`, switches it first.
#[tauri::command]
pub fn autostart(app: AppHandle, on: Option<bool>) -> Result<bool, String> {
    let exe = std::env::current_exe().map_err(|_| "找不到兔啃的程序文件".to_string())?;
    let name = entry(&app);
    if let Some(on) = on {
        system::set(&name, &exe, on).map_err(|e| {
            log::warn!("autostart {on}: {e}");
            "开机自动启动没有设置成功，请稍后重试".to_string()
        })?;
    }
    Ok(system::is_on(&name, &exe))
}

/// The entry's name: the product's on Windows (`Tuken`, or the dev build's
/// `Tuken Dev`), the app's identifier on macOS, where it labels the agent.
fn entry(app: &AppHandle) -> String {
    if cfg!(target_os = "macos") {
        app.state::<State>().app_id.clone()
    } else {
        app.package_info().name.clone()
    }
}

/// The `Run` command line, the program quoted: its path may hold spaces.
#[cfg(any(windows, test))]
fn command_line(exe: &Path) -> String {
    format!("\"{}\" {ARG}", exe.display())
}

/// Whether a `Run` command line starts `exe`, quoted or not, in any case.
/// The CC Switch–based client wrote its own under the same name.
#[cfg(any(windows, test))]
fn starts(command: &str, exe: &Path) -> bool {
    let command = command.trim();
    let program = match command.strip_prefix('"') {
        Some(rest) => rest.split('"').next(),
        None => command.split(" --").next(),
    };
    program.is_some_and(|p| p.eq_ignore_ascii_case(&exe.to_string_lossy()))
}

/// The LaunchAgent that starts `exe` at sign-in.
#[cfg(any(target_os = "macos", test))]
fn agent_plist(label: &str, exe: &Path) -> plist::Value {
    let mut agent = plist::Dictionary::new();
    agent.insert("Label".into(), label.into());
    agent.insert(
        "ProgramArguments".into(),
        plist::Value::Array(vec![exe.to_string_lossy().into_owned().into(), ARG.into()]),
    );
    agent.insert("RunAtLoad".into(), true.into());
    plist::Value::Dictionary(agent)
}

/// Whether a LaunchAgent starts `exe`, not a copy of 兔啃 elsewhere.
#[cfg(any(target_os = "macos", test))]
fn agent_runs(agent: &plist::Value, exe: &Path) -> bool {
    agent
        .as_dictionary()
        .and_then(|d| d.get("ProgramArguments"))
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|p| p.as_string())
        == Some(&*exe.to_string_lossy())
}

#[cfg(windows)]
mod system {
    use std::io;
    use std::path::Path;

    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
    use winreg::RegKey;

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// Where Task Manager's Startup tab (and Settings › Apps › Startup)
    /// switches an entry off, leaving its `Run` value as it was.
    const APPROVED: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

    pub fn is_on(name: &str, exe: &Path) -> bool {
        let user = RegKey::predef(HKEY_CURRENT_USER);
        let ours = user
            .open_subkey_with_flags(RUN, KEY_READ)
            .and_then(|k| k.get_value::<String, _>(name))
            .is_ok_and(|command| super::starts(&command, exe));
        // Its first byte is odd while switched off there.
        let off = user
            .open_subkey_with_flags(APPROVED, KEY_READ)
            .and_then(|k| k.get_raw_value(name))
            .is_ok_and(|v| v.bytes.first().is_some_and(|b| b & 1 == 1));
        ours && !off
    }

    pub fn set(name: &str, exe: &Path, on: bool) -> io::Result<()> {
        let user = RegKey::predef(HKEY_CURRENT_USER);
        let (run, _) = user.create_subkey(RUN)?;
        if on {
            run.set_value(name, &super::command_line(exe))?;
        } else if let Err(e) = run.delete_value(name) {
            if e.kind() != io::ErrorKind::NotFound {
                return Err(e);
            }
        }
        // Task Manager's own switch goes back to its default, on.
        if let Ok(approved) = user.open_subkey_with_flags(APPROVED, KEY_SET_VALUE) {
            let _ = approved.delete_value(name);
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod system {
    use std::io;
    use std::path::{Path, PathBuf};

    fn agent(label: &str) -> io::Result<PathBuf> {
        let home = dirs::home_dir().ok_or_else(|| io::Error::other("no home directory"))?;
        Ok(home
            .join("Library/LaunchAgents")
            .join(format!("{label}.plist")))
    }

    pub fn is_on(label: &str, exe: &Path) -> bool {
        agent(label)
            .ok()
            .and_then(|path| plist::Value::from_file(path).ok())
            .is_some_and(|agent| super::agent_runs(&agent, exe))
    }

    pub fn set(label: &str, exe: &Path, on: bool) -> io::Result<()> {
        let path = agent(label)?;
        if !on {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        super::agent_plist(label, exe)
            .to_file_xml(&path)
            .map_err(io::Error::other)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod system {
    use std::io;
    use std::path::Path;

    pub fn is_on(_: &str, _: &Path) -> bool {
        false
    }

    pub fn set(_: &str, _: &Path, _: bool) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn run_value_starts_this_program_quoted() {
        let exe = Path::new(r"C:\Users\Xiao Ming\AppData\Local\Tuken\tuken.exe");
        let line = super::command_line(exe);
        assert_eq!(
            line,
            r#""C:\Users\Xiao Ming\AppData\Local\Tuken\tuken.exe" --autostart"#
        );
        assert!(super::starts(&line, exe));
        // Windows paths ignore case, and an older value may be unquoted.
        assert!(super::starts(
            r"c:\users\xiao ming\appdata\local\tuken\TUKEN.EXE --autostart",
            exe
        ));
        // The CC Switch–based client under the same name is not this one.
        assert!(!super::starts(
            r#""C:\Users\Xiao Ming\AppData\Local\Tuken\cc-switch.exe""#,
            exe
        ));
        assert!(!super::starts("", exe));
    }

    #[test]
    fn launch_agent_starts_this_program_at_sign_in() {
        let exe = Path::new("/Applications/兔啃.app/Contents/MacOS/tuken");
        let mut xml = Vec::new();
        super::agent_plist("site.tuken.desktop", exe)
            .to_writer_xml(&mut xml)
            .unwrap();
        let agent = plist::Value::from_reader_xml(&xml[..]).unwrap();
        assert!(super::agent_runs(&agent, exe));
        assert!(!super::agent_runs(
            &agent,
            Path::new("/Users/xm/Downloads/兔啃.app/Contents/MacOS/tuken")
        ));
        let agent = agent.as_dictionary().unwrap();
        assert_eq!(
            agent.get("Label").and_then(|v| v.as_string()),
            Some("site.tuken.desktop")
        );
        assert_eq!(
            agent
                .get("ProgramArguments")
                .and_then(|v| v.as_array())
                .map(|a| a.len()),
            Some(2)
        );
        assert_eq!(
            agent.get("RunAtLoad").and_then(|v| v.as_boolean()),
            Some(true)
        );
    }
}
