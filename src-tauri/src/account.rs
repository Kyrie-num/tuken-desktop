//! Everything the UI asks of the signed-in account: sign-in and switching,
//! profile, keys, models, bills, and pointing Claude Desktop / Claude Code at
//! a key. Requests run without holding the vault, so pages load in parallel.

use std::time::{Duration, Instant};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::Method;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::api::{self, str_field, SITE};
use crate::claude_config;
use crate::vault::{Account, Login};
use crate::State;

const PLAZA_TTL: Duration = Duration::from_secs(10 * 60);

fn positive_id(v: &Value) -> Result<i64, String> {
    v.as_i64()
        .filter(|x| *x > 0)
        .ok_or_else(|| "编号无效".into())
}

/// The sign-in page is done with (signed in, cancelled or replaced).
fn close_login_window(app: &AppHandle) {
    crate::panel::finish(app);
}

/// A desktop API call as `account`. A token the server rejects is dropped, so
/// the UI asks that account to sign in again instead of failing on every page.
async fn call(
    state: &State,
    account: &Account,
    path: &str,
    method: Method,
    body: Option<Value>,
) -> Result<Value, String> {
    let r = api::desktop(Some(&account.token), path, method, body).await;
    if matches!(&r, Err(e) if e == api::EXPIRED) {
        let mut v = state.vault().await?;
        v.remove(account.user_id);
        state.save(&v)?;
    }
    r
}

/// `call` for a feature the server may not have yet (`None`: not there).
async fn call_optional(
    state: &State,
    account: &Account,
    path: &str,
    method: Method,
    body: Option<Value>,
) -> Result<Option<Value>, String> {
    let r = api::desktop_optional(&account.token, path, method, body).await;
    if matches!(&r, Err(e) if e == api::EXPIRED) {
        let mut v = state.vault().await?;
        v.remove(account.user_id);
        state.save(&v)?;
    }
    r
}

/// An announcement as the UI shows it; Markdown text, capped.
fn announcement_view(a: &Value) -> Value {
    let content: String = a["content"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(20_000)
        .collect();
    json!({
        "id": a["id"],
        "title": a["title"].as_str().unwrap_or_default(),
        "content": content,
        "popup": a["notify_mode"] == "popup",
        "read": !a["read_at"].is_null(),
        "created_at": a["created_at"],
    })
}

/// The site's contact details, from its public settings. Not cached: an
/// admin's edit shows the next time the sheet opens.
/// The site's public settings: contact details, the sign-in providers the
/// admin has switched on.
async fn public_settings() -> Result<Value, String> {
    let r = api::send(
        &format!("{SITE}/api/v1/settings/public"),
        Method::GET,
        None,
        None,
    )
    .await?;
    Ok(r["data"].clone())
}

async fn site() -> Result<Value, String> {
    let s = public_settings().await?;
    let text = |k: &str| s[k].as_str().unwrap_or_default().trim().to_string();
    Ok(json!({ "contact": text("contact_info") }))
}

/// `YYYY-MM`, the month of a check-in calendar.
fn month(input: &Value) -> Result<String, String> {
    let m = input["month"].as_str().unwrap_or_default();
    let ok = m.len() == 7
        && m.as_bytes()[4] == b'-'
        && m[..4].bytes().all(|b| b.is_ascii_digit())
        && m[5..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n));
    if ok {
        Ok(m.to_string())
    } else {
        Err("月份不对".into())
    }
}

fn query(pairs: &[(&str, String)]) -> String {
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        if !v.is_empty() {
            q.append_pair(k, v);
        }
    }
    q.finish()
}

fn date(input: &Value, field: &str) -> String {
    input[field]
        .as_str()
        .filter(|d| d.len() == 10 && d.bytes().all(|b| b.is_ascii_digit() || b == b'-'))
        .unwrap_or("")
        .to_string()
}

fn page(input: &Value) -> String {
    input["page"]
        .as_u64()
        .unwrap_or(1)
        .clamp(1, 100_000)
        .to_string()
}

fn paged(data: &Value, row: impl Fn(&Value) -> Value) -> Value {
    json!({
        "items": data["items"].as_array().map(|items| items.iter().map(row).collect::<Vec<_>>()).unwrap_or_default(),
        "total": data["total"],
        "page": data["page"],
        "pages": data["pages"],
    })
}

fn profile_view(me: &Value) -> Value {
    json!({
        "id": me["id"],
        "email": me["email"],
        "username": me["username"],
        "avatar_url": me["avatar_url"],
        "balance": me["balance"],
        "frozen_balance": me["frozen_balance"],
        "total_recharged": me["total_recharged"],
        "concurrency": me["concurrency"],
        "created_at": me["created_at"],
    })
}

