//! Buying credit from the catfk.com (云猫) shop without leaving the app. The
//! catalog and price come from the shop's API; the order's payment link opens
//! Alipay's own cashier in a narrow window, whose code the phone's Alipay
//! pays (小荷包 included); and the redemption code the shop delivers is
//! redeemed to the buyer's account by the native worker.

use std::time::{Duration, Instant};

use reqwest::Method;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::api::{self, number, str_field};
use crate::vault::{Account, Order};
use crate::State;

/// The 云猫 shop that sells 兔啃 credit.
const SHOP_ID: &str = "HD69FQCE";
/// Unpaid orders are given up after this long, so they stop being polled.
const ORDER_TTL: i64 = 30 * 60;
const CATALOG_TTL: Duration = Duration::from_secs(5 * 60);
const QUOTE_TTL: Duration = Duration::from_secs(2 * 60);

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn secret() -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use sha2::{Digest, Sha256};
    URL_SAFE_NO_PAD.encode(Sha256::digest(
        format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4()).as_bytes(),
    ))
}

fn valid_pay_url(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|u| {
        u.scheme() == "https"
            && u.username().is_empty()
            && u.password().is_none()
            && u.host_str().is_some_and(|h| {
                h.parse::<std::net::IpAddr>().is_err() && h.contains('.') && h != "localhost"
            })
    })
}

async fn fetch_catalog() -> Result<Value, String> {
    let data = api::cat(
        "Shop/goodsList",
        json!({"token":SHOP_ID,"goods_type":"card","current":1,"pageSize":100}),
    )
    .await?;
    let goods = data["list"]
        .as_array()
        .ok_or("商品列表格式已变化")?
        .iter()
        // Only cards of this shop that can be looked up again by password, so
        // a paid order can always be finished after a restart.
        .filter(|g| {
            g["user"]["token"] == SHOP_ID
                && g["goods_type"] == "card"
                && g["extend"]["query_password_status"].as_i64() == Some(1)
        })
        .map(|g| {
            json!({
                "id": g["goods_key"],
                "name": g["name"],
                "price": number(&g["price"]),
                "stock": g["extend"]["stock_count"],
            })
        })
        .collect::<Vec<_>>();
    let channels = api::cat("Shop/getUserChannel", json!({ "token": SHOP_ID })).await?;
    let channels = channels
        .as_array()
        .ok_or("支付方式格式已变化")?
        .iter()
        .filter(|c| c["status"].as_i64() == Some(1))
        .map(|c| json!({"id": c["id"], "name": c["show_name"], "rate": number(&c["rate"])}))
        .collect::<Vec<_>>();
    Ok(json!({ "goods": goods, "channels": channels }))
}

pub async fn catalog(state: &State, fresh: bool) -> Result<Value, String> {
    if !fresh {
        if let Some((at, data)) = &state.cache.lock().unwrap().catalog {
            if at.elapsed() < CATALOG_TTL {
                return Ok(data.clone());
            }
        }
    }
    let data = fetch_catalog().await?;
    state.cache.lock().unwrap().catalog = Some((Instant::now(), data.clone()));
    Ok(data)
}

fn listed(catalog: &Value, goods_id: &str, channel: &Value) -> Option<Value> {
    let goods = catalog["goods"]
        .as_array()?
        .iter()
        .find(|g| g["id"] == goods_id)?
        .clone();
    catalog["channels"]
        .as_array()?
        .iter()
        .any(|c| c["id"] == *channel)
        .then_some(goods)
}

async fn quote(state: &State, goods_id: &str, channel: &Value, fresh: bool) -> Result<f64, String> {
    let key = format!("{goods_id}/{channel}");
    if !fresh {
        if let Some((at, amount)) = state.cache.lock().unwrap().quotes.get(&key) {
            if at.elapsed() < QUOTE_TTL {
                return Ok(*amount);
            }
        }
    }
    let catalog = catalog(state, false).await?;
    listed(&catalog, goods_id, channel).ok_or("请选择当前在售的金额和付款方式")?;
    let q = api::cat(
        "Shop/getGoodsPrice",
        json!({"goods_key":goods_id,"quantity":1,"coupon_code":"","channel_id":channel}),
    )
    .await?;
    let amount = number(&q["total_amount"])
        .filter(|a| *a > 0.0)
        .ok_or("无法确认付款金额")?;
    state
        .cache
        .lock()
        .unwrap()
        .quotes
        .insert(key, (Instant::now(), amount));
    Ok(amount)
}

