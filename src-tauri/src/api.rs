//! HTTP plumbing for the 兔啃 server and the catfk.com (云猫) shop.

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::{Client, Method};
use serde_json::{json, Value};

pub const SITE: &str = "https://www.tuken.site";
pub const SHOP: &str = "https://catfk.com";
/// Every response body is capped; the server never sends more than a page of JSON.
const MAX_BODY: usize = 2 * 1024 * 1024;

fn client() -> Result<&'static Client, String> {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    if let Some(c) = CLIENT.get() {
        return Ok(c);
    }
    let c = Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("Tuken-Desktop/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "无法建立网络连接".to_string())?;
    Ok(CLIENT.get_or_init(|| c))
}

/// User-facing text for a failed HTTP status. `data` is the JSON body, if any:
/// without one the response came from Cloudflare or a proxy, not the server.
pub fn failure(status: u16, data: Option<&Value>) -> String {
    let reason = data
        .and_then(|d| d["reason"].as_str().or(d["code"].as_str()))
        .unwrap_or("");
    match reason {
        "REDEEM_CODE_NOT_FOUND" | "REDEEM_CODE_INVALID" => "兑换码不存在",
        "REDEEM_CODE_USED" => "这张兑换码已经用过了",
        "REDEEM_CODE_EXPIRED" => "兑换码已过期",
        "INVALID_PASSWORD" | "INCORRECT_PASSWORD" | "PASSWORD_INCORRECT" => "原密码不正确",
        "GROUP_NOT_ALLOWED" => "这个分组不能用于你的账户",
        "INSUFFICIENT_BALANCE" => "余额为 0，充值后才能使用",
        "CHECKIN_ALREADY" => "今天已经签到过了",
        "CHECKIN_DISABLED" => "签到暂时关闭了",
        "CHECKIN_EMAIL_UNVERIFIED" => "邮箱还没验证，暂时不能签到",
        "CHECKIN_USER_INACTIVE" => "这个账户已被停用，请联系客服",
        "CHECKIN_SETTINGS_CHANGED" => "签到奖励刚调整过，请再点一次",
        "API_KEY_DISABLED" => "这个密钥已停用，请先启用",
        "USER_INACTIVE" => "这个账户已被停用，请联系客服",
        "GROUP_DISABLED" | "GROUP_DELETED" => "这个分组已停用，请换一个分组",
        "EXPIRED" => "登录验证已过期，请重新登录",
        _ if status == 429 => "操作较频繁，请稍后再试",
        _ if data.is_none() && status == 403 => "请求被网站防护拦截，请稍后重试",
        _ if data.is_none() || status >= 500 => "服务器暂时无法访问，请稍后重试",
        _ if status == 403 => "当前账户没有权限完成此操作",
        _ if status == 404 => "未找到这条记录，请刷新后重试",
        _ if status == 400 => "信息未通过校验，请检查输入后重试",
        _ => "服务暂时无法完成操作，请稍后重试",
    }
    .into()
}

/// catfk explains refusals (sold out, too many orders…) in `msg`. It is shown
/// as plain text, and only when it is short.
pub fn shop_message(data: &Value) -> Option<String> {
    let msg = data["msg"].as_str()?.trim();
    (!msg.is_empty() && msg.chars().count() <= 60 && !msg.contains(['<', '\r', '\n']))
        .then(|| format!("云猫：{msg}"))
}