fn group_view(g: &Value) -> Value {
    json!({
        "id": g["id"],
        "name": g["name"],
        "description": g["description"],
        "platform": g["platform"],
        "rate_multiplier": g["rate_multiplier"],
        "subscription_type": g["subscription_type"],
        "is_exclusive": g["is_exclusive"],
    })
}

/// Keys without their secrets; `in_desktop` / `in_cli` say which client sends it.
/// A key as the list shows it until asked: its start and end only, and as
/// long as the key itself, so showing the whole one moves nothing.
fn masked(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 12 {
        return "•".repeat(chars.len());
    }
    let head: String = chars[..7].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}{}{tail}", "•".repeat(chars.len() - 11))
}

fn key_view(k: &Value, desktop: Option<&str>, cli: Option<&str>) -> Value {
    let secret = k["key"].as_str();
    json!({
        "masked": secret.map(masked),
        "id": k["id"],
        "name": k["name"],
        "status": k["status"],
        "group_id": k["group_id"],
        "group": k.get("group").filter(|g| g.is_object()).map(|g| json!({
            "id": g["id"], "name": g["name"], "platform": g["platform"],
        })),
        "quota": k["quota"],
        "quota_used": k["quota_used"],
        "last_used_at": k["last_used_at"],
        "created_at": k["created_at"],
        "expires_at": k["expires_at"],
        "in_desktop": secret.is_some() && secret == desktop,
        "in_cli": secret.is_some() && secret == cli,
    })
}

/// One usage line: the nested key object carries the full secret and stays here.
fn usage_view(u: &Value) -> Value {
    json!({
        "id": u["id"],
        "model": u["model"],
        "api_key_id": u["api_key_id"],
        "key_name": u["api_key"]["name"],
        "input_tokens": u["input_tokens"],
        "output_tokens": u["output_tokens"],
        "cache_creation_tokens": u["cache_creation_tokens"],
        "cache_read_tokens": u["cache_read_tokens"],
        "actual_cost": u["actual_cost"],
        "duration_ms": u["duration_ms"],
        "first_token_ms": u["first_token_ms"],
        "stream": u["stream"],
        "created_at": u["created_at"],
    })
}

/// A credit line; the redemption code itself never leaves.
fn history_view(r: &Value) -> Value {
    json!({
        "id": r["id"],
        "type": r["type"],
        "value": r["value"],
        "status": r["status"],
        "used_at": r["used_at"],
        "created_at": r["created_at"],
        "notes": r["notes"],
    })
}

/// The public model plaza (groups with their models and prices), when the
/// server has it switched on. Remembered for a while either way.
async fn plaza(state: &State) -> Option<Value> {
    if let Some((at, data)) = &state.cache.lock().unwrap().plaza {
        if at.elapsed() < PLAZA_TTL {
            return data.clone();
        }
    }
    // Sent without the desktop token: the plaza rejects tokens it can't parse.
    let data = api::send(
        &format!("{SITE}/api/v1/model-plaza"),
        Method::GET,
        None,
        None,
    )
    .await
    .ok()
    .filter(|d| d["code"].as_i64() == Some(0))
    .map(|d| d["data"].clone());
    state.cache.lock().unwrap().plaza = Some((Instant::now(), data.clone()));
    data
}

