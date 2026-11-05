//! 手机遥控: a paired phone sends prompts through www.tuken.site's relay to
//! Claude Code (Claude Desktop's Code engine) running here, headless, in the
//! folder the user chose; what Claude does goes back to the phone.
//!
//! - While 手机遥控 is on, `listen` long-polls the relay's inbox.
//! - A prompt starts the engine if needed: stream-json in and out, permission
//!   prompts over stdio, the way Claude's own SDK runs it, with 兔啃's gateway
//!   key. Its output becomes short events for the phone and this window,
//!   posted in batches by `send_loop`.
//! - Claude runs with what a new conversation in Claude Desktop gets: its
//!   permission mode (bypass permissions when it has none: nothing asks),
//!   model and effort. The phone may choose others (set_permission_mode and
//!   set_model on the running engine; a new effort restarts it, resuming).
//! - Claude's text streams to the phone as it is written (partial messages).
//! - A permission prompt, in the modes that ask, waits for the phone's
//!   允许 / 拒绝 (10 minutes).
//! - The phone's 对话 menu lists Claude Desktop's conversations (see
//!   `conversations`) and the phone's own. One opened there shows what was
//!   said so far and goes on in the same conversation, in its own folder
//!   with its own settings, so Claude Desktop shows it all when it next
//!   opens it; unless Claude Desktop holds it open right now, then it goes on
//!   in a copy. A new conversation may go in any folder offered there.
//! - A phone conversation that has ended (新对话 or another one opened, the
//!   computer back in use, or 30 minutes quiet) goes into Claude Desktop's
//!   list through its own import link, `claude://resume?session=<id>`:
//!   Claude Desktop shows a terminal conversation only once imported. So it
//!   goes in once, when it is over, while Claude Desktop is open and the
//!   computer isn't being used or its user just came back (the import brings
//!   Claude Desktop to the front). Claude Desktop then holds it open, so
//!   talking on after that continues in a copy of it.
//!
//! The relay only sees pc_key. The phone's token is shown once as a QR code
//! and never kept here.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use reqwest::{Client, Method};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Notify;

use crate::api::SITE;
use crate::vault::{Remote, RemoteSync};
use crate::State;

/// How long a permission prompt waits for the phone.
const ASK_WAIT: Duration = Duration::from_secs(600);
/// Events kept for the window, and at most waiting to be sent.
const LOG_MAX: usize = 200;
const OUTBOX_MAX: usize = 500;
/// Text from Claude Code sent to the phone, per event.
const TEXT_MAX: usize = 20_000;
const DETAIL_MAX: usize = 1_500;
/// A phone conversation is over after this long without a turn, or when the
/// computer is back in use and the last turn was QUIET ago.
const CONVERSATION_IDLE: Duration = Duration::from_secs(30 * 60);
const QUIET: Duration = Duration::from_secs(120);
/// Claude Desktop jumps to an imported conversation: only after this long
/// without keyboard or mouse, unless asked to now.
const PC_IDLE: Duration = Duration::from_secs(90);
const SYNC_EVERY: Duration = Duration::from_secs(20);
const SYNC_TRIES: u32 = 3;
const UNSYNCED_MAX: usize = 20;
/// Permission modes the phone may choose, the one that never asks first.
const MODES: [&str; 3] = ["bypassPermissions", "acceptEdits", "default"];
/// Streamed text merged into one waiting event, at most.
const DELTA_MAX: usize = 4_000;

#[derive(Default)]
struct Runtime {
    /// Bumped whenever listening starts or stops, so old loops end.
    generation: u64,
    listening: bool,
    phone_online: bool,
    error: Option<String>,
    busy: bool,
    model: Option<String>,
    log: VecDeque<Value>,
    outbox: VecDeque<Value>,
    engine: Option<Arc<Engine>>,
    /// Permission prompts waiting for the phone: request id → tool input.
    pending: HashMap<String, Value>,
    /// The Claude Code conversation to resume if the engine has to restart.
    session: Option<String>,
    /// What the engine has spent so far: its results count from its start.
    spent: f64,
    /// Claude Desktop's models, the ones the phone may choose from.
    models: Vec<String>,
    /// The folder the engine (and so the conversation) works in.
    folder: Option<String>,
    /// The conversation's first prompt (or its title), and its last turn.
    title: Option<String>,
    last_turn: Option<Instant>,
    /// The conversation in `session` is over (queued for Claude Desktop).
    ended: bool,
    /// The conversation was opened from the phone's 对话 menu: its folder
    /// and its own settings in Claude Desktop.
    opened: Option<Opened>,
    /// Put conversations into Claude Desktop now, whatever the computer does.
    sync_now: bool,
    /// The pairing as last saved: the phone's status and new engines read
    /// it, so a setting changes without restarting the relay loops.
    cfg: Option<Remote>,
    /// What a new conversation in Claude Desktop starts with.
    defaults: Defaults,
    /// The mode and effort the running engine has.
    mode: Option<String>,
    effort: Option<String>,
    /// A new effort level: restart the engine once this turn is done.
    restart_after_turn: bool,
    /// The message and content block Claude is writing (streamed text).
    stream_msg: Option<String>,
    stream_block: i64,
}

/// Claude Desktop's own defaults for a new conversation.
#[derive(Clone, Default)]
struct Defaults {
    mode: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}

#[derive(Clone)]
struct Opened {
    folder: String,
    settings: Defaults,
}

static RT: LazyLock<Mutex<Runtime>> = LazyLock::new(Default::default);
static OUTBOX: LazyLock<Notify> = LazyLock::new(Notify::new);
static SYNC: LazyLock<Notify> = LazyLock::new(Notify::new);

struct Engine {
    stdin: tokio::sync::Mutex<ChildStdin>,
    child: Mutex<Option<Child>>,
    ids: AtomicU64,
}

impl Engine {
    async fn write(&self, line: &Value) -> Result<(), String> {
        let mut text = line.to_string();
        text.push('\n');
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(text.as_bytes())
            .await
            .map_err(|_| "Claude Code 已经退出".to_string())?;
        stdin
            .flush()
            .await
            .map_err(|_| "Claude Code 已经退出".to_string())
    }

    fn next_id(&self, kind: &str) -> String {
        format!("tuken-{kind}-{}", self.ids.fetch_add(1, Ordering::Relaxed))
    }

    fn kill(&self) {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.start_kill();
        }
    }
}

// ---------------------------------------------------------------- relay

enum RelayError {
    /// The pairing is gone: unpaired elsewhere, replaced, or lapsed.
    Unpaired,
    Other(String),
}

fn relay_client() -> &'static Client {
    static CLIENT: LazyLock<Client> = LazyLock::new(|| {
        Client::builder()
            .connect_timeout(Duration::from_secs(8))
            // The relay holds a poll for up to 20 seconds.
            .timeout(Duration::from_secs(40))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("Tuken-Desktop/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default()
    });
    &CLIENT
}

/// A dev build may talk to a stand-in relay (`TUKEN_REMOTE_RELAY`, e.g.
/// http://127.0.0.1:8790) instead of the site.
pub fn dev_relay() -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var("TUKEN_REMOTE_RELAY")
        .ok()
        .filter(|s| !s.is_empty())
}

fn relay_base() -> String {
    dev_relay().unwrap_or_else(|| SITE.to_string())
}

/// Pairs through the stand-in relay of a dev build.
pub async fn dev_pair(relay: &str) -> Result<Value, String> {
    let res = relay_client()
        .post(format!("{relay}/api/v1/desktop/remote"))
        .json(&json!({}))
        .send()
        .await
        .map_err(|_| "连不上测试用的转发服务".to_string())?;
    let body: Value = res
        .json()
        .await
        .map_err(|_| "测试用的转发服务回复不对".to_string())?;
    Ok(body["data"].clone())
}

async fn relay(
    method: Method,
    path: &str,
    pc_key: &str,
    body: Option<Value>,
) -> Result<Value, RelayError> {
    let mut r = relay_client()
        .request(
            method,
            format!("{}/api/v1/desktop/remote{path}", relay_base()),
        )
        .header("X-Tuken-Remote", pc_key);
    if let Some(b) = body {
        r = r.json(&b);
    }
    let res = r
        .send()
        .await
        .map_err(|_| RelayError::Other("连不上兔啃服务器，正在重试".into()))?;
    let status = res.status().as_u16();
    let data: Option<Value> = res.json().await.ok();
    if status == 401 {
        return Err(RelayError::Unpaired);
    }
    match data {
        Some(d) if status == 200 && d["code"] == 0 => Ok(d["data"].clone()),
        _ => Err(RelayError::Other(format!(
            "兔啃服务器暂时不可用（{status}），正在重试"
        ))),
    }
}

// ---------------------------------------------------------------- state for the UI

fn status_view(v: &crate::vault::Vault) -> Value {
    let cfg = v.remote.as_ref();
    let rt = RT.lock().unwrap();
    let folder = cfg
        .map(|c| c.folder.clone())
        .filter(|f| !f.is_empty())
        .or_else(|| v.remote_folder.clone());
    json!({
        "supported": cfg!(any(windows, target_os = "macos")),
        "paired": cfg.is_some(),
        "on": cfg.is_some_and(|c| c.on),
        "folder": folder,
        "listening": rt.listening,
        "phone_online": rt.phone_online,
        "busy": rt.busy,
        "error": rt.error,
        "settings": settings_view(cfg, &rt),
        "models": rt.models,
        "sync": !v.remote_sync_off,
        "unsynced": v.remote_unsynced.iter().map(|u| json!({ "title": u.title, "folder": folder_name(&u.folder), "ended": u.ended })).collect::<Vec<_>>(),
        "log": rt.log.iter().cloned().collect::<Vec<_>>(),
    })
}

