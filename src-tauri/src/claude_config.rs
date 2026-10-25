//! Points Claude Desktop (a third-party gateway profile) and Claude Code
//! (`~/.claude/settings.json` env) at 兔啃, and back.
//!
//! Edits keep every unrelated key, the key order, indentation, line endings
//! and BOM of the user's files. Every file is read and checked before any is
//! written, and the first edit of each file is backed up.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::api::{is_tuken_url, SITE};

/// 兔啃's own entry in Claude Desktop's profile library, next to any other
/// app's (CC Switch uses its own id), so neither overwrites the other.
pub const PROFILE_ID: &str = "8b0e4a52-6c1f-4d3b-9e27-1a5c0f7d2e40";
const PROFILE_NAME: &str = "兔啃";
const CONFIG: &str = "claude_desktop_config.json";
/// Profile fields that decide where Claude Desktop sends requests. They are
/// cleared before the new gateway is written, so nothing stale survives.
const GATEWAY_FIELDS: [&str; 6] = [
    "inferenceProvider",
    "inferenceGatewayBaseUrl",
    "inferenceGatewayApiKey",
    "inferenceGatewayAuthScheme",
    "inferenceModels",
    "modelDiscoveryEnabled",
];
/// What 兔啃 sets about models: the default effort, and that every new chat
/// starts on the default model (the first of `inferenceModels`).
const MODEL_FIELDS: [&str; 2] = ["defaultModelEffort", "alwaysStartWithDefaultModel"];
/// The reasoning efforts Claude Desktop takes, lowest first.
pub const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
/// Claude Desktop's model list holds at most this many.
const MAX_MODELS: usize = 200;
/// Claude Code settings that would route requests elsewhere or override our token.
const CLI_ROUTING_KEYS: [&str; 4] = [
    "apiKeyHelper",
    "awsAuthRefresh",
    "awsCredentialExport",
    "gcpAuthRefresh",
];

fn cli_routing_env(key: &str) -> bool {
    key.starts_with("ANTHROPIC_")
        || matches!(
            key,
            "CLAUDE_CODE_USE_BEDROCK"
                | "CLAUDE_CODE_USE_VERTEX"
                | "CLAUDE_CODE_USE_FOUNDRY"
                | "CLAUDE_CODE_OAUTH_TOKEN"
        )
}

pub struct Paths {
    normal_config: PathBuf,
    threep_config: PathBuf,
    library: PathBuf,
    cli_settings: PathBuf,
    backups: PathBuf,
    /// Why these files may not be changed from here, if they may not.
    read_only: Option<&'static str>,
}

/// A dev build changes Claude's files only in a sandbox: TUKEN_HOME (which
/// `tuken-dev.sh` sets) moves every path into it. Without it, they would be
/// the developer's own Claude Desktop and Claude Code.
fn dev_guard(debug: bool, tuken_home: bool) -> Option<&'static str> {
    (debug && !tuken_home).then_some(
        "开发版不改动 Claude Desktop 和 Claude Code 的配置。请用 tuken-dev.sh 运行，或把 TUKEN_HOME 设成一个沙盒文件夹",
    )
}

impl Paths {
    /// The real locations under %LOCALAPPDATA% and the home directory. A
    /// sandboxed dev run redirects both (LOCALAPPDATA and TUKEN_HOME).
    #[cfg(not(target_os = "macos"))]
    pub fn system(backups: PathBuf) -> Result<Paths, String> {
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or("找不到 %LOCALAPPDATA%")?;
        let home = std::env::var_os("TUKEN_HOME")
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
            .ok_or("找不到用户目录")?;
        let mut paths = Self::under(&local, &home, backups);
        paths.read_only = dev_guard(
            cfg!(debug_assertions),
            std::env::var_os("TUKEN_HOME").is_some(),
        );
        Ok(paths)
    }

    /// The real locations under the home directory; TUKEN_HOME redirects
    /// them all for a sandboxed dev run.
    #[cfg(target_os = "macos")]
    pub fn system(backups: PathBuf) -> Result<Paths, String> {
        let home = std::env::var_os("TUKEN_HOME")
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
            .ok_or("找不到用户目录")?;
        let mut paths = Self::mac(&home, backups);
        paths.read_only = dev_guard(
            cfg!(debug_assertions),
            std::env::var_os("TUKEN_HOME").is_some(),
        );
        Ok(paths)
    }