/// The payment link of an order still waiting for payment.
fn pay_link(o: &Order) -> Result<url::Url, String> {
    o.pay_url
        .as_deref()
        .filter(|u| valid_pay_url(u))
        .and_then(|u| url::Url::parse(u).ok())
        .ok_or_else(|| "这笔订单没有付款链接".to_string())
}

/// An order waiting for payment, as the paying screen shows it. The link
/// itself stays here: `order_cashier` opens it in the cashier window.
fn payable(o: &Order) -> Result<Value, String> {
    pay_link(o)?;
    Ok(o.public())
}

// Require all identifiers, payment status, quantity and fulfillment to agree.
// A UI redirect or a client's claim of payment is never proof of payment.
fn fulfilled_card(order: &Order, data: &Value) -> Result<Option<String>, String> {
    if data["trade_no"].as_str() != order.trade.as_deref()
        || data["user"]["token"] != SHOP_ID
        || data["goods"]["goods_key"] != order.goods
    {
        return Err("订单归属或商品与下单记录不一致，已停止自动兑换".into());
    }
    if data["status"].as_i64() != Some(1) {
        return Ok(None);
    }
    if data["quantity"].as_i64() != Some(1) {
        return Err("订单数量不符合自动兑换要求".into());
    }
    let cards = data["response"]["cards"].as_array();
    if data["sendout"].as_i64() != Some(1) || cards.is_none_or(|c| c.is_empty()) {
        return Ok(None);
    }
    let cards = cards.unwrap();
    if cards.len() != 1 {
        return Err("发货数量异常，已停止自动兑换".into());
    }
    let code = cards[0].as_str().ok_or("兑换码格式异常")?.trim();
    // Accept a bare redemption code only. Never extract a guessed substring
    // from a URL, instructions, HTML, or a multi-line card delivery.
    if code.len() < 8
        || code.len() > 128
        || !code
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("云猫发货内容不是单个兑换码，请到云猫核对订单".into());
    }
    Ok(Some(code.to_string()))
}

enum Outcome {
    Redeemed,
    Waiting,
    Expired,
}

/// Finishes every open order whose buyer is signed in. One pass at a time;
/// the vault is only locked to read and record, never across the network.
pub async fn sync(app: &AppHandle, state: &State) -> Result<(), String> {
    let _one_at_a_time = state.sync.lock().await;
    let (accounts, orders): (Vec<Account>, Vec<Order>) = {
        let v = state.vault().await?;
        (
            v.accounts.clone(),
            v.orders.iter().filter(|o| o.is_open()).cloned().collect(),
        )
    };
    let mut changed = false;
    let mut verified: Vec<i64> = Vec::new();
    for o in orders {
        let Some(account) = accounts.iter().find(|a| a.user_id == o.owner) else {
            continue; // Finished once its buyer signs in again.
        };
        let result = async {
            // Verify the identity before a purchased code is sent to the redeem API.
            if !verified.contains(&account.user_id) {
                let me = api::desktop(Some(&account.token), "/me", Method::GET, None).await?;
                if me["id"].as_i64() != Some(account.user_id) {
                    return Err("账户身份不一致，已停止自动兑换".to_string());
                }
                verified.push(account.user_id);
            }
            let data = api::cat(
                "Order/info",
                json!({"trade_no":o.trade,"query_password":o.password,"dump":1}),
            )
            .await?;
            match fulfilled_card(&o, &data)? {
                None => {
                    // A paid order keeps waiting for its card; only unpaid ones expire.
                    let expired = o.status == "awaiting_payment"
                        && now() - o.created_at > ORDER_TTL
                        && !api::paid(o.trade.as_deref().unwrap_or_default()).await?;
                    Ok(if expired {
                        Outcome::Expired
                    } else {
                        Outcome::Waiting
                    })
                }
                Some(code) => {
                    state
                        .update_order(&o.id, |x| x.status = "redeeming".into())
                        .await?;
                    let r = api::desktop(
                        Some(&account.token),
                        "/redeem",
                        Method::POST,
                        Some(json!({ "code": code })),
                    )
                    .await?;
                    if r["redeemed"] != true {
                        return Err("服务器未确认兑换成功".to_string());
                    }
                    Ok(Outcome::Redeemed)
                }
            }
        }
        .await;
        let done = matches!(result, Ok(Outcome::Redeemed));
        state
            .update_order(&o.id, |x| match result {
                Ok(Outcome::Redeemed) => {
                    x.status = "completed".into();
                    x.message = "额度已到账".into();
                    x.pay_url = None;
                    x.password.clear();
                }
                Ok(Outcome::Waiting) => {
                    x.message = if x.status == "awaiting_payment" {
                        "等待付款"
                    } else {
                        "已付款，等待发货"
                    }
                    .into();
                }
                Ok(Outcome::Expired) => {
                    x.status = "closed".into();
                    x.message = "30 分钟内没有付款，订单已取消".into();
                    x.pay_url = None;
                }
                Err(e) => x.message = e,
            })
            .await?;
        changed |= done;
    }
    if changed {
        // Tells the UI to refresh the balance, also when it didn't start the sync.
        let _ = app.emit("tuken://credited", ());
    }
    crate::cashier::settle(app, &state.vault().await?.orders);
    Ok(())
}