/// What the window shows: pairing, switch, folder, and the latest events.
pub async fn status(state: &State) -> Result<Value, String> {
    let v = state.vault().await?;
    Ok(status_view(&v))
}

fn folder_name(folder: &str) -> String {
    Path::new(folder)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.to_string())
}

/// What the phone sees about this computer.
fn phone_status(fallback: &Remote) -> Value {
    let rt = RT.lock().unwrap();
    let cfg = rt.cfg.as_ref().unwrap_or(fallback);
    let mut status = settings_view(Some(cfg), &rt);
    let folder = rt
        .opened
        .as_ref()
        .map_or(cfg.folder.as_str(), |o| o.folder.as_str());
    status["folder"] = json!(folder_name(folder));
    status["models"] = json!(rt.models);
    status["efforts"] = json!(crate::claude_config::EFFORTS);
    status["busy"] = json!(rt.busy);
    status["session"] = json!(rt.session);
    status["title"] = json!(rt.title.as_deref().map(|t| clip(t, 60)));
    // The 对话 menu: older versions of 兔啃 don't answer it.
    status["conversations"] = json!(true);
    status
}

/// The mode, model and effort a phone conversation runs with: chosen on the
/// phone, else the opened conversation's own in Claude Desktop, else Claude
/// Desktop's defaults; bypass permissions when none says (the user wants
/// nothing to ask).
fn chosen(cfg: Option<&Remote>, rt: &Runtime) -> (String, Option<String>, Option<String>) {
    let own = rt.opened.as_ref().map(|o| &o.settings);
    let mode = [
        cfg.and_then(|c| c.mode.clone()),
        own.and_then(|o| o.mode.clone()),
        rt.defaults.mode.clone(),
    ]
    .into_iter()
    .flatten()
    .find(|m| MODES.contains(&m.as_str()))
    .unwrap_or_else(|| MODES[0].to_string());
    let model = cfg
        .and_then(|c| c.model.clone())
        .filter(|m| rt.models.contains(m))
        .or_else(|| {
            own.and_then(|o| o.model.clone())
                .filter(|m| rt.models.is_empty() || rt.models.contains(m))
        })
        .or_else(|| rt.defaults.model.clone());
    let effort = [
        cfg.and_then(|c| c.effort.clone()),
        own.and_then(|o| o.effort.clone()),
        rt.defaults.effort.clone(),
    ]
    .into_iter()
    .flatten()
    .find(|e| crate::claude_config::EFFORTS.contains(&e.as_str()));
    (mode, model, effort)
}

/// The settings as the phone and this window show them, and which of them
/// were chosen here rather than following Claude Desktop.
fn settings_view(cfg: Option<&Remote>, rt: &Runtime) -> Value {
    let (mode, model, effort) = chosen(cfg, rt);
    let custom: Vec<&str> = [
        ("mode", cfg.and_then(|c| c.mode.as_ref()).is_some()),
        ("model", cfg.and_then(|c| c.model.as_ref()).is_some()),
        ("effort", cfg.and_then(|c| c.effort.as_ref()).is_some()),
    ]
    .into_iter()
    .filter_map(|(k, set)| set.then_some(k))
    .collect();
    json!({ "mode": mode, "model": model, "effort": effort, "custom": custom })
}

/// One event for the phone and the window.
fn event(app: &AppHandle, e: Value) {
    {
        let mut rt = RT.lock().unwrap();
        rt.log.push_back(e.clone());
        while rt.log.len() > LOG_MAX {
            rt.log.pop_front();
        }
        if rt.listening {
            rt.outbox.push_back(e.clone());
            while rt.outbox.len() > OUTBOX_MAX {
                rt.outbox.pop_front();
            }
        }
    }
    OUTBOX.notify_one();
    let _ = app.emit("tuken://remote", json!({ "event": e }));
}

/// Streamed text, for the phone only (the window lists finished text):
/// merged into the delta before it while that waits to be sent.
fn delta(id: &str, text: &str) {
    {
        let mut rt = RT.lock().unwrap();
        if !rt.listening {
            return;
        }
        match rt.outbox.back_mut() {
            Some(last)
                if last["kind"] == "delta"
                    && last["id"] == id
                    && last["text"].as_str().map_or(0, str::len) < DELTA_MAX =>
            {
                let joined = format!("{}{text}", last["text"].as_str().unwrap_or(""));
                last["text"] = json!(joined);
            }
            _ => rt
                .outbox
                .push_back(json!({ "kind": "delta", "id": id, "text": text })),
        }
    }
    OUTBOX.notify_one();
}

fn note(app: &AppHandle, text: impl Into<String>) {
    event(app, json!({ "kind": "note", "text": text.into() }));
}

/// An event for the phone only (a list, a conversation's history).
fn to_phone(e: Value) {
    {
        let mut rt = RT.lock().unwrap();
        if !rt.listening {
            return;
        }
        rt.outbox.push_back(e);
        while rt.outbox.len() > OUTBOX_MAX {
            rt.outbox.pop_front();
        }
    }
    OUTBOX.notify_one();
}

/// A line for this window's 最近的操作 only.
fn log_only(app: &AppHandle, text: impl Into<String>) {
    let e = json!({ "kind": "note", "text": text.into() });
    {
        let mut rt = RT.lock().unwrap();
        rt.log.push_back(e.clone());
        while rt.log.len() > LOG_MAX {
            rt.log.pop_front();
        }
    }
    let _ = app.emit("tuken://remote", json!({ "event": e }));
}

fn changed(app: &AppHandle) {
    OUTBOX.notify_one();
    let _ = app.emit("tuken://remote", json!({}));
}

fn set_busy(app: &AppHandle, busy: bool) {
    RT.lock().unwrap().busy = busy;
    changed(app);
}

// ---------------------------------------------------------------- pairing and settings

/// Keeps the new pairing; returns the phone's link, once, for the QR code.
pub async fn paired(
    app: &AppHandle,
    state: &State,
    user_id: i64,
    data: &Value,
) -> Result<Value, String> {
    let (Some(pair_id), Some(pc_key), Some(token)) = (
        data["pair_id"].as_str(),
        data["pc_key"].as_str(),
        data["phone_token"].as_str(),
    ) else {
        return Err("兔啃服务器暂时不支持手机遥控，请稍后再试".into());
    };
    let mut v = state.vault().await?;
    let folder = v
        .remote
        .as_ref()
        .map(|r| r.folder.clone())
        .or_else(|| v.remote_folder.clone())
        .filter(|f| Path::new(f).is_dir())
        .unwrap_or_default();
    let accept_edits = v.remote.as_ref().is_some_and(|r| r.accept_edits);
    let model = v.remote.as_ref().and_then(|r| r.model.clone());
    let mode = v.remote.as_ref().and_then(|r| r.mode.clone());
    let effort = v.remote.as_ref().and_then(|r| r.effort.clone());
    v.remote = Some(Remote {
        user_id,
        pair_id: pair_id.into(),
        pc_key: pc_key.into(),
        folder,
        accept_edits,
        on: true,
        model,
        mode,
        effort,
    });
    state.save(&v)?;
    let cfg = v.remote.clone();
    drop(v);
    restart(app, cfg);
    let url = format!("{}/remote#t={token}", relay_base());
    let (size, path) = qr(&url)?;
    let mut view = status(state).await?;
    view["link"] = json!(url);
    view["qr"] = json!({ "size": size, "path": path });
    // Typed into the phone app instead of scanning; shown once, 10 minutes.
    view["code"] = json!(data["code"].as_str().filter(|c| c.len() == 8));
    Ok(view)
}

/// Ends the pairing, on the server too, so the phone can't get in any more.
pub async fn unpair(app: &AppHandle, state: &State) -> Result<Value, String> {
    // What was said on the phone still goes into Claude Desktop.
    end_conversation(app, true).await;
    let key = state
        .vault()
        .await?
        .remote
        .as_ref()
        .map(|r| r.pc_key.clone());
    if let Some(key) = key {
        match relay(Method::POST, "/end", &key, Some(json!({}))).await {
            Ok(_) | Err(RelayError::Unpaired) => {}
            Err(RelayError::Other(why)) => return Err(why.replace("，正在重试", "，请稍后再试")),
        }
    }
    let mut v = state.vault().await?;
    if let Some(r) = v.remote.take() {
        // The folder is offered again for the next pairing.
        v.remote_folder = Some(r.folder).filter(|f| !f.is_empty());
    }
    state.save(&v)?;
    drop(v);
    restart(app, None);
    status(state).await
}

/// Chooses the folder Claude works in, before or after pairing.
pub async fn choose_folder(app: &AppHandle, state: &State) -> Result<Value, String> {
    let Some(folder) = pick_folder(app, state).await? else {
        return status(state).await; // cancelled
    };
    let paired = state.vault().await?.remote.is_some();
    if paired {
        return set(app, state, &json!({ "folder": folder })).await;
    }
    let mut v = state.vault().await?;
    v.remote_folder = Some(folder);
    state.save(&v)?;
    drop(v);
    status(state).await
}

