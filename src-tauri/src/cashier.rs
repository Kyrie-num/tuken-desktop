//! Alipay's own cashier for an order, in a narrow window of 兔啃's own: the
//! code it shows, scanned with the phone, also offers 小荷包. Only the amount
//! and the code show; the window closes once the order is paid, cancelled or
//! expired. The page is remote, so the window gets no IPC (`lib.rs`).

use std::sync::atomic::Ordering;
use std::sync::Mutex;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::vault::Order;
use crate::State;

pub const LABEL: &str = "tuken-pay";
const WIDTH: f64 = 380.0;
const HEIGHT: f64 = 440.0;

/// The order whose cashier the window shows.
static SHOWING: Mutex<Option<String>> = Mutex::new(None);

/// Runs on each cashier page: brings the code to scan (the largest square
/// image or canvas, whatever Alipay calls it) to the middle, a little below
/// the top so its amount shows above it, and hides the other pictures and
/// the scrollbars: the window shows the amount and the code.
const FOCUS_CODE: &str = r#"(function focus(n) {
  var best = null, size = 0;
  document.querySelectorAll('img, canvas').forEach(function (el) {
    var r = el.getBoundingClientRect();
    if (r.width >= 100 && Math.abs(r.width - r.height) < 8 && r.width > size) {
      best = el;
      size = r.width;
    }
  });
  if (!best) {
    if (n < 40) setTimeout(function () { focus(n + 1); }, 150);
    return;
  }
  document.querySelectorAll('img').forEach(function (img) {
    var r = img.getBoundingClientRect();
    if (img !== best && r.width > 120 && r.height > 120) img.style.visibility = 'hidden';
  });
  document.documentElement.style.overflow = 'hidden';
  document.body.style.overflow = 'hidden';
  var r = best.getBoundingClientRect();
  window.scrollBy(r.left + r.width / 2 - window.innerWidth / 2, r.top - 100);
})(0);"#;

/// The shop and Alipay only: nothing else loads in the window.
fn allowed(u: &url::Url) -> bool {
    if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    let host = u.host_str().unwrap_or_default();
    host == "catfk.com"
        || host.ends_with(".catfk.com")
        || host == "alipay.com"
        || host.ends_with(".alipay.com")
}

/// Opens the order's cashier, or brings the window back to it.
pub fn open(app: &AppHandle, order: &str, url: url::Url) -> Result<(), String> {
    *SHOWING.lock().unwrap() = Some(order.to_string());
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.navigate(url);
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(());
    }
    let dark = app.state::<State>().dark.load(Ordering::SeqCst);
    let mut builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(url))
        .title("支付宝付款")
        .theme(Some(if dark {
            tauri::Theme::Dark
        } else {
            tauri::Theme::Light
        }))
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .maximizable(false)
        .zoom_hotkeys_enabled(false)
        // No Alipay session stays behind.
        .incognito(true)
        .on_navigation(allowed)
        .on_page_load(|window, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                let _ = window.eval(FOCUS_CODE);
            }
        });
    if let Some(main) = app.get_webview_window("main") {
        if let Some((x, y)) = crate::google::centered_over(&main, WIDTH, HEIGHT) {
            builder = builder.position(x, y);
        }
        // Owned by the main window: stays above it, goes away with it.
        builder = builder.parent(&main).map_err(|_| "无法打开付款窗口")?;
    }
    builder.build().map_err(|_| "无法打开付款窗口")?;
    Ok(())
}

/// Closes the window once the order it shows no longer waits for payment.
pub fn settle(app: &AppHandle, orders: &[Order]) {
    let mut showing = SHOWING.lock().unwrap();
    let Some(id) = showing.as_deref() else {
        return;
    };
    if orders
        .iter()
        .any(|o| o.id == id && o.status == "awaiting_payment")
    {
        return;
    }
    showing.take();
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_stays_with_the_shop_and_alipay() {
        let ok = |s: &str| allowed(&url::Url::parse(s).unwrap());
        assert!(ok("https://catfk.com/pay/example"));
        assert!(ok(
            "https://excashier.alipay.com/standard/auth.htm?payOrderId=1"
        ));
        assert!(ok("https://openapi.alipay.com/gateway.do?x=1"));
        assert!(!ok("http://excashier.alipay.com/"));
        assert!(!ok("https://alipay.com.evil.example/"));
        assert!(!ok("https://evilalipay.com/"));
        assert!(!ok("https://user:pw@excashier.alipay.com/"));
        assert!(!ok("https://www.tuken.site/login"));
    }
}
