//! 兔啃桌面客户端。账户、充值、密钥，以及把 Claude Desktop /
//! Claude Code 接到兔啃。

mod account;
mod api;
mod claude_config;
mod desktop;
mod google;
mod panel;
mod shop;
mod update;
mod vault;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::vault::{Account, Order, Vault};

#[derive(Default)]
pub struct Cache {
    catalog: Option<(Instant, Value)>,
    quotes: HashMap<String, (Instant, f64)>,
    plaza: Option<(Instant, Option<Value>)>,
    /// Whether the server has the desktop sign-in API, and its rules.
    auth: Option<(Instant, Value)>,
}

pub struct State {
    vault: tokio::sync::Mutex<Option<Vault>>,
    /// One order sync at a time, without blocking the vault for the UI.
    sync: tokio::sync::Mutex<()>,
    vault_path: PathBuf,
    backups: PathBuf,
    cache: std::sync::Mutex<Cache>,
    /// The window material in use: "mica", or "none" before Windows 11.
    backdrop: std::sync::Mutex<&'static str>,
    /// Whether the app shows its dark theme, for the windows it opens.
    dark: std::sync::atomic::AtomicBool,
    /// The site's sign-in page the in-app web panel shows next.
    panel: std::sync::Mutex<Option<panel::Target>>,
    /// The Google sign-in in progress, if any.
    google: std::sync::Mutex<Option<google::Flow>>,
    /// The short-lived ticket between a password and its two-step code.
    totp: std::sync::Mutex<Option<String>>,
}

impl State {
    fn new(dir: PathBuf) -> State {
        State {
            vault: tokio::sync::Mutex::new(None),
            sync: tokio::sync::Mutex::new(()),
            vault_path: dir.join("tuken.vault"),
            backups: dir.join("backups"),
            cache: std::sync::Mutex::new(Cache::default()),
            backdrop: std::sync::Mutex::new("none"),
            dark: std::sync::atomic::AtomicBool::new(false),
            panel: std::sync::Mutex::new(None),
            google: std::sync::Mutex::new(None),
            totp: std::sync::Mutex::new(None),
        }
    }

    pub async fn vault(&self) -> Result<tokio::sync::MappedMutexGuard<'_, Vault>, String> {
        let mut guard = self.vault.lock().await;
        if guard.is_none() {
            *guard = Some(vault::load(&self.vault_path)?);
        }
        Ok(tokio::sync::MutexGuard::map(guard, |v| {
            v.as_mut().expect("loaded above")
        }))
    }

    pub fn save(&self, v: &Vault) -> Result<(), String> {
        vault::save(&self.vault_path, v)
    }

    pub async fn current(&self) -> Result<Account, String> {
        self.vault()
            .await?
            .current()
            .cloned()
            .ok_or_else(|| "请先登录兔啃".into())
    }

    pub async fn push_order(&self, order: Order) -> Result<(), String> {
        let mut v = self.vault().await?;
        v.orders.push(order);
        // Finished orders older than 90 days only make the file bigger.
        let cutoff = shop::now() - 90 * 24 * 3600;
        v.orders.retain(|o| {
            o.created_at > cutoff || !matches!(o.status.as_str(), "completed" | "closed")
        });
        self.save(&v)
    }

    pub async fn update_order(
        &self,
        id: &str,
        f: impl FnOnce(&mut Order),
    ) -> Result<Order, String> {
        let mut v = self.vault().await?;
        let order = v
            .orders
            .iter_mut()
            .find(|o| o.id == id)
            .ok_or("找不到这笔订单")?;
        f(order);
        let order = order.clone();
        self.save(&v)?;
        Ok(order)
    }
}

/// Where Claude Desktop and Claude Code keep the settings 兔啃 edits.
pub fn config_paths(app: &AppHandle) -> Result<claude_config::Paths, String> {
    let state = app.state::<State>();
    claude_config::Paths::system(state.backups.clone())
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Mica needs Windows 11 (build 22000); older systems get the opaque look.
#[cfg(windows)]
fn supports_mica() -> bool {
    use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .and_then(|k| k.get_value::<String, _>("CurrentBuildNumber"))
        .ok()
        .and_then(|b| b.parse::<u32>().ok())
        .is_some_and(|b| b >= 22000)
}

#[cfg(not(windows))]
fn supports_mica() -> bool {
    false
}

fn apply_backdrop(app: &AppHandle, dark: Option<bool>) -> &'static str {
    use tauri::window::{Effect, EffectsBuilder};
    let Some(w) = app.get_webview_window("main") else {
        return "none";
    };
    if !supports_mica() {
        return "none";
    }
    let effect = match dark {
        Some(true) => Effect::MicaDark,
        Some(false) => Effect::MicaLight,
        None => Effect::Mica,
    };
    match w.set_effects(EffectsBuilder::new().effect(effect).build()) {
        Ok(()) => "mica",
        Err(_) => "none",
    }
}

/// The window material, so the page knows whether to paint its own background.
#[tauri::command]
fn backdrop(app: AppHandle, state: tauri::State<'_, State>) -> &'static str {
    let _ = app;
    *state.backdrop.lock().unwrap()
}

/// Matches Mica's tint to the app's light or dark theme.
#[tauri::command]
fn set_backdrop_theme(app: AppHandle, state: tauri::State<'_, State>, dark: bool) -> &'static str {
    let used = apply_backdrop(&app, Some(dark));
    *state.backdrop.lock().unwrap() = used;
    state.dark.store(dark, std::sync::atomic::Ordering::SeqCst);
    used
}

/// Quits for real; closing the window only hides it to the tray.
#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

fn create_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "打开兔啃", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出兔啃", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("兔啃")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_main(app)
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_page_load(|webview, payload| {
            // The page shows the window after its first paint; this is only the
            // fallback for a page that fails to start.
            if webview.label() == "main"
                && matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
            {
                let window = webview.window();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    if !window.is_visible().unwrap_or(true) {
                        let _ = window.show();
                    }
                });
            }
        })
        .on_window_event(|window, event| {
            // Closing the main window keeps 兔啃 in the tray, where it finishes
            // paid orders; the tray menu quits.
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            let dir = app
                .path()
                .app_data_dir()
                .map_err(|_| "找不到应用数据目录")?;
            app.manage(State::new(dir));
            let used = apply_backdrop(app.handle(), None);
            *app.state::<State>().backdrop.lock().unwrap() = used;
            create_tray(app)?;
            shop::start_worker(app.handle().clone());
            Ok(())
        })
        .invoke_handler({
            let handler: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
                account::tuken,
                desktop::desktop_status,
                desktop::desktop_install,
                desktop::desktop_open,
                desktop::desktop_models,
                backdrop,
                set_backdrop_theme,
                quit,
                panel::panel_show,
                panel::panel_hide,
            ];
            move |invoke: tauri::ipc::Invoke<tauri::Wry>| {
                // The sign-in panel and the Google window show remote pages.
                // App commands aren't covered by window capabilities, so
                // refuse them here.
                if invoke.message.webview().label() != "main" {
                    invoke.resolver.reject("这个窗口不能使用兔啃的功能");
                    return true;
                }
                handler(invoke)
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running 兔啃");
}