    /// Windows: Claude Desktop's folders under %LOCALAPPDATA%.
    #[cfg(any(not(target_os = "macos"), test))]
    pub fn under(local_app_data: &Path, home: &Path, backups: PathBuf) -> Paths {
        Self::new(
            &local_app_data.join("Claude"),
            &local_app_data.join("Claude-3p"),
            home,
            backups,
        )
    }

    /// macOS: Claude Desktop is an Electron app named "Claude", so its data
    /// folder is `app.getPath("userData")`, ~/Library/Application Support/Claude.
    /// For a third-party gateway it switches to that path plus "-3p". From
    /// Claude Desktop's own code (app.asar of 2.42304 on Windows and of
    /// 3.6556.2 for macOS, `.vite/build/index.pre.js`):
    ///
    /// ```js
    /// var KQ="-3p", qQ="Claude", ZQ=`${qQ}${KQ}`, QQ="claude_desktop_config.json";
    /// function $Q(){ if(process.env.CLAUDE_USER_DATA_DIR) return E.app.getPath("userData");
    ///   if(process.platform==="win32"&&process.env.LOCALAPPDATA) return h.join(process.env.LOCALAPPDATA,ZQ);
    ///   let e=E.app.getPath("userData"); return e.endsWith(KQ)?e:`${e}${KQ}` }
    /// function c$(){ return h.join($Q(),QQ) }              // read for "deploymentMode"
    /// function u$(){ return h.join($Q(),"configLibrary") } // <appliedId>.json, _meta.json
    /// ```
    ///
    /// and the plain folder is `[n.join(a.app.getPath("appData"),"Claude")]`
    /// off Windows. Managed settings come only from MDM profiles
    /// (`/Library/Managed Preferences/[<user>/]com.anthropic.claudefordesktop.plist`),
    /// not from `defaults`, so 兔啃 writes the same files as on Windows.
    #[cfg(any(target_os = "macos", test))]
    pub fn mac(home: &Path, backups: PathBuf) -> Paths {
        let support = home.join("Library").join("Application Support");
        Self::new(
            &support.join("Claude"),
            &support.join("Claude-3p"),
            home,
            backups,
        )
    }

    /// `claude`: Claude Desktop's own data folder; `threep`: the one it uses
    /// for a third-party gateway.
    fn new(claude: &Path, threep: &Path, home: &Path, backups: PathBuf) -> Paths {
        Paths {
            normal_config: claude.join(CONFIG),
            threep_config: threep.join(CONFIG),
            library: threep.join("configLibrary"),
            cli_settings: home.join(".claude").join("settings.json"),
            backups,
            read_only: None,
        }
    }

    /// Refuses before anything is written where this run may not write.
    fn writable(&self) -> Result<(), String> {
        self.read_only.map_or(Ok(()), |why| Err(why.into()))
    }

    fn profile(&self, id: &str) -> PathBuf {
        self.library.join(format!("{id}.json"))
    }

    fn meta(&self) -> PathBuf {
        self.library.join("_meta.json")
    }
}

#[derive(Clone)]
struct Style {
    bom: bool,
    crlf: bool,
    indent: String,
    newline_at_end: bool,
}

impl Default for Style {
    /// How Claude Desktop itself writes these files.
    fn default() -> Self {
        Style {
            bom: false,
            crlf: false,
            indent: "  ".into(),
            newline_at_end: false,
        }
    }
}

/// A JSON object file, read with its formatting so it can be written back alike.
struct Doc {
    path: PathBuf,
    root: Map<String, Value>,
    style: Style,
    before: Option<Vec<u8>>,
}

fn unreadable(path: &Path) -> String {
    format!(
        "{} 的内容不是有效的设置格式，为免弄坏它，兔啃没有做任何修改",
        path.display()
    )
}

fn detect_indent(text: &str) -> String {
    text.lines()
        .skip(1)
        .map(|l| &l[..l.len() - l.trim_start().len()])
        .find(|ws| !ws.is_empty())
        .unwrap_or("  ")
        .to_string()
}

