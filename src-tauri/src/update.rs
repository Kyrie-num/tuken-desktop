//! Updates from 兔啃's Cloudflare R2 release feed. Each installer is signed with the
//! release key, and the updater checks that signature before running it.

use serde_json::{json, Value};
use tauri::AppHandle;
use tauri_plugin_updater::{Update, UpdaterExt};

async fn newer(app: &AppHandle) -> Result<Option<Update>, String> {
    app.updater()
        .map_err(|_| "无法检查更新".to_string())?
        .check()
        .await
        .map_err(|_| "没能检查更新，请稍后再试".to_string())
}

/// The newer version, if there is one: `{version, notes}`, else `null`.
pub async fn check(app: &AppHandle) -> Result<Value, String> {
    Ok(newer(app).await?.map_or(
        Value::Null,
        |u| json!({ "version": u.version, "notes": u.body }),
    ))
}

/// Downloads the newer version and runs its installer, which closes 兔啃 and
/// starts the new version.
pub async fn install(app: &AppHandle) -> Result<Value, String> {
    // A dev build would install the released 兔啃 over the user's own.
    if cfg!(debug_assertions) {
        return Err("开发版不安装更新".into());
    }
    let update = newer(app).await?.ok_or("已经是最新版本了")?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|_| "更新没有装上，请稍后再试")?;
    app.restart()
}