/// Keeps finishing paid orders in the background, also after a restart.
/// Only while the vault is open: opening it is left to the page, since on
/// macOS that may ask the user.
pub fn start_worker(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            let state = app.state::<State>();
            let open = match state.vault_if_open().await {
                Some(v) => v.orders.iter().any(Order::is_open),
                None => false,
            };
            if open {
                let _ = sync(&app, &state).await;
            }
        }
    });
}

fn order_of(v: &crate::vault::Vault, id: &str, owner: i64) -> Result<Order, String> {
    v.orders
        .iter()
        .find(|o| o.id == id && o.owner == owner)
        .cloned()
        .ok_or_else(|| "找不到这笔订单".into())
}

pub async fn action(
    app: &AppHandle,
    state: &State,
    action: &str,
    input: &Value,
) -> Result<Value, String> {
    match action {
        "catalog" => catalog(state, input["fresh"] == true).await,
        "quote" => {
            let goods = str_field(input, "goods_id")?;
            let amount = quote(state, goods, &input["channel_id"], input["fresh"] == true).await?;
            Ok(json!({ "total_amount": amount }))
        }
        "order_create" => {
            let account = state.current().await?;
            let goods_id = str_field(input, "goods_id")?.to_string();
            let channel = input["channel_id"].clone();
            // Pressing pay again for the same amount reopens the same unpaid order.
            let recent = {
                let v = state.vault().await?;
                let recent = v.orders.iter().find(|o| {
                    o.owner == account.user_id
                        && o.goods == goods_id
                        && o.status == "awaiting_payment"
                        && now() - o.created_at < 10 * 60
                });
                if let Some(o) = recent {
                    Some(o.clone())
                } else {
                    let open = v
                        .orders
                        .iter()
                        .filter(|o| {
                            o.owner == account.user_id && (o.is_open() || o.status == "creating")
                        })
                        .count();
                    if open >= 3 {
                        return Err("还有 3 笔订单没处理完，请先付款或取消其中一笔".into());
                    }
                    None
                }
            };
            if let Some(o) = recent {
                return payable(&o);
            }
            let me = api::desktop(Some(&account.token), "/me", Method::GET, None).await?;
            if me["id"].as_i64() != Some(account.user_id) {
                return Err("账户身份不一致".into());
            }
            let catalog = catalog(state, false).await?;
            let goods =
                listed(&catalog, &goods_id, &channel).ok_or("这个金额已下架，请刷新后重选")?;
            let amount = quote(state, &goods_id, &channel, true).await?;
            if number(&input["confirmed_amount"]).is_none_or(|a| (a - amount).abs() > 0.001) {
                return Err("价格有变动，请确认新的应付金额".into());
            }
            // The account email is the order-lookup contact on 云猫.
            let contact = me["email"]
                .as_str()
                .unwrap_or(&account.email)
                .trim()
                .to_string();
            if contact.len() < 3 || contact.len() > 100 || contact.contains(['\r', '\n']) {
                return Err("账户邮箱无效，无法下单".into());
            }
            let order = Order {
                id: uuid::Uuid::new_v4().to_string(),
                owner: account.user_id,
                goods: goods_id.clone(),
                name: goods["name"].as_str().unwrap_or("兔啃额度").into(),
                amount,
                contact,
                password: secret(),
                trade: None,
                pay_url: None,
                status: "creating".into(),
                created_at: now(),
                message: "正在下单；结果不明时请先到云猫查单，不要重复付款".into(),
            };
            // Persist the contact and lookup password BEFORE creating the
            // remote order, and never retry this non-idempotent POST.
            state.push_order(order.clone()).await?;
            let created = api::cat(
                "Pay/order",
                json!({"goods_key":order.goods,"quantity":1,"coupon_code":"","channel_id":channel,
                    "contact":order.contact,"query_password":order.password,"select_cards_ids":[],"extend":{}}),
            )
            .await;
            let created = match created {
                Ok(c) => c,
                Err(e) => {
                    state
                        .update_order(&order.id, |x| {
                            x.status = "attention".into();
                            x.message = format!("下单没有完成：{e}");
                        })
                        .await?;
                    return Err(e);
                }
            };
            let trade = str_field(&created, "trade_no")?.to_string();
            let pay_url = str_field(&created, "payurl")?.to_string();
            let sane = number(&created["total_amount"])
                .is_some_and(|a| (a - amount).abs() <= 0.001)
                && valid_pay_url(&pay_url);
            let updated = state
                .update_order(&order.id, |x| {
                    x.trade = Some(trade.clone());
                    if sane {
                        x.pay_url = Some(pay_url.clone());
                        x.status = "awaiting_payment".into();
                        x.message = "等待付款".into();
                    } else {
                        x.status = "attention".into();
                        x.message = "付款金额或付款地址异常，请到云猫核对订单".into();
                    }
                })
                .await?;
            if !sane {
                return Err(updated.message);
            }
            payable(&updated)
        }
        "order_pay" => {
            let account = state.current().await?;
            let o = order_of(
                &*state.vault().await?,
                str_field(input, "id")?,
                account.user_id,
            )?;
            if o.status != "awaiting_payment" {
                return Err("这笔订单已经不能付款了".into());
            }
            payable(&o)
        }
        "order_cashier" => {
            // Alipay's own cashier in a window of its own: the code it
            // shows, scanned with the phone, also offers 小荷包.
            let account = state.current().await?;
            let o = order_of(
                &*state.vault().await?,
                str_field(input, "id")?,
                account.user_id,
            )?;
            if o.status != "awaiting_payment" {
                return Err("这笔订单已经不能付款了".into());
            }
            crate::cashier::open(app, &o.id, pay_link(&o)?)?;
            Ok(json!({}))
        }
        "order_poll" => {
            // Called every few seconds while a payment is in progress, so the
            // credit lands right after paying instead of on the worker's next pass.
            let account = state.current().await?;
            let id = str_field(input, "id")?.to_string();
            let o = order_of(&*state.vault().await?, &id, account.user_id)?;
            let ready = match o.status.as_str() {
                "awaiting_payment" => api::paid(o.trade.as_deref().unwrap_or_default()).await?,
                "awaiting_delivery" | "redeeming" => true,
                _ => false,
            };
            if ready {
                sync(app, state).await?;
                let paid_waiting = order_of(&*state.vault().await?, &id, account.user_id)?.status
                    == "awaiting_payment";
                if paid_waiting {
                    state
                        .update_order(&id, |x| {
                            x.status = "awaiting_delivery".into();
                            x.message = "已付款，等待发货".into();
                        })
                        .await?;
                }
            }
            Ok(order_of(&*state.vault().await?, &id, account.user_id)?.public())
        }
        "order_close" => {
            let account = state.current().await?;
            let id = str_field(input, "id")?.to_string();
            order_of(&*state.vault().await?, &id, account.user_id)?;
            let o = state
                .update_order(&id, |x| {
                    x.status = "closed".into();
                    x.message = "已取消。如果其实已经付款，可凭查单信息到云猫找回兑换码".into();
                    x.pay_url = None;
                })
                .await?;
            crate::cashier::settle(app, &state.vault().await?.orders);
            Ok(o.public())
        }
        "order_recovery" => {
            let account = state.current().await?;
            let o = order_of(
                &*state.vault().await?,
                str_field(input, "id")?,
                account.user_id,
            )?;
            // Explicit recovery only; never part of the order list.
            Ok(json!({"contact": o.contact, "query_password": o.password, "trade_no": o.trade}))
        }
        "sync" => {
            sync(app, state).await?;
            Ok(state.vault().await?.view())
        }
        _ => Err("不支持的操作".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order() -> Order {
        Order {
            id: "local".into(),
            owner: 1,
            goods: "glnbux".into(),
            name: "1刀额度".into(),
            amount: 1.0,
            contact: "test".into(),
            password: "secret".into(),
            trade: Some("owned-trade".into()),
            pay_url: None,
            status: "awaiting_payment".into(),
            created_at: 0,
            message: "".into(),
        }
    }

    fn delivery() -> Value {
        json!({"trade_no":"owned-trade","user":{"token":SHOP_ID},"goods":{"goods_key":"glnbux"},
            "status":1,"quantity":1,"sendout":1,"response":{"cards":["TEST-CODE-12345"]}})
    }

    #[test]
    fn rejects_wrong_shop_order_or_product() {
        for (field, value) in [
            ("trade_no", json!("other")),
            ("user", json!({"token":"other"})),
            ("goods", json!({"goods_key":"other"})),
        ] {
            let mut d = delivery();
            d[field] = value;
            assert!(fulfilled_card(&order(), &d).is_err());
        }
    }

    #[test]
    fn waits_for_actual_payment_and_fulfillment() {
        let mut d = delivery();
        d["status"] = json!(0);
        assert_eq!(fulfilled_card(&order(), &d).unwrap(), None);
        d["status"] = json!(1);
        d["sendout"] = json!(0);
        assert_eq!(fulfilled_card(&order(), &d).unwrap(), None);
    }

    #[test]
    fn rejects_instructions_and_multiple_cards() {
        let mut d = delivery();
        d["response"]["cards"] = json!(["请使用 TEST-CODE-12345"]);
        assert!(fulfilled_card(&order(), &d).is_err());
        d["response"]["cards"] = json!(["TEST-CODE-12345", "SECOND-CODE-123"]);
        assert!(fulfilled_card(&order(), &d).is_err());
    }

    #[test]
    fn accepts_exact_delivery() {
        assert_eq!(
            fulfilled_card(&order(), &delivery()).unwrap(),
            Some("TEST-CODE-12345".into())
        );
    }

    #[test]
    fn the_paying_screen_gets_no_payment_link() {
        let mut o = order();
        assert!(payable(&o).is_err());
        o.pay_url = Some("https://catfk.com/pay/example".into());
        let v = payable(&o).unwrap();
        assert!(!v.to_string().contains("catfk.com/pay"));
        assert_eq!(
            pay_link(&o).unwrap().as_str(),
            "https://catfk.com/pay/example"
        );
        o.pay_url = Some("http://catfk.com/pay/example".into());
        assert!(payable(&o).is_err());
    }

    #[test]
    fn refuses_unsafe_payment_links() {
        for u in [
            "http://pay.example/x",
            "file:///c:/x",
            "https://user:pass@pay.example/",
            "https://127.0.0.1/x",
            "javascript:alert(1)",
        ] {
            assert!(!valid_pay_url(u));
        }
        assert!(valid_pay_url("https://pay.example/order"));
    }
}
