//! Claude Desktop installs its Code engine separately on first use. Seed the
//! gateway profile's cache from our mirror, using the manifest embedded in the
//! installed official app. Never change Claude's app files or gateway settings.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const ORIGIN: &str = "https://download.tuken.site/claude-code-runtime";
const OFFICIAL: &str = "https://downloads.claude.ai/claude-code-releases";
const MAX_SIZE: u64 = 512 * 1024 * 1024;
const MANIFEST_ERROR: &str =
    "无法识别这个 Claude Desktop 版本需要的运行组件，请更新兔啃后重试或联系客服";

#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    version: String,
    checksum: String,
    size: u64,
}

impl Spec {
    fn from_manifest(value: &Value) -> Option<Self> {
        if value["baseUrl"].as_str()? != OFFICIAL {
            return None;
        }
        let version = value["version"].as_str()?;
        let parts: Vec<_> = version.split('.').collect();
        if parts.len() != 3
            || parts
                .iter()
                .any(|p| p.is_empty() || p.len() > 8 || !p.bytes().all(|b| b.is_ascii_digit()))
        {
            return None;
        }
        let platform = &value["manifest"]["platforms"]["win32-x64"];
        let checksum = platform["checksum"].as_str()?;
        let size = platform["size"].as_u64()?;
        if platform["binary"].as_str()? != "claude.exe.zst"
            || !is_hash(checksum)
            || !(1..=MAX_SIZE).contains(&size)
        {
            return None;
        }
        Some(Self {
            version: version.into(),
            checksum: checksum.to_ascii_lowercase(),
            size,
        })
    }

    fn folder(&self, root: &Path) -> PathBuf {
        root.join(&self.version).join(&self.checksum[..12])
    }

    fn url(&self) -> String {
        format!(
            "{ORIGIN}/{}/{}/win32-x64/claude.exe.zst",
            self.version, self.checksum
        )
    }
}

fn is_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

// app.asar is a Chromium pickle header followed by file bytes. Read bounded
// metadata and main-process JS only; never execute JavaScript from the app.
fn read_spec(asar: &Path) -> Result<Spec, String> {
    let mut file = File::open(asar).map_err(|_| MANIFEST_ERROR)?;
    let length = file.metadata().map_err(|_| MANIFEST_ERROR)?.len();
    let mut prefix = [0u8; 16];
    file.read_exact(&mut prefix).map_err(|_| MANIFEST_ERROR)?;
    let header_size = u32::from_le_bytes(prefix[4..8].try_into().unwrap()) as u64;
    let json_size = u32::from_le_bytes(prefix[12..16].try_into().unwrap()) as u64;
    let start = 8 + header_size;
    if json_size > 16 * 1024 * 1024 || json_size + 8 > header_size || start > length {
        return Err(MANIFEST_ERROR.into());
    }
    let mut bytes = vec![0; json_size as usize];
    file.read_exact(&mut bytes).map_err(|_| MANIFEST_ERROR)?;
    let header: Value = serde_json::from_slice(&bytes).map_err(|_| MANIFEST_ERROR)?;
    let entries = header
        .pointer("/files/.vite/files/build/files")
        .and_then(Value::as_object)
        .ok_or(MANIFEST_ERROR)?;
    let mut scanned = 0u64;
    for (name, entry) in entries {
        if !name.ends_with(".js") || entry.get("unpacked").is_some() {
            continue;
        }
        let Some(size) = entry["size"].as_u64().filter(|n| *n <= 16 * 1024 * 1024) else {
            continue;
        };
        let Some(offset) = entry["offset"].as_str().and_then(|n| n.parse::<u64>().ok()) else {
            continue;
        };
        let Some(position) = start.checked_add(offset) else {
            continue;
        };
        if position.checked_add(size).is_none_or(|end| end > length) {
            return Err(MANIFEST_ERROR.into());
        }
        scanned += size;
        if scanned > 64 * 1024 * 1024 {
            return Err(MANIFEST_ERROR.into());
        }
        file.seek(SeekFrom::Start(position))
            .map_err(|_| MANIFEST_ERROR)?;
        let mut source = vec![0; size as usize];
        file.read_exact(&mut source).map_err(|_| MANIFEST_ERROR)?;
        if let Some(spec) = spec_from_js(&String::from_utf8_lossy(&source)) {
            return Ok(spec);
        }
    }
    Err(MANIFEST_ERROR.into())
}

