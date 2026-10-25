//! The site's sign-in page inside the main window, for as long as the server
//! has no desktop sign-in API. It is a native webview laid over a frame the
//! page keeps free for it, so nothing opens in a window of its own.
//!
//! What it shows is decided here (by sign-in); the page only says where the
//! frame is. Like any non-main webview it gets no IPC.

use serde::Deserialize;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl};

pub const LABEL: &str = "tuken-panel";

#[derive(Clone)]
pub struct Target {
    /// Opens the site's sign-up form instead of sign-in.
    pub register: bool,
    pub url: url::Url,
}

/// The frame's place in the page, in CSS (logical) pixels.
#[derive(Deserialize, Clone, Copy)]
pub struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Runs once on the sign-up page: waits until the site has stored where to
/// return after signing in, then opens its sign-up form instead of sign-in.
const TO_SIGN_UP: &str = r#"(function go(n) {
  if (location.pathname.indexOf('/register') === 0) return;
  if (sessionStorage.getItem('tuken-desktop-return') || n > 50) {
    location.replace('/register');
    return;
  }
  setTimeout(function () { go(n + 1); }, 100);
})(0);"#;

/// Runs before the site's own scripts: its pages follow 兔啃's light look
/// unless the user picked dark on the site itself.
const LIGHT_SITE: &str = r#"try {
  if (location.hostname === 'www.tuken.site' && !localStorage.getItem('theme')) {
    localStorage.setItem('theme', 'light');
  }
} catch (e) {}"#;

fn target(app: &AppHandle) -> Option<Target> {
    app.state::<crate::State>().panel.lock().unwrap().clone()
}

/// Sets what the panel shows next. An open panel follows to the new page,
/// unless it switches between sign-in and sign-up: then it closes until the
/// page shows it again.
pub fn set(app: &AppHandle, next: Target) {
    let previous = app
        .state::<crate::State>()
        .panel
        .lock()
        .unwrap()
        .replace(next.clone());
    if let Some(w) = app.get_webview(LABEL) {
        if previous.is_some_and(|p| p.register == next.register) {
            let _ = w.navigate(next.url);
        } else {
            let _ = w.close();
        }
    }
}

/// Closes the panel for good (signed in, cancelled or replaced).
pub fn finish(app: &AppHandle) {
    let state = app.state::<crate::State>();
    if state.panel.lock().unwrap().take().is_some() {
        if let Some(w) = app.get_webview(LABEL) {
            let _ = w.close();
        }
    }
}

/// The site's own pages only; anything else stays out of the frame.
fn allowed(u: &url::Url) -> bool {
    u.scheme() == "https" && u.host_str() == Some("www.tuken.site")
}

/// Shows the current page in the frame, or moves it there.
#[tauri::command]
pub async fn panel_show(app: AppHandle, rect: Rect) -> Result<(), String> {
    let target = target(&app).ok_or("没有要显示的页面")?;
    if let Some(w) = app.get_webview(LABEL) {
        return place(&w, rect);
    }
    let window = app.get_window("main").ok_or("找不到主窗口")?;
    let switched = std::sync::atomic::AtomicBool::new(false);
    let register = target.register;
    let builder = tauri::webview::WebviewBuilder::new(LABEL, WebviewUrl::External(target.url))
        .zoom_hotkeys_enabled(false)
        // A private profile per sign-in, so another account can sign in next.
        .incognito(true)
        .initialization_script(LIGHT_SITE)
        .on_navigation(allowed)
        .on_page_load(move |webview, payload| {
            if register
                && matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
                && !switched.swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                let _ = webview.eval(TO_SIGN_UP);
            }
        });
    window
        .add_child(
            builder,
            LogicalPosition::new(rect.x, rect.y),
            LogicalSize::new(rect.width, rect.height),
        )
        .map_err(|_| "无法打开页面，请重试")?;
    Ok(())
}

fn place(w: &tauri::Webview, rect: Rect) -> Result<(), String> {
    w.set_position(LogicalPosition::new(rect.x, rect.y))
        .and_then(|_| w.set_size(LogicalSize::new(rect.width, rect.height)))
        .map_err(|_| "无法调整页面位置".to_string())
}

/// Hides the panel while its frame is off screen; it can be shown again.
#[tauri::command]
pub async fn panel_hide(app: AppHandle) {
    if let Some(w) = app.get_webview(LABEL) {
        let _ = w.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_stays_on_the_site() {
        for ok in [
            "https://www.tuken.site/login",
            "https://www.tuken.site/desktop-authorize?code=AB",
        ] {
            assert!(allowed(&url::Url::parse(ok).unwrap()), "{ok}");
        }
        for bad in [
            "http://www.tuken.site/login",
            "https://tuken.site.evil.example/",
            "https://accounts.google.com/",
        ] {
            assert!(!allowed(&url::Url::parse(bad).unwrap()), "{bad}");
        }
    }
}
