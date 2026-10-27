//! 兔啃桌面客户端。账户、充值、密钥，以及把 Claude Desktop /
//! Claude Code 接到兔啃。

mod account;
mod api;
mod cashier;
mod claude_config;
mod desktop;
#[cfg(any(target_os = "macos", test))]
mod desktop_mac;
#[cfg(windows)]
pub mod desktop_windows;
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
    /// The sign-in rules last fetched, for sign-up's password minimum.
    auth: Option<(Instant, Value)>,
}

pub struct State {
    vault: tokio::sync::Mutex<Option<Vault>>,
    /// macOS: why the vault couldn't be opened this run (the keychain said
    /// no). Kept, so nothing asks again until the user presses 重试.
    locked: std::sync::Mutex<Option<String>>,
    /// The vault had to be set aside: told to the user once.
    notice: std::sync::Mutex<Option<String>>,
    /// One order sync at a time, without blocking the vault for the UI.
    sync: tokio::sync::Mutex<()>,
    vault_path: PathBuf,
    /// The app's identifier: on macOS it names the vault's keychain item.
    app_id: String,
    backups: PathBuf,
    cache: std::sync::Mutex<Cache>,
    /// The window material in use: "mica" (Windows 11), "vibrancy" (macOS),
    /// or "none".
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
    fn new(dir: PathBuf, app_id: String) -> State {
        State {
            vault: tokio::sync::Mutex::new(None),
            locked: std::sync::Mutex::new(None),
            notice: std::sync::Mutex::new(None),
            sync: tokio::sync::Mutex::new(()),
            vault_path: dir.join("tuken.vault"),
            app_id,
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
            if let Some(why) = self.locked.lock().unwrap().clone() {
                return Err(why);
            }
            match vault::load(&self.vault_path, &self.app_id) {
                Ok(loaded) => {
                    if loaded.lost.is_some() {
                        *self.notice.lock().unwrap() =
                            Some("这台电脑上保存的登录信息读不出来了，请重新登录".into());
                    }
                    *guard = Some(loaded.vault);
                }
                Err(why) => {
                    if cfg!(target_os = "macos") {
                        *self.locked.lock().unwrap() = Some(why.clone());
                    }
                    return Err(why);
                }
            }
        }
        Ok(tokio::sync::MutexGuard::map(guard, |v| {
            v.as_mut().expect("loaded above")
        }))
    }

    /// The vault if it is open already. Never opens it: on macOS that may
    /// ask the user, which only something the user does should.
    pub async fn vault_if_open(&self) -> Option<tokio::sync::MappedMutexGuard<'_, Vault>> {
        let guard = self.vault.lock().await;
        guard
            .is_some()
            .then(|| tokio::sync::MutexGuard::map(guard, |v| v.as_mut().expect("checked above")))
    }

    /// Why the vault can't be opened this run, if it can't.
    pub fn locked(&self) -> Option<String> {
        self.locked.lock().unwrap().clone()
    }

    /// Forgets that the vault couldn't be opened, so the next use tries again.
    pub fn unlock(&self) {
        self.locked.lock().unwrap().take();
    }

    /// What the user should know about the vault, once.
    pub fn take_notice(&self) -> Option<String> {
        self.notice.lock().unwrap().take()
    }

    pub fn save(&self, v: &Vault) -> Result<(), String> {
        vault::save(&self.vault_path, &self.app_id, v)
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

#[cfg(not(any(windows, target_os = "macos")))]
fn supports_mica() -> bool {
    false
}

/// `system`: the app follows the system's light or dark look (macOS only).
#[cfg(not(target_os = "macos"))]
fn apply_backdrop(app: &AppHandle, dark: Option<bool>, system: bool) -> &'static str {
    use tauri::window::{Effect, EffectsBuilder};
    let _ = system;
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

/// macOS: the window's vibrancy is set up in tauri.macos.conf.json
/// (`windowEffects`). Its material follows the window's appearance, so the
/// window takes the app's theme, or the system's while the app follows it.
#[cfg(target_os = "macos")]
fn apply_backdrop(app: &AppHandle, dark: Option<bool>, system: bool) -> &'static str {
    let Some(w) = app.get_webview_window("main") else {
        return "none";
    };
    if let Some(dark) = dark {
        let theme = (!system).then_some(if dark {
            tauri::Theme::Dark
        } else {
            tauri::Theme::Light
        });
        let _ = w.set_theme(theme);
    }
    if reduce_transparency() {
        "none"
    } else {
        "vibrancy"
    }
}