fn spec_from_js(source: &str) -> Option<Spec> {
    for after in source.split("JSON.parse('").skip(1) {
        let Some(end) = after.find("')") else {
            continue;
        };
        let raw = &after[..end];
        if raw.len() > 1024 * 1024 || !raw.contains(OFFICIAL) {
            continue;
        }
        if let Ok(value) = serde_json::from_str(raw) {
            if let Some(spec) = Spec::from_manifest(&value) {
                return Some(spec);
            }
        }
    }
    None
}

struct CachedSpec {
    path: PathBuf,
    modified: SystemTime,
    length: u64,
    spec: Spec,
}
static SPEC: Mutex<Option<CachedSpec>> = Mutex::new(None);

pub fn installed_spec(exe: &Path) -> Result<Spec, String> {
    let asar = exe
        .parent()
        .ok_or(MANIFEST_ERROR)?
        .join("resources/app.asar");
    let metadata = fs::metadata(&asar).map_err(|_| MANIFEST_ERROR)?;
    let modified = metadata.modified().map_err(|_| MANIFEST_ERROR)?;
    let mut cached = SPEC.lock().unwrap();
    if let Some(old) = cached.as_ref() {
        if old.path == asar && old.modified == modified && old.length == metadata.len() {
            return Ok(old.spec.clone());
        }
    }
    let spec = read_spec(&asar)?;
    *cached = Some(CachedSpec {
        path: asar,
        modified,
        length: metadata.len(),
        spec: spec.clone(),
    });
    Ok(spec)
}

#[derive(Deserialize, Serialize)]
struct Payload {
    sha256: String,
    size: u64,
}

fn cached_payload(root: &Path, spec: &Spec) -> Option<Payload> {
    let folder = spec.folder(root);
    if fs::metadata(folder.join(".verified")).ok()?.len() > 128
        || fs::metadata(folder.join(".payload")).ok()?.len() > 4096
    {
        return None;
    }
    let marker = fs::read_to_string(folder.join(".verified")).ok()?;
    if marker.trim() != spec.checksum {
        return None;
    }
    let bytes = fs::read(folder.join(".payload")).ok()?;
    if bytes.len() > 4096 {
        return None;
    }
    let payload: Payload = serde_json::from_slice(&bytes).ok()?;
    let size = fs::metadata(folder.join("claude.exe")).ok()?.len();
    (is_hash(&payload.sha256) && payload.size == size && (2..=MAX_SIZE).contains(&size))
        .then_some(payload)
}

pub fn ready(root: &Path, spec: &Spec) -> bool {
    cached_payload(root, spec).is_some()
}

fn disk_error(error: impl std::fmt::Display) -> String {
    format!("无法保存 Claude Code 运行组件，请检查磁盘空间和文件权限后重试：{error}")
}

fn digest(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(disk_error)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(disk_error)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

async fn download(spec: &Spec, output: &mut File, progress: &impl Fn(Value)) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("Tuken/", env!("CARGO_PKG_VERSION")))
        .no_proxy()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(1800))
        .build()
        .map_err(|e| e.to_string())?;
    progress(json!({"step":"code_download","percent":0}));
    let mut response = client
        .get(spec.url())
        .send()
        .await
        .map_err(|_| "连接兔啃下载站失败，Claude Code 运行组件尚未装好，请重试".to_string())?;
    if response.status() != reqwest::StatusCode::OK || response.content_length() != Some(spec.size)
    {
        return Err(format!(
            "兔啃下载站暂未提供当前版本的完整运行组件（{}），请稍后点击修复或联系客服",
            spec.version
        ));
    }
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut previous = 0;
    loop {
        let chunk = tokio::time::timeout(Duration::from_secs(60), response.chunk())
            .await
            .map_err(|_| "运行组件下载超时，请检查网络后重试")?
            .map_err(|_| "运行组件下载中断，请重试")?;
        let Some(chunk) = chunk else { break };
        size += chunk.len() as u64;
        if size > spec.size {
            return Err("运行组件大小不正确，请重试".into());
        }
        output.write_all(&chunk).map_err(disk_error)?;
        hash.update(&chunk);
        let percent = size * 100 / spec.size;
        if percent != previous {
            previous = percent;
            progress(json!({"step":"code_download","percent":percent}));
        }
    }
    if size != spec.size || format!("{:x}", hash.finalize()) != spec.checksum {
        return Err("Claude Code 运行组件校验未通过，请重试；如果仍然失败，请联系客服".into());
    }
    output.sync_all().map_err(disk_error)
}

