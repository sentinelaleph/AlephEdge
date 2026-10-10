//! Tauri command surface for the vault.
//!
//! Commands take only what the UI can safely provide and return only
//! secret-free types. The vault file lives in the OS app-data directory.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::exchange::model::BinanceKeyCheckError;
use crate::exchange::ExchangeManager;

use super::model::{CredentialMeta, CredentialPermission, ExchangeCredential, VaultStatus};
use super::VaultManager;

/// New-credential input from the UI. `added_at` is stamped server-side so the UI
/// never controls it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddCredentialInput {
    pub exchange_id: String,
    pub label: String,
    pub api_key: String,
    pub api_secret: String,
    pub passphrase: Option<String>,
    // The UI's declared permission is not read: the vault stores only what
    // Binance itself reports (serde ignores the field the UI still sends).
}

/// The vault file's location. `pub(super)` because the idle watchdog needs the
/// same path to report status after an auto-lock, and a second copy of this
/// join is a second place for the filename to drift.
pub(crate) fn vault_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "appDataDirUnavailable".to_string())?;
    Ok(dir.join("vault.edge"))
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Shortest vault password accepted (the UI's rule, enforced here).
pub const MIN_VAULT_PASSWORD_CHARS: usize = 8;

#[tauri::command]
pub fn vault_status(app: AppHandle, vault: State<VaultManager>) -> Result<VaultStatus, String> {
    Ok(vault.status(&vault_path(&app)?))
}

#[tauri::command]
pub fn vault_create(
    app: AppHandle,
    vault: State<VaultManager>,
    password: String,
) -> Result<VaultStatus, String> {
    // The UI enforces this too; the command is the boundary that counts.
    if password.chars().count() < MIN_VAULT_PASSWORD_CHARS {
        return Err(format!("vaultPasswordTooShort|{MIN_VAULT_PASSWORD_CHARS}"));
    }
    let path = vault_path(&app)?;
    vault.create(&path, &password)?;
    Ok(vault.status(&path))
}

#[tauri::command]
pub fn vault_unlock(
    app: AppHandle,
    vault: State<VaultManager>,
    password: String,
) -> Result<VaultStatus, String> {
    let path = vault_path(&app)?;
    vault.unlock(&path, &password)?;
    Ok(vault.status(&path))
}

/// While real money is in play the engine needs the key for every stop move,
/// partial, exit and emergency flatten, and it reads "the first Binance key".
/// Locking, resetting, renewing or removing a key then would leave a real
/// position unmanaged, or point the engine at another account that looks flat
/// (audit 2026-10-04, M1/M2). Close the positions and switch live off first.
fn refuse_while_live(app: &AppHandle) -> Result<(), String> {
    if crate::bot::strategy_live::live_exposure(app) {
        return Err("vaultBusyLive".to_string());
    }
    Ok(())
}

#[tauri::command]
pub fn vault_lock(app: AppHandle, vault: State<VaultManager>) -> Result<VaultStatus, String> {
    refuse_while_live(&app)?;
    vault.lock();
    Ok(vault.status(&vault_path(&app)?))
}

/// User activity in the window (pointer or key input, throttled by the UI).
/// Resets the auto-lock budget of an unlocked vault; a vault whose budget is
/// already spent locks here, and the UI hears it through the same event the
/// watchdog sends.
#[tauri::command]
pub fn vault_touch(app: AppHandle, vault: State<VaultManager>) -> Result<VaultStatus, String> {
    let locked = vault.touch_if_unlocked();
    let status = vault.status(&vault_path(&app)?);
    if locked {
        let _ = app.emit(super::watchdog::AUTO_LOCKED_EVENT, status.clone());
    }
    Ok(status)
}

pub const VAULT_RESET_CONFIRMATION: &str = "RESET";

/// Destroys the vault + keychain salt (no password recovery — this is the
/// escape hatch). Returns the fresh Absent status so the UI shows "create".
/// Needs the typed word from the reset dialog, checked here too.
#[tauri::command]
pub fn vault_reset(app: AppHandle, vault: State<VaultManager>, confirmation: String) -> Result<VaultStatus, String> {
    if confirmation.trim() != VAULT_RESET_CONFIRMATION {
        return Err("resetConfirmRequired".to_string());
    }
    refuse_while_live(&app)?;
    let path = vault_path(&app)?;
    vault.reset(&path)?;
    Ok(vault.status(&path))
}

/// Adds a credential. Its REAL permissions are read from Binance's signed
/// `apiRestrictions` endpoint first (PRD §5.5): only a verified trade-only key
/// is stored. Exchanges without a verifier, keys that can withdraw, and keys
/// Binance could not be asked about are refused.
#[tauri::command]
pub async fn vault_add_credential(
    app: AppHandle,
    vault: State<'_, VaultManager>,
    exchange: State<'_, ExchangeManager>,
    input: AddCredentialInput,
) -> Result<Vec<CredentialMeta>, String> {
    let label = checked_label(&input.label)?;
    let permission = detect_permission(&exchange, &input).await?;
    let cred = ExchangeCredential {
        exchange_id: input.exchange_id.clone(),
        label,
        api_key: input.api_key.clone(),
        api_secret: input.api_secret.clone(),
        passphrase: input.passphrase.clone().filter(|p| !p.is_empty()),
        permission,
        added_at: now_millis(),
        updated_at: None,
    };
    vault.add(&vault_path(&app)?, cred)?;
    vault.list()
}

/// Renews the key material of an existing (exchange, label): the same checks
/// as adding (verified permissions, withdraw-enabled refused), then an
/// in-place swap. The old key stays until the new one is sealed.
#[tauri::command]
pub async fn vault_replace_credential(
    app: AppHandle,
    vault: State<'_, VaultManager>,
    exchange: State<'_, ExchangeManager>,
    input: AddCredentialInput,
) -> Result<Vec<CredentialMeta>, String> {
    refuse_while_live(&app)?;
    let permission = detect_permission(&exchange, &input).await?;
    let cred = ExchangeCredential {
        exchange_id: input.exchange_id.clone(),
        label: input.label.clone(),
        api_key: input.api_key.clone(),
        api_secret: input.api_secret.clone(),
        passphrase: input.passphrase.clone().filter(|p| !p.is_empty()),
        permission,
        added_at: 0,
        updated_at: Some(now_millis()),
    };
    vault.replace(&vault_path(&app)?, cred)?;
    vault.list()
}

#[tauri::command]
pub fn vault_rename_credential(
    app: AppHandle,
    vault: State<VaultManager>,
    exchange_id: String,
    label: String,
    new_label: String,
) -> Result<Vec<CredentialMeta>, String> {
    vault.rename(&vault_path(&app)?, &exchange_id, &label, &new_label)?;
    vault.list()
}

#[tauri::command]
pub fn vault_change_password(
    app: AppHandle,
    vault: State<VaultManager>,
    current: String,
    new_password: String,
) -> Result<VaultStatus, String> {
    if new_password.chars().count() < MIN_VAULT_PASSWORD_CHARS {
        return Err(format!("vaultPasswordTooShort|{MIN_VAULT_PASSWORD_CHARS}"));
    }
    if new_password == current {
        return Err("vaultPasswordUnchanged".to_string());
    }
    let path = vault_path(&app)?;
    vault.change_password(&path, &current, &new_password)?;
    Ok(vault.status(&path))
}

/// Auto-lock choices offered in Settings: minutes, then 4/8/12/24 hours,
/// a week and a month (30 days). Owner request 2026-10-05: a desk left
/// running for days should not ask for the password every half hour.
pub const IDLE_MINUTE_CHOICES: [u64; 10] = [5, 15, 30, 60, 240, 480, 720, 1_440, 10_080, 43_200];

/// Meta key holding the user's auto-lock choice (overrides the env default).
pub const IDLE_MINUTES_META: &str = "vault_idle_minutes";

#[tauri::command]
pub fn vault_set_idle_minutes(
    app: AppHandle,
    vault: State<VaultManager>,
    store: State<crate::store::StoreManager>,
    minutes: u64,
) -> Result<VaultStatus, String> {
    if !IDLE_MINUTE_CHOICES.contains(&minutes) {
        return Err("vaultIdleInvalid".to_string());
    }
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "appDataDirUnavailable".to_string())?;
    store.set_meta(&dir, IDLE_MINUTES_META, &minutes.to_string())?;
    vault.set_idle_minutes(minutes);
    Ok(vault.status(&vault_path(&app)?))
}