/// Switches 手机遥控 on or off, or changes its folder or edit approvals.
pub async fn set(app: &AppHandle, state: &State, input: &Value) -> Result<Value, String> {
    let mut v = state.vault().await?;
    let Some(cfg) = v.remote.as_mut() else {
        return Err("先在手机上扫码配对".into());
    };
    let mut restart_engine = false;
    let mut new_folder = false;
    if let Some(folder) = input["folder"].as_str() {
        if !Path::new(folder).is_dir() {
            return Err("这个文件夹不存在".into());
        }
        new_folder = cfg.folder != folder;
        restart_engine |= new_folder;
        cfg.folder = folder.into();
    }
    if let Some(model) = input["model"].as_str() {
        if !RT.lock().unwrap().models.iter().any(|m| m == model) {
            return Err("Claude Desktop 里没有这个模型".into());
        }
        cfg.model = Some(model.into());
    }
    if let Some(mode) = input["mode"].as_str() {
        if !MODES.contains(&mode) {
            return Err("没有这个权限模式".into());
        }
        cfg.mode = Some(mode.into());
    }
    if let Some(effort) = input["effort"].as_str() {
        if !crate::claude_config::EFFORTS.contains(&effort) {
            return Err("没有这个推理程度".into());
        }
        cfg.effort = Some(effort.into());
    }
    if input["follow"] == true {
        cfg.mode = None;
        cfg.model = None;
        cfg.effort = None;
    }
    let toggled = input["on"].as_bool().is_some_and(|on| on != cfg.on);
    if let Some(on) = input["on"].as_bool() {
        cfg.on = on;
    }
    let cfg = cfg.clone();
    if let Some(sync) = input["sync"].as_bool() {
        v.remote_sync_off = !sync;
    }
    state.save(&v)?;
    drop(v);
    let before = {
        let mut rt = RT.lock().unwrap();
        let before = chosen(rt.cfg.as_ref(), &rt);
        rt.cfg = Some(cfg.clone());
        before
    };
    apply_settings(app, before).await;
    if new_folder {
        // Another project: the conversation so far is over, the next starts afresh.
        end_conversation(app, true).await;
        {
            let mut rt = RT.lock().unwrap();
            rt.session = None;
            rt.ended = false;
            rt.title = None;
            rt.opened = None;
        }
        fresh_on_phone(&cfg.folder);
    }
    if restart_engine {
        // The next prompt starts Claude Code again, in the new folder.
        stop_engine(app, true);
        note(
            app,
            format!("之后的指令在「{}」里执行", folder_name(&cfg.folder)),
        );
    }
    // Only switching it on or off restarts the relay loops: a restart drops
    // what waits to be sent, these notes included.
    if toggled {
        restart(app, Some(cfg));
    } else {
        changed(app);
    }
    status(state).await
}

/// Starts or stops the listening loop to match `cfg`.
fn restart(app: &AppHandle, cfg: Option<Remote>) {
    let generation = {
        let mut rt = RT.lock().unwrap();
        rt.generation += 1;
        rt.listening = false;
        rt.phone_online = false;
        rt.error = None;
        rt.outbox.clear();
        rt.cfg = cfg.clone();
        rt.generation
    };
    match cfg {
        Some(cfg) if cfg.on => {
            RT.lock().unwrap().listening = true;
            // The phone's settings and model list, from the start.
            if let Ok(paths) = crate::config_paths(app) {
                refresh_desktop(&paths);
            }
            let a = app.clone();
            let c = cfg.clone();
            tauri::async_runtime::spawn(async move { listen(a, c, generation).await });
            tauri::async_runtime::spawn(async move { send_loop(cfg, generation).await });
        }
        _ => stop_engine(app, true),
    }
    changed(app);
}

fn current(generation: u64) -> bool {
    RT.lock().unwrap().generation == generation
}