fn publish(archive: &Path, root: &Path, spec: &Spec) -> Result<(), String> {
    let folder = spec.folder(root);
    fs::create_dir_all(&folder).map_err(disk_error)?;
    let mut output = tempfile::NamedTempFile::new_in(&folder).map_err(disk_error)?;
    let file = File::open(archive).map_err(disk_error)?;
    let mut decoded = zstd::stream::read::Decoder::new(file).map_err(disk_error)?;
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = decoded.read(&mut buffer).map_err(disk_error)?;
        if count == 0 {
            break;
        }
        if size == 0 && !buffer[..count].starts_with(b"MZ") {
            return Err("Claude Code 运行组件格式不正确，请重试".into());
        }
        size += count as u64;
        if size > MAX_SIZE {
            return Err("Claude Code 运行组件解压后过大，请重试".into());
        }
        output.write_all(&buffer[..count]).map_err(disk_error)?;
        hash.update(&buffer[..count]);
    }
    if size < 2 {
        return Err("Claude Code 运行组件不完整，请重试".into());
    }
    output.as_file().sync_all().map_err(disk_error)?;
    let payload = Payload {
        sha256: format!("{:x}", hash.finalize()),
        size,
    };
    // Publish only a complete file. Keep other versions and running sessions.
    output
        .persist(folder.join("claude.exe"))
        .map_err(|_| "无法替换运行组件，请先退出 Claude Desktop，再点击修复重试")?;
    atomic_write(
        &folder.join(".payload"),
        &serde_json::to_vec(&payload).unwrap(),
    )?;
    // Claude's ready marker is deliberately last, after verified decompression.
    atomic_write(&folder.join(".verified"), spec.checksum.as_bytes())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap()).map_err(disk_error)?;
    file.write_all(bytes).map_err(disk_error)?;
    file.as_file().sync_all().map_err(disk_error)?;
    file.persist(path).map_err(disk_error)?;
    Ok(())
}

