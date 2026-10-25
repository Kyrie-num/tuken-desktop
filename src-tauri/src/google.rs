//! Google sign-in in a small window of 兔啃's own. Google's pages load in it,
//! and the server's desktop OAuth flow (RFC 8252) answers on a loopback port
//! with a one-time code, so there is no approval page and no browser tab.
//! If Google refuses to sign in inside an app, the same flow carries on in
//! the browser and still comes back here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::Method;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

use crate::api::{self, SITE};
use crate::State;

pub const LABEL: &str = "tuken-google";
/// Google's sign-in pages are laid out for about this size.
const WIDTH: f64 = 460.0;
const HEIGHT: f64 = 640.0;
const TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// A Google sign-in in progress.
pub struct Flow {
    /// The start of the flow, for the browser to pick up.
    start: String,
    /// Once the browser has it, closing the window no longer cancels.
    in_browser: Arc<AtomicBool>,
    cancel: Arc<Notify>,
}

/// 43 URL-safe characters from two random UUIDs: a PKCE verifier or a state.
fn random() -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(
        format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4()).as_bytes(),
    ))
}

/// `invite`: the code a new account signs up with while sign-up is by
/// invitation only.
fn start_url(port: u16, challenge: &str, state: &str, invite: Option<&str>) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("port", &port.to_string())
        .append_pair("code_challenge", challenge)
        .append_pair("state", state);
    if let Some(code) = invite {
        query.append_pair("invite", code);
    }
    format!(
        "{SITE}/api/v1/desktop/oauth/google/start?{}",
        query.finish()
    )
}

/// How a Google sign-in came back.
#[derive(Debug, PartialEq)]
enum Outcome {
    Code(String),
    Cancelled,
    /// A new account, and sign-up needs an invitation code.
    NeedsInvite,
    Failed(String),
}

/// Google's own "this browser or app may not be secure" page.
fn rejected_by_google(u: &url::Url) -> bool {
    u.host_str() == Some("accounts.google.com")
        && (u.path().contains("/signin/rejected")
            || u.query()
                .is_some_and(|q| q.contains("disallowed_useragent")))
}

/// What the small window may show: https pages (Google, and a school's own
/// sign-in for Workspace accounts) and the loopback callback.
fn allowed(u: &url::Url, port: u16) -> bool {
    match u.scheme() {
        "https" | "about" => true,
        "http" => u.host_str() == Some("127.0.0.1") && u.port() == Some(port),
        _ => false,
    }
}

/// What the server's error codes mean for the user.
fn failure(error: &str, description: &str) -> Outcome {
    let text = match error {
        "provider_error" if description == "access_denied" => return Outcome::Cancelled,
        "invite_required" => return Outcome::NeedsInvite,
        "invite_invalid" => "邀请码不对，请检查后再试",
        "login_blocked" => "兔啃正在维护，暂时不能登录",
        "registration_unavailable" => "暂时不能用 Google 注册新账户",
        "login_failed" => "这个 Google 账号没能登录兔啃，请换一个试试",
        _ => "Google 登录没有完成，请重试",
    };
    Outcome::Failed(text.into())
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>兔啃</title>\
<body style=\"margin:0;height:100vh;display:grid;place-items:center;\
font:16px system-ui,'Microsoft YaHei UI',sans-serif;color:#003F43\">\
<p>已经登录，可以回到兔啃了。</p>";

async fn reply(stream: &mut TcpStream, status: &str) {
    let body = if status.starts_with("200") {
        DONE_PAGE
    } else {
        ""
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// The request line's target of one HTTP request, e.g. `/callback?code=…`.
async fn request_target(stream: &mut TcpStream) -> Option<String> {
    let mut buf = vec![0u8; 8192];
    let mut len = 0;
    let read = tokio::time::timeout(Duration::from_secs(5), async {
        while len < buf.len() {
            let n = stream.read(&mut buf[len..]).await.ok()?;
            if n == 0 {
                break;
            }
            len += n;
            if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        Some(())
    })
    .await;
    read.ok().flatten()?;
    let head = String::from_utf8_lossy(&buf[..len]);
    let mut line = head.lines().next()?.split(' ');
    (line.next()? == "GET").then(|| line.next().map(str::to_string))?
}

/// What came back to `/callback`; `None` for requests that aren't ours.
fn read_callback(target: &str, state: &str) -> Option<Outcome> {
    let url = url::Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/callback" {
        return None;
    }
    let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
    // Anything without this flow's state is someone else's request.
    if q.get("state").map(String::as_str) != Some(state) {
        return None;
    }
    if let Some(code) = q.get("code").filter(|c| !c.is_empty()) {
        return Some(Outcome::Code(code.clone()));
    }
    let error = q.get("error").map(String::as_str).unwrap_or_default();
    let description = q
        .get("error_description")
        .map(String::as_str)
        .unwrap_or_default();
    Some(failure(error, description))
}

/// Waits for the server to send the browser back to the loopback port.
async fn callback(listener: TcpListener, state: &str) -> Result<Outcome, String> {
    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|_| "Google 登录没有完成，请重试".to_string())?;
        let Some(target) = request_target(&mut stream).await else {
            reply(&mut stream, "400 Bad Request").await;
            continue;
        };
        match read_callback(&target, state) {
            Some(outcome) => {
                reply(&mut stream, "200 OK").await;
                return Ok(outcome);
            }
            None => reply(&mut stream, "404 Not Found").await,
        }
    }
}

fn close_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.close();
    }
}