/// Starts listening once the vault is open (at launch, it opens with the window).
pub fn start_worker(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let opened = {
                let state = app.state::<State>();
                let v = state.vault_if_open().await;
                let cfg = v.as_ref().map(|v| v.remote.clone());
                drop(v);
                cfg
            };
            if let Some(cfg) = opened {
                restart(&app, cfg);
                let a = app.clone();
                tauri::async_runtime::spawn(async move { sync_loop(a).await });
                return;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

/// Ends Claude Code when 兔啃 quits.
pub fn shutdown() {
    let engine = RT.lock().unwrap().engine.take();
    if let Some(e) = engine {
        e.kill();
    }
}

// ---------------------------------------------------------------- loops

async fn listen(app: AppHandle, cfg: Remote, generation: u64) {
    let mut after = String::new();
    let mut wait = 1u64;
    while current(generation) {
        let path = format!(
            "/inbox?after={}",
            url::form_urlencoded::byte_serialize(after.as_bytes()).collect::<String>()
        );
        match relay(Method::GET, &path, &cfg.pc_key, None).await {
            Ok(data) => {
                if !current(generation) {
                    return;
                }
                wait = 1;
                {
                    let mut rt = RT.lock().unwrap();
                    let online = data["phone_online"] == true;
                    let was = rt.phone_online;
                    rt.phone_online = online;
                    let had_error = rt.error.take().is_some();
                    drop(rt);
                    if was != online || had_error {
                        changed(&app);
                    }
                }
                if let Some(last) = data["last"].as_str() {
                    after = last.to_string();
                }
                for item in data["items"].as_array().into_iter().flatten() {
                    handle(&app, &cfg, &item["data"]).await;
                }
            }
            Err(RelayError::Unpaired) => {
                // Unpaired from elsewhere: forget it here too.
                let state = app.state::<State>();
                if let Ok(mut v) = state.vault().await {
                    if v.remote.as_ref().is_some_and(|r| r.pc_key == cfg.pc_key) {
                        v.remote_folder = v.remote.take().map(|r| r.folder);
                        let _ = state.save(&v);
                    }
                }
                restart(&app, None);
                note(&app, "手机遥控的配对已失效，需要重新扫码");
                return;
            }
            Err(RelayError::Other(why)) => {
                RT.lock().unwrap().error = Some(why);
                changed(&app);
                tokio::time::sleep(Duration::from_secs(wait)).await;
                wait = (wait * 2).min(30);
            }
        }
    }
}

async fn send_loop(cfg: Remote, generation: u64) {
    let mut last_status = Value::Null;
    let mut wait = 1u64;
    while current(generation) {
        // Wake on new events, and every few seconds to keep the status fresh.
        let _ = tokio::time::timeout(Duration::from_secs(15), OUTBOX.notified()).await;
        if !current(generation) {
            return;
        }
        // Events that arrive together go together.
        tokio::time::sleep(Duration::from_millis(150)).await;
        let batch: Vec<Value> = {
            let mut rt = RT.lock().unwrap();
            let n = rt.outbox.len().min(50);
            rt.outbox.drain(..n).collect()
        };
        let status = phone_status(&cfg);
        if batch.is_empty() && status == last_status {
            continue;
        }
        let body = json!({ "events": batch, "status": status });
        match relay(Method::POST, "/events", &cfg.pc_key, Some(body)).await {
            Ok(_) => {
                last_status = status;
                wait = 1;
                if !RT.lock().unwrap().outbox.is_empty() {
                    OUTBOX.notify_one();
                }
            }
            Err(e) => {
                // Put them back in front; the listen loop handles a lost pairing.
                {
                    let mut rt = RT.lock().unwrap();
                    for e in batch.into_iter().rev() {
                        rt.outbox.push_front(e);
                    }
                }
                if let RelayError::Unpaired = e {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(wait)).await;
                wait = (wait * 2).min(30);
                OUTBOX.notify_one();
            }
        }
    }
}

async fn handle(app: &AppHandle, cfg: &Remote, msg: &Value) {
    match msg["type"].as_str() {
        Some("prompt") => {
            let text = msg["text"].as_str().unwrap_or("").trim();
            if !text.is_empty() {
                prompt(app, cfg, text).await;
            }
        }
        Some(kind @ ("allow" | "deny")) => {
            if let Some(id) = msg["request_id"].as_str() {
                answer(app, id, kind == "allow").await;
            }
        }
        Some("stop") => {
            let engine = RT.lock().unwrap().engine.clone();
            if let Some(e) = engine {
                let id = e.next_id("stop");
                let _ = e
                    .write(&json!({ "type": "control_request", "request_id": id, "request": { "subtype": "interrupt" } }))
                    .await;
                note(app, "已让 Claude 停下");
            }
        }
        Some("new") => {
            let folder = msg["folder"].as_str().filter(|f| !f.is_empty());
            new_conversation(app, folder).await;
        }
        Some("sessions") => send_conversations(app).await,
        Some("open") => {
            if let Some(id) = msg["session"].as_str().filter(|s| is_uuid(s)) {
                open_conversation(app, id).await;
            }
        }
        Some(kind @ ("model" | "mode" | "effort" | "follow")) => {
            let change = match kind {
                "follow" => json!({ "follow": true }),
                _ => json!({ kind: msg[kind].as_str().unwrap_or("") }),
            };
            let state = app.state::<State>();
            if let Err(why) = set(app, &state, &change).await {
                note(app, why);
            }
        }
        Some("sync") => {
            end_conversation(app, false).await;
            RT.lock().unwrap().sync_now = true;
            SYNC.notify_one();
        }
        _ => {}
    }
}

async fn prompt(app: &AppHandle, cfg: &Remote, text: &str) {
    // The pairing as saved now (its folder may have changed since listening began).
    let cfg = RT
        .lock()
        .unwrap()
        .cfg
        .clone()
        .unwrap_or_else(|| cfg.clone());
    // Claude Desktop opened the conversation meanwhile: what's said here goes
    // into a copy, from a fresh engine that reads all of it.
    let running_on = {
        let rt = RT.lock().unwrap();
        rt.engine.as_ref().and(rt.session.clone())
    };
    if let Some(session) = running_on {
        if held_by_desktop(app, &session).await {
            stop_engine(app, true);
        }
    }
    let engine = RT.lock().unwrap().engine.clone();
    let engine = match engine {
        Some(e) => e,
        None => match start(app, &cfg).await {
            Ok(e) => e,
            Err(why) => {
                event(app, json!({ "kind": "user", "text": text }));
                note(app, why);
                return;
            }
        },
    };
    event(app, json!({ "kind": "user", "text": text }));
    {
        let mut rt = RT.lock().unwrap();
        rt.last_turn = Some(Instant::now());
        if rt.title.is_none() {
            rt.title = Some(clip(text.lines().next().unwrap_or(text), 60));
        }
    }
    set_busy(app, true);
    let line = json!({
        "type": "user",
        "session_id": "",
        "message": { "role": "user", "content": [{ "type": "text", "text": text }] },
        "parent_tool_use_id": null,
    });
    if engine.write(&line).await.is_err() {
        set_busy(app, false);
        note(app, "Claude Code 刚刚退出了，请再发一次");
        stop_engine(app, false);
    }
}

async fn answer(app: &AppHandle, request_id: &str, allow: bool) {
    let (engine, input) = {
        let mut rt = RT.lock().unwrap();
        (rt.engine.clone(), rt.pending.remove(request_id))
    };
    let (Some(engine), Some(input)) = (engine, input) else {
        return;
    };
    let response = if allow {
        json!({ "behavior": "allow", "updatedInput": input })
    } else {
        json!({ "behavior": "deny", "message": "用户在手机上拒绝了这个操作" })
    };
    let reply = json!({
        "type": "control_response",
        "response": { "subtype": "success", "request_id": request_id, "response": response },
    });
    let _ = engine.write(&reply).await;
    RT.lock().unwrap().last_turn = Some(Instant::now());
    event(
        app,
        json!({ "kind": "asked", "request_id": request_id, "allowed": allow }),
    );
}

/// Ends Claude Code; `quiet`: no word about it.
fn stop_engine(app: &AppHandle, quiet: bool) {
    let engine = {
        let mut rt = RT.lock().unwrap();
        rt.pending.clear();
        rt.busy = false;
        rt.engine.take()
    };
    if let Some(e) = engine {
        e.kill();
        if !quiet {
            note(app, "Claude Code 已结束");
        }
    }
    changed(app);
}

// ---------------------------------------------------------------- the engine

async fn start(app: &AppHandle, cfg: &Remote) -> Result<Arc<Engine>, String> {
    // An opened conversation works in its own folder.
    let folder = RT
        .lock()
        .unwrap()
        .opened
        .as_ref()
        .map_or_else(|| cfg.folder.clone(), |o| o.folder.clone());
    if folder.is_empty() || !Path::new(&folder).is_dir() {
        return Err("还没有选文件夹：请在电脑上的兔啃里选择让 Claude 干活的文件夹".into());
    }
    let exe = crate::desktop::code_engine(app)?;
    let paths = crate::config_paths(app)?;
    let key = crate::claude_config::desktop_key(&paths)
        .or_else(|| crate::claude_config::cli_key(&paths))
        .ok_or("电脑上的兔啃还没接入 Claude Desktop：请先在首页点「一键接入」")?;
    refresh_desktop(&paths);
    let (mode, model, effort) = chosen(Some(cfg), &RT.lock().unwrap());

    let mut args: Vec<String> = [
        "--output-format",
        "stream-json",
        "--verbose",
        "--input-format",
        "stream-json",
        "--permission-prompt-tool",
        "stdio",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.extend([
        "--permission-mode".into(),
        mode.clone(),
        // So the mode can become bypass permissions later, as in Claude Desktop.
        "--allow-dangerously-skip-permissions".into(),
        // Claude's text as it is written.
        "--include-partial-messages".into(),
        // A question tool the phone can't show; Claude asks in its reply instead.
        "--disallowedTools".into(),
        "AskUserQuestion".into(),
    ]);
    if let Some(m) = &model {
        args.extend(["--model".into(), m.clone()]);
    }
    if let Some(e) = &effort {
        args.extend(["--effort".into(), e.clone()]);
    }
    let resume = RT.lock().unwrap().session.clone();
    // Continue the conversation, unless its record is gone. One Claude
    // Desktop may hold open continues in a copy: it would go on from what
    // it remembers, without what is said here.
    let mut fork = false;
    if let Some(s) = resume.filter(|s| transcript_exists(&paths, s)) {
        fork = held_by_desktop(app, &s).await;
        args.extend(["--resume".into(), s]);
        if fork {
            args.push("--fork-session".into());
        }
    }

    let mut command = Command::new(&exe);
    command
        .args(&args)
        .current_dir(&folder)
        // The gateway as Claude Code takes it at start (a --settings file
        // isn't read in time for signing in).
        .env("ANTHROPIC_BASE_URL", SITE)
        .env("ANTHROPIC_AUTH_TOKEN", &key)
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("CLAUDE_CODE_OAUTH_TOKEN")
        .env("CLAUDE_CODE_ENTRYPOINT", "sdk-ts")
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .env("DISABLE_AUTOUPDATER", "1")
        .env_remove("NODE_OPTIONS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        // A console program started by a window app would open a console.
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        if !has_git_bash() {
            command.env("CLAUDE_CODE_USE_POWERSHELL_TOOL", "1");
        }
    }
    // A dev build never touches the real ~/.claude.
    if cfg!(debug_assertions) {
        if let Ok(home) = std::env::var("TUKEN_HOME") {
            command
                .env("CLAUDE_CONFIG_DIR", Path::new(&home).join(".claude"))
                .env("USERPROFILE", &home)
                .env("HOME", &home);
        }
    }
    let mut child = command
        .spawn()
        .map_err(|_| "启动不了 Claude Code：请在首页修复 Claude Desktop 后再试".to_string())?;
    let stdin = child.stdin.take().ok_or("启动不了 Claude Code")?;
    let stdout = child.stdout.take().ok_or("启动不了 Claude Code")?;
    let stderr = child.stderr.take().ok_or("启动不了 Claude Code")?;
    let engine = Arc::new(Engine {
        stdin: tokio::sync::Mutex::new(stdin),
        child: Mutex::new(Some(child)),
        ids: AtomicU64::new(1),
    });
    {
        let mut rt = RT.lock().unwrap();
        rt.engine = Some(engine.clone());
        rt.model = model.clone();
        rt.mode = Some(mode.clone());
        rt.effort = effort.clone();
        rt.restart_after_turn = false;
        rt.spent = 0.0;
        rt.folder = Some(folder.clone());
        rt.ended = false;
        if fork {
            // A new conversation, in Claude Desktop's eyes: its id comes with init.
            rt.session = None;
        }
    }
    if fork {
        note(
            app,
            "电脑上的 Claude Desktop 开着这个对话，接下来的内容会另存为一个新对话，聊完同步到电脑",
        );
    }
    let init = engine.next_id("init");
    engine
        .write(&json!({ "type": "control_request", "request_id": init, "request": { "subtype": "initialize" } }))
        .await?;

    let tail = Arc::new(Mutex::new(VecDeque::<String>::new()));
    let t = tail.clone();
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let mut t = t.lock().unwrap();
            t.push_back(line);
            while t.len() > 20 {
                t.pop_front();
            }
        }
    });
    let a = app.clone();
    let e = engine.clone();
    let workdir = folder.clone();
    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                output(&a, &e, &workdir, &v).await;
            }
        }
        // The engine is gone. If it went on its own, say why.
        let ours = {
            let mut rt = RT.lock().unwrap();
            let ours = rt.engine.as_ref().is_some_and(|x| Arc::ptr_eq(x, &e));
            if ours {
                rt.engine = None;
                rt.busy = false;
                rt.pending.clear();
            }
            ours
        };
        if ours {
            let why = tail
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|l| !l.trim().is_empty())
                .cloned();
            let quick = started.elapsed() < Duration::from_secs(10);
            note(
                &a,
                match (why, quick) {
                    (Some(w), true) => format!("Claude Code 没能启动：{}", clip(&w, 300)),
                    _ => "Claude Code 已结束，下一条指令会重新启动它".into(),
                },
            );
            changed(&a);
        }
    });
    Ok(engine)
}