pub async fn ensure(
    root: PathBuf,
    spec: Spec,
    progress: impl Fn(Value) + Send + Sync + 'static,
) -> Result<(), String> {
    let progress = std::sync::Arc::new(progress);
    progress(json!({"step":"code_verify"}));
    let check_root = root.clone();
    let check_spec = spec.clone();
    let valid = tokio::task::spawn_blocking(move || {
        cached_payload(&check_root, &check_spec).is_some_and(|payload| {
            digest(&check_spec.folder(&check_root).join("claude.exe"))
                .is_ok_and(|hash| hash == payload.sha256)
        })
    })
    .await
    .map_err(|_| "检查 Claude Code 运行组件失败，请重试")?;
    if valid {
        return Ok(());
    }
    // A failed repair must not leave an old ready marker on damaged bytes.
    let marker = spec.folder(&root).join(".verified");
    match fs::remove_file(&marker) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(disk_error(e)),
    }
    fs::create_dir_all(&root).map_err(disk_error)?;
    let mut archive = tempfile::NamedTempFile::new_in(&root).map_err(disk_error)?;
    download(&spec, archive.as_file_mut(), &|step| progress(step)).await?;
    progress(json!({"step":"code_install"}));
    tokio::task::spawn_blocking(move || publish(archive.path(), &root, &spec))
        .await
        .map_err(|_| "安装 Claude Code 运行组件失败，请重试")?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Value {
        json!({"version":"2.1.308", "baseUrl":OFFICIAL, "manifest":{"platforms":{"win32-x64":{
            "binary":"claude.exe.zst", "checksum":"c17a819afb40d41ed558bacd5c432b52efdd004fe98068e6b47e77f540156bb9", "size":106384354
        }}}})
    }

    #[test]
    fn parses_pinned_manifest_and_refuses_paths_or_untrusted_hosts() {
        let m = manifest();
        let spec = spec_from_js(&format!("function x(){{return JSON.parse('{m}')}}")).unwrap();
        assert_eq!(spec.version, "2.1.308");
        assert!(spec.url().starts_with(ORIGIN));
        for bad in ["../escape", "2.1.308/../../bad", "", "2.1.-1"] {
            let mut m = manifest();
            m["version"] = json!(bad);
            assert!(Spec::from_manifest(&m).is_none());
        }
        let mut m = manifest();
        m["baseUrl"] = json!("https://other.invalid");
        assert!(Spec::from_manifest(&m).is_none());
        m = manifest();
        m["manifest"]["platforms"]["win32-x64"]["checksum"] = json!("../bad");
        assert!(Spec::from_manifest(&m).is_none());
    }

    #[test]
    fn publishes_complete_payload_and_markers_and_detects_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let spec = Spec::from_manifest(&manifest()).unwrap();
        let archive = dir.path().join("code.zst");
        fs::write(
            &archive,
            zstd::stream::encode_all(&b"MZtest-payload"[..], 1).unwrap(),
        )
        .unwrap();
        publish(&archive, dir.path(), &spec).unwrap();
        assert!(ready(dir.path(), &spec));
        let payload = cached_payload(dir.path(), &spec).unwrap();
        let exe = spec.folder(dir.path()).join("claude.exe");
        assert_eq!(digest(&exe).unwrap(), payload.sha256);
        fs::write(exe, b"MZcut").unwrap();
        assert!(!ready(dir.path(), &spec));
    }

    #[test]
    fn never_publishes_invalid_or_truncated_compressed_payload() {
        let dir = tempfile::tempdir().unwrap();
        let spec = Spec::from_manifest(&manifest()).unwrap();
        let archive = dir.path().join("bad.zst");
        for data in [
            b"invalid zstd".to_vec(),
            zstd::stream::encode_all(&b"not an executable"[..], 1).unwrap(),
        ] {
            fs::write(&archive, data).unwrap();
            assert!(publish(&archive, dir.path(), &spec).is_err());
            assert!(!spec.folder(dir.path()).join(".verified").exists());
        }
    }

    #[test]
    fn rejects_invalid_asar_offsets_without_allocating() {
        let dir = tempfile::tempdir().unwrap();
        let asar = dir.path().join("app.asar");
        fs::write(&asar, [255; 16]).unwrap();
        assert!(read_spec(&asar).is_err());
    }

    #[test]
    fn reads_the_manifest_from_asar_and_checks_file_bounds() {
        let dir = tempfile::tempdir().unwrap();
        let asar = dir.path().join("app.asar");
        let source = format!("function pin(){{return JSON.parse('{}')}}", manifest());
        let header = json!({"files":{".vite":{"files":{"build":{"files":{
            "index.chunk-test.js":{"offset":"0", "size":source.len()}
        }}}}}});
        let header = serde_json::to_vec(&header).unwrap();
        let padding = (4 - header.len() % 4) % 4;
        let header_size = (8 + header.len() + padding) as u32;
        let mut bytes = Vec::new();
        for n in [4u32, header_size, header_size - 4, header.len() as u32] {
            bytes.extend(n.to_le_bytes());
        }
        bytes.extend(header);
        bytes.extend(vec![0; padding]);
        bytes.extend(source.as_bytes());
        fs::write(&asar, &bytes).unwrap();
        assert_eq!(
            read_spec(&asar).unwrap(),
            Spec::from_manifest(&manifest()).unwrap()
        );
        bytes.pop();
        fs::write(&asar, bytes).unwrap();
        assert!(read_spec(&asar).is_err());
    }

    /// Downloads only to a disposable sandbox; never touches the user's cache.
    #[tokio::test]
    #[ignore]
    async fn windows_e2e_code_runtime() {
        let asar = std::env::var_os("TUKEN_TEST_ASAR").expect("set an official app.asar path");
        let spec = read_spec(Path::new(&asar)).unwrap();
        let root = tempfile::tempdir().unwrap();
        ensure(root.path().to_path_buf(), spec.clone(), |step| {
            println!("{step}")
        })
        .await
        .unwrap();
        let exe = spec.folder(root.path()).join("claude.exe");
        let output = std::process::Command::new(&exe)
            .arg("--version")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains(&spec.version));
        println!(
            "Claude Code starts: {}",
            String::from_utf8_lossy(&output.stdout).trim()
        );
        // Already complete: even without a download, re-check and keep it.
        ensure(root.path().to_path_buf(), spec, |step| {
            assert_ne!(step["step"], "code_download")
        })
        .await
        .unwrap();
    }
}