/// Applies a saved auto-lock choice at launch. An unreadable or foreign
/// value keeps the configured default.
pub fn restore_idle_minutes(app: &AppHandle) {
    let Ok(dir) = app.path().app_data_dir() else { return };
    let saved = app
        .state::<crate::store::StoreManager>()
        .get_meta(&dir, IDLE_MINUTES_META)
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|m| IDLE_MINUTE_CHOICES.contains(m));
    if let Some(m) = saved {
        app.state::<VaultManager>().set_idle_minutes(m);
    }
}

fn checked_label(raw: &str) -> Result<String, String> {
    let label = raw.trim();
    if label.is_empty() || label.chars().count() > super::MAX_LABEL_CHARS {
        return Err(format!("credentialLabelInvalid|{}", super::MAX_LABEL_CHARS));
    }
    Ok(label.to_string())
}

async fn detect_permission(
    exchange: &ExchangeManager,
    input: &AddCredentialInput,
) -> Result<CredentialPermission, String> {
    if input.exchange_id != "binance" {
        return detect_venue_permission(exchange, input).await;
    }
    match exchange
        .check_binance_permissions(&input.api_key, &input.api_secret)
        .await
    {
        // A key that cannot trade futures would fail at the first live order
        // or manual close; refuse it now, with the reason.
        Ok(perms) if !perms.futures_enabled => Err("keyNoFutures".to_string()),
        Ok(perms) => refuse_withdraw(if perms.withdraw_enabled {
            CredentialPermission::WithdrawEnabled
        } else {
            CredentialPermission::TradeOnly
        }),
        Err(BinanceKeyCheckError::InvalidCredentials) => {
            Err("keyRejected".to_string())
        }
        // Binance unreachable: the permissions are unknown, and an unknown
        // key could be a withdraw key. Refuse with a retryable reason rather
        // than store it (the old "Unknown" let withdraw keys in on a network
        // hiccup).
        Err(_) => Err("keyCheckUnavailable".to_string()),
    }
}