impl Doc {
    fn read(path: &Path) -> Result<Doc, String> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Doc {
                    path: path.into(),
                    root: Map::new(),
                    style: Style::default(),
                    before: None,
                })
            }
            Err(_) => return Err(format!("无法读取 {}", path.display())),
        };
        let bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
        let text =
            std::str::from_utf8(&bytes[if bom { 3 } else { 0 }..]).map_err(|_| unreadable(path))?;
        let root = if text.trim().is_empty() {
            Map::new()
        } else {
            match serde_json::from_str::<Value>(text) {
                Ok(Value::Object(map)) => map,
                _ => return Err(unreadable(path)),
            }
        };
        let style = Style {
            bom,
            crlf: text.contains("\r\n"),
            indent: detect_indent(text),
            newline_at_end: text.ends_with('\n'),
        };
        Ok(Doc {
            path: path.into(),
            root,
            style,
            before: Some(bytes),
        })
    }

    fn render(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(self.style.indent.as_bytes());
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
        Value::Object(self.root.clone())
            .serialize(&mut ser)
            .expect("a JSON value always serializes");
        let mut text = String::from_utf8(buf).expect("serde_json writes UTF-8");
        if self.style.newline_at_end {
            text.push('\n');
        }
        if self.style.crlf {
            text = text.replace('\n', "\r\n");
        }
        let mut out = Vec::with_capacity(text.len() + 3);
        if self.style.bom {
            out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        out.extend_from_slice(text.as_bytes());
        out
    }

    /// Writes the file if its content changed, backing up the original first.
    fn write(&self, backups: &Path) -> Result<(), String> {
        let bytes = self.render();
        if self.before.as_deref() == Some(&bytes[..]) {
            return Ok(());
        }
        if let Some(before) = &self.before {
            backup_once(&self.path, before, backups)?;
        }
        write_atomic(&self.path, &bytes)
    }
}

/// Replaces `key` in place, or appends it.
fn set(map: &mut Map<String, Value>, key: &str, value: Value) {
    match map.get_mut(key) {
        Some(slot) => *slot = value,
        None => {
            map.insert(key.to_string(), value);
        }
    }
}

/// The first copy of each file we edit, so the user can always go back.
fn backup_once(path: &Path, original: &[u8], backups: &Path) -> Result<(), String> {
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    let tag: String = digest[..6].iter().map(|b| format!("{b:02x}")).collect();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let target = backups.join(format!("{name}.{tag}.bak"));
    if target.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(backups).map_err(|_| "无法创建备份目录")?;
    std::fs::write(&target, original).map_err(|_| format!("无法备份 {}", path.display()))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let dir = path.parent().ok_or("配置目录无效")?;
    std::fs::create_dir_all(dir).map_err(|_| format!("无法创建 {}", dir.display()))?;
    let failed = || format!("无法写入 {}，请关闭占用它的程序后重试", path.display());
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|_| failed())?;
    tmp.write_all(bytes).map_err(|_| failed())?;
    tmp.as_file().sync_all().map_err(|_| failed())?;
    tmp.persist(path).map_err(|_| failed())?;
    Ok(())
}

fn read_json(path: &Path) -> Option<Map<String, Value>> {
    let doc = Doc::read(path).ok()?;
    doc.before.is_some().then_some(doc.root)
}

/// Profile ids come from a file the user may edit; they only ever name a file
/// inside the library.
fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn entries_of<'a>(
    meta: &'a mut Map<String, Value>,
    path: &Path,
) -> Result<&'a mut Vec<Value>, String> {
    meta.entry("entries")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| unreadable(path))
}

fn is_ours(entry: &Value) -> bool {
    entry.get("id").and_then(Value::as_str) == Some(PROFILE_ID)
}