/// One line of the engine's output.
async fn output(app: &AppHandle, engine: &Arc<Engine>, folder: &str, v: &Value) {
    match v["type"].as_str() {
        Some("system") if v["subtype"] == "init" => {
            let mut rt = RT.lock().unwrap();
            if let Some(s) = v["session_id"].as_str() {
                rt.session = Some(s.to_string());
            }
            if let Some(m) = v["model"].as_str() {
                rt.model = Some(m.to_string());
            }
            drop(rt);
            changed(app);
        }
        Some("stream_event") => {
            let ev = &v["event"];
            match ev["type"].as_str() {
                Some("message_start") => {
                    RT.lock().unwrap().stream_msg =
                        ev["message"]["id"].as_str().map(str::to_string);
                }
                Some("content_block_start") => {
                    RT.lock().unwrap().stream_block = ev["index"].as_i64().unwrap_or(0);
                }
                Some("content_block_delta") if ev["delta"]["type"] == "text_delta" => {
                    let id = {
                        let rt = RT.lock().unwrap();
                        format!(
                            "{}:{}",
                            rt.stream_msg.as_deref().unwrap_or(""),
                            ev["index"].as_i64().unwrap_or(0)
                        )
                    };
                    if let Some(text) = ev["delta"]["text"].as_str() {
                        delta(&id, text);
                    }
                }
                _ => {}
            }
        }
        Some("assistant") => {
            // Each finished block comes as a message of its own, with the id of
            // the message being streamed: the block is the one last started.
            let msg_id = v["message"]["id"].as_str().unwrap_or("");
            let block = {
                let rt = RT.lock().unwrap();
                (rt.stream_msg.as_deref() == Some(msg_id)).then_some(rt.stream_block)
            };
            for (i, block_value) in v["message"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                let block_id = format!("{msg_id}:{}", block.unwrap_or(i as i64));
                let block = block_value;
                match block["type"].as_str() {
                    Some("text") => {
                        let text = block["text"].as_str().unwrap_or("").trim();
                        if !text.is_empty() {
                            event(
                                app,
                                json!({ "kind": "text", "id": block_id, "text": clip(text, TEXT_MAX) }),
                            );
                        }
                    }
                    Some("tool_use") => event(
                        app,
                        json!({
                            "kind": "tool",
                            "id": block["id"],
                            "title": tool_title(block["name"].as_str().unwrap_or(""), &block["input"], folder),
                        }),
                    ),
                    _ => {}
                }
            }
        }
        Some("user") => {
            for block in v["message"]["content"].as_array().into_iter().flatten() {
                if block["type"] == "tool_result" {
                    event(
                        app,
                        json!({ "kind": "tool_done", "id": block["tool_use_id"], "ok": block["is_error"] != true }),
                    );
                }
            }
        }
        Some("result") => {
            let ok = v["is_error"] != true && v["subtype"] == "success";
            // The engine reports its total since it started; the phone wants this turn's.
            let total = v["total_cost_usd"].as_f64();
            let cost = total.map(|t| {
                let mut rt = RT.lock().unwrap();
                let turn = if t >= rt.spent { t - rt.spent } else { t };
                rt.spent = t;
                turn
            });
            let mut e = json!({
                "kind": "done",
                "ok": ok,
                "seconds": v["duration_ms"].as_f64().map(|ms| ms / 1000.0),
                "cost": cost,
            });
            if !ok {
                e["text"] = json!(clip(v["result"].as_str().unwrap_or(""), 300));
            }
            let restart = {
                let mut rt = RT.lock().unwrap();
                if let Some(s) = v["session_id"].as_str() {
                    rt.session = Some(s.to_string());
                }
                rt.last_turn = Some(Instant::now());
                std::mem::take(&mut rt.restart_after_turn)
            };
            event(app, e);
            set_busy(app, false);
            if restart {
                // A new effort level: the next prompt resumes with it.
                stop_engine(app, true);
            }
        }
        Some("control_request") if v["request"]["subtype"] == "can_use_tool" => {
            let Some(id) = v["request_id"].as_str() else {
                return;
            };
            let name = v["request"]["tool_name"].as_str().unwrap_or("");
            let input = v["request"]["input"].clone();
            let (title, detail) = ask_text(name, &input, folder);
            RT.lock().unwrap().pending.insert(id.to_string(), input);
            event(
                app,
                json!({ "kind": "ask", "request_id": id, "title": title, "detail": detail }),
            );
            // No answer in time: say no, so Claude doesn't wait forever.
            let a = app.clone();
            let id = id.to_string();
            let e = engine.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(ASK_WAIT).await;
                let still = {
                    let rt = RT.lock().unwrap();
                    rt.pending.contains_key(&id)
                        && rt.engine.as_ref().is_some_and(|x| Arc::ptr_eq(x, &e))
                };
                if still {
                    answer(&a, &id, false).await;
                    note(&a, "手机上一直没回应，已替你拒绝");
                }
            });
        }
        Some("control_request") => {
            // Something this version can't answer: say so instead of hanging.
            if let Some(id) = v["request_id"].as_str() {
                let _ = engine
                    .write(&json!({
                        "type": "control_response",
                        "response": { "subtype": "error", "request_id": id, "error": "not supported" },
                    }))
                    .await;
            }
        }
        Some("control_cancel_request") => {
            if let Some(id) = v["request_id"].as_str() {
                if RT.lock().unwrap().pending.remove(id).is_some() {
                    event(
                        app,
                        json!({ "kind": "asked", "request_id": id, "allowed": false }),
                    );
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- model

/// Claude Desktop's models (the ones 兔啃 set there) and what a new
/// conversation there starts with, kept for the phone.
fn refresh_desktop(paths: &crate::claude_config::Paths) -> Vec<String> {
    let set = crate::claude_config::desktop_models(paths).unwrap_or_default();
    let models: Vec<String> = set["models"]
        .as_array()
        .map(|ms| {
            ms.iter()
                .filter_map(|m| m.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let mut rt = RT.lock().unwrap();
    rt.defaults = Defaults {
        mode: crate::claude_config::default_permission_mode(paths),
        model: models.first().cloned(),
        effort: set["effort"].as_str().map(str::to_string),
    };
    rt.models = models.clone();
    models
}

fn mode_name(mode: &str) -> &'static str {
    match mode {
        "bypassPermissions" => "全部自动",
        "acceptEdits" => "改文件自动",
        _ => "每次询问",
    }
}

fn effort_name(effort: &str) -> &'static str {
    match effort {
        "low" => "低",
        "medium" => "中",
        "high" => "高",
        "xhigh" => "超高",
        _ => "最高",
    }
}

/// Says what changed in the chosen mode, model and effort (the note also
/// brings the phone the new status), and brings a running engine to them;
/// a stopped one starts with them.
async fn apply_settings(app: &AppHandle, before: (String, Option<String>, Option<String>)) {
    let (engine, busy, (mode, model, effort)) = {
        let rt = RT.lock().unwrap();
        (rt.engine.clone(), rt.busy, chosen(rt.cfg.as_ref(), &rt))
    };
    if mode != before.0 {
        if let Some(e) = &engine {
            let id = e.next_id("mode");
            let line = json!({ "type": "control_request", "request_id": id, "request": { "subtype": "set_permission_mode", "mode": mode } });
            if e.write(&line).await.is_ok() {
                RT.lock().unwrap().mode = Some(mode.clone());
            }
        }
        note(app, format!("权限改为「{}」", mode_name(&mode)));
    }
    if model != before.1 {
        if let (Some(e), Some(m)) = (&engine, &model) {
            let id = e.next_id("model");
            let line = json!({ "type": "control_request", "request_id": id, "request": { "subtype": "set_model", "model": m } });
            if e.write(&line).await.is_ok() {
                RT.lock().unwrap().model = Some(m.clone());
            }
        }
        if let Some(m) = &model {
            note(app, format!("已切换到 {m}"));
        }
    }
    if effort != before.2 {
        let name = effort_name(effort.as_deref().unwrap_or("max"));
        if engine.is_some() {
            // Claude Code takes the effort when it starts: it restarts, resuming.
            if busy {
                RT.lock().unwrap().restart_after_turn = true;
            } else {
                stop_engine(app, true);
            }
            note(app, format!("推理程度改为「{name}」，下一条指令开始生效"));
        } else {
            note(app, format!("推理程度改为「{name}」"));
        }
    }
    changed(app);
}

// ---------------------------------------------------------------- the phone's 对话 menu

/// Folders offered for a new conversation, at most, and the menu's size
/// (the relay takes 64 KB an event).
const FOLDERS_MAX: usize = 12;
const MENU_BYTES: usize = 60_000;

/// Whether Claude Desktop may hold this conversation open (see `conversations`).
async fn held_by_desktop(app: &AppHandle, session: &str) -> bool {
    let Ok(paths) = crate::config_paths(app) else {
        return true;
    };
    let session = session.to_string();
    tauri::async_runtime::spawn_blocking(move || crate::conversations::held(&paths).holds(&session))
        .await
        .unwrap_or(true)
}

/// What the menu lists: Claude Desktop's conversations and the phone's own
/// it doesn't have yet (this one, those waiting to go in), with the folders
/// a new one may work in (the one chosen here first) and which ones Claude
/// Desktop holds open.
async fn menu(
    app: &AppHandle,
) -> Option<(
    Vec<crate::conversations::Conversation>,
    Vec<String>,
    crate::conversations::Held,
)> {
    let paths = crate::config_paths(app).ok()?;
    let state = app.state::<State>();
    let (unsynced, chosen_folder) = {
        let v = state.vault_if_open().await?;
        (
            v.remote_unsynced.clone(),
            v.remote.as_ref().map(|r| r.folder.clone()),
        )
    };
    let mut own: Vec<crate::conversations::Own> = Vec::new();
    {
        let rt = RT.lock().unwrap();
        if let (Some(id), Some(folder)) = (rt.session.clone(), rt.folder.clone()) {
            own.push(crate::conversations::Own {
                id,
                title: rt.title.clone().unwrap_or_default(),
                folder,
            });
        }
    }
    own.extend(unsynced.into_iter().map(|u| crate::conversations::Own {
        id: u.session,
        title: u.title,
        folder: u.folder,
    }));
    tauri::async_runtime::spawn_blocking(move || {
        let list = crate::conversations::list(&paths, &own);
        let mut folders: Vec<String> = Vec::new();
        for f in chosen_folder
            .into_iter()
            .chain(list.iter().map(|c| c.folder.clone()))
        {
            if folders.len() < FOLDERS_MAX
                && !f.is_empty()
                && !folders.contains(&f)
                && Path::new(&f).is_dir()
            {
                folders.push(f);
            }
        }
        let held = crate::conversations::held(&paths);
        (list, folders, held)
    })
    .await
    .ok()
}

/// Sends the phone its 对话 menu.
async fn send_conversations(app: &AppHandle) {
    let Some((list, folders, held)) = menu(app).await else {
        return;
    };
    let current = RT.lock().unwrap().session.clone();
    let mut items: Vec<Value> = list
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "title": c.title,
                "folder": folder_name(&c.folder),
                "path": c.folder,
                "at": c.at,
                "open": held.holds(&c.id),
            })
        })
        .collect();
    let folders: Vec<Value> = folders
        .iter()
        .map(|f| json!({ "path": f, "name": folder_name(f) }))
        .collect();
    loop {
        let e =
            json!({ "kind": "sessions", "items": items, "folders": folders, "current": current });
        // Within what the relay takes for one event.
        if e.to_string().len() <= MENU_BYTES || items.is_empty() {
            to_phone(e);
            return;
        }
        items.truncate(items.len() * 3 / 4);
    }
}

