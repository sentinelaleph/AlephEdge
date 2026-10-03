//! Signed self-update (desktop): check the feed pinned in tauri.conf.json and,
//! on the user's word, download, verify and install the new version.
//!
//! The updater plugin was registered from the first release but nothing ever
//! called it, so an installed 0.1.0 never learned that a newer build existed.
//! These two commands are the only path to it; the webview gets no updater
//! permission of its own. The plugin verifies every download against the
//! public key in tauri.conf.json before installing it.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// What the feed offers, if anything newer than the running build.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub current: String,
    pub available: Option<AvailableUpdate>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AvailableUpdate {
    pub version: String,
    pub notes: Option<String>,
    /// Publication date from the feed, UNIX ms.
    pub date_ms: Option<i64>,
}

/// Download progress for the install, `app:update-progress`.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// The word the UI sends with an install; a stray IPC call cannot restart
/// the desk into a new version on its own.
pub const UPDATE_CONFIRMATION: &str = "UPDATE";

#[tauri::command]
pub async fn app_check_update(app: AppHandle) -> Result<UpdateCheck, String> {
    check(&app).await
}

#[tauri::command]
pub async fn app_install_update(app: AppHandle, confirmation: String) -> Result<(), String> {
    if confirmation.trim() != UPDATE_CONFIRMATION {
        return Err(format!("updateConfirmRequired|{UPDATE_CONFIRMATION}"));
    }
    install(&app).await
}

#[cfg(desktop)]
async fn check(app: &AppHandle) -> Result<UpdateCheck, String> {
    use tauri_plugin_updater::UpdaterExt;
    let current = app.package_info().version.to_string();
    let updater = app.updater().map_err(|e| format!("updateCheckFailed|{e}"))?;
    let found = updater.check().await.map_err(|e| format!("updateCheckFailed|{e}"))?;
    Ok(UpdateCheck {
        current,
        available: found.map(|u| AvailableUpdate {
            version: u.version.clone(),
            notes: u.body.clone(),
            date_ms: u.date.map(|d| d.unix_timestamp() * 1000),
        }),
    })
}

#[cfg(desktop)]
async fn install(app: &AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| format!("updateCheckFailed|{e}"))?;
    let Some(update) = updater.check().await.map_err(|e| format!("updateCheckFailed|{e}"))? else {
        return Err("updateNone".to_string());
    };
    let mut downloaded: u64 = 0;
    let progress = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = progress.emit("app:update-progress", UpdateProgress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|e| format!("updateInstallFailed|{e}"))?;
    // On Windows the installer has already closed the app; elsewhere restart
    // into the new version.
    app.restart();
}

#[cfg(not(desktop))]
async fn check(app: &AppHandle) -> Result<UpdateCheck, String> {
    Ok(UpdateCheck { current: app.package_info().version.to_string(), available: None })
}

#[cfg(not(desktop))]
async fn install(_app: &AppHandle) -> Result<(), String> {
    Err("updateNone".to_string())
}
