//! Claude Desktop on macOS: find Claude.app, install it from Anthropic's own
//! disk image once its signature checks out, and its processes.
//!
//! What this relies on comes from Claude.app 3.6556.2 itself (its Info.plist,
//! code signature and app.asar): bundle id `com.anthropic.claudefordesktop`,
//! program `Contents/MacOS/Claude`, macOS 13.0 or later, signed "Developer ID
//! Application: Anthropic PBC (Q6L2SF6YDW)". The disk image is signed by the
//! same team.
//!
//! The pure parts (bundles, signatures, hdiutil's answer, download hosts)
//! build everywhere for the tests; the rest only on macOS.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Claude Desktop's bundle id (`CFBundleIdentifier`; its own code also gives
/// it as the app's id for computer use and for its updater's cache).
pub const BUNDLE_ID: &str = "com.anthropic.claudefordesktop";
/// Anthropic PBC's Apple developer team.
pub const TEAM_ID: &str = "Q6L2SF6YDW";
/// The macOS button on claude.ai/download (now claude.com/download). It
/// redirects to the current release, e.g.
/// downloads.claude.ai/releases/darwin/universal/3.6556.2/Claude-<hash>.dmg.
pub const DMG_URL: &str = "https://claude.ai/api/desktop/darwin/universal/dmg/latest/redirect";
/// Claude Desktop's own macOS update feed (Squirrel.Mac's RELEASES.json), on
/// Anthropic's download host rather than claude.ai, whose bot check turns
/// some networks away (GitHub's Macs, data-centre VPNs). Its update, e.g.
/// `.../3.6556.2/Claude-<hash>.zip`, sits next to the disk image of the same
/// name, `.../3.6556.2/Claude-<hash>.dmg`.
pub const FEED_URL: &str = "https://downloads.claude.ai/releases/darwin/universal/RELEASES.json";
/// Code signed by Anthropic: Apple's Developer ID chain, issued to its team.
pub const SIGNED_BY_ANTHROPIC: &str = concat!(
    "anchor apple generic",
    // A Developer ID intermediate and a Developer ID Application leaf.
    " and certificate 1[field.1.2.840.113635.100.6.2.6]",
    " and certificate leaf[field.1.2.840.113635.100.6.1.13]",
    " and certificate leaf[subject.OU] = \"Q6L2SF6YDW\"",
);

/// Where an install stands, for the page: `{step: "download", percent}`,
/// `{step: "verify"}`, `{step: "copy"}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "step", rename_all = "lowercase")]
pub enum Step {
    Download { percent: u8 },
    Verify,
    Copy,
}

/// What 兔啃 needs to know of an installed Claude.app.
#[derive(Debug, PartialEq, Eq)]
pub struct Bundle {
    pub version: String,
    /// The main program, whose processes are Claude Desktop.
    pub exe: PathBuf,
}

/// The bundle at `app` if its Info.plist says it is Claude Desktop.
pub fn bundle_of(app: &Path, info: &plist::Value) -> Option<Bundle> {
    let info = info.as_dictionary()?;
    let text = |key: &str| info.get(key).and_then(plist::Value::as_string);
    if text("CFBundleIdentifier")? != BUNDLE_ID {
        return None;
    }
    let program = text("CFBundleExecutable").filter(|p| !p.is_empty() && !p.contains('/'))?;
    let version = text("CFBundleShortVersionString")
        .or_else(|| text("CFBundleVersion"))
        .unwrap_or_default();
    Some(Bundle {
        version: version.to_string(),
        exe: app.join("Contents").join("MacOS").join(program),
    })
}

/// Reads Claude.app's Info.plist (XML or binary).
pub fn read_bundle(app: &Path) -> Option<Bundle> {
    let info = plist::Value::from_file(app.join("Contents").join("Info.plist")).ok()?;
    bundle_of(app, &info)
}

/// Who signed a piece of code, from `codesign -dv` (which writes to stderr).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Signer {
    pub identifier: String,
    pub team: String,
}