/// Opens a conversation from the menu on the phone: the one going on ends,
/// and the phone gets what was said so far.
async fn open_conversation(app: &AppHandle, id: &str) {
    let Some((list, _, _)) = menu(app).await else {
        return;
    };
    let Some(c) = list.into_iter().find(|c| c.id == id) else {
        note(app, "电脑上找不到这个对话了");
        return;
    };
    let same = RT.lock().unwrap().session.as_deref() == Some(id);
    if !same {
        end_conversation(app, true).await;
        stop_engine(app, true);
        let mut rt = RT.lock().unwrap();
        rt.session = Some(c.id.clone());
        rt.ended = false;
        rt.title = Some(c.title.clone());
        rt.last_turn = None;
        rt.folder = Some(c.folder.clone());
        rt.opened = Some(Opened {
            folder: c.folder.clone(),
            settings: Defaults {
                mode: c.mode.clone(),
                model: c.model.clone(),
                effort: c.effort.clone(),
            },
        });
    }
    let (file, folder) = (c.transcript.clone(), c.folder.clone());
    let history =
        tauri::async_runtime::spawn_blocking(move || crate::conversations::history(&file, &folder))
            .await
            .unwrap_or(crate::conversations::History {
                items: Vec::new(),
                more: true,
            });
    to_phone(json!({
        "kind": "history",
        "session": c.id,
        "title": c.title,
        "folder": folder_name(&c.folder),
        "items": history.items,
        "more": history.more,
    }));
    if !same {
        log_only(app, format!("手机上打开了「{}」", c.title));
    }
    changed(app);
}

/// Starts a new conversation on the phone, in `folder` if it is one the
/// menu offered.
async fn new_conversation(app: &AppHandle, folder: Option<&str>) {
    let folder = match folder {
        Some(f) => {
            let offered = menu(app)
                .await
                .is_some_and(|(_, folders, _)| folders.iter().any(|o| o == f));
            if !offered {
                note(app, "电脑上没有这个文件夹了");
                return;
            }
            Some(f.to_string())
        }
        None => None,
    };
    end_conversation(app, true).await;
    stop_engine(app, true);
    {
        let mut rt = RT.lock().unwrap();
        rt.session = None;
        rt.ended = false;
        rt.title = None;
        rt.last_turn = None;
        rt.opened = None;
    }
    let state = app.state::<State>();
    let current = state
        .vault()
        .await
        .ok()
        .and_then(|v| v.remote.as_ref().map(|r| r.folder.clone()))
        .unwrap_or_default();
    match folder.filter(|f| *f != current) {
        // Where new conversations go from now on, as choosing it here does
        // (which starts the phone afresh too).
        Some(f) => {
            if let Err(why) = set(app, &state, &json!({ "folder": f })).await {
                note(app, why);
            }
        }
        None => {
            fresh_on_phone(&current);
            note(app, "已开始新对话");
        }
    }
    changed(app);
}

/// The phone shows a new, empty conversation (before the notes that follow).
fn fresh_on_phone(folder: &str) {
    to_phone(
        json!({ "kind": "history", "session": null, "title": null, "folder": folder_name(folder), "items": [], "more": false }),
    );
}

// ---------------------------------------------------------------- Claude Desktop sync

/// Ends the phone conversation: the engine stops, and the conversation is
/// queued for Claude Desktop unless it is there already (opened from its
/// list). `quiet`: no word about it.
async fn end_conversation(app: &AppHandle, quiet: bool) -> bool {
    let (session, folder, title) = {
        let mut rt = RT.lock().unwrap();
        let Some(session) = rt.session.clone().filter(|_| !rt.ended) else {
            return false;
        };
        rt.ended = true;
        (
            session,
            rt.folder.clone().unwrap_or_default(),
            rt.title.clone().unwrap_or_default(),
        )
    };
    let engine = RT.lock().unwrap().engine.clone();
    if let Some(e) = engine {
        if RT.lock().unwrap().busy {
            let id = e.next_id("stop");
            let _ = e
                .write(&json!({ "type": "control_request", "request_id": id, "request": { "subtype": "interrupt" } }))
                .await;
            tokio::time::sleep(Duration::from_millis(800)).await;
        }
    }
    stop_engine(app, true);
    let known = crate::config_paths(app).is_ok_and(|p| desktop_has(&p, &session));
    if known {
        changed(app);
        return true;
    }
    let state = app.state::<State>();
    if let Ok(mut v) = state.vault().await {
        if !v.remote_unsynced.iter().any(|u| u.session == session) {
            v.remote_unsynced.push(RemoteSync {
                session,
                folder,
                title,
                ended: chrono_now_ms(),
                tries: 0,
            });
            let extra = v.remote_unsynced.len().saturating_sub(UNSYNCED_MAX);
            v.remote_unsynced.drain(..extra);
            let _ = state.save(&v);
        }
    }
    if !quiet {
        note(app, "这段对话结束了，会同步到电脑上的 Claude Desktop");
    }
    SYNC.notify_one();
    changed(app);
    true
}

fn chrono_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// Whether the conversation is over by now: no turn running or waiting for
/// the phone, and long quiet, or the computer back in use (whoever is at it
/// is done with the phone) after a quiet moment.
fn conversation_over(rt: &Runtime, now: Instant, pc_back: bool) -> bool {
    if rt.session.is_none() || rt.ended || rt.busy || !rt.pending.is_empty() {
        return false;
    }
    let Some(last) = rt.last_turn else {
        return false;
    };
    let quiet = now.duration_since(last);
    quiet >= CONVERSATION_IDLE || (pc_back && quiet >= QUIET)
}

/// Puts ended phone conversations into Claude Desktop, one at a time.
async fn sync_loop(app: AppHandle) {
    let mut was_idle = false;
    loop {
        let _ = tokio::time::timeout(SYNC_EVERY, SYNC.notified()).await;
        if let Ok(paths) = crate::config_paths(&app) {
            refresh_desktop(&paths);
        }
        // Claude Desktop jumps to what it imports: not while the computer is
        // in use, unless its user just came back to it.
        let idle = pc_idle().await.is_none_or(|d| d >= PC_IDLE);
        let back = was_idle && !idle;
        was_idle = idle;
        if conversation_over(&RT.lock().unwrap(), Instant::now(), back) {
            end_conversation(&app, false).await;
        }
        let state = app.state::<State>();
        let (next, off) = match state.vault_if_open().await {
            Some(v) => (v.remote_unsynced.first().cloned(), v.remote_sync_off),
            None => continue,
        };
        let now_asked = std::mem::take(&mut RT.lock().unwrap().sync_now);
        let Some(item) = next else {
            continue;
        };
        if off && !now_asked {
            continue;
        }
        if !(now_asked || idle || back) || !sync_allowed() || !crate::desktop::running() {
            continue;
        }
        let done = sync_one(&app, &item).await;
        if let Ok(mut v) = state.vault().await {
            if let Some(pos) = v
                .remote_unsynced
                .iter()
                .position(|u| u.session == item.session)
            {
                if done {
                    v.remote_unsynced.remove(pos);
                } else {
                    v.remote_unsynced[pos].tries += 1;
                    if v.remote_unsynced[pos].tries >= SYNC_TRIES {
                        // Its record may be gone; Claude Desktop says why in its own log.
                        v.remote_unsynced.remove(pos);
                        note(&app, "有一段手机上的对话没能同步到 Claude Desktop");
                    }
                }
                let _ = state.save(&v);
            }
        }
        changed(&app);
        if done {
            // The rest follow, a moment apart.
            tokio::time::sleep(Duration::from_secs(3)).await;
            SYNC.notify_one();
        }
    }
}