/// Hands the flow to the browser: the same start URL, the same loopback port.
fn to_browser(app: &AppHandle, start: &str, in_browser: &AtomicBool) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    in_browser.store(true, Ordering::SeqCst);
    app.opener()
        .open_url(start, None::<&str>)
        .map_err(|_| "无法打开浏览器".to_string())?;
    let _ = app.emit("tuken://google", "browser");
    // Not from inside a navigation handler: close on the next turn.
    let app = app.clone();
    tauri::async_runtime::spawn(async move { close_window(&app) });
    Ok(())
}

/// Over the middle of the main window, in logical pixels.
fn centered_over(main: &tauri::WebviewWindow) -> Option<(f64, f64)> {
    let scale = main.scale_factor().ok()?;
    let pos = main.outer_position().ok()?.to_logical::<f64>(scale);
    let size = main.outer_size().ok()?.to_logical::<f64>(scale);
    Some((
        pos.x + (size.width - WIDTH) / 2.0,
        (pos.y + (size.height - HEIGHT) / 2.0).max(0.0),
    ))
}

fn open_window(app: &AppHandle, flow: &Flow, port: u16) -> Result<(), String> {
    let url = url::Url::parse(&flow.start).map_err(|_| "登录地址无效")?;
    let browser = app.clone();
    let start = flow.start.clone();
    let in_browser = flow.in_browser.clone();
    // Google's pages and the title bar follow 兔啃's theme, not Windows'.
    let dark = app.state::<State>().dark.load(Ordering::SeqCst);
    let mut builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(url))
        .title("使用 Google 登录")
        .theme(Some(if dark {
            tauri::Theme::Dark
        } else {
            tauri::Theme::Light
        }))
        .inner_size(WIDTH, HEIGHT)
        .min_inner_size(380.0, 520.0)
        .on_navigation(move |u| {
            if rejected_by_google(u) {
                let _ = to_browser(&browser, &start, &in_browser);
                return false;
            }
            allowed(u, port)
        });
    if let Some(main) = app.get_webview_window("main") {
        if let Some((x, y)) = centered_over(&main) {
            builder = builder.position(x, y);
        }
        // Owned by the main window: stays above it, goes away with it.
        builder = builder.parent(&main).map_err(|_| "无法打开登录窗口")?;
    }
    let window = builder.build().map_err(|_| "无法打开登录窗口")?;
    let in_browser = flow.in_browser.clone();
    let cancel = flow.cancel.clone();
    window.on_window_event(move |e| {
        if matches!(e, tauri::WindowEvent::Destroyed) && !in_browser.load(Ordering::SeqCst) {
            cancel.notify_one();
        }
    });
    Ok(())
}

/// Ends the flow however it ends: no flow left, no window left.
struct Done(AppHandle);

impl Drop for Done {
    fn drop(&mut self) {
        self.0.state::<State>().google.lock().unwrap().take();
        close_window(&self.0);
    }
}

