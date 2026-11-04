//! 手机遥控: a paired phone sends prompts through www.tuken.site's relay to
//! Claude Code (Claude Desktop's Code engine) running here, headless, in the
//! folder the user chose; what Claude does goes back to the phone.
//!
//! - While 手机遥控 is on, `listen` long-polls the relay's inbox.
//! - A prompt starts the engine if needed: stream-json in and out, permission
//!   prompts over stdio, the way Claude's own SDK runs it, with 兔啃's gateway
//!   key. Its output becomes short events for the phone and this window,
//!   posted in batches by `send_loop`.
//! - A permission prompt waits for the phone's 允许 / 拒绝 (10 minutes).
//! - The phone may switch Claude's model among Claude Desktop's (set_model).
//! - A phone conversation that has ended (新对话, the phone away for a
//!   minute, or quiet for 15) goes into Claude Desktop's list through its own
//!   import link, `claude://resume?session=<id>`: Claude Desktop shows a
//!   terminal conversation only once imported, and only as it was then. So
//!   it goes in once, when it is over, while Claude Desktop is open and the
//!   computer isn't being used (the import brings Claude Desktop to the
//!   front); talking on after that continues in a copy of it.
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
/// A phone conversation is over after this long without a turn, or when
/// the phone has been away for PHONE_GONE and the last turn was QUIET ago.
const CONVERSATION_IDLE: Duration = Duration::from_secs(15 * 60);
const PHONE_GONE: Duration = Duration::from_secs(60);
const QUIET: Duration = Duration::from_secs(120);
/// Claude Desktop jumps to an imported conversation: only after this long
/// without keyboard or mouse, unless asked to now.
const PC_IDLE: Duration = Duration::from_secs(90);
const SYNC_EVERY: Duration = Duration::from_secs(20);
const SYNC_TRIES: u32 = 3;
const UNSYNCED_MAX: usize = 20;

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
    /// The conversation's first prompt, and its last turn.
    title: Option<String>,
    last_turn: Option<Instant>,
    /// Since when the phone has been away.
    phone_gone: Option<Instant>,
    /// The conversation in `session` is over (queued for Claude Desktop);
    /// the next prompt continues it in a copy (`--fork-session`).
    ended: bool,
    /// Put conversations into Claude Desktop now, whatever the computer does.
    sync_now: bool,
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
        "accept_edits": cfg.is_some_and(|c| c.accept_edits),
        "listening": rt.listening,
        "phone_online": rt.phone_online,
        "busy": rt.busy,
        "error": rt.error,
        "model": rt.model.clone().or_else(|| cfg.and_then(|c| c.model.clone())),
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
fn phone_status(cfg: &Remote) -> Value {
    let rt = RT.lock().unwrap();
    json!({
        "folder": folder_name(&cfg.folder),
        "model": rt.model.clone().or_else(|| cfg.model.clone()).or_else(|| rt.models.first().cloned()),
        "models": rt.models,
        "busy": rt.busy,
        "mode": if cfg.accept_edits { "acceptEdits" } else { "default" },
    })
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

fn note(app: &AppHandle, text: impl Into<String>) {
    event(app, json!({ "kind": "note", "text": text.into() }));
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
    v.remote = Some(Remote {
        user_id,
        pair_id: pair_id.into(),
        pc_key: pc_key.into(),
        folder,
        accept_edits,
        on: true,
        model,
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
    if let Some(accept) = input["accept_edits"].as_bool() {
        restart_engine |= cfg.accept_edits != accept;
        cfg.accept_edits = accept;
    }
    if let Some(on) = input["on"].as_bool() {
        cfg.on = on;
    }
    let cfg = cfg.clone();
    if let Some(sync) = input["sync"].as_bool() {
        v.remote_sync_off = !sync;
    }
    state.save(&v)?;
    drop(v);
    if let Some(model) = input["model"].as_str() {
        apply_model(app, model).await;
    }
    if new_folder {
        // Another project: the conversation so far is over, the next starts afresh.
        end_conversation(app, true).await;
        let mut rt = RT.lock().unwrap();
        rt.session = None;
        rt.ended = false;
    }
    if restart_engine {
        // The next prompt starts Claude Code again, in the new folder or mode.
        stop_engine(app, true);
        note(
            app,
            format!("之后的指令在「{}」里执行", folder_name(&cfg.folder)),
        );
    }
    restart(app, Some(cfg));
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
        rt.generation
    };
    match cfg {
        Some(cfg) if cfg.on => {
            RT.lock().unwrap().listening = true;
            // The phone's model list, from the start.
            if let Ok(paths) = crate::config_paths(app) {
                refresh_models(&paths);
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
                    if online {
                        rt.phone_gone = None;
                    } else if rt.phone_gone.is_none() {
                        rt.phone_gone = Some(Instant::now());
                    }
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
            end_conversation(app, true).await;
            let mut rt = RT.lock().unwrap();
            rt.session = None;
            rt.ended = false;
            drop(rt);
            note(app, "已开始新对话");
        }
        Some("model") => {
            let model = msg["model"].as_str().unwrap_or("");
            let state = app.state::<State>();
            match set(app, &state, &json!({ "model": model })).await {
                Ok(_) => {}
                Err(why) => note(app, why),
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
    let engine = RT.lock().unwrap().engine.clone();
    let engine = match engine {
        Some(e) => e,
        None => match start(app, cfg).await {
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
    if cfg.folder.is_empty() || !Path::new(&cfg.folder).is_dir() {
        return Err("还没有选文件夹：请在电脑上的兔啃里选择让 Claude 干活的文件夹".into());
    }
    let exe = crate::desktop::code_engine(app)?;
    let paths = crate::config_paths(app)?;
    let key = crate::claude_config::desktop_key(&paths)
        .or_else(|| crate::claude_config::cli_key(&paths))
        .ok_or("电脑上的兔啃还没接入 Claude Desktop：请先在首页点「一键接入」")?;
    let models = refresh_models(&paths);
    // The one chosen from the phone, while Claude Desktop still offers it.
    let model = cfg
        .model
        .clone()
        .filter(|m| models.contains(m))
        .or_else(|| models.first().cloned());
    let effort = crate::claude_config::desktop_models(&paths).unwrap_or_default()["effort"]
        .as_str()
        .map(str::to_string);

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
    if cfg.accept_edits {
        args.extend(["--permission-mode".into(), "acceptEdits".into()]);
    }
    if let Some(m) = &model {
        args.extend(["--model".into(), m.clone()]);
    }
    if let Some(e) = &effort {
        args.extend(["--effort".into(), e.clone()]);
    }
    let (resume, fork) = {
        let rt = RT.lock().unwrap();
        (rt.session.clone(), rt.ended)
    };
    // Continue the conversation, unless its record is gone. One that went
    // into Claude Desktop continues in a copy: Claude Desktop shows it as
    // it was when it went in, and may go on with it itself.
    if let Some(s) = resume.filter(|s| transcript_exists(&paths, s)) {
        args.extend(["--resume".into(), s]);
        if fork {
            args.push("--fork-session".into());
        }
    }

    let mut command = Command::new(&exe);
    command
        .args(&args)
        .current_dir(&cfg.folder)
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
        rt.spent = 0.0;
        rt.folder = Some(cfg.folder.clone());
        if rt.ended {
            // A new conversation, in Claude Desktop's eyes.
            rt.ended = false;
            rt.session = None;
        }
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
    let folder = cfg.folder.clone();
    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                output(&a, &e, &folder, &v).await;
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
        Some("assistant") => {
            for block in v["message"]["content"].as_array().into_iter().flatten() {
                match block["type"].as_str() {
                    Some("text") => {
                        let text = block["text"].as_str().unwrap_or("").trim();
                        if !text.is_empty() {
                            event(app, json!({ "kind": "text", "text": clip(text, TEXT_MAX) }));
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
            {
                let mut rt = RT.lock().unwrap();
                if let Some(s) = v["session_id"].as_str() {
                    rt.session = Some(s.to_string());
                }
                rt.last_turn = Some(Instant::now());
            }
            event(app, e);
            set_busy(app, false);
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

/// Claude Desktop's models (the ones 兔啃 set there), kept for the phone.
fn refresh_models(paths: &crate::claude_config::Paths) -> Vec<String> {
    let models: Vec<String> = crate::claude_config::desktop_models(paths)
        .and_then(|m| {
            m["models"].as_array().map(|ms| {
                ms.iter()
                    .filter_map(|m| m.as_str().map(str::to_string))
                    .collect()
            })
        })
        .unwrap_or_default();
    RT.lock().unwrap().models = models.clone();
    models
}

/// Switches the running engine to `model`; a stopped one starts with it.
async fn apply_model(app: &AppHandle, model: &str) {
    let engine = RT.lock().unwrap().engine.clone();
    match engine {
        Some(e) => {
            let id = e.next_id("model");
            let line = json!({ "type": "control_request", "request_id": id, "request": { "subtype": "set_model", "model": model } });
            if e.write(&line).await.is_ok() {
                RT.lock().unwrap().model = Some(model.to_string());
                note(app, format!("已切换到 {model}"));
            }
        }
        None => {
            RT.lock().unwrap().model = Some(model.to_string());
            note(app, format!("之后用 {model}"));
        }
    }
    changed(app);
}

// ---------------------------------------------------------------- Claude Desktop sync

/// Ends the phone conversation: the engine stops, the conversation is queued
/// for Claude Desktop, and the next prompt continues it in a copy. `quiet`:
/// no word about it.
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
            rt.title.take().unwrap_or_default(),
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
/// the phone, and the phone away for a while, or nothing for long.
fn conversation_over(rt: &Runtime, now: Instant) -> bool {
    if rt.session.is_none() || rt.ended || rt.busy || !rt.pending.is_empty() {
        return false;
    }
    let Some(last) = rt.last_turn else {
        return false;
    };
    let quiet = now.duration_since(last);
    quiet >= CONVERSATION_IDLE
        || rt
            .phone_gone
            .is_some_and(|g| now.duration_since(g) >= PHONE_GONE && quiet >= QUIET)
}

/// Puts ended phone conversations into Claude Desktop, one at a time.
async fn sync_loop(app: AppHandle) {
    let mut was_idle = false;
    loop {
        let _ = tokio::time::timeout(SYNC_EVERY, SYNC.notified()).await;
        if let Ok(paths) = crate::config_paths(&app) {
            refresh_models(&paths);
        }
        if conversation_over(&RT.lock().unwrap(), Instant::now()) {
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
        // Claude Desktop jumps to what it imports: not while the computer is in use.
        let idle = pc_idle().await.is_none_or(|d| d >= PC_IDLE);
        let back = was_idle && !idle;
        was_idle = idle;
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
    let projects = std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|_| !cfg!(debug_assertions))
        .map(|d| std::path::PathBuf::from(d).join("projects"))
        .or_else(|| paths.cli_projects());
    let Some(projects) = projects else {
        return false;
    };
    let name = format!("{session}.jsonl");
    std::fs::read_dir(projects)
        .into_iter()
        .flatten()
        .flatten()
        .any(|dir| dir.path().join(&name).is_file())
}

fn is_uuid(s: &str) -> bool {
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

fn clip(s: &str, max: usize) -> String {
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
fn tool_title(name: &str, input: &Value, folder: &str) -> String {
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
    fn a_conversation_is_over_when_the_phone_is_away_or_it_is_long_quiet() {
        let now = Instant::now();
        let ago = |s: u64| now.checked_sub(Duration::from_secs(s));
        let rt = |last: u64, gone: Option<u64>| Runtime {
            session: Some("6f1e1d2c-0000-4000-8000-000000000001".into()),
            last_turn: ago(last),
            phone_gone: gone.and_then(ago),
            ..Default::default()
        };
        // The phone is still there: only a long quiet ends it.
        assert!(!conversation_over(&rt(5 * 60, None), now));
        assert!(conversation_over(&rt(16 * 60, None), now));
        // The phone went away a minute ago and the last turn was a while back.
        assert!(conversation_over(&rt(3 * 60, Some(70)), now));
        assert!(
            !conversation_over(&rt(30, Some(70)), now),
            "a turn just finished"
        );
        assert!(
            !conversation_over(&rt(3 * 60, Some(20)), now),
            "the phone just went"
        );
        // Not while Claude works or waits for an answer, nor twice.
        let mut busy = rt(16 * 60, Some(600));
        busy.busy = true;
        assert!(!conversation_over(&busy, now));
        let mut asking = rt(16 * 60, Some(600));
        asking.pending.insert("r1".into(), json!({}));
        assert!(!conversation_over(&asking, now));
        let mut ended = rt(16 * 60, Some(600));
        ended.ended = true;
        assert!(!conversation_over(&ended, now));
        let mut none = rt(16 * 60, Some(600));
        none.session = None;
        assert!(!conversation_over(&none, now));
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
