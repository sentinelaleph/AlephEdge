//! Signed self-update (desktop): check the feed pinned in tauri.conf.json and,
//! on the user's word, download, verify and install the new version.
//!
//! These two commands are the only path to the updater plugin; the webview
//! gets no updater permission of its own. The plugin verifies every download
//! against the public key in tauri.conf.json before installing it.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// What the feed offers, if anything newer than the running build.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub current: String,
    pub available: Option<AvailableUpdate>,
    /// This build does not update itself (the TESTNET identity): the feed is
    /// never read and nothing is offered.
    pub off: bool,
}

/// The public feed ships the normal build under the normal identity.
/// Offered to the TESTNET build, its installer would replace the normal app
/// and close the testnet one, so that build never reads the feed.
pub fn updates_off(identifier: &str) -> bool {
    identifier.ends_with(".testnet")
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
    if updates_off(&app.config().identifier) {
        return Ok(UpdateCheck { current: app.package_info().version.to_string(), available: None, off: true });
    }
    check(&app).await
}

/// Why an install may not run (pure): the typed word, and the TESTNET
/// identity, which never installs the public feed's build.
///
/// No real-money check: since 0.3.19 the feed ships the one build every
/// install runs (`LIVE_TRADING_ENABLED` is always on, lib.rs), so an update
/// can no longer put a paper build over a Live install. Installing restarts
/// the app like any restart: Live bots come back stopped until their typed
/// LIVE, real positions stay under their exchange stops and are managed
/// again on the next start.
pub fn install_refusal(identifier: &str, confirmation: &str) -> Option<String> {
    if confirmation.trim() != UPDATE_CONFIRMATION {
        return Some(format!("updateConfirmRequired|{UPDATE_CONFIRMATION}"));
    }
    if updates_off(identifier) {
        return Some("updateOffTestnet".to_string());
    }
    None
}

#[tauri::command]
pub async fn app_install_update(app: AppHandle, confirmation: String) -> Result<(), String> {
    if let Some(refusal) = install_refusal(&app.config().identifier, &confirmation) {
        return Err(refusal);
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
        off: false,
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
    // Before any byte is downloaded: a copy of the database, the settings
    // file and the vault (app/backup.rs). No backup, no install.
    let from = app.package_info().version.to_string();
    crate::app::backup::take_now(app, crate::app::backup::Reason::Update { from, to: update.version.clone() })
        .map_err(|e| format!("updateBackupFailed|{e}"))?;
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
    Ok(UpdateCheck { current: app.package_info().version.to_string(), available: None, off: false })
}

#[cfg(not(desktop))]
async fn install(_app: &AppHandle) -> Result<(), String> {
    Err("updateNone".to_string())
}

#[cfg(test)]
mod tests {
    use super::updates_off;

    #[test]
    fn only_the_testnet_identity_skips_the_public_feed() {
        assert!(updates_off("com.sentinelaleph.edge.testnet"));
        assert!(!updates_off("com.sentinelaleph.edge"));
    }
}