/// Models a group serves: from the plaza, else asked of the gateway with one
/// of the account's keys in that group.
async fn models(state: &State, account: &Account, group_id: i64) -> Result<Value, String> {
    if let Some(plaza) = plaza(state).await {
        if let Some(g) = plaza["groups"]
            .as_array()
            .and_then(|gs| gs.iter().find(|g| g["id"].as_i64() == Some(group_id)))
        {
            let models = g["models"]
                .as_array()
                .map(|ms| {
                    ms.iter()
                        .map(|m| {
                            json!({
                                "id": m["name"],
                                "name": m["name"],
                                "input_price": m["pricing"]["input_price"],
                                "output_price": m["pricing"]["output_price"],
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            return Ok(json!({ "source": "plaza", "models": models }));
        }
    }
    let keys = call(
        state,
        account,
        &format!("/keys?page=1&page_size=50&group_id={group_id}&status=active"),
        Method::GET,
        None,
    )
    .await?;
    let key = keys["items"].as_array().and_then(|items| {
        items
            .iter()
            .find(|k| k["group_id"].as_i64() == Some(group_id) && k["status"] == "active")
            .and_then(|k| k["key"].as_str().map(str::to_string))
    });
    let Some(key) = key else {
        return Ok(json!({ "source": "none", "models": [] }));
    };
    let data = match api::gateway("/v1/models", &key).await {
        Ok(data) => data,
        // Without credit the gateway refuses even the model list.
        Err(r) if r.reason == "INSUFFICIENT_BALANCE" => {
            return Ok(json!({ "source": "balance", "models": [] }))
        }
        Err(r) => return Err(r.message),
    };
    let models = data["data"]
        .as_array()
        .map(|ms| {
            ms.iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?;
                    Some(json!({ "id": id, "name": m["display_name"].as_str().unwrap_or(id) }))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(json!({ "source": "key", "models": models }))
}

/// Points a client at key `id` of the account, creating a key first when no
/// id is given.
async fn connect(
    app: &AppHandle,
    state: &State,
    account: &Account,
    input: &Value,
) -> Result<Value, String> {
    let target = input["app"].as_str().unwrap_or("desktop");
    if !matches!(target, "desktop" | "cli") {
        return Err("只能接入 Claude Desktop 或 Claude Code".into());
    }
    let id = match input["id"].as_i64().filter(|x| *x > 0) {
        Some(id) => id,
        None => {
            let name = input["name"]
                .as_str()
                .map(str::trim)
                .filter(|n| !n.is_empty() && n.chars().count() <= 100)
                .unwrap_or(if target == "desktop" {
                    "Claude Desktop"
                } else {
                    "Claude Code"
                });
            let created = call(
                state,
                account,
                "/keys",
                Method::POST,
                Some(json!({ "name": name, "group_id": positive_id(&input["group_id"])? })),
            )
            .await?;
            positive_id(&created["id"]).map_err(|_| "服务没有返回新密钥".to_string())?
        }
    };
    let key = call(state, account, &format!("/keys/{id}"), Method::GET, None).await?;
    if key["status"] != "active" {
        return Err("这个密钥已停用，请先启用".into());
    }
    let secret = str_field(&key, "key")?.to_string();
    let paths = crate::config_paths(app)?;
    let target = target.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        if target == "desktop" {
            claude_config::connect_desktop(&paths, &secret)
        } else {
            claude_config::connect_cli(&paths, &secret)
        }
    })
    .await
    .map_err(|_| "写入配置失败".to_string())??;
    Ok(json!({ "key_id": id }))
}

async fn login_start(app: &AppHandle, state: &State, input: &Value) -> Result<Value, String> {
    let verifier = URL_SAFE_NO_PAD.encode(Sha256::digest(
        format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4()).as_bytes(),
    ));
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let result = api::desktop(
        None,
        "/device/start",
        Method::POST,
        Some(json!({ "code_challenge": challenge })),
    )
    .await?;
    let code = str_field(&result, "user_code")?.to_string();
    let uri = str_field(&result, "verification_uri")?.to_string();
    if uri != format!("{SITE}/desktop-authorize?code={code}")
        || code.len() != 10
        || !code.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err("授权地址不正确".into());
    }
    {
        let mut v = state.vault().await?;
        v.login = Some(Login {
            device: str_field(&result, "device_code")?.into(),
            verifier,
            code: code.clone(),
            expires: crate::shop::now() + 600,
        });
        state.save(&v)?;
    }
    close_login_window(app);
    // Shown inside the main window, in a private web profile so another
    // account can sign in. The page gets no IPC; only the PKCE poll below
    // carries a session.
    let url = url::Url::parse(&format!("{uri}&embedded=1")).map_err(|_| "登录地址无效")?;
    crate::panel::set(
        app,
        crate::panel::Target {
            register: input["register"] == true,
            url,
        },
    );
    Ok(json!({ "user_code": code, "expires_in": 600 }))
}

async fn login_poll(app: &AppHandle, state: &State) -> Result<Value, String> {
    let login = state.vault().await?.login.clone().ok_or("请重新登录")?;
    if crate::shop::now() > login.expires {
        let mut v = state.vault().await?;
        v.login = None;
        state.save(&v)?;
        return Err("登录已超时，请重新登录".into());
    }
    let r = api::desktop(
        None,
        "/device/token",
        Method::POST,
        Some(json!({ "device_code": login.device, "code_verifier": login.verifier })),
    )
    .await?;
    if r["status"] == "pending" {
        return Ok(json!({ "status": "pending" }));
    }
    signed_in(app, state, &r).await
}

/// A desktop session was issued (`{access_token, user_id}`): keep the account
/// and make it the current one.
pub(crate) async fn signed_in(
    app: &AppHandle,
    state: &State,
    issued: &Value,
) -> Result<Value, String> {
    let token = str_field(issued, "access_token")?.to_string();
    let user_id = issued["user_id"].as_i64().ok_or("授权响应无效")?;
    let me = api::desktop(Some(&token), "/me", Method::GET, None).await?;
    if me["id"].as_i64() != Some(user_id) {
        return Err("授权账户不一致".into());
    }
    {
        let mut v = state.vault().await?;
        v.upsert(Account {
            token,
            user_id,
            email: me["email"].as_str().unwrap_or_default().into(),
            username: me["username"].as_str().unwrap_or_default().into(),
        });
        v.active = Some(user_id);
        v.login = None;
        state.save(&v)?;
    }
    close_login_window(app);
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    }
    Ok(json!({ "status": "approved", "user_id": user_id }))
}

fn email(input: &Value) -> Result<String, String> {
    let e = input["email"].as_str().unwrap_or("").trim().to_lowercase();
    let ok = e.len() <= 254
        && e.split_once('@').is_some_and(|(user, host)| {
            !user.is_empty() && host.contains('.') && !e.contains(char::is_whitespace)
        });
    if ok {
        Ok(e)
    } else {
        Err("请输入正确的邮箱".into())
    }
}

/// The rules the sign-up screen follows, from the server's sign-in config. The
/// names match the site's public settings.
/// `public`: the site's public settings, for Google sign-in's switch in the
/// admin. Without them Google stays hidden.
fn auth_rules(c: &Value, public: Option<&Value>) -> Value {
    let suffixes: Vec<&str> = c["registration_email_suffix_whitelist"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    json!({
        "available": true,
        "registration_enabled": c["registration_enabled"].as_bool().unwrap_or(true),
        "invitation_code_enabled": c["invitation_code_enabled"].as_bool().unwrap_or(false),
        "password_reset_enabled": c["password_reset_enabled"].as_bool().unwrap_or(false),
        "password_min_length": c["password_min_length"].as_u64().unwrap_or(6).clamp(1, 72),
        "email_suffixes": suffixes,
        "invite_enabled": c["invite_enabled"].as_bool().unwrap_or(false),
        "invite_required": c["invite_required"].as_bool().unwrap_or(false),
        "google": public.is_some_and(|p| p["google_oauth_enabled"] == true),
    })
}

/// A friend's invitation code as typed: short, letters, digits, `-` and `_`.
fn invite_code(input: &Value) -> Option<String> {
    input["affiliate_code"]
        .as_str()
        .map(str::trim)
        .filter(|c| {
            !c.is_empty()
                && c.len() <= 32
                && c.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        .map(str::to_string)
}

/// A new password, checked against the server's minimum. bcrypt reads at most
/// 72 bytes, so longer ones are refused rather than silently cut.
fn new_password(input: &Value, min: usize) -> Result<String, String> {
    let p = input["password"].as_str().unwrap_or("");
    if p.chars().count() < min {
        return Err(format!("密码至少 {min} 位"));
    }
    if p.len() > 72 {
        return Err("密码太长了，请控制在 72 个字符以内".into());
    }
    Ok(p.to_string())
}

/// The ticket of a sign-in that still needs the two-step code, if it does.
fn two_step_ticket(r: &Value) -> Option<String> {
    (r["requires_2fa"] == true || r["status"] == "totp_required")
        .then(|| r["temp_token"].as_str().map(str::to_string))
        .flatten()
}

/// The native sign-in, sign-up and password screens. Refusals come back as
/// `{status: "refused", reason, message}` so a screen can react to the reason.
async fn auth_action(
    app: &AppHandle,
    state: &State,
    action: &str,
    input: &Value,
) -> Result<Value, String> {
    let refused =
        |r: api::Refusal| json!({ "status": "refused", "reason": r.reason, "message": r.message });
    let cached = || {
        state
            .cache
            .lock()
            .unwrap()
            .auth
            .as_ref()
            .filter(|(at, _)| at.elapsed() < Duration::from_secs(5 * 60))
            .map(|(_, data)| data.clone())
    };
    match action {
        "auth_config" => {
            if let Some(data) = cached() {
                return Ok(data);
            }
            // Until the server has the desktop sign-in API, sign-in uses the site.
            let (config, public) =
                tokio::join!(api::auth(Method::GET, "/config", None), public_settings());
            let data = match config {
                Ok(c) => auth_rules(&c, public.ok().as_ref()),
                Err(r) if r.reason == "NOT_SUPPORTED" => json!({ "available": false }),
                // A network failure is no answer: ask again next time.
                Err(_) => return Ok(json!({ "available": false, "offline": true })),
            };
            state.cache.lock().unwrap().auth = Some((Instant::now(), data.clone()));
            Ok(data)
        }
        "auth_login" => {
            let body = json!({ "email": email(input)?, "password": str_field(input, "password")? });
            match api::auth(Method::POST, "/login", Some(body)).await {
                Ok(r) => match two_step_ticket(&r) {
                    Some(ticket) => {
                        *state.totp.lock().unwrap() = Some(ticket);
                        Ok(json!({ "status": "totp_required" }))
                    }
                    None => signed_in(app, state, &r).await,
                },
                Err(r) => Ok(refused(r)),
            }
        }
        "auth_totp" => {
            let ticket = state.totp.lock().unwrap().clone().ok_or("请重新登录")?;
            let code = input["code"].as_str().unwrap_or("").trim();
            if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
                return Err("两步验证码是 6 位数字".into());
            }
            let body = json!({ "temp_token": ticket, "totp_code": code });
            match api::auth(Method::POST, "/login/2fa", Some(body)).await {
                Ok(r) => {
                    *state.totp.lock().unwrap() = None;
                    signed_in(app, state, &r).await
                }
                Err(r) => Ok(refused(r)),
            }
        }
        "auth_send_code" => {
            let path = if input["purpose"] == "reset" {
                "/password/code"
            } else {
                "/register/code"
            };
            match api::auth(Method::POST, path, Some(json!({ "email": email(input)? }))).await {
                Ok(r) => {
                    let wait = r["countdown"].as_u64().or(r["cooldown"].as_u64());
                    Ok(json!({ "status": "sent", "cooldown": wait.unwrap_or(60).min(600) }))
                }
                Err(r) => Ok(refused(r)),
            }
        }
        "auth_register" | "auth_reset" => {
            let min = cached()
                .and_then(|c| c["password_min_length"].as_u64())
                .unwrap_or(6) as usize;
            let password = new_password(input, min)?;
            let code = input["code"].as_str().unwrap_or("").trim();
            if code.is_empty() {
                return Err("请填写邮件里的验证码".into());
            }
            let (path, body) = if action == "auth_reset" {
                (
                    "/password/reset",
                    json!({ "email": email(input)?, "verify_code": code, "new_password": password }),
                )
            } else {
                let mut body =
                    json!({ "email": email(input)?, "verify_code": code, "password": password });
                if let Some(invite) = input["invitation_code"]
                    .as_str()
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                {
                    body["invitation_code"] = json!(invite);
                }
                if let Some(code) = invite_code(input) {
                    body["affiliate_code"] = json!(code);
                }
                ("/register", body)
            };
            match api::auth(Method::POST, path, Some(body)).await {
                Ok(r) => signed_in(app, state, &r).await,
                Err(r) => Ok(refused(r)),
            }
        }
        _ => Err("不支持的操作".into()),
    }
}

#[tauri::command]
pub async fn tuken(
    app: AppHandle,
    state: tauri::State<'_, State>,
    action: String,
    input: Option<Value>,
) -> Result<Value, String> {
    let state = state.inner();
    let input = input.unwrap_or_else(|| json!({}));
    match action.as_str() {
        "state" => Ok(state.vault().await?.view()),
        "login_start" => login_start(&app, state, &input).await,
        "login_poll" => login_poll(&app, state).await,
        "login_cancel" => {
            let mut v = state.vault().await?;
            v.login = None;
            state.save(&v)?;
            close_login_window(&app);
            Ok(v.view())
        }
        "switch" => {
            let id = positive_id(&input["user_id"])?;
            let mut v = state.vault().await?;
            v.account(id).ok_or("这个账户已退出，请重新登录")?;
            v.active = Some(id);
            state.save(&v)?;
            Ok(v.view())
        }
        "auth_config" | "auth_login" | "auth_totp" | "auth_send_code" | "auth_register"
        | "auth_reset" => auth_action(&app, state, &action, &input).await,
        "google_sign_in" => {
            let invite = invite_code(&json!({ "affiliate_code": input["invite"] }));
            crate::google::sign_in(&app, state, invite).await
        }
        "google_cancel" => {
            crate::google::cancel(state);
            Ok(json!({}))
        }
        "google_browser" => {
            crate::google::browser(&app, state)?;
            Ok(json!({}))
        }
        "logout" => {
            let id = match input["user_id"].as_i64() {
                Some(id) => id,
                None => state.current().await?.user_id,
            };
            let account = state.vault().await?.account(id).cloned();
            if let Some(a) = account {
                // Revoke the token on the server too; an already dead one is fine.
                let _ =
                    api::desktop(Some(&a.token), "/logout", Method::POST, Some(json!({}))).await;
            }
            let mut v = state.vault().await?;
            v.remove(id);
            state.save(&v)?;
            Ok(v.view())
        }
        "catalog" | "quote" | "order_create" | "order_pay" | "order_poll" | "order_close"
        | "order_recovery" | "sync" => crate::shop::action(&app, state, &action, &input).await,
        "site" => site().await,
        "update_check" => crate::update::check(&app).await,
        "update_install" => crate::update::install(&app).await,
        "open_link" => {
            use tauri_plugin_opener::OpenerExt;
            let url = api::web_link(input["url"].as_str().unwrap_or_default())
                .ok_or("只能打开 https 网页")?;
            app.opener()
                .open_url(url.as_str(), None::<&str>)
                .map_err(|_| "无法打开浏览器")?;
            Ok(json!({}))
        }
        _ => {
            let account = state.current().await?;
            // The UI caches pages per account; a page asked for another account
            // must not be answered with the current one's data.
            if input["as_user"]
                .as_i64()
                .is_some_and(|uid| uid != account.user_id)
            {
                return Err("账户刚刚切换，请稍候".into());
            }
            account_action(&app, state, &account, &action, &input).await
        }
    }
}

async fn account_action(
    app: &AppHandle,
    state: &State,
    account: &Account,
    action: &str,
    input: &Value,
) -> Result<Value, String> {
    match action {
        "announcements" => {
            let list = call_optional(state, account, "/announcements", Method::GET, None).await?;
            let Some(list) = list else {
                return Ok(json!({ "supported": false, "items": [] }));
            };
            let items: Vec<Value> = list
                .as_array()
                .map(|a| a.iter().take(50).map(announcement_view).collect())
                .unwrap_or_default();
            Ok(json!({ "supported": true, "items": items }))
        }
        "invite" => {
            let Some(i) = call_optional(state, account, "/invite", Method::GET, None).await? else {
                return Ok(json!({ "supported": false }));
            };
            Ok(json!({
                "supported": true,
                "enabled": i["enabled"] == true,
                "code": i["code"].as_str().unwrap_or_default(),
                "reward": api::number(&i["reward"]),
                "invitee_reward": api::number(&i["invitee_reward"]),
                "invited": i["invited"].as_i64().unwrap_or(0),
                "earned": api::number(&i["earned"]).unwrap_or(0.0),
            }))
        }
        "announcement_read" => {
            let path = format!("/announcements/{}/read", positive_id(&input["id"])?);
            call_optional(state, account, &path, Method::POST, Some(json!({}))).await?;
            Ok(json!({}))
        }
        "checkin_status" => {
            let c = call_optional(state, account, "/checkin", Method::GET, None).await?;
            let Some(c) = c else {
                return Ok(json!({ "supported": false }));
            };
            Ok(json!({
                "supported": true,
                "enabled": c["enabled"] == true,
                "checked_in": c["checked_in"] == true,
                "reward_min": api::number(&c["reward_min"]),
                "reward_max": api::number(&c["reward_max"]),
                "streak": c["streak"].as_i64().unwrap_or(0),
            }))
        }
        "checkin_history" => {
            let path = format!("/checkin/history?month={}", month(input)?);
            let Some(h) = call_optional(state, account, &path, Method::GET, None).await? else {
                return Ok(json!({ "supported": false, "days": [] }));
            };
            let days: Vec<Value> = h["days"]
                .as_array()
                .map(|days| {
                    days.iter()
                        .filter_map(|d| {
                            let date = d["date"].as_str().filter(|s| s.len() == 10)?;
                            Some(json!({ "date": date, "reward": api::number(&d["reward"]) }))
                        })
                        .take(31)
                        .collect()
                })
                .unwrap_or_default();
            Ok(json!({ "supported": true, "days": days }))
        }
        "checkin" => {
            let r = call_optional(state, account, "/checkin", Method::POST, Some(json!({})))
                .await?
                .ok_or("暂时还不能签到")?;
            Ok(json!({
                "reward": api::number(&r["reward"]),
                "streak": r["streak"].as_i64().unwrap_or(0),
            }))
        }
        "profile" => {
            let me = call(state, account, "/me", Method::GET, None).await?;
            let (email, username) = (
                me["email"].as_str().unwrap_or_default(),
                me["username"].as_str().unwrap_or_default(),
            );
            if email != account.email || username != account.username {
                let mut v = state.vault().await?;
                if let Some(a) = v.accounts.iter_mut().find(|a| a.user_id == account.user_id) {
                    a.email = email.into();
                    a.username = username.into();
                }
                state.save(&v)?;
            }
            Ok(profile_view(&me))
        }
        "profile_update" => {
            let name = input["username"].as_str().unwrap_or("").trim();
            if name.chars().count() > 50 {
                return Err("昵称最多 50 个字".into());
            }
            let me = call(
                state,
                account,
                "/me",
                Method::PUT,
                Some(json!({ "username": name })),
            )
            .await?;
            Ok(profile_view(&me))
        }
        "password_change" => {
            let old = str_field(input, "old_password")?;
            let new = str_field(input, "new_password")?;
            if new.len() < 8 || new.len() > 72 {
                return Err("新密码需要 8 到 72 个字符".into());
            }
            call(
                state,
                account,
                "/password",
                Method::PUT,
                Some(json!({ "old_password": old, "new_password": new })),
            )
            .await?;
            // The server signs every desktop out after a password change.
            let mut v = state.vault().await?;
            v.remove(account.user_id);
            state.save(&v)?;
            Ok(v.view())
        }
        "dashboard" => call(state, account, "/usage/dashboard", Method::GET, None).await,
        "usage" => {
            let q = query(&[
                ("page", page(input)),
                ("page_size", "20".into()),
                ("start_date", date(input, "start_date")),
                ("end_date", date(input, "end_date")),
                ("timezone", "Asia/Shanghai".into()),
            ]);
            let data = call(state, account, &format!("/usage?{q}"), Method::GET, None).await?;
            Ok(paged(&data, usage_view))
        }
        "usage_stats" => {
            let q = query(&[
                ("start_date", date(input, "start_date")),
                ("end_date", date(input, "end_date")),
                ("timezone", "Asia/Shanghai".into()),
            ]);
            call(
                state,
                account,
                &format!("/usage/stats?{q}"),
                Method::GET,
                None,
            )
            .await
        }
        "recharge_history" => {
            let q = query(&[("page", page(input)), ("page_size", "20".into())]);
            let data = call(
                state,
                account,
                &format!("/redeem/history?{q}"),
                Method::GET,
                None,
            )
            .await?;
            Ok(paged(&data, history_view))
        }
        "groups" => {
            let data = call(state, account, "/groups", Method::GET, None).await?;
            Ok(json!(data
                .as_array()
                .map(|gs| gs.iter().map(group_view).collect::<Vec<_>>())
                .unwrap_or_default()))
        }
        "models" => models(state, account, positive_id(&input["group_id"])?).await,
        "keys" => {
            let data = call(
                state,
                account,
                "/keys?page=1&page_size=100",
                Method::GET,
                None,
            )
            .await?;
            let paths = crate::config_paths(app)?;
            let desktop = claude_config::desktop_key(&paths);
            let cli = claude_config::cli_key(&paths);
            Ok(json!(data["items"]
                .as_array()
                .map(|items| items
                    .iter()
                    .map(|k| key_view(k, desktop.as_deref(), cli.as_deref()))
                    .collect::<Vec<_>>())
                .unwrap_or_default()))
        }
        "key_create" => {
            let name = str_field(input, "name")?.trim();
            if name.chars().count() > 50 {
                return Err("密钥名称最多 50 个字".into());
            }
            let k = call(
                state,
                account,
                "/keys",
                Method::POST,
                Some(json!({ "name": name, "group_id": positive_id(&input["group_id"])? })),
            )
            .await?;
            Ok(key_view(&k, None, None))
        }
        "key_update" => {
            let id = positive_id(&input["id"])?;
            let mut body = json!({});
            if let Some(name) = input["name"].as_str() {
                let name = name.trim();
                if name.is_empty() || name.chars().count() > 50 {
                    return Err("密钥名称需要 1 到 50 个字".into());
                }
                body["name"] = json!(name);
            }
            if let Some(status) = input["status"].as_str() {
                if !matches!(status, "active" | "inactive") {
                    return Err("密钥状态无效".into());
                }
                body["status"] = json!(status);
            }
            let k = call(
                state,
                account,
                &format!("/keys/{id}"),
                Method::PUT,
                Some(body),
            )
            .await?;
            Ok(key_view(&k, None, None))
        }
        "key_delete" => {
            let id = positive_id(&input["id"])?;
            call(state, account, &format!("/keys/{id}"), Method::DELETE, None).await?;
            Ok(json!({}))
        }
        "key_copy" => {
            let id = positive_id(&input["id"])?;
            let k = call(state, account, &format!("/keys/{id}"), Method::GET, None).await?;
            Ok(json!({ "key": str_field(&k, "key")? }))
        }
        "connect" => connect(app, state, account, input).await,
        "disconnect" => {
            let paths = crate::config_paths(app)?;
            let cli = input["app"] == "cli";
            tauri::async_runtime::spawn_blocking(move || {
                if cli {
                    claude_config::disconnect_cli(&paths)
                } else {
                    claude_config::disconnect_desktop(&paths)
                }
            })
            .await
            .map_err(|_| "写入配置失败".to_string())??;
            Ok(json!({}))
        }
        _ => Err("不支持的操作".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_never_carry_their_secret() {
        let k = json!({"id": 7, "name": "Desk", "key": "sk-secret", "status": "active",
            "group_id": 3, "group": {"id": 3, "name": "Claude", "platform": "anthropic", "rate_multiplier": 1}});
        let v = key_view(&k, Some("sk-secret"), None);
        assert!(!v.to_string().contains("sk-secret"));
        assert_eq!(v["in_desktop"], true);
        assert_eq!(v["in_cli"], false);
        assert_eq!(v["group"]["platform"], "anthropic");
        // A missing secret never matches "no client configured".
        let v = key_view(&json!({"id": 8}), None, None);
        assert_eq!(v["in_desktop"], false);
    }

    #[test]
    fn usage_and_history_rows_drop_secrets() {
        let u = json!({"id": 1, "model": "claude-x", "api_key": {"name": "Desk", "key": "sk-secret"},
            "user": {"email": "a@b"}, "actual_cost": 0.1});
        let v = usage_view(&u);
        assert!(!v.to_string().contains("sk-secret"));
        assert!(!v.to_string().contains("a@b"));
        assert_eq!(v["key_name"], "Desk");
        let r =
            history_view(&json!({"id": 1, "code": "CODE-SECRET", "type": "balance", "value": 10}));
        assert!(!r.to_string().contains("CODE-SECRET"));
    }

    #[test]
    fn emails_are_checked_before_any_request() {
        assert_eq!(
            email(&json!({"email": " Me@Mail.SUSTech.edu.cn "})).unwrap(),
            "me@mail.sustech.edu.cn"
        );
        for bad in ["", "me", "me@host", "@mail.example", "me @mail.example"] {
            assert!(email(&json!({ "email": bad })).is_err(), "{bad}");
        }
    }

    #[test]
    fn sign_up_follows_the_server_rules() {
        let config = json!({
            "registration_enabled": true,
            "password_min_length": 6,
            "registration_email_suffix_whitelist": ["@mail.sustech.edu.cn", 3],
        });
        let rules = auth_rules(&config, Some(&json!({"google_oauth_enabled": true})));
        assert_eq!(rules["available"], true);
        assert_eq!(rules["google"], true);
        // Google shows only when the admin has it on, and we know it.
        let off = json!({"google_oauth_enabled": false});
        assert_eq!(auth_rules(&config, Some(&off))["google"], false);
        assert_eq!(auth_rules(&config, None)["google"], false);
        assert_eq!(rules["password_reset_enabled"], false);
        assert_eq!(rules["email_suffixes"], json!(["@mail.sustech.edu.cn"]));

        assert!(new_password(&json!({"password": "12345"}), 6).is_err());
        assert_eq!(
            new_password(&json!({"password": "123456"}), 6).unwrap(),
            "123456"
        );
        assert!(new_password(&json!({"password": "密".repeat(25)}), 6).is_err());
    }

    #[test]
    fn two_step_sign_in_is_recognized_in_either_shape() {
        let site = json!({"requires_2fa": true, "temp_token": "t1"});
        let spec = json!({"status": "totp_required", "temp_token": "t2"});
        assert_eq!(two_step_ticket(&site).as_deref(), Some("t1"));
        assert_eq!(two_step_ticket(&spec).as_deref(), Some("t2"));
        assert_eq!(
            two_step_ticket(&json!({"access_token": "a", "user_id": 1})),
            None
        );
    }

    #[test]
    fn announcements_keep_text_and_read_state_only() {
        let a = json!({"id": 3, "title": "停机维护", "content": "**周六** 凌晨",
            "notify_mode": "popup", "read_at": null, "created_at": "2026-10-25T08:00:00Z",
            "targeting": {"any": []}});
        let v = announcement_view(&a);
        assert_eq!(v["popup"], true);
        assert_eq!(v["read"], false);
        assert_eq!(v["content"], "**周六** 凌晨");
        assert!(v.get("targeting").is_none());
    }

    #[test]
    fn invitation_codes_are_checked() {
        assert_eq!(
            invite_code(&json!({"affiliate_code": " AB12-cd "})).as_deref(),
            Some("AB12-cd")
        );
        assert_eq!(invite_code(&json!({"affiliate_code": "bad code"})), None);
        assert_eq!(invite_code(&json!({"affiliate_code": ""})), None);
        assert_eq!(invite_code(&json!({})), None);
    }

    #[test]
    fn keys_show_only_their_start_and_end() {
        let key = "sk-abcdef1234567890wxyz";
        assert_eq!(masked(key), "sk-abcd••••••••••••wxyz");
        assert_eq!(masked(key).chars().count(), key.chars().count());
        assert_eq!(masked("short"), "•••••");
    }

    #[test]
    fn calendar_months_are_validated() {
        assert_eq!(month(&json!({"month": "2026-10"})).unwrap(), "2026-10");
        for bad in ["2026-13", "2026-1", "26-10", "2026-10&x=1", ""] {
            assert!(month(&json!({ "month": bad })).is_err(), "{bad}");
        }
    }

    #[test]
    fn query_dates_are_validated() {
        let input = json!({"start_date": "2026-10-01", "end_date": "2026-10-01&page=9"});
        assert_eq!(date(&input, "start_date"), "2026-10-01");
        assert_eq!(date(&input, "end_date"), "");
    }
}