/// One request: its status and its JSON body (if the body is JSON), whatever
/// the status. Only the network and oversized bodies fail here.
async fn exchange(
    url: &str,
    method: Method,
    body: Option<Value>,
    bearer: Option<&str>,
) -> Result<(u16, Option<Value>), String> {
    let is_create_key = method == Method::POST && url.ends_with("/desktop/keys");
    let mut r = client()?.request(method, url);
    if is_create_key {
        // This write is sent once. Do not silently retry it after a timeout.
        r = r.header("Idempotency-Key", uuid::Uuid::new_v4().to_string());
    }
    if let Some(t) = bearer {
        r = r.bearer_auth(t);
    }
    if let Some(b) = body {
        r = r.json(&b);
    }
    let mut r = r
        .send()
        .await
        .map_err(|_| "连接超时或中断，请检查网络后重试".to_string())?;
    let status = r.status().as_u16();
    if r.content_length().is_some_and(|n| n > MAX_BODY as u64) {
        return Err("服务返回内容过大".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = r.chunk().await.map_err(|_| "服务响应中断".to_string())? {
        if bytes.len() + chunk.len() > MAX_BODY {
            return Err("服务返回内容过大".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((status, serde_json::from_slice::<Value>(&bytes).ok()))
}

/// Sends one request and returns the whole JSON body of a successful response.
pub async fn send(
    url: &str,
    method: Method,
    body: Option<Value>,
    bearer: Option<&str>,
) -> Result<Value, String> {
    let (status, data) = exchange(url, method, body, bearer).await?;
    if status == 401 {
        return Err(EXPIRED.into());
    }
    if !(200..300).contains(&status) {
        return Err(failure(status, data.as_ref()));
    }
    data.ok_or_else(|| "服务响应格式不兼容".into())
}

/// A gateway call made with an API key (`/v1/…`): the JSON body, or the
/// gateway's refusal with its code (e.g. `INSUFFICIENT_BALANCE`).
pub async fn gateway(path: &str, key: &str) -> Result<Value, Refusal> {
    let (status, data) = exchange(&format!("{SITE}{path}"), Method::GET, None, Some(key))
        .await
        .map_err(Refusal::plain)?;
    if (200..300).contains(&status) {
        return data.ok_or_else(|| Refusal::plain("服务响应格式不兼容"));
    }
    let reason = data
        .as_ref()
        .and_then(|d| d["code"].as_str().or(d["error"]["code"].as_str()))
        .unwrap_or("")
        .to_string();
    Err(Refusal {
        message: failure(status, data.as_ref()),
        reason,
    })
}

/// A refusal of the sign-in API: the server's reason code, for flows that
/// branch on it (an unregistered email, a wrong code), and the text to show.
#[derive(Debug)]
pub struct Refusal {
    pub reason: String,
    pub message: String,
}

impl Refusal {
    fn plain(message: impl Into<String>) -> Refusal {
        Refusal {
            reason: String::new(),
            message: message.into(),
        }
    }
}

/// What a sign-in refusal means for the user, by the server's reason codes.
/// Here 401 is a wrong password, not an expired session.
pub fn auth_failure(status: u16, reason: &str, data: Option<&Value>) -> String {
    match reason {
        "INVALID_CREDENTIALS" => "邮箱或密码不对",
        "EMAIL_NOT_REGISTERED" => "这个邮箱还没注册",
        "EMAIL_EXISTS" => "这个邮箱已经注册过了，直接登录吧",
        "INVALID_VERIFY_CODE" | "VERIFY_CODE_REQUIRED" | "VERIFY_CODE_EXPIRED" => {
            "验证码不对或已过期，请重新获取"
        }
        "VERIFY_CODE_TOO_FREQUENT" => "验证码刚发过，请稍等再获取",
        "VERIFY_CODE_MAX_ATTEMPTS" => "验证码输错太多次，请重新获取",
        "TOTP_INVALID_CODE" => "两步验证码不对",
        "TOTP_TOO_MANY_ATTEMPTS" => "两步验证码输错太多次，请稍后再试",
        "PASSWORD_TOO_SHORT" => "密码太短了",
        "INVITATION_CODE_INVALID" => "邀请码无效",
        "INVITATION_CODE_REQUIRED" | "INVITE_REQUIRED" => "需要邀请码才能注册",
        "INVITE_INVALID" => "邀请码不对，请检查后再试",
        "EMAIL_SUFFIX_NOT_ALLOWED" => "这个邮箱不能用来注册，请换一个",
        "EMAIL_RESERVED" | "INVALID_EMAIL" => "这个邮箱不能使用，请换一个",
        "EMAIL_NOT_CONFIGURED" => "暂时发不了邮件，请稍后再试",
        "REGISTRATION_DISABLED" => "暂时没有开放注册",
        "PASSWORD_RESET_DISABLED" => "暂时不能自助重设密码，请联系客服",
        "USER_NOT_ACTIVE" => "这个账户已被停用，请联系客服",
        "BACKEND_MODE_ADMIN_ONLY" => "兔啃正在维护，暂时不能登录",
        _ if status == 429 => "操作太频繁，请稍后再试",
        _ if status == 401 => "邮箱或密码不对",
        _ => return failure(status, data),
    }
    .into()
}

/// A call to the desktop sign-in API (`/api/v1/desktop/auth…`), which the
/// native sign-in and sign-up screens use.
pub async fn auth(method: Method, path: &str, body: Option<Value>) -> Result<Value, Refusal> {
    let url = format!("{SITE}/api/v1/desktop/auth{path}");
    let (status, data) = exchange(&url, method, body, None)
        .await
        .map_err(Refusal::plain)?;
    let reason = data
        .as_ref()
        .and_then(|d| d["reason"].as_str().or(d["code"].as_str()))
        .unwrap_or("")
        .to_string();
    if !(200..300).contains(&status) {
        // A server without these routes answers 404 without JSON.
        let reason = if status == 404 && data.is_none() {
            "NOT_SUPPORTED".to_string()
        } else {
            reason
        };
        let message = auth_failure(status, &reason, data.as_ref());
        return Err(Refusal { reason, message });
    }
    let data = data.ok_or_else(|| Refusal::plain("服务响应格式不兼容"))?;
    if data["code"].as_i64() != Some(0) {
        return Err(Refusal {
            message: auth_failure(status, &reason, Some(&data)),
            reason,
        });
    }
    Ok(data["data"].clone())
}

/// Error text for a desktop token the server no longer accepts. The UI signs
/// that account out when an error starts with it.
pub const EXPIRED: &str = "登录授权已过期，请重新登录兔啃";

/// `{code, data}` envelopes: the server answers code 0 on success, catfk code 1.
async fn enveloped(
    url: &str,
    method: Method,
    body: Option<Value>,
    bearer: Option<&str>,
    ok_code: i64,
) -> Result<Value, String> {
    let data = send(url, method, body, bearer).await?;
    if data["code"].as_i64() != Some(ok_code) {
        return Err((ok_code == 1)
            .then(|| shop_message(&data))
            .flatten()
            .unwrap_or_else(|| "服务未确认操作成功，请检查订单或账户状态".into()));
    }
    Ok(data["data"].clone())
}

/// A `/api/v1/desktop` call, signed in with `token` when given.
pub async fn desktop(
    token: Option<&str>,
    path: &str,
    method: Method,
    body: Option<Value>,
) -> Result<Value, String> {
    enveloped(
        &format!("{SITE}/api/v1/desktop{path}"),
        method,
        body,
        token,
        0,
    )
    .await
}

/// A `/api/v1/desktop` call to a feature the server may not have yet:
/// `None` when the route is missing (a plain 404), so its screen stays hidden.
pub async fn desktop_optional(
    token: &str,
    path: &str,
    method: Method,
    body: Option<Value>,
) -> Result<Option<Value>, String> {
    let url = format!("{SITE}/api/v1/desktop{path}");
    let (status, data) = exchange(&url, method, body, Some(token)).await?;
    if status == 404 && data.is_none() {
        return Ok(None);
    }
    if status == 401 {
        return Err(EXPIRED.into());
    }
    if !(200..300).contains(&status) {
        return Err(failure(status, data.as_ref()));
    }
    let data = data.ok_or("服务响应格式不兼容")?;
    if data["code"].as_i64() != Some(0) {
        return Err(failure(status, Some(&data)));
    }
    Ok(Some(data["data"].clone()))
}

/// Links from the site's own texts (contact details, announcements): plain
/// https pages only, opened in the browser.
pub fn web_link(raw: &str) -> Option<url::Url> {
    url::Url::parse(raw.trim()).ok().filter(|u| {
        u.scheme() == "https"
            && u.username().is_empty()
            && u.password().is_none()
            && u.host_str().is_some_and(|h| h.contains('.'))
    })
}

/// A catfk.com shop call, always a POST.
pub async fn cat(path: &str, body: Value) -> Result<Value, String> {
    enveloped(
        &format!("{SHOP}/shopApi/{path}"),
        Method::POST,
        Some(body),
        None,
        1,
    )
    .await
}

/// Whether catfk has received the payment for `trade`; its own shop page polls
/// the same endpoint while the buyer pays. Answers code 1 once paid.
pub async fn paid(trade: &str) -> Result<bool, String> {
    let data = send(
        &format!("{SHOP}/shopApi/Pay/query"),
        Method::POST,
        Some(json!({ "trade_no": trade })),
        None,
    )
    .await?;
    Ok(data["code"].as_i64() == Some(1))
}

pub fn str_field<'a>(v: &'a Value, field: &str) -> Result<&'a str, String> {
    v[field]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("缺少 {field}"))
}

pub fn number(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str()?.parse::<f64>().ok())
        .filter(|v| v.is_finite())
}

/// The base URLs that count as "this client points at 兔啃".
pub fn is_tuken_url(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|u| {
        u.scheme() == "https"
            && u.username().is_empty()
            && u.password().is_none()
            && u.port_or_known_default() == Some(443)
            && matches!(u.host_str(), Some("www.tuken.site"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_proxy_pages_by_status() {
        // Cloudflare's own error and challenge pages are HTML, not server JSON.
        assert_eq!(failure(522, None), "服务器暂时无法访问，请稍后重试");
        assert_eq!(failure(403, None), "请求被网站防护拦截，请稍后重试");
        assert_eq!(
            failure(403, Some(&json!({"code": 403}))),
            "当前账户没有权限完成此操作"
        );
        assert_eq!(
            failure(409, Some(&json!({"reason": "REDEEM_CODE_USED"}))),
            "这张兑换码已经用过了"
        );
        // The gateway refuses every key of an account without credit.
        assert_eq!(
            failure(
                403,
                Some(
                    &json!({"code": "INSUFFICIENT_BALANCE", "message": "Insufficient account balance"})
                )
            ),
            "余额为 0，充值后才能使用"
        );
    }

    #[test]
    fn sign_in_refusals_read_as_what_to_do() {
        // On the sign-in API, 401 is a wrong password, not an expired login.
        assert_eq!(auth_failure(401, "", None), "邮箱或密码不对");
        assert_eq!(
            auth_failure(404, "EMAIL_NOT_REGISTERED", None),
            "这个邮箱还没注册"
        );
        assert_eq!(
            auth_failure(409, "EMAIL_EXISTS", None),
            "这个邮箱已经注册过了，直接登录吧"
        );
        assert_eq!(
            auth_failure(429, "VERIFY_CODE_TOO_FREQUENT", None),
            "验证码刚发过，请稍等再获取"
        );
        assert_eq!(auth_failure(429, "", None), "操作太频繁，请稍后再试");
        assert_eq!(
            auth_failure(502, "", None),
            "服务器暂时无法访问，请稍后重试"
        );
    }

    #[test]
    fn shows_only_short_plain_shop_messages() {
        assert_eq!(
            shop_message(&json!({"code": 0, "msg": "库存不足"})).as_deref(),
            Some("云猫：库存不足")
        );
        assert_eq!(shop_message(&json!({"code": 0, "msg": "<b>x</b>"})), None);
        assert_eq!(
            shop_message(&json!({"code": 0, "msg": "长".repeat(61)})),
            None
        );
        assert_eq!(shop_message(&json!({"code": 0})), None);
    }

    #[test]
    fn opens_only_plain_https_links() {
        assert!(web_link("https://www.tuken.site/docs").is_some());
        assert!(web_link(" https://qm.qq.com/q/abc ").is_some());
        assert!(web_link("http://example.com").is_none());
        assert!(web_link("https://user@example.com").is_none());
        assert!(web_link("javascript:alert(1)").is_none());
        assert!(web_link("file:///C:/x").is_none());
    }

    #[test]
    fn recognizes_only_the_tuken_gateway() {
        assert!(is_tuken_url("https://www.tuken.site"));
        assert!(is_tuken_url("https://www.tuken.site/"));
        assert!(!is_tuken_url("https://api.example.com/"));
        assert!(!is_tuken_url("https://user@www.tuken.site/"));
        assert!(!is_tuken_url("https://www.tuken.site:8443/"));
        assert!(!is_tuken_url("http://www.tuken.site"));
        assert!(!is_tuken_url("https://www.tuken.site.evil.example"));
        assert!(!is_tuken_url("not a url"));
    }
}