/// Bybit / OKX through ccxt: the venue's own permission read. Any other
/// exchange (Bitget, MEXC...) has no order path and no verifier and is
/// refused: its permissions could only be taken on the user's word, and the
/// vault holds verified trade-only keys.
async fn detect_venue_permission(exchange: &ExchangeManager, input: &AddCredentialInput) -> Result<CredentialPermission, String> {
    let probe = ExchangeCredential {
        exchange_id: input.exchange_id.clone(),
        label: String::new(),
        api_key: input.api_key.clone(),
        api_secret: input.api_secret.clone(),
        passphrase: input.passphrase.clone().filter(|p| !p.is_empty()),
        permission: CredentialPermission::Unknown,
        added_at: 0,
        updated_at: None,
    };
    match exchange.venue_key_permissions(&probe).await {
        Ok((false, _)) => Err("keyNoFutures".to_string()),
        Ok((true, withdraw)) => refuse_withdraw(if withdraw {
            CredentialPermission::WithdrawEnabled
        } else {
            CredentialPermission::TradeOnly
        }),
        // No order path, so no verifier.
        Err(BinanceKeyCheckError::Rejected { code: 0, .. }) if !crate::exchange::has_order_path(&input.exchange_id) => {
            Err(format!("keyExchangeUnverified|{}", input.exchange_id))
        }
        Err(BinanceKeyCheckError::InvalidCredentials) => Err("keyRejected".to_string()),
        Err(_) => Err("keyCheckUnavailable".to_string()),
    }
}

/// Owner decision 2026-10-03: a key that can withdraw funds never enters the
/// vault. Aleph Edge only trades; a leaked withdraw key empties the account.
fn refuse_withdraw(p: CredentialPermission) -> Result<CredentialPermission, String> {
    if p == CredentialPermission::WithdrawEnabled {
        Err("keyWithdrawEnabled".to_string())
    } else {
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_withdraw_enabled_key_is_refused() {
        assert_eq!(refuse_withdraw(CredentialPermission::WithdrawEnabled).unwrap_err(), "keyWithdrawEnabled");
        assert_eq!(refuse_withdraw(CredentialPermission::TradeOnly).unwrap(), CredentialPermission::TradeOnly);
        assert_eq!(refuse_withdraw(CredentialPermission::Unknown).unwrap(), CredentialPermission::Unknown);
    }

    #[test]
    fn labels_are_trimmed_and_bounded() {
        assert_eq!(checked_label("  main ").unwrap(), "main");
        assert!(checked_label("   ").is_err());
        assert!(checked_label(&"x".repeat(41)).is_err());
    }
}

#[tauri::command]
pub fn vault_list_credentials(vault: State<VaultManager>) -> Result<Vec<CredentialMeta>, String> {
    vault.list()
}

#[tauri::command]
pub fn vault_remove_credential(
    app: AppHandle,
    vault: State<VaultManager>,
    exchange_id: String,
    label: String,
) -> Result<Vec<CredentialMeta>, String> {
    refuse_while_live(&app)?;
    vault.remove(&vault_path(&app)?, &exchange_id, &label)?;
    vault.list()
}