/// A dev build's Claude Code keeps its conversations in a sandbox the real
/// Claude Desktop can't import from.
fn sync_allowed() -> bool {
    !cfg!(debug_assertions) || std::env::var("TUKEN_REMOTE_SYNC").is_ok_and(|v| v == "1")
}

/// Asks Claude Desktop to import the conversation, and waits for its record.
async fn sync_one(app: &AppHandle, item: &RemoteSync) -> bool {
    use tauri_plugin_opener::OpenerExt;
    let Ok(paths) = crate::config_paths(app) else {
        return false;
    };
    if !is_uuid(&item.session) || !transcript_exists(&paths, &item.session) {
        return false;
    }
    if desktop_has(&paths, &item.session) {
        return true; // already there, imported or continued in Claude Desktop
    }
    let url = format!("claude://resume?session={}", item.session);
    if app.opener().open_url(&url, None::<&str>).is_err() {
        return false;
    }
    for _ in 0..15 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if desktop_has(&paths, &item.session) {
            let what = if item.title.is_empty() {
                "手机上的对话".to_string()
            } else {
                format!("「{}」", item.title)
            };
            note(app, format!("{what}已同步到电脑上的 Claude Desktop"));
            return true;
        }
    }
    false
}

/// Claude Desktop has a record of this conversation (`local_<id>.json`).
fn desktop_has(paths: &crate::claude_config::Paths, session: &str) -> bool {
    let Some(root) = paths.desktop_sessions() else {
        return false;
    };
    let name = format!("local_{session}.json");
    // <account>/<org>/local_<id>.json
    std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .any(|account| {
            std::fs::read_dir(account.path())
                .into_iter()
                .flatten()
                .flatten()
                .any(|org| org.path().join(&name).is_file())
        })
}

/// Claude Code still has the conversation (`<projects>/*/<id>.jsonl`).
fn transcript_exists(paths: &crate::claude_config::Paths, session: &str) -> bool {
    if !is_uuid(session) {
        return false;
    }
    let Some(projects) = crate::conversations::projects_dir(paths) else {
        return false;
    };
    let name = format!("{session}.jsonl");
    std::fs::read_dir(projects)
        .into_iter()
        .flatten()
        .flatten()
        .any(|dir| dir.path().join(&name).is_file())
}

pub(crate) fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// How long since the last keyboard or mouse input; None if unknown.
#[cfg(windows)]
async fn pc_idle() -> Option<Duration> {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: a plain call with a sized struct.
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return None;
    }
    let now = unsafe { GetTickCount() };
    Some(Duration::from_millis(u64::from(
        now.wrapping_sub(info.dwTime),
    )))
}

#[cfg(target_os = "macos")]
async fn pc_idle() -> Option<Duration> {
    let out = Command::new("/usr/sbin/ioreg")
        .args(["-c", "IOHIDSystem", "-d", "4"])
        .stdin(Stdio::null())
        .output()
        .await
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.contains("\"HIDIdleTime\""))?;
    let nanos: u64 = line.rsplit('=').next()?.trim().parse().ok()?;
    Some(Duration::from_nanos(nanos))
}

#[cfg(not(any(windows, target_os = "macos")))]
async fn pc_idle() -> Option<Duration> {
    None
}

/// Puts the phone conversations into Claude Desktop now (the button here).
pub async fn sync_now(app: &AppHandle, state: &State) -> Result<Value, String> {
    if !crate::desktop::running() {
        return Err("请先打开 Claude Desktop".into());
    }
    if !sync_allowed() {
        return Err("开发版不同步到 Claude Desktop".into());
    }
    end_conversation(app, true).await;
    RT.lock().unwrap().sync_now = true;
    SYNC.notify_one();
    status(state).await
}

// ---------------------------------------------------------------- words for the phone

pub(crate) fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// A path as the user knows it: inside the folder, without the folder.
fn short_path(path: &str, folder: &str) -> String {
    let p = Path::new(path);
    p.strip_prefix(folder)
        .map(|r| r.to_string_lossy().into_owned())
        .ok()
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| path.to_string())
}

fn first_line(s: &str, max: usize) -> String {
    clip(s.lines().next().unwrap_or("").trim(), max)
}

/// What a tool call does, in a few words.
pub(crate) fn tool_title(name: &str, input: &Value, folder: &str) -> String {
    let path = || {
        short_path(
            input["file_path"]
                .as_str()
                .or(input["notebook_path"].as_str())
                .unwrap_or(""),
            folder,
        )
    };
    match name {
        "Read" => format!("读取 {}", path()),
        "Write" => format!("写入 {}", path()),
        "Edit" | "MultiEdit" | "NotebookEdit" => format!("修改 {}", path()),
        "Bash" | "PowerShell" => format!(
            "运行命令 {}",
            first_line(input["command"].as_str().unwrap_or(""), 80)
        ),
        "Glob" => format!(
            "查找文件 {}",
            clip(input["pattern"].as_str().unwrap_or(""), 60)
        ),
        "Grep" => format!(
            "搜索内容 {}",
            clip(input["pattern"].as_str().unwrap_or(""), 60)
        ),
        "WebFetch" => format!("打开网页 {}", clip(input["url"].as_str().unwrap_or(""), 80)),
        "WebSearch" => format!(
            "搜索网页 {}",
            clip(input["query"].as_str().unwrap_or(""), 60)
        ),
        "TodoWrite" => "更新待办清单".into(),
        "Task" | "Agent" => format!(
            "派出子任务 {}",
            clip(input["description"].as_str().unwrap_or(""), 60)
        ),
        "" => "使用工具".into(),
        other => format!("使用 {}", clip(other, 40)),
    }
}

/// The question for the phone, and what it is about.
fn ask_text(name: &str, input: &Value, folder: &str) -> (String, String) {
    let path = || {
        short_path(
            input["file_path"]
                .as_str()
                .or(input["notebook_path"].as_str())
                .unwrap_or(""),
            folder,
        )
    };
    match name {
        "Bash" | "PowerShell" => (
            "Claude 想运行命令".into(),
            clip(input["command"].as_str().unwrap_or(""), DETAIL_MAX),
        ),
        "Write" => ("Claude 想写入文件".into(), path()),
        "Edit" | "MultiEdit" | "NotebookEdit" => ("Claude 想修改文件".into(), path()),
        "WebFetch" => (
            "Claude 想打开网页".into(),
            clip(input["url"].as_str().unwrap_or(""), DETAIL_MAX),
        ),
        _ => (
            format!("Claude 想使用 {}", clip(name, 40)),
            clip(
                &serde_json::to_string_pretty(input).unwrap_or_default(),
                DETAIL_MAX,
            ),
        ),
    }
}

// ---------------------------------------------------------------- helpers

/// Claude Code's Bash tool needs Git for Windows; without it, PowerShell.
#[cfg(windows)]
fn has_git_bash() -> bool {
    if std::env::var_os("CLAUDE_CODE_GIT_BASH_PATH").is_some() {
        return true;
    }
    let roots = [
        std::env::var_os("ProgramFiles"),
        std::env::var_os("ProgramFiles(x86)"),
        std::env::var_os("LOCALAPPDATA").map(|l| {
            std::path::PathBuf::from(l)
                .join("Programs")
                .into_os_string()
        }),
    ];
    roots.into_iter().flatten().any(|r| {
        std::path::PathBuf::from(r)
            .join("Git")
            .join("bin")
            .join("bash.exe")
            .is_file()
    })
}

/// The QR code of `text`: its size in modules, and an SVG path of the dark ones.
pub fn qr(text: &str) -> Result<(i32, String), String> {
    use std::fmt::Write;
    let code = qrcodegen::QrCode::encode_text(text, qrcodegen::QrCodeEcc::Medium)
        .map_err(|_| "生成二维码失败".to_string())?;
    let n = code.size();
    let mut path = String::new();
    for y in 0..n {
        for x in 0..n {
            if code.get_module(x, y) {
                let _ = write!(path, "M{x} {y}h1v1h-1z");
            }
        }
    }
    Ok((n, path))
}

/// Lets the user choose the folder, starting from the current one.
async fn pick_folder(app: &AppHandle, state: &State) -> Result<Option<String>, String> {
    let start = {
        let v = state.vault().await?;
        v.remote
            .as_ref()
            .map(|r| r.folder.clone())
            .or_else(|| v.remote_folder.clone())
            .filter(|f| Path::new(f).is_dir())
    };
    #[cfg(windows)]
    {
        let owner = app
            .get_webview_window("main")
            .and_then(|w| w.hwnd().ok())
            .map(|h| h.0 as isize);
        tauri::async_runtime::spawn_blocking(move || pick_folder_windows(owner, start.as_deref()))
            .await
            .map_err(|_| "打不开选择文件夹的窗口".to_string())?
    }
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        let script = match start {
            Some(s) => format!(
                "POSIX path of (choose folder with prompt \"选择让 Claude 干活的文件夹\" default location (POSIX file \"{}\"))",
                s.replace('\\', "\\\\").replace('"', "\\\"")
            ),
            None => "POSIX path of (choose folder with prompt \"选择让 Claude 干活的文件夹\")".into(),
        };
        let out = Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(script)
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|_| "打不开选择文件夹的窗口".to_string())?;
        if !out.status.success() {
            return Ok(None); // cancelled
        }
        let path = String::from_utf8_lossy(&out.stdout)
            .trim()
            .trim_end_matches('/')
            .to_string();
        Ok((!path.is_empty()).then_some(path))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (app, start);
        Err("这个系统暂不支持手机遥控".into())
    }
}