pub fn signer(codesign_dv: &str) -> Signer {
    let mut s = Signer::default();
    for line in codesign_dv.lines() {
        if let Some(v) = line.strip_prefix("Identifier=") {
            s.identifier = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("TeamIdentifier=") {
            s.team = v.trim().to_string();
        }
    }
    s
}

/// Where `hdiutil attach -plist` mounted the image.
pub fn mount_point(hdiutil_plist: &[u8]) -> Option<PathBuf> {
    // Anything hdiutil prints before its plist is no part of it.
    let start = hdiutil_plist.windows(5).position(|w| w == b"<?xml")?;
    let answer = plist::Value::from_reader(std::io::Cursor::new(&hdiutil_plist[start..])).ok()?;
    answer
        .as_dictionary()?
        .get("system-entities")?
        .as_array()?
        .iter()
        .filter_map(|e| e.as_dictionary()?.get("mount-point")?.as_string())
        .find(|p| !p.is_empty())
        .map(PathBuf::from)
}

/// The disk image only comes from Anthropic's own servers, over https.
pub fn from_anthropic(url: &url::Url) -> bool {
    let host = url.host_str().unwrap_or_default();
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && ["claude.ai", "claude.com", "anthropic.com"]
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

/// The disk image of the feed's current release.
pub fn dmg_from_feed(feed: &[u8]) -> Option<url::Url> {
    let feed: serde_json::Value = serde_json::from_slice(feed).ok()?;
    let current = feed["currentRelease"].as_str()?;
    let release = feed["releases"]
        .as_array()?
        .iter()
        .find(|r| r["version"].as_str() == Some(current))?;
    let zip = release["updateTo"]["url"].as_str()?;
    let dmg = url::Url::parse(&format!("{}.dmg", zip.strip_suffix(".zip")?)).ok()?;
    (from_anthropic(&dmg) && dmg.host_str() == Some("downloads.claude.ai")).then_some(dmg)
}

/// Whether a URL names a disk image.
pub fn is_dmg(url: &url::Url) -> bool {
    url.path().ends_with(".dmg")
}

/// A half-made copy that an interrupted install left next to Claude.app.
pub fn is_partial_copy(name: &str) -> bool {
    name.strip_prefix(".Claude.app.tuken-")
        .is_some_and(|rest| rest.len() == 32 && rest.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The disks (`/dev/diskN`) of images still attached from a 兔啃 download
/// folder, by `hdiutil info -plist`: an install that was cut short.
pub fn stale_images(hdiutil_info: &[u8]) -> Vec<String> {
    let Some(start) = hdiutil_info.windows(5).position(|w| w == b"<?xml") else {
        return Vec::new();
    };
    let Ok(info) = plist::Value::from_reader(std::io::Cursor::new(&hdiutil_info[start..])) else {
        return Vec::new();
    };
    let images = info
        .as_dictionary()
        .and_then(|d| d.get("images"))
        .and_then(plist::Value::as_array);
    images
        .into_iter()
        .flatten()
        .filter_map(plist::Value::as_dictionary)
        .filter(|image| {
            image
                .get("image-path")
                .and_then(plist::Value::as_string)
                .is_some_and(|p| p.contains("/tuken-claude-"))
        })
        .filter_map(|image| {
            image
                .get("system-entities")?
                .as_array()?
                .iter()
                .filter_map(|e| e.as_dictionary()?.get("dev-entry")?.as_string())
                .min_by_key(|d| d.len())
                .map(str::to_string)
        })
        .collect()
}

/// Whole percent of `total`, never more than 100.
pub fn percent(received: u64, total: u64) -> u8 {
    (received.min(total) * 100).checked_div(total).unwrap_or(0) as u8
}

#[cfg(target_os = "macos")]
pub use system::*;

#[cfg(target_os = "macos")]
mod system {
    use std::ffi::{OsStr, OsString};
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::process::{Output, Stdio};
    use std::time::{Duration, Instant};

    use tokio::io::AsyncWriteExt;

    use super::*;

    /// A disk image bigger than this isn't Claude Desktop (3.6556.2 is 377 MB).
    const MAX_DMG: u64 = 2 << 30;
    /// The download folder of an install, in the temp folder.
    const WORK_PREFIX: &str = "tuken-claude-";

    /// Why an install stopped.
    #[derive(Debug)]
    pub enum Failure {
        /// Something on this Mac the user can put right (no room, no
        /// permission, another app called Claude.app), said as it is.
        Local(String),
        /// The network, a signature, the disk image: the official download
        /// page takes over.
        Elsewhere(String),
    }

    impl Failure {
        fn elsewhere(why: impl Into<String>) -> Failure {
            Failure::Elsewhere(why.into())
        }
    }

    const NO_ROOM: &str = "磁盘空间不够：装 Claude Desktop 需要大约 2 GB。请清理一些空间后重试";

    /// What writing the download failed on, in plain words.
    fn write_failure(e: &std::io::Error) -> Failure {
        if e.kind() == std::io::ErrorKind::StorageFull {
            Failure::Local(NO_ROOM.into())
        } else {
            Failure::Local("没法保存 Claude Desktop 的安装包，请看看磁盘空间后重试".into())
        }
    }

    /// One clean-up or install at a time.
    static TIDY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// Where Claude.app goes: /Applications, or ~/Applications for someone
    /// who may not write there.
    pub fn applications() -> Vec<PathBuf> {
        let mut dirs = vec![PathBuf::from("/Applications")];
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join("Applications"));
        }
        dirs
    }

    /// The user a process runs as.
    fn uid_of(pid: libc::pid_t) -> Option<libc::uid_t> {
        // SAFETY: proc_pidinfo writes at most `size` bytes into `info`.
        unsafe {
            let mut info: libc::proc_bsdinfo = std::mem::zeroed();
            let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
            let n = libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            );
            (n == size).then_some(info.pbi_uid)
        }
    }

    /// This user's processes started from `exe`, matched by full path like
    /// on Windows.
    pub fn running_pids(exe: &Path) -> Vec<u32> {
        let wanted = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
        // SAFETY: getuid can't fail.
        let me = unsafe { libc::getuid() };
        let mut pids: Vec<libc::pid_t> = Vec::new();
        // SAFETY: the buffer is as long as the size passed for it.
        unsafe {
            let count = libc::proc_listallpids(std::ptr::null_mut(), 0);
            if count <= 0 {
                return Vec::new();
            }
            pids.resize(count as usize + 64, 0);
            let size = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
            let count = libc::proc_listallpids(pids.as_mut_ptr().cast(), size);
            if count <= 0 {
                return Vec::new();
            }
            pids.truncate(count as usize);
        }
        let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        pids.into_iter()
            .filter(|pid| *pid > 0)
            .filter(|pid| {
                // SAFETY: as above.
                let len = unsafe {
                    libc::proc_pidpath(*pid, path.as_mut_ptr().cast(), path.len() as u32)
                };
                len > 0 && Path::new(OsStr::from_bytes(&path[..len as usize])) == wanted
            })
            .filter(|pid| uid_of(*pid) == Some(me))
            .map(|pid| pid as u32)
            .collect()
    }

    /// SIGTERM is how Claude Desktop is asked to quit: Electron answers it on
    /// macOS by quitting the app as ⌘Q does (`Browser::Quit`). Unlike an
    /// AppleScript `quit`, it needs no "control Claude" permission, which an
    /// ad-hoc signed 兔啃 would have to ask for again after every update.
    pub fn signal(pids: &[u32], signal: libc::c_int) {
        for pid in pids {
            // SAFETY: kill only takes the numbers.
            unsafe {
                libc::kill(*pid as libc::pid_t, signal);
            }
        }
    }

    async fn output(
        mut command: tokio::process::Command,
        limit: Duration,
    ) -> Result<Output, String> {
        command.stdin(Stdio::null()).kill_on_drop(true);
        match tokio::time::timeout(limit, command.output()).await {
            Ok(Ok(out)) => Ok(out),
            Ok(Err(_)) => Err("无法运行系统工具".into()),
            Err(_) => Err("安装超时了".into()),
        }
    }

    fn tool(program: &str, args: &[&OsStr]) -> tokio::process::Command {
        let mut c = tokio::process::Command::new(program);
        c.args(args);
        c
    }

    /// Checks that `path` (the disk image, or Claude.app with all it holds)
    /// is signed by Anthropic and intact.
    pub(super) async fn verify(path: &Path, app: bool) -> Result<(), String> {
        let bad = || "安装包的签名不是 Anthropic 的，已停止安装".to_string();
        let requirement: OsString = if app {
            format!("-R=identifier \"{BUNDLE_ID}\" and {SIGNED_BY_ANTHROPIC}").into()
        } else {
            format!("-R={SIGNED_BY_ANTHROPIC}").into()
        };
        let mut args: Vec<&OsStr> = vec!["--verify".as_ref(), "--strict".as_ref()];
        if app {
            args.push("--deep".as_ref());
        }
        args.extend([requirement.as_os_str(), path.as_os_str()]);
        let out = output(tool("/usr/bin/codesign", &args), Duration::from_secs(600)).await?;
        if !out.status.success() {
            log::warn!(
                "codesign --verify: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            return Err(bad());
        }
        if app {
            // The same, read back: who signed it, and for which app.
            let out = output(
                tool("/usr/bin/codesign", &["-dv".as_ref(), path.as_os_str()]),
                Duration::from_secs(60),
            )
            .await?;
            let who = signer(&String::from_utf8_lossy(&out.stderr));
            if who.team != TEAM_ID || who.identifier != BUNDLE_ID {
                log::warn!("unexpected signer {who:?}");
                return Err(bad());
            }
            // And Gatekeeper's view: notarized by Apple for Anthropic.
            let out = output(
                tool(
                    "/usr/sbin/spctl",
                    &[
                        "--assess".as_ref(),
                        "--type".as_ref(),
                        "execute".as_ref(),
                        path.as_os_str(),
                    ],
                ),
                Duration::from_secs(300),
            )
            .await?;
            if !out.status.success() {
                log::warn!("spctl: {}", String::from_utf8_lossy(&out.stderr));
                return Err(bad());
            }
        }
        Ok(())
    }

    /// A mounted disk image, detached when dropped.
    struct Mount(PathBuf);

    impl Mount {
        async fn attach(dmg: &Path, under: &Path) -> Result<Mount, String> {
            let args: [&OsStr; 8] = [
                "attach".as_ref(),
                "-nobrowse".as_ref(),
                "-readonly".as_ref(),
                "-noautoopen".as_ref(),
                "-plist".as_ref(),
                "-mountrandom".as_ref(),
                under.as_os_str(),
                dmg.as_os_str(),
            ];
            let out = output(tool("/usr/bin/hdiutil", &args), Duration::from_secs(600)).await?;
            match mount_point(&out.stdout) {
                Some(path) if out.status.success() => Ok(Mount(path)),
                _ => {
                    log::warn!("hdiutil attach: {}", String::from_utf8_lossy(&out.stderr));
                    Err("无法打开安装包".into())
                }
            }
        }
    }

    fn detach(disk: &OsStr) -> bool {
        let detach = |force: bool| {
            let mut c = std::process::Command::new("/usr/bin/hdiutil");
            c.arg("detach").arg(disk).arg("-quiet");
            if force {
                c.arg("-force");
            }
            c.stdin(Stdio::null()).status().is_ok_and(|s| s.success())
        };
        detach(false) || detach(true)
    }

    impl Drop for Mount {
        fn drop(&mut self) {
            detach(self.0.as_os_str());
        }
    }

    /// What an interrupted install leaves behind: a half-copied app next to
    /// Claude.app, its disk image still attached, its download folder.
    fn clean_up() {
        for dir in applications() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    if is_partial_copy(&entry.file_name().to_string_lossy()) {
                        let _ = std::fs::remove_dir_all(entry.path());
                    }
                }
            }
        }
        let info = std::process::Command::new("/usr/bin/hdiutil")
            .args(["info", "-plist"])
            .stdin(Stdio::null())
            .output();
        if let Ok(info) = info {
            for disk in stale_images(&info.stdout) {
                detach(disk.as_ref());
            }
        }
        if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with(WORK_PREFIX) {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }
    }

    /// Clears what an install that was cut short left, when 兔啃 starts.
    pub async fn tidy() {
        let _one = TIDY.lock().await;
        let _ = tokio::task::spawn_blocking(clean_up).await;
    }

    /// The current disk image: where claude.ai sends the download button, or
    /// what the update feed names when claude.ai turns the request away or
    /// answers with anything but a disk image.
    async fn start(client: &reqwest::Client) -> Result<reqwest::Response, Failure> {
        let failed = || Failure::elsewhere("没能下载 Claude Desktop，请检查网络后重试");
        let ok = |r: &reqwest::Response| {
            r.status().is_success() && from_anthropic(r.url()) && is_dmg(r.url())
        };
        match client.get(DMG_URL).send().await {
            Ok(r) if ok(&r) => return Ok(r),
            Ok(r) => log::warn!("claude.ai answered {} from {}", r.status(), r.url()),
            Err(e) => log::warn!("claude.ai unreachable: {e}"),
        }
        let mut feed = client.get(FEED_URL).send().await.map_err(|_| failed())?;
        if !feed.status().is_success() || !from_anthropic(feed.url()) {
            return Err(failed());
        }
        let mut body = Vec::new();
        while let Some(chunk) = feed.chunk().await.map_err(|_| failed())? {
            body.extend_from_slice(&chunk);
            if body.len() > 64 * 1024 {
                return Err(failed());
            }
        }
        let url = dmg_from_feed(&body).ok_or_else(failed)?;
        match client.get(url).send().await {
            Ok(r) if ok(&r) => Ok(r),
            _ => Err(failed()),
        }
    }

    /// Downloads Anthropic's disk image to `to`, through the system proxy
    /// when one is set (reqwest's `system-proxy`, see Cargo.toml).
    async fn download(to: &Path, progress: &(impl Fn(Step) + Sync)) -> Result<(), Failure> {
        let failed = || Failure::elsewhere("没能下载 Claude Desktop，请检查网络后重试");
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .user_agent(concat!("Tuken-Desktop/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::custom(|next| {
                if next.previous().len() < 5 && from_anthropic(next.url()) {
                    next.follow()
                } else {
                    next.stop()
                }
            }))
            .build()
            .map_err(|_| failed())?;
        let mut r = start(&client).await?;
        let total = r.content_length().unwrap_or(0);
        if total > MAX_DMG {
            return Err(failed());
        }
        let mut file = tokio::fs::File::create(to)
            .await
            .map_err(|e| write_failure(&e))?;
        let started = Instant::now();
        let (mut received, mut shown) = (0u64, None);
        loop {
            // A stalled download is given up after a minute without data.
            let chunk = tokio::time::timeout(Duration::from_secs(60), r.chunk())
                .await
                .map_err(|_| failed())?
                .map_err(|_| failed())?;
            let Some(chunk) = chunk else { break };
            received += chunk.len() as u64;
            if received > MAX_DMG || started.elapsed() > Duration::from_secs(40 * 60) {
                return Err(failed());
            }
            file.write_all(&chunk)
                .await
                .map_err(|e| write_failure(&e))?;
            let now = percent(received, total);
            if shown != Some(now) {
                shown = Some(now);
                progress(Step::Download { percent: now });
            }
        }
        file.flush().await.map_err(|e| write_failure(&e))?;
        file.sync_all().await.map_err(|e| write_failure(&e))?;
        if total > 0 && received != total {
            return Err(failed());
        }
        Ok(())
    }

    /// Copies Claude.app into `dir` under a temporary name, checks the copy
    /// as closely as the original, then gives it its name. Another app of
    /// that name stays where it is.
    async fn copy_into(source: &Path, dir: &Path) -> Result<PathBuf, Failure> {
        let no_permission = || {
            Failure::Local(
                "没有权限把 Claude Desktop 放进「应用程序」，请用管理员账户登录后重试".into(),
            )
        };
        if tokio::fs::create_dir_all(dir).await.is_err() {
            return Err(no_permission());
        }
        let target = dir.join("Claude.app");
        if tokio::fs::symlink_metadata(&target).await.is_ok() {
            return Err(Failure::Local(
                "「应用程序」里已经有一个叫 Claude.app 的程序，但它不是 Claude Desktop。请先把它移走，再重试".into(),
            ));
        }
        let partial = dir.join(format!(
            ".Claude.app.tuken-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let copied = output(
            tool("/usr/bin/ditto", &[source.as_os_str(), partial.as_os_str()]),
            Duration::from_secs(900),
        )
        .await;
        let result = match copied {
            Ok(out) if out.status.success() => match verify(&partial, true).await {
                Ok(()) => tokio::fs::rename(&partial, &target)
                    .await
                    .map(|_| target)
                    .map_err(|_| no_permission()),
                Err(why) => Err(Failure::elsewhere(why)),
            },
            Ok(out) => {
                let said = String::from_utf8_lossy(&out.stderr);
                log::warn!("ditto: {said}");
                Err(if said.contains("No space left") {
                    Failure::Local(NO_ROOM.into())
                } else if said.contains("ermission denied") || said.contains("not permitted") {
                    no_permission()
                } else {
                    Failure::Local("没法把 Claude Desktop 装进「应用程序」，请重试".into())
                })
            }
            Err(why) => Err(Failure::Local(why)),
        };
        if result.is_err() {
            let _ = tokio::fs::remove_dir_all(&partial).await;
        }
        result
    }

    /// Downloads Claude Desktop, checks it is Anthropic's and intact, and
    /// puts Claude.app in /Applications (else ~/Applications).
    pub async fn install(progress: impl Fn(Step) + Sync) -> Result<PathBuf, Failure> {
        let _one = TIDY.lock().await;
        let _ = tokio::task::spawn_blocking(clean_up).await;
        let work = tempfile::Builder::new()
            .prefix(WORK_PREFIX)
            .tempdir()
            .map_err(|e| write_failure(&e))?;
        let dmg = work.path().join("Claude.dmg");
        progress(Step::Download { percent: 0 });
        download(&dmg, &progress).await?;
        progress(Step::Verify);
        verify(&dmg, false).await.map_err(Failure::elsewhere)?;
        // Dropped before `work`, so the image is detached before its folder goes.
        let mount = Mount::attach(&dmg, work.path())
            .await
            .map_err(Failure::elsewhere)?;
        let source = mount.0.join("Claude.app");
        read_bundle(&source).ok_or_else(|| Failure::elsewhere("安装包里没有 Claude Desktop"))?;
        verify(&source, true).await.map_err(Failure::elsewhere)?;
        progress(Step::Copy);
        let mut first = None;
        for dir in applications() {
            match copy_into(&source, &dir).await {
                Ok(app) => return Ok(app),
                // The other folder is on the same disk.
                Err(Failure::Local(why)) if why == NO_ROOM => return Err(Failure::Local(why)),
                Err(failure) => {
                    first.get_or_insert(failure);
                }
            }
        }
        Err(first.unwrap_or_else(|| Failure::elsewhere("没能装好 Claude Desktop")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(xml: &str) -> plist::Value {
        plist::Value::from_reader(std::io::Cursor::new(xml.as_bytes())).unwrap()
    }

    const CLAUDE_INFO: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDisplayName</key>
    <string>Claude</string>
    <key>CFBundleExecutable</key>
    <string>Claude</string>
    <key>CFBundleIdentifier</key>
    <string>com.anthropic.claudefordesktop</string>
    <key>CFBundleShortVersionString</key>
    <string>3.6556.2</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>ElectronTeamID</key>
    <string>Q6L2SF6YDW</string>
</dict>
</plist>"#;

    #[test]
    fn knows_claude_desktop_by_its_bundle_id() {
        let app = Path::new("/Applications/Claude.app");
        assert_eq!(
            bundle_of(app, &info(CLAUDE_INFO)),
            Some(Bundle {
                version: "3.6556.2".into(),
                exe: PathBuf::from("/Applications/Claude.app/Contents/MacOS/Claude"),
            })
        );
        // Another app called Claude.app, a program outside MacOS/, no plist.
        let other = CLAUDE_INFO.replace("com.anthropic.claudefordesktop", "com.example.claude");
        assert_eq!(bundle_of(app, &info(&other)), None);
        let escape = CLAUDE_INFO.replace(
            "<string>Claude</string>\n    <key>CFBundleIdentifier",
            "<string>../../evil</string>\n    <key>CFBundleIdentifier",
        );
        assert_eq!(bundle_of(app, &info(&escape)), None);
        assert_eq!(read_bundle(Path::new("/nowhere/Claude.app")), None);
    }

    #[test]
    fn reads_who_signed_the_app() {
        // `codesign -dv` of Claude.app 3.6556.2, shortened.
        let out = "Executable=/Volumes/Claude/Claude.app/Contents/MacOS/Claude\n\
                   Identifier=com.anthropic.claudefordesktop\n\
                   Format=app bundle with Mach-O universal (x86_64 arm64)\n\
                   CodeDirectory v=20500 size=761 flags=0x10000(runtime) hashes=13+7 location=embedded\n\
                   Signature size=9046\n\
                   Authority=Developer ID Application: Anthropic PBC (Q6L2SF6YDW)\n\
                   TeamIdentifier=Q6L2SF6YDW\n";
        assert_eq!(
            signer(out),
            Signer {
                identifier: BUNDLE_ID.into(),
                team: TEAM_ID.into()
            }
        );
        assert_eq!(signer("TeamIdentifier=not set\n").team, "not set");
        assert!(SIGNED_BY_ANTHROPIC.contains(&format!("\"{TEAM_ID}\"")));
    }

    #[test]
    fn finds_where_hdiutil_mounted_the_image() {
        let answer = br#"Checksumming whole disk...
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>system-entities</key>
	<array>
		<dict>
			<key>content-hint</key>
			<string>GUID_partition_scheme</string>
			<key>dev-entry</key>
			<string>/dev/disk4</string>
		</dict>
		<dict>
			<key>content-hint</key>
			<string>Apple_HFS</string>
			<key>dev-entry</key>
			<string>/dev/disk4s1</string>
			<key>mount-point</key>
			<string>/private/var/folders/x/T/tuken-claude-1/dmg.AbC123</string>
		</dict>
	</array>
</dict>
</plist>"#;
        assert_eq!(
            mount_point(answer),
            Some(PathBuf::from(
                "/private/var/folders/x/T/tuken-claude-1/dmg.AbC123"
            ))
        );
        assert_eq!(
            mount_point(b"hdiutil: attach failed - no mountable file systems"),
            None
        );
    }

    #[test]
    fn downloads_only_from_anthropic() {
        let ok = |s: &str| from_anthropic(&url::Url::parse(s).unwrap());
        assert!(ok(DMG_URL));
        assert!(ok(
            "https://downloads.claude.ai/releases/darwin/universal/3.6556.2/Claude-20c019a1.dmg"
        ));
        assert!(ok("https://claude.com/download"));
        assert!(!ok("http://downloads.claude.ai/Claude.dmg"));
        assert!(!ok("https://claude.ai.evil.example/Claude.dmg"));
        assert!(!ok("https://evilclaude.ai/Claude.dmg"));
        assert!(!ok("https://user:pw@downloads.claude.ai/Claude.dmg"));
        assert!(!ok("https://storage.example.com/Claude.dmg"));
    }

    #[test]
    fn falls_back_to_the_update_feed() {
        // downloads.claude.ai's RELEASES.json on 2026-10-25.
        let feed = br#"{"currentRelease":"3.6556.2","releases":[{"version":"3.6556.2","updateTo":{"name":"Claude 3.6556.2","version":"3.6556.2","pub_date":"2026-10-22T23:07:11.824Z","url":"https://downloads.claude.ai/releases/darwin/universal/3.6556.2/Claude-20c019a1e61fa5b687616af57d31f97bb3181723.zip","notes":""}}]}"#;
        assert_eq!(
            dmg_from_feed(feed).unwrap().as_str(),
            "https://downloads.claude.ai/releases/darwin/universal/3.6556.2/Claude-20c019a1e61fa5b687616af57d31f97bb3181723.dmg"
        );
        assert!(from_anthropic(&url::Url::parse(FEED_URL).unwrap()));
        // Only Anthropic's download host, only a zip next to its disk image.
        let elsewhere = String::from_utf8_lossy(feed).replace("downloads.claude.ai", "example.com");
        assert_eq!(dmg_from_feed(elsewhere.as_bytes()), None);
        let no_zip = String::from_utf8_lossy(feed).replace(".zip", ".tar.gz");
        assert_eq!(dmg_from_feed(no_zip.as_bytes()), None);
        assert_eq!(
            dmg_from_feed(br#"{"currentRelease":"9","releases":[]}"#),
            None
        );
        assert_eq!(dmg_from_feed(b"<html>Just a moment...</html>"), None);
    }

    #[test]
    fn takes_only_a_disk_image_from_claude_ai() {
        let url = |s: &str| url::Url::parse(s).unwrap();
        assert!(is_dmg(&url(
            "https://downloads.claude.ai/releases/darwin/universal/3.6556.2/Claude-20c019a1.dmg"
        )));
        assert!(!is_dmg(&url("https://claude.com/download")));
        assert!(!is_dmg(&url("https://claude.ai/login?next=Claude.dmg")));
    }

    #[test]
    fn knows_what_an_interrupted_install_left() {
        assert!(is_partial_copy(
            ".Claude.app.tuken-0123456789abcdef0123456789abcdef"
        ));
        assert!(!is_partial_copy("Claude.app"));
        assert!(!is_partial_copy(".Claude.app.tuken-../../etc"));
        assert!(!is_partial_copy(".Claude.app.tuken-0123"));

        // `hdiutil info -plist`, shortened: one image from a 兔啃 download
        // folder, one the user attached.
        let info = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>framework</key>
	<string>683</string>
	<key>images</key>
	<array>
		<dict>
			<key>image-path</key>
			<string>/private/var/folders/x/T/tuken-claude-Ab12/Claude.dmg</string>
			<key>system-entities</key>
			<array>
				<dict>
					<key>dev-entry</key>
					<string>/dev/disk4s1</string>
					<key>mount-point</key>
					<string>/private/var/folders/x/T/tuken-claude-Ab12/dmg.Xy</string>
				</dict>
				<dict>
					<key>dev-entry</key>
					<string>/dev/disk4</string>
				</dict>
			</array>
		</dict>
		<dict>
			<key>image-path</key>
			<string>/Users/xiaoming/Downloads/Other.dmg</string>
			<key>system-entities</key>
			<array>
				<dict>
					<key>dev-entry</key>
					<string>/dev/disk5</string>
				</dict>
			</array>
		</dict>
	</array>
</dict>
</plist>"#;
        assert_eq!(stale_images(info), vec!["/dev/disk4".to_string()]);
        assert!(stale_images(b"hdiutil: info failed").is_empty());
    }

    /// Code that isn't Anthropic's is turned away: Apple's own Calculator,
    /// and a disk image signed by nobody.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn only_anthropics_code_passes() {
        let calculator = Path::new("/System/Applications/Calculator.app");
        assert!(calculator.exists());
        assert!(system::verify(calculator, true).await.is_err());

        let dir = tempfile::tempdir().unwrap();
        let content = dir.path().join("content");
        std::fs::create_dir(&content).unwrap();
        std::fs::write(content.join("Claude.app"), "not an app").unwrap();
        let dmg = dir.path().join("Claude.dmg");
        let made = std::process::Command::new("/usr/bin/hdiutil")
            .args(["create", "-quiet", "-format", "UDZO", "-srcfolder"])
            .arg(&content)
            .arg(&dmg)
            .status()
            .unwrap();
        assert!(made.success());
        assert!(system::verify(&dmg, false).await.is_err());
    }

    #[test]
    fn progress_is_whole_percent() {
        assert_eq!(percent(0, 0), 0);
        assert_eq!(percent(50, 200), 25);
        assert_eq!(percent(377, 377), 100);
        assert_eq!(percent(500, 377), 100);
        assert_eq!(
            serde_json::to_value(Step::Download { percent: 42 }).unwrap(),
            serde_json::json!({ "step": "download", "percent": 42 })
        );
        assert_eq!(
            serde_json::to_value(Step::Verify).unwrap(),
            serde_json::json!({ "step": "verify" })
        );
        assert_eq!(
            serde_json::to_value(Step::Copy).unwrap(),
            serde_json::json!({ "step": "copy" })
        );
    }
}
