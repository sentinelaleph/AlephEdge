//! Whether the data every bot runs on was read: the risk settings file and
//! the store. When either could not be read, no bot starts, resumes or goes
//! live (`BotManager::start`, `bot_set_live`, `strategy_start`, the DCA /
//! Grid restore): a bot must never trade on defaults that stand in for
//! settings the user chose, or on a book it cannot persist.

use tauri::{AppHandle, Manager};

use crate::risk::RiskManager;
use crate::store::StoreManager;

/// Code when the risk settings file could not be read (see `risk::load`).
pub const RISK_CONFIG_UNREADABLE: &str = "riskConfigUnreadable";

/// Why trading is blocked, from what was read (pure). The risk file wins:
/// it is the one the user can fix on the risk page.
pub fn blocked_code(risk_unreadable: bool, store_error: Option<String>) -> Option<String> {
    if risk_unreadable {
        return Some(RISK_CONFIG_UNREADABLE.to_string());
    }
    store_error
}

/// Why no bot may start now, as an i18n code, or None.
pub fn trading_blocked(app: &AppHandle) -> Option<String> {
    let risk_unreadable = app.state::<RiskManager>().unreadable().is_some();
    let store_error = match app.path().app_data_dir() {
        Ok(dir) => app.state::<StoreManager>().open_error(&dir),
        Err(_) => Some("appDataDirUnavailable".to_string()),
    };
    blocked_code(risk_unreadable, store_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn either_unreadable_source_blocks_and_the_risk_file_is_named_first() {
        assert_eq!(blocked_code(false, None), None);
        assert_eq!(blocked_code(true, None).as_deref(), Some(RISK_CONFIG_UNREADABLE));
        assert_eq!(blocked_code(false, Some("storeSchemaNewer|2".into())).as_deref(), Some("storeSchemaNewer|2"));
        assert_eq!(blocked_code(true, Some("storeOpenFailed".into())).as_deref(), Some(RISK_CONFIG_UNREADABLE));
    }
}