#[cfg(windows)]
fn pick_folder_windows(
    owner: Option<isize>,
    start: Option<&str>,
) -> Result<Option<String>, String> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IBindCtx,
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName,
        FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
    };
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let result = (|| -> windows::core::Result<Option<String>> {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetOptions(dialog.GetOptions()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)?;
            dialog.SetTitle(&HSTRING::from("选择让 Claude 干活的文件夹"))?;
            if let Some(start) = start {
                if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(
                    &HSTRING::from(start),
                    None::<&IBindCtx>,
                ) {
                    let _ = dialog.SetFolder(&item);
                }
            }
            if dialog
                .Show(owner.map(|h| HWND(h as *mut core::ffi::c_void)))
                .is_err()
            {
                return Ok(None); // cancelled
            }
            let name = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
            let path = name.to_string().unwrap_or_default();
            CoTaskMemFree(Some(name.0 as *const core::ffi::c_void));
            Ok((!path.is_empty()).then_some(path))
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        result.map_err(|_| "打不开选择文件夹的窗口".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_calls_read_as_short_chinese_lines() {
        let folder = if cfg!(windows) {
            r"C:\work\app"
        } else {
            "/work/app"
        };
        let file = Path::new(folder).join("src").join("main.rs");
        let file = file.to_string_lossy();
        assert_eq!(
            tool_title("Read", &json!({ "file_path": file }), folder),
            format!("读取 {}", Path::new("src").join("main.rs").display())
        );
        assert_eq!(
            tool_title(
                "Bash",
                &json!({ "command": "npm test\nnpm run lint" }),
                folder
            ),
            "运行命令 npm test"
        );
        assert_eq!(
            tool_title("Edit", &json!({ "file_path": "/elsewhere/x.txt" }), folder),
            "修改 /elsewhere/x.txt"
        );
        assert_eq!(
            tool_title("mcp__x__y", &json!({}), folder),
            "使用 mcp__x__y"
        );
        let (title, detail) = ask_text("Bash", &json!({ "command": "rm -rf build" }), folder);
        assert_eq!(
            (title.as_str(), detail.as_str()),
            ("Claude 想运行命令", "rm -rf build")
        );
    }

    #[test]
    fn long_text_is_clipped_by_characters() {
        assert_eq!(clip("一二三四", 3), "一二三…");
        assert_eq!(clip("短", 3), "短");
    }

    #[test]
    fn a_conversation_is_over_when_long_quiet_or_the_computer_is_back_in_use() {
        // An hour ahead: going back from now can't, on a machine up for minutes.
        let now = Instant::now() + Duration::from_secs(3600);
        let ago = |s: u64| Some(now - Duration::from_secs(s));
        let rt = |last: u64| Runtime {
            session: Some("6f1e1d2c-0000-4000-8000-000000000001".into()),
            last_turn: ago(last),
            ..Default::default()
        };
        // Only a long quiet ends it while the computer stays unused.
        assert!(!conversation_over(&rt(20 * 60), now, false));
        assert!(conversation_over(&rt(31 * 60), now, false));
        // Someone is back at the computer: done with the phone, unless a
        // turn just finished.
        assert!(conversation_over(&rt(3 * 60), now, true));
        assert!(
            !conversation_over(&rt(30), now, true),
            "a turn just finished"
        );
        // Not while Claude works or waits for an answer, nor twice.
        let mut busy = rt(31 * 60);
        busy.busy = true;
        assert!(!conversation_over(&busy, now, true));
        let mut asking = rt(31 * 60);
        asking.pending.insert("r1".into(), json!({}));
        assert!(!conversation_over(&asking, now, true));
        let mut ended = rt(31 * 60);
        ended.ended = true;
        assert!(!conversation_over(&ended, now, true));
        let mut none = rt(31 * 60);
        none.session = None;
        assert!(!conversation_over(&none, now, true));
        // Opened from the menu and not talked in: nothing to end.
        let mut opened = rt(0);
        opened.last_turn = None;
        assert!(!conversation_over(&opened, now, true));
    }

    fn pairing(mode: Option<&str>, model: Option<&str>, effort: Option<&str>) -> Remote {
        Remote {
            user_id: 1,
            pair_id: "p".into(),
            pc_key: "k".into(),
            folder: String::new(),
            accept_edits: false,
            on: true,
            model: model.map(str::to_string),
            mode: mode.map(str::to_string),
            effort: effort.map(str::to_string),
        }
    }

    #[test]
    fn the_phone_follows_claude_desktop_unless_it_chose() {
        let rt = Runtime {
            models: vec!["claude-opus-5-5".into(), "claude-sonnet-5-5".into()],
            defaults: Defaults {
                mode: Some("acceptEdits".into()),
                model: Some("claude-opus-5-5".into()),
                effort: Some("max".into()),
            },
            ..Default::default()
        };
        let follow = pairing(None, None, None);
        assert_eq!(
            chosen(Some(&follow), &rt),
            (
                "acceptEdits".into(),
                Some("claude-opus-5-5".into()),
                Some("max".into())
            )
        );
        assert_eq!(settings_view(Some(&follow), &rt)["custom"], json!([]));
        // Nothing set anywhere, or a mode the phone doesn't offer: nothing asks.
        assert_eq!(chosen(None, &Runtime::default()).0, "bypassPermissions");
        let mut auto = Runtime::default();
        auto.defaults.mode = Some("auto".into());
        assert_eq!(chosen(None, &auto).0, "bypassPermissions");
        // Chosen on the phone wins, while Claude Desktop still offers it.
        let own = pairing(Some("default"), Some("claude-sonnet-5-5"), Some("low"));
        assert_eq!(
            chosen(Some(&own), &rt),
            (
                "default".into(),
                Some("claude-sonnet-5-5".into()),
                Some("low".into())
            )
        );
        assert_eq!(
            settings_view(Some(&own), &rt)["custom"],
            json!(["mode", "model", "effort"])
        );
        let gone = pairing(Some("plan"), Some("claude-gone"), Some("extreme"));
        assert_eq!(chosen(Some(&gone), &rt), chosen(Some(&follow), &rt));
    }

    #[test]
    fn an_opened_conversation_keeps_its_own_settings_unless_the_phone_chose() {
        let rt = Runtime {
            models: vec!["claude-opus-5-5".into(), "claude-sonnet-5-5".into()],
            defaults: Defaults {
                mode: Some("bypassPermissions".into()),
                model: Some("claude-opus-5-5".into()),
                effort: Some("max".into()),
            },
            opened: Some(Opened {
                folder: "work".into(),
                settings: Defaults {
                    mode: Some("acceptEdits".into()),
                    model: Some("claude-sonnet-5-5".into()),
                    effort: Some("high".into()),
                },
            }),
            ..Default::default()
        };
        assert_eq!(
            chosen(Some(&pairing(None, None, None)), &rt),
            (
                "acceptEdits".into(),
                Some("claude-sonnet-5-5".into()),
                Some("high".into())
            )
        );
        assert_eq!(
            chosen(Some(&pairing(Some("default"), None, Some("low"))), &rt),
            (
                "default".into(),
                Some("claude-sonnet-5-5".into()),
                Some("low".into())
            )
        );
        // Its own ones Claude Desktop no longer offers: Claude Desktop's defaults.
        let mut odd = rt;
        odd.opened.as_mut().unwrap().settings = Defaults {
            mode: Some("plan".into()),
            model: Some("claude-gone".into()),
            effort: None,
        };
        assert_eq!(
            chosen(None, &odd),
            (
                "bypassPermissions".into(),
                Some("claude-opus-5-5".into()),
                Some("max".into())
            )
        );
    }

    #[test]
    fn streamed_text_waiting_to_be_sent_goes_as_one_event() {
        {
            let mut rt = RT.lock().unwrap();
            rt.listening = true;
            rt.outbox.clear();
        }
        delta("m1:0", "你");
        delta("m1:0", "好");
        delta("m1:1", "!");
        let sent: Vec<Value> = {
            let mut rt = RT.lock().unwrap();
            rt.listening = false;
            rt.outbox.drain(..).collect()
        };
        assert_eq!(
            sent,
            vec![
                json!({ "kind": "delta", "id": "m1:0", "text": "你好" }),
                json!({ "kind": "delta", "id": "m1:1", "text": "!" }),
            ]
        );
        // Not while nothing is sent to a phone.
        delta("m1:0", "x");
        assert!(RT.lock().unwrap().outbox.is_empty());
    }

    #[test]
    fn only_real_session_ids_go_into_links() {
        assert!(is_uuid("1986d9fb-ee6a-441d-85be-067dbf00bbd8"));
        assert!(!is_uuid("1986d9fb-ee6a-441d-85be-067dbf00bbd8&x=1"));
        assert!(!is_uuid("../../etc/passwd-0000-0000-0000-000000"));
        assert!(!is_uuid(""));
    }

    #[test]
    fn qr_codes_cover_the_phone_link() {
        let (size, path) =
            qr("https://www.tuken.site/remote#t=AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdEf")
                .unwrap();
        assert!(size >= 21 && size % 4 == 1, "a valid QR size: {size}");
        assert!(path.starts_with('M') && path.contains("h1v1h-1z"));
    }
}
