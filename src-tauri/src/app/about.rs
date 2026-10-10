//! Settings, About: what this installation is, and the few places it may send
//! the user outside the app.
//!
//! The webview has no opener permission (capabilities/default.json grants
//! `core:default` only), so it cannot open an arbitrary URL or path. These
//! commands open a fixed list instead: the Sentinel user guide and account
//! pages (derived from the configured API base, like every other host), the public source and
//! licence, the support address, and this app's own data folder.

use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    /// "windows", "macos", "linux".
    pub os: String,
    /// "x86_64", "aarch64".
    pub arch: String,
    pub tauri: String,
    /// The WebView runtime's version; None when the platform does not say.
    pub webview: Option<String>,
    /// Compiled with the `live` feature: real orders are possible after the
    /// per-bot opt-in.
    pub live_build: bool,
    /// The bundle identifier: the TESTNET build has its own.
    pub identifier: String,
    /// Where the vault, the database and the bot state live.
    pub data_dir: Option<String>,
}

#[tauri::command]
pub fn app_info(app: tauri::AppHandle) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        tauri: tauri::VERSION.to_string(),
        webview: tauri::webview_version().ok(),
        live_build: cfg!(feature = "live"),
        identifier: app.config().identifier.clone(),
        data_dir: app.path().app_data_dir().ok().map(|d| d.display().to_string()),
    }
}

/// The places About may open. Anything else is not representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HelpLink {
    /// The user guide on the Sentinel site this desk talks to.
    UserGuide,
    /// The account pages on that site: profile and password, billing, API keys.
    WebAccount,
    WebBilling,
    WebApiKeys,
    Source,
    Releases,
    Licence,
    Support,
    Security,
}

pub const REPO_URL: &str = "https://github.com/sentinelaleph/AlephEdge";
pub const LICENCE_URL: &str = "https://polyformproject.org/licenses/perimeter/1.0.1";
pub const SUPPORT_EMAIL: &str = "support@ribqa.com";

/// The URL for a link, given the configured Sentinel API base.
pub fn help_url(link: HelpLink, api_base: &str) -> String {
    match link {
        HelpLink::UserGuide => format!("{}/edge", api_base.trim_end_matches('/')),
        HelpLink::WebAccount => format!("{}/account", api_base.trim_end_matches('/')),
        HelpLink::WebBilling => format!("{}/account/billing", api_base.trim_end_matches('/')),
        HelpLink::WebApiKeys => format!("{}/account/api-keys", api_base.trim_end_matches('/')),
        HelpLink::Source => REPO_URL.to_string(),
        HelpLink::Releases => format!("{REPO_URL}/releases"),
        HelpLink::Licence => LICENCE_URL.to_string(),
        HelpLink::Support => format!("mailto:{SUPPORT_EMAIL}"),
        HelpLink::Security => format!("mailto:{SUPPORT_EMAIL}?subject=security"),
    }
}

#[tauri::command]
pub fn open_help_link(app: tauri::AppHandle, link: HelpLink) -> Result<(), String> {
    let url = help_url(link, &crate::app::endpoints::api_base());
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

/// Opens the app's data folder in the file manager.
#[tauri::command]
pub fn open_data_dir(app: tauri::AppHandle) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    app.opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_user_guide_follows_the_configured_deployment() {
        assert_eq!(help_url(HelpLink::UserGuide, "https://ribqa.com"), "https://ribqa.com/edge");
        assert_eq!(help_url(HelpLink::UserGuide, "https://staging.example.com/"), "https://staging.example.com/edge");
        assert_eq!(help_url(HelpLink::WebAccount, "https://ribqa.com"), "https://ribqa.com/account");
        assert_eq!(help_url(HelpLink::WebBilling, "https://ribqa.com/"), "https://ribqa.com/account/billing");
        assert_eq!(help_url(HelpLink::WebApiKeys, "https://ribqa.com"), "https://ribqa.com/account/api-keys");
    }

    #[test]
    fn fixed_links_are_the_public_ones() {
        assert_eq!(help_url(HelpLink::Source, "x"), "https://github.com/sentinelaleph/AlephEdge");
        assert_eq!(help_url(HelpLink::Releases, "x"), "https://github.com/sentinelaleph/AlephEdge/releases");
        assert_eq!(help_url(HelpLink::Support, "x"), "mailto:support@ribqa.com");
        assert_eq!(help_url(HelpLink::Security, "x"), "mailto:support@ribqa.com?subject=security");
    }

    #[test]
    fn an_unknown_link_does_not_deserialize() {
        assert!(serde_json::from_str::<HelpLink>("\"userGuide\"").is_ok());
        assert!(serde_json::from_str::<HelpLink>("\"https://evil.example\"").is_err());
    }
}