/// 减少透明度 (Accessibility): macOS then draws the window material as a
/// flat grey, so the page paints its own background instead.
#[cfg(target_os = "macos")]
fn reduce_transparency() -> bool {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    // SAFETY: NSWorkspace and this read-only property exist since macOS 10.10.
    unsafe {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        !workspace.is_null() && msg_send![workspace, accessibilityDisplayShouldReduceTransparency]
    }
}

/// The window material, so the page knows whether to paint its own background.
#[tauri::command]
fn backdrop(app: AppHandle, state: tauri::State<'_, State>) -> &'static str {
    let _ = app;
    *state.backdrop.lock().unwrap()
}

/// Matches the window material's tint to the app's light or dark theme.
#[tauri::command]
fn set_backdrop_theme(
    app: AppHandle,
    state: tauri::State<'_, State>,
    dark: bool,
    system: Option<bool>,
) -> &'static str {
    let used = apply_backdrop(&app, Some(dark), system.unwrap_or(false));
    *state.backdrop.lock().unwrap() = used;
    state.dark.store(dark, std::sync::atomic::Ordering::SeqCst);
    used
}

/// The system 兔啃 runs on, for what the page draws around the window.
#[tauri::command]
fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
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
        // A menu bar item opens its menu when clicked on macOS; on Windows a
        // click opens 兔啃 and the menu waits for a right click.
        .show_menu_on_left_click(cfg!(target_os = "macos"))
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
                if !cfg!(target_os = "macos") {
                    show_main(tray.app_handle());
                }
            }
        });
    // macOS: a black-and-clear template image that the menu bar tints itself,
    // 18 pt tall, drawn at 2x.
    #[cfg(target_os = "macos")]
    {
        tray = tray
            .icon(tauri::image::Image::from_bytes(include_bytes!(
                "../icons/tray/menubar@2x.png"
            ))?)
            .icon_as_template(true);
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Where the traffic lights go to sit centred in the 64 pt toolbar. Tauri
/// keeps AppKit's own offset of the buttons, which macOS 26 changed: on
/// GitHub's Macs, y 29.5 centres them on macOS 14 and 15, and macOS 26 draws
/// them 4.5 pt higher.
#[cfg(any(target_os = "macos", test))]
fn traffic_light_y(product_version: &str) -> f64 {
    let major = product_version
        .split('.')
        .next()
        .and_then(|m| m.parse::<u32>().ok())
        .unwrap_or(0);
    if major >= 26 {
        34.0
    } else {
        29.5
    }
}

/// The identifier a build keeps its data under: a debug build's ends in
/// `.dev` (as `tuken-dev.sh` makes it on Windows).
#[cfg(any(target_os = "macos", test))]
fn dev_identity(identifier: &str, debug: bool) -> String {
    if debug && !identifier.ends_with(".dev") {
        format!("{identifier}.dev")
    } else {
        identifier.to_string()
    }
}

/// macOS builds the main window here (`create: false` in
/// tauri.macos.conf.json): Tauri reads the traffic lights' place only when
/// it builds a window, and the right place depends on the macOS version.
/// It goes in the window's config: the page is a child webview (Tauri's
/// `unstable` multi-webview), and only the window applies it then.
#[cfg(target_os = "macos")]
fn create_main_window(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .cloned()
        .ok_or("找不到主窗口的设置")?;
    let version = plist::Value::from_file("/System/Library/CoreServices/SystemVersion.plist")
        .ok()
        .and_then(|v| {
            v.as_dictionary()?
                .get("ProductVersion")?
                .as_string()
                .map(str::to_string)
        })
        .unwrap_or_default();
    config.traffic_light_position = Some(tauri::utils::config::LogicalPosition {
        x: 20.0,
        y: traffic_light_y(&version),
    });
    tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?.build()?;
    Ok(())
}

/// The macOS menu bar, in Chinese like the rest of 兔啃: it also gives text
/// fields their ⌘C, ⌘V, ⌘X, ⌘A and ⌘Z, and ⌘Q quits.
#[cfg(target_os = "macos")]
fn app_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    use tauri::menu::{AboutMetadata, PredefinedMenuItem as Item, Submenu, WINDOW_SUBMENU_ID};
    let about = AboutMetadata {
        name: Some("兔啃".into()),
        version: Some(app.package_info().version.to_string()),
        ..Default::default()
    };
    let tuken = Submenu::with_items(
        app,
        "兔啃",
        true,
        &[
            &Item::about(app, Some("关于兔啃"), Some(about))?,
            &Item::separator(app)?,
            &Item::services(app, Some("服务"))?,
            &Item::separator(app)?,
            &Item::hide(app, Some("隐藏兔啃"))?,
            &Item::hide_others(app, Some("隐藏其他"))?,
            &Item::show_all(app, Some("全部显示"))?,
            &Item::separator(app)?,
            &Item::quit(app, Some("退出兔啃"))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "编辑",
        true,
        &[
            &Item::undo(app, Some("撤销"))?,
            &Item::redo(app, Some("重做"))?,
            &Item::separator(app)?,
            &Item::cut(app, Some("剪切"))?,
            &Item::copy(app, Some("拷贝"))?,
            &Item::paste(app, Some("粘贴"))?,
            &Item::select_all(app, Some("全选"))?,
        ],
    )?;
    // Tauri makes this id the Window menu, where macOS lists the windows.
    let window = Submenu::with_id_and_items(
        app,
        WINDOW_SUBMENU_ID,
        "窗口",
        true,
        &[
            &Item::minimize(app, Some("最小化"))?,
            &Item::maximize(app, Some("缩放"))?,
            &Item::separator(app)?,
            // Closing hides the window; 兔啃 keeps finishing paid orders.
            &Item::close_window(app, Some("关闭窗口"))?,
        ],
    )?;
    Menu::with_items(app, &[&tuken, &edit, &window])
}

pub fn run() {
    let builder = tauri::Builder::default()
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
            // Closing the main window keeps 兔啃 in the tray (the menu bar and
            // the Dock on macOS), where it finishes paid orders; the tray
            // menu quits, and so does ⌘Q.
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            let id = app.config().identifier.clone();
            #[cfg(not(target_os = "macos"))]
            let dir = app
                .path()
                .app_data_dir()
                .map_err(|_| "找不到应用数据目录")?;
            // macOS: a dev build (`pnpm tauri dev`) keeps its own accounts and
            // keychain item, never the installed 兔啃's.
            #[cfg(target_os = "macos")]
            let (id, dir) = {
                let id = dev_identity(&id, cfg!(debug_assertions));
                let dir = app
                    .path()
                    .data_dir()
                    .map_err(|_| "找不到应用数据目录")?
                    .join(&id);
                (id, dir)
            };
            app.manage(State::new(dir, id));
            #[cfg(target_os = "macos")]
            create_main_window(app)?;
            // What a Claude Desktop install that was cut short left behind.
            #[cfg(target_os = "macos")]
            tauri::async_runtime::spawn(desktop_mac::tidy());
            let used = apply_backdrop(app.handle(), None, false);
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
                platform,
                quit,
                panel::panel_show,
                panel::panel_hide,
            ];
            move |invoke: tauri::ipc::Invoke<tauri::Wry>| {
                // The sign-in panel, the Google window and the payment
                // window show remote pages.
                // App commands aren't covered by window capabilities, so
                // refuse them here.
                if invoke.message.webview().label() != "main" {
                    invoke.resolver.reject("这个窗口不能使用兔啃的功能");
                    return true;
                }
                handler(invoke)
            }
        });
    #[cfg(target_os = "macos")]
    let builder = builder.menu(app_menu);
    builder
        .build(tauri::generate_context!())
        .expect("error while running 兔啃")
        .run(|app, event| {
            // Clicking 兔啃 in the Dock brings back its closed window.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                show_main(app);
            }
            let _ = (app, event);
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn traffic_lights_sit_centred_on_each_macos() {
        assert_eq!(super::traffic_light_y("14.7.6"), 29.5);
        assert_eq!(super::traffic_light_y("15.7.1"), 29.5);
        assert_eq!(super::traffic_light_y("26.6.2"), 34.0);
        assert_eq!(super::traffic_light_y("27.0"), 34.0);
        assert_eq!(super::traffic_light_y(""), 29.5);
    }

    #[test]
    fn dev_builds_keep_their_own_data() {
        assert_eq!(
            super::dev_identity("site.tuken.desktop", true),
            "site.tuken.desktop.dev"
        );
        assert_eq!(
            super::dev_identity("site.tuken.desktop.dev", true),
            "site.tuken.desktop.dev"
        );
        assert_eq!(
            super::dev_identity("site.tuken.desktop", false),
            "site.tuken.desktop"
        );
    }
}