/// Makes Claude Desktop send its requests to 兔啃 with `key`. Takes effect the
/// next time Claude Desktop starts.
pub fn connect_desktop(p: &Paths, key: &str) -> Result<(), String> {
    p.writable()?;
    let mut normal = Doc::read(&p.normal_config)?;
    let mut threep = Doc::read(&p.threep_config)?;
    let mut profile = Doc::read(&p.profile(PROFILE_ID))?;
    let mut meta = Doc::read(&p.meta())?;

    for doc in [&mut normal, &mut threep] {
        set(&mut doc.root, "deploymentMode", json!("3p"));
    }
    // A new key starts from Claude Desktop's own model discovery until a
    // default model is chosen again.
    for field in GATEWAY_FIELDS.iter().chain(&MODEL_FIELDS) {
        profile.root.shift_remove(*field);
    }
    for (field, value) in [
        ("inferenceProvider", json!("gateway")),
        ("inferenceGatewayBaseUrl", json!(SITE)),
        ("inferenceGatewayApiKey", json!(key)),
        ("inferenceGatewayAuthScheme", json!("bearer")),
    ] {
        set(&mut profile.root, field, value);
    }
    // Defaults Claude Desktop needs for a gateway; a user's own choice stays.
    profile
        .root
        .entry("coworkEgressAllowedHosts")
        .or_insert_with(|| json!(["*"]));
    profile
        .root
        .entry("disableDeploymentModeChooser")
        .or_insert_with(|| json!(true));
    let meta_path = p.meta();
    let entries = entries_of(&mut meta.root, &meta_path)?;
    match entries.iter_mut().find(|e| is_ours(e)) {
        Some(Value::Object(entry)) => set(entry, "name", json!(PROFILE_NAME)),
        _ => entries.push(json!({ "id": PROFILE_ID, "name": PROFILE_NAME })),
    }
    set(&mut meta.root, "appliedId", json!(PROFILE_ID));

    for doc in [&normal, &threep, &profile, &meta] {
        doc.write(&p.backups)?;
    }
    Ok(())
}

/// Takes 兔啃 out of Claude Desktop. Another app's gateway profile, if there is
/// one, becomes the applied one; otherwise Claude Desktop returns to its
/// official sign-in.
pub fn disconnect_desktop(p: &Paths) -> Result<(), String> {
    p.writable()?;
    let mut meta = Doc::read(&p.meta())?;
    let mut profile = Doc::read(&p.profile(PROFILE_ID))?;
    let mut normal = Doc::read(&p.normal_config)?;
    let mut threep = Doc::read(&p.threep_config)?;

    let meta_path = p.meta();
    let entries = entries_of(&mut meta.root, &meta_path)?;
    entries.retain(|e| !is_ours(e));
    let next = entries
        .iter()
        .find_map(|e| e.get("id").and_then(Value::as_str))
        .map(str::to_string);
    if meta.root.get("appliedId").and_then(Value::as_str) == Some(PROFILE_ID) {
        match &next {
            Some(id) => set(&mut meta.root, "appliedId", json!(id)),
            None => {
                meta.root.shift_remove("appliedId");
            }
        }
    }
    for field in GATEWAY_FIELDS.iter().chain(&MODEL_FIELDS) {
        profile.root.shift_remove(*field);
    }
    if next.is_none() {
        for doc in [&mut normal, &mut threep] {
            if doc.before.is_some() {
                set(&mut doc.root, "deploymentMode", json!("1p"));
            }
        }
    }

    for doc in [&meta, &normal, &threep] {
        doc.write(&p.backups)?;
    }
    if profile.before.is_some() {
        profile.write(&p.backups)?;
    }
    Ok(())
}