/// Signs in with Google in the small window. Resolves when it is over:
/// `{status: "approved", user_id}`, `{status: "cancelled"}`, or
/// `{status: "invite_required"}` for a new account without an invitation.
pub async fn sign_in(
    app: &AppHandle,
    state: &State,
    invite: Option<String>,
) -> Result<Value, String> {
    if state.google.lock().unwrap().is_some() {
        if let Some(w) = app.get_webview_window(LABEL) {
            let _ = w.set_focus();
        }
        return Err("Google 登录已经打开了".into());
    }
    let verifier = random();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let nonce = random();
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|_| "无法开始 Google 登录")?;
    let port = listener
        .local_addr()
        .map_err(|_| "无法开始 Google 登录")?
        .port();
    let flow = Flow {
        start: start_url(port, &challenge, &nonce, invite.as_deref()),
        in_browser: Arc::new(AtomicBool::new(false)),
        cancel: Arc::new(Notify::new()),
    };
    let cancel = flow.cancel.clone();
    open_window(app, &flow, port)?;
    *state.google.lock().unwrap() = Some(flow);
    let _done = Done(app.clone());

    let outcome = tokio::select! {
        r = callback(listener, &nonce) => r?,
        _ = cancel.notified() => Outcome::Cancelled,
        _ = tokio::time::sleep(TIMEOUT) => return Err("Google 登录超时了，请重试".into()),
    };
    let code = match outcome {
        Outcome::Code(code) => code,
        Outcome::Cancelled => return Ok(json!({ "status": "cancelled" })),
        Outcome::NeedsInvite => return Ok(json!({ "status": "invite_required" })),
        Outcome::Failed(text) => return Err(text),
    };
    close_window(app);
    let issued = api::desktop(
        None,
        "/oauth/token",
        Method::POST,
        Some(json!({ "code": code, "code_verifier": verifier })),
    )
    .await?;
    crate::account::signed_in(app, state, &issued).await
}

/// Stops the sign-in in progress (the window's own close button does too).
pub fn cancel(state: &State) {
    if let Some(flow) = state.google.lock().unwrap().as_ref() {
        flow.cancel.notify_one();
    }
}

/// Moves the sign-in in progress to the browser.
pub fn browser(app: &AppHandle, state: &State) -> Result<(), String> {
    let (start, in_browser) = match state.google.lock().unwrap().as_ref() {
        Some(flow) => (flow.start.clone(), flow.in_browser.clone()),
        None => return Err("Google 登录已经结束了".into()),
    };
    to_browser(app, &start, &in_browser)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_the_desktop_flow_with_pkce_and_the_loopback_port() {
        let url = url::Url::parse(&start_url(53682, "ch", "st", None)).unwrap();
        assert_eq!(url.host_str(), Some("www.tuken.site"));
        assert_eq!(url.path(), "/api/v1/desktop/oauth/google/start");
        let q: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["port"], "53682");
        assert_eq!(q["code_challenge"], "ch");
        assert_eq!(q["state"], "st");
        assert!(!q.contains_key("invite"));
        let with = url::Url::parse(&start_url(53682, "ch", "st", Some("XM2026"))).unwrap();
        assert!(with
            .query_pairs()
            .any(|(k, v)| k == "invite" && v == "XM2026"));
        // What the server accepts: 43 URL-safe characters.
        let v = random();
        assert_eq!(v.len(), 43);
        assert!(v
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
    }

    #[test]
    fn reads_only_this_flows_callback() {
        assert_eq!(
            read_callback("/callback?code=abc&state=s1", "s1"),
            Some(Outcome::Code("abc".into()))
        );
        // Another state, another path: not ours, keep waiting.
        assert_eq!(read_callback("/callback?code=abc&state=s2", "s1"), None);
        assert_eq!(read_callback("/favicon.ico", "s1"), None);
        // Cancelling at Google is no error; a refusal says what happened.
        assert_eq!(
            read_callback(
                "/callback?error=provider_error&error_description=access_denied&state=s1",
                "s1"
            ),
            Some(Outcome::Cancelled)
        );
        assert_eq!(
            read_callback("/callback?error=login_blocked&state=s1", "s1"),
            Some(Outcome::Failed("兔啃正在维护，暂时不能登录".into()))
        );
        // Invitation-only sign-up: ask for a code, then try again.
        assert_eq!(
            read_callback("/callback?error=invite_required&state=s1", "s1"),
            Some(Outcome::NeedsInvite)
        );
    }

    #[test]
    fn the_window_shows_https_and_its_own_callback_only() {
        let ok = |s: &str| allowed(&url::Url::parse(s).unwrap(), 53682);
        assert!(ok("https://accounts.google.com/o/oauth2/v2/auth?x=1"));
        assert!(ok("https://sso.sustech.edu.cn/login"));
        assert!(ok("http://127.0.0.1:53682/callback?code=a&state=b"));
        assert!(!ok("http://127.0.0.1:8080/callback"));
        assert!(!ok("http://example.com/"));
        assert!(!ok("file:///C:/Windows/win.ini"));
        assert!(rejected_by_google(
            &url::Url::parse("https://accounts.google.com/v3/signin/rejected?x=1").unwrap()
        ));
        assert!(!rejected_by_google(
            &url::Url::parse("https://accounts.google.com/v3/signin/identifier").unwrap()
        ));
    }
}