/// The key Claude Desktop sends, when its applied profile points at 兔啃.
/// A model id Claude Desktop lists on a gateway: a Claude model, plainly named.
fn claude_model(id: &str) -> bool {
    let name = id.strip_prefix("anthropic/").unwrap_or(id);
    name.len() > 7
        && name.len() <= 100
        && name[..7].eq_ignore_ascii_case("claude-")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

/// The models to offer, cleaned up: Claude models only, each once, in order.
pub fn model_list(models: &[String]) -> Result<Vec<String>, String> {
    let mut list: Vec<String> = Vec::new();
    for m in models.iter().map(|m| m.trim()) {
        if !claude_model(m) {
            return Err(format!("Claude Desktop 用不了这个模型：{m}"));
        }
        if !list.iter().any(|x| x == m) {
            list.push(m.to_string());
        }
    }
    if list.is_empty() {
        return Err("请选一个默认模型".into());
    }
    list.truncate(MAX_MODELS);
    Ok(list)
}

/// Sets the models Claude Desktop offers on 兔啃 and how hard it thinks by
/// default. New chats always start on the first model. Only on 兔啃's own
/// profile while it is the applied one.
pub fn set_desktop_models(p: &Paths, models: &[String], effort: &str) -> Result<(), String> {
    p.writable()?;
    let applied = read_json(&p.meta())
        .and_then(|m| {
            m.get("appliedId")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .is_some_and(|id| id == PROFILE_ID);
    if !applied || desktop_key(p).is_none() {
        return Err("Claude Desktop 还没接入兔啃，请先接入".into());
    }
    if !EFFORTS.contains(&effort) {
        return Err("推理程度不对".into());
    }
    let models = model_list(models)?;
    let mut profile = Doc::read(&p.profile(PROFILE_ID))?;
    set(&mut profile.root, "inferenceModels", json!(models));
    set(&mut profile.root, "defaultModelEffort", json!(effort));
    set(
        &mut profile.root,
        "alwaysStartWithDefaultModel",
        json!(true),
    );
    profile.write(&p.backups)
}

/// The models and effort 兔啃 set in Claude Desktop (the default model first),
/// while it is connected; empty until they are chosen.
pub fn desktop_models(p: &Paths) -> Option<Value> {
    desktop_key(p)?;
    let profile = read_json(&p.profile(PROFILE_ID)).unwrap_or_default();
    let models: Vec<&str> = profile
        .get("inferenceModels")
        .and_then(Value::as_array)
        .map(|ms| {
            ms.iter()
                .filter_map(|m| m.as_str().or_else(|| m.get("name")?.as_str()))
                .collect()
        })
        .unwrap_or_default();
    let effort = profile
        .get("defaultModelEffort")
        .and_then(Value::as_str)
        .filter(|e| EFFORTS.contains(e));
    Some(json!({ "models": models, "effort": effort }))
}

pub fn desktop_key(p: &Paths) -> Option<String> {
    let third_party = [&p.threep_config, &p.normal_config]
        .iter()
        .filter_map(|path| read_json(path))
        .any(|c| c.get("deploymentMode").and_then(Value::as_str) == Some("3p"));
    if !third_party {
        return None;
    }
    let meta = read_json(&p.meta())?;
    let id = meta.get("appliedId")?.as_str().filter(|id| safe_id(id))?;
    let profile = read_json(&p.profile(id))?;
    let gateway = profile.get("inferenceProvider").and_then(Value::as_str) == Some("gateway");
    let ours = profile
        .get("inferenceGatewayBaseUrl")
        .and_then(Value::as_str)
        .is_some_and(is_tuken_url);
    (gateway && ours)
        .then(|| {
            profile
                .get("inferenceGatewayApiKey")?
                .as_str()
                .map(str::to_string)
        })
        .flatten()
}

/// Points Claude Code (CLI and IDE extensions) at 兔啃 with `key`. New
/// sessions pick it up; running ones keep their old settings.
pub fn connect_cli(p: &Paths, key: &str) -> Result<(), String> {
    p.writable()?;
    let mut settings = Doc::read(&p.cli_settings)?;
    for k in CLI_ROUTING_KEYS {
        settings.root.shift_remove(k);
    }
    let env = settings
        .root
        .entry("env")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| unreadable(&p.cli_settings))?;
    env.retain(|k, _| !cli_routing_env(k));
    set(env, "ANTHROPIC_BASE_URL", json!(SITE));
    set(env, "ANTHROPIC_AUTH_TOKEN", json!(key));
    settings.write(&p.backups)
}

/// Removes 兔啃 from Claude Code; settings that point elsewhere are left alone.
pub fn disconnect_cli(p: &Paths) -> Result<(), String> {
    p.writable()?;
    let mut settings = Doc::read(&p.cli_settings)?;
    let Some(env) = settings.root.get_mut("env").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    if !env
        .get("ANTHROPIC_BASE_URL")
        .and_then(Value::as_str)
        .is_some_and(is_tuken_url)
    {
        return Ok(());
    }
    env.shift_remove("ANTHROPIC_BASE_URL");
    env.shift_remove("ANTHROPIC_AUTH_TOKEN");
    if env.is_empty() {
        settings.root.shift_remove("env");
    }
    settings.write(&p.backups)
}

/// The key Claude Code sends, when it points at 兔啃.
pub fn cli_key(p: &Paths) -> Option<String> {
    let settings = read_json(&p.cli_settings)?;
    let env = settings.get("env")?.as_object()?;
    env.get("ANTHROPIC_BASE_URL")?
        .as_str()
        .filter(|u| is_tuken_url(u))?;
    env.get("ANTHROPIC_AUTH_TOKEN")?
        .as_str()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sandbox {
        _dir: tempfile::TempDir,
        local: PathBuf,
        home: PathBuf,
        paths: Paths,
    }

    fn sandbox() -> Sandbox {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("Local");
        let home = dir.path().join("home");
        let paths = Paths::under(&local, &home, dir.path().join("backups"));
        Sandbox {
            _dir: dir,
            local,
            home,
            paths,
        }
    }

    fn read(path: &Path) -> Value {
        let bytes = std::fs::read(path).unwrap();
        let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
        serde_json::from_slice(bytes).unwrap()
    }

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn macos_files_are_where_claude_desktop_reads_them() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("Users").join("xiaoming");
        let p = Paths::mac(&home, dir.path().join("backups"));
        let support = home.join("Library").join("Application Support");
        let library = support.join("Claude-3p").join("configLibrary");
        assert_eq!(p.normal_config, support.join("Claude").join(CONFIG));
        assert_eq!(p.threep_config, support.join("Claude-3p").join(CONFIG));
        assert_eq!(p.meta(), library.join("_meta.json"));
        assert_eq!(
            p.profile(PROFILE_ID),
            library.join(format!("{PROFILE_ID}.json"))
        );
        assert_eq!(p.cli_settings, home.join(".claude").join("settings.json"));

        connect_desktop(&p, "sk-mac").unwrap();
        assert_eq!(read(&p.threep_config)["deploymentMode"], "3p");
        assert_eq!(read(&p.meta())["appliedId"], PROFILE_ID);
        assert_eq!(desktop_key(&p).as_deref(), Some("sk-mac"));
    }

    #[test]
    fn dev_builds_write_only_in_their_sandbox() {
        assert!(dev_guard(true, false).is_some());
        assert_eq!(dev_guard(true, true), None);
        assert_eq!(dev_guard(false, false), None);

        let mut s = sandbox();
        s.paths.read_only = dev_guard(true, false);
        let models = ["claude-opus-4-1".to_string()];
        for refused in [
            connect_desktop(&s.paths, "sk-dev"),
            disconnect_desktop(&s.paths),
            set_desktop_models(&s.paths, &models, "max"),
            connect_cli(&s.paths, "sk-dev"),
            disconnect_cli(&s.paths),
        ] {
            assert!(refused.unwrap_err().contains("TUKEN_HOME"));
        }
        // Nothing was created anywhere.
        assert!(!s.local.exists() && !s.home.exists() && !s.paths.backups.exists());
    }

    /// Claude Desktop only applies a library file whose id matches
    /// `/^[a-f0-9-]{36}$/` (its `appliedId` check).
    #[test]
    fn our_profile_id_is_one_claude_desktop_applies() {
        assert_eq!(PROFILE_ID.len(), 36);
        assert!(PROFILE_ID
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-'));
    }

    #[test]
    fn connects_a_fresh_claude_desktop_and_disconnects_it() {
        let s = sandbox();
        connect_desktop(&s.paths, "sk-one").unwrap();
        let normal = read(&s.local.join("Claude").join(CONFIG));
        assert_eq!(normal["deploymentMode"], "3p");
        let profile = read(&s.paths.profile(PROFILE_ID));
        assert_eq!(profile["inferenceProvider"], "gateway");
        assert_eq!(profile["inferenceGatewayBaseUrl"], SITE);
        assert_eq!(profile["inferenceGatewayApiKey"], "sk-one");
        assert_eq!(profile["inferenceGatewayAuthScheme"], "bearer");
        let meta = read(&s.paths.meta());
        assert_eq!(meta["appliedId"], PROFILE_ID);
        assert_eq!(meta["entries"][0]["name"], PROFILE_NAME);
        assert_eq!(desktop_key(&s.paths).as_deref(), Some("sk-one"));

        disconnect_desktop(&s.paths).unwrap();
        assert_eq!(desktop_key(&s.paths), None);
        assert_eq!(read(&s.paths.threep_config)["deploymentMode"], "1p");
        let meta = read(&s.paths.meta());
        assert!(meta.get("appliedId").is_none());
        assert_eq!(meta["entries"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn sets_the_default_model_and_effort_of_claude_desktop() {
        let s = sandbox();
        let models = |list: &[&str]| list.iter().map(|m| m.to_string()).collect::<Vec<_>>();
        // Nothing to set before 兔啃 is connected.
        assert!(set_desktop_models(&s.paths, &models(&["claude-sonnet-4-5"]), "high").is_err());
        assert_eq!(desktop_models(&s.paths), None);

        connect_desktop(&s.paths, "sk-one").unwrap();
        assert_eq!(
            desktop_models(&s.paths),
            Some(json!({ "models": [], "effort": null }))
        );
        let chosen = models(&["claude-sonnet-4-5", "claude-opus-4-1", "claude-sonnet-4-5"]);
        set_desktop_models(&s.paths, &chosen, "xhigh").unwrap();
        let profile = read(&s.paths.profile(PROFILE_ID));
        assert_eq!(
            profile["inferenceModels"],
            json!(["claude-sonnet-4-5", "claude-opus-4-1"])
        );
        assert_eq!(profile["defaultModelEffort"], "xhigh");
        assert_eq!(profile["alwaysStartWithDefaultModel"], true);
        assert_eq!(profile["inferenceGatewayApiKey"], "sk-one");
        assert_eq!(
            desktop_models(&s.paths),
            Some(json!({ "models": ["claude-sonnet-4-5", "claude-opus-4-1"], "effort": "xhigh" }))
        );

        // Only Claude models and known efforts.
        assert!(set_desktop_models(&s.paths, &models(&["gpt-5"]), "high").is_err());
        assert!(set_desktop_models(&s.paths, &models(&["claude-x y"]), "high").is_err());
        assert!(set_desktop_models(&s.paths, &models(&["claude-sonnet-4-5"]), "ultra").is_err());
        assert!(set_desktop_models(&s.paths, &[], "high").is_err());

        // A new key starts over from Claude Desktop's own discovery.
        connect_desktop(&s.paths, "sk-two").unwrap();
        let profile = read(&s.paths.profile(PROFILE_ID));
        assert!(profile.get("inferenceModels").is_none());
        assert!(profile.get("defaultModelEffort").is_none());
        assert!(profile.get("alwaysStartWithDefaultModel").is_none());
    }

    #[test]
    fn keeps_other_profiles_keys_order_and_formatting() {
        let s = sandbox();
        // Another app's profile and user settings, CRLF, BOM and a 4-space indent.
        let normal = s.local.join("Claude").join(CONFIG);
        put(
            &normal,
            "\u{feff}{\r\n    \"mcpServers\": {\r\n        \"fs\": {\"command\": \"npx\"}\r\n    },\r\n    \"deploymentMode\": \"1p\",\r\n    \"zoom\": 1\r\n}\r\n",
        );
        put(
            &s.paths.meta(),
            r#"{"entries":[{"id":"00000000-0000-4000-8000-000000157210","name":"CC Switch"}],"appliedId":"00000000-0000-4000-8000-000000157210"}"#,
        );
        connect_desktop(&s.paths, "sk-two").unwrap();

        let text = String::from_utf8(std::fs::read(&normal).unwrap()).unwrap();
        assert!(text.starts_with('\u{feff}'));
        assert!(text.contains("\r\n    \"mcpServers\": {\r\n        \"fs\""));
        assert!(text.ends_with("}\r\n"));
        let keys: Vec<String> = serde_json::from_str::<Map<String, Value>>(&text[3..])
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(keys, ["mcpServers", "deploymentMode", "zoom"]);
        assert!(text.contains("\"deploymentMode\": \"3p\""));

        // CC Switch's entry survives, and comes back when 兔啃 is removed.
        disconnect_desktop(&s.paths).unwrap();
        let meta = read(&s.paths.meta());
        assert_eq!(meta["appliedId"], "00000000-0000-4000-8000-000000157210");
        assert_eq!(read(&normal)["deploymentMode"], "3p");
        // The first edit was backed up once, byte for byte.
        let backups: Vec<_> = std::fs::read_dir(&s.paths.backups).unwrap().collect();
        assert!(backups.iter().any(|b| b
            .as_ref()
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(CONFIG)));
    }

    #[test]
    fn a_broken_file_stops_every_write() {
        let s = sandbox();
        put(&s.paths.meta(), "{ this is not json");
        assert!(connect_desktop(&s.paths, "sk-x").is_err());
        assert!(!s.local.join("Claude").join(CONFIG).exists());
        assert!(!s.paths.profile(PROFILE_ID).exists());
        assert_eq!(
            std::fs::read_to_string(s.paths.meta()).unwrap(),
            "{ this is not json"
        );
    }

    #[test]
    fn only_a_tuken_gateway_counts_as_connected() {
        let s = sandbox();
        put(&s.paths.threep_config, r#"{"deploymentMode":"3p"}"#);
        put(&s.paths.meta(), r#"{"entries":[],"appliedId":"other"}"#);
        put(
            &s.paths.profile("other"),
            r#"{"inferenceProvider":"gateway","inferenceGatewayBaseUrl":"https://api.example","inferenceGatewayApiKey":"sk-else"}"#,
        );
        assert_eq!(desktop_key(&s.paths), None);
        // A profile id that tries to leave the library is ignored.
        put(&s.paths.meta(), r#"{"appliedId":"..\\..\\secrets"}"#);
        assert_eq!(desktop_key(&s.paths), None);
        // Official sign-in mode wins over a leftover gateway profile.
        connect_desktop(&s.paths, "sk-three").unwrap();
        put(&s.paths.threep_config, r#"{"deploymentMode":"1p"}"#);
        put(
            &s.local.join("Claude").join(CONFIG),
            r#"{"deploymentMode":"1p"}"#,
        );
        assert_eq!(desktop_key(&s.paths), None);
    }

    #[test]
    fn claude_code_keeps_user_settings() {
        let s = sandbox();
        let path = s.home.join(".claude").join("settings.json");
        put(
            &path,
            r#"{
  "apiKeyHelper": "get-key.sh",
  "env": {
    "ANTHROPIC_API_KEY": "sk-old",
    "ANTHROPIC_BASE_URL": "https://api.example",
    "DISABLE_TELEMETRY": "1"
  },
  "permissions": {"allow": ["Bash(ls)"]},
  "hooks": {}
}
"#,
        );
        connect_cli(&s.paths, "sk-cli").unwrap();
        let v = read(&path);
        assert!(v.get("apiKeyHelper").is_none());
        assert_eq!(v["env"]["DISABLE_TELEMETRY"], "1");
        assert!(v["env"].get("ANTHROPIC_API_KEY").is_none());
        assert_eq!(v["env"]["ANTHROPIC_BASE_URL"], SITE);
        assert_eq!(v["permissions"]["allow"][0], "Bash(ls)");
        assert_eq!(cli_key(&s.paths).as_deref(), Some("sk-cli"));

        disconnect_cli(&s.paths).unwrap();
        let v = read(&path);
        assert_eq!(cli_key(&s.paths), None);
        assert_eq!(v["env"]["DISABLE_TELEMETRY"], "1");
        assert!(v["env"].get("ANTHROPIC_AUTH_TOKEN").is_none());
    }

    #[test]
    fn disconnecting_leaves_other_gateways_alone() {
        let s = sandbox();
        let path = s.home.join(".claude").join("settings.json");
        let original = r#"{"env":{"ANTHROPIC_BASE_URL":"https://api.example","ANTHROPIC_AUTH_TOKEN":"sk-else"}}"#;
        put(&path, original);
        disconnect_cli(&s.paths).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }
}
