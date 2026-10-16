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

use crate::exchange::model::BinancePermissions;

use super::model::{CredentialMeta, CredentialPermission, ExchangeCredential, KeyType, VaultStatus};
use super::{KeyFacts, VaultManager};

/// New-credential input from the UI. `added_at` is stamped server-side so the UI
/// never controls it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddCredentialInput {
    pub exchange_id: String,
    pub label: String,
    pub api_key: String,
    /// The exchange's secret for an HMAC key. Empty for an Ed25519 key: its
    /// private key never crosses IPC, it is the pending one generated here.
    #[serde(default)]
    pub api_secret: String,
    pub passphrase: Option<String>,
    /// Absent = HMAC (what every UI before 0.3.15 sent).
    #[serde(default)]
    pub key_type: KeyType,
    // The UI's declared permission is not read: the vault stores only what
    // Binance itself reports (serde ignores the field the UI still sends).
}

/// Hand-written so a stray `{:?}` can never print the key, secret or passphrase.
impl std::fmt::Debug for AddCredentialInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AddCredentialInput")
            .field("exchange_id", &self.exchange_id)
            .field("label", &self.label)
            .field("api_key", &"<redacted>")
            .field("api_secret", &"<redacted>")
            .field("passphrase", &self.passphrase.as_ref().map(|_| "<redacted>"))
            .field("key_type", &self.key_type)
            .finish()
    }
}

/// The secret a new credential is stored with: the HMAC secret the user
/// pasted, or for Ed25519 the pending private seed generated in this process.
/// Only Binance takes Ed25519 keys.
fn secret_for(vault: &VaultManager, input: &AddCredentialInput) -> Result<String, String> {
    match input.key_type {
        KeyType::Hmac => Ok(input.api_secret.clone()),
        KeyType::Ed25519 if input.exchange_id != "binance" => Err("keyTypeUnsupported".to_string()),
        KeyType::Ed25519 => vault.pending_ed25519_seed().ok_or_else(|| "ed25519NoPendingKey".to_string()),
    }
}

/// Generates an Ed25519 keypair for a Binance "Self-generated" API key and
/// returns ONLY its public key (PEM) for the user to register on Binance. The
/// private key stays in this process until `vault_add_credential` /
/// `vault_replace_credential` with `keyType: "ed25519"` seals it.
#[tauri::command]
pub fn vault_ed25519_generate(vault: State<VaultManager>) -> Result<String, String> {
    vault.generate_ed25519()
}

/// The public key of the keypair still waiting for its API key, if any.
#[tauri::command]
pub fn vault_ed25519_pending(vault: State<VaultManager>) -> Option<String> {
    vault.pending_ed25519_public()
}

/// Drops the waiting keypair (the user cancelled).
#[tauri::command]
pub fn vault_ed25519_discard(vault: State<VaultManager>) {
    vault.discard_ed25519();
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
/// position unmanaged, or point the engine at another account that looks
/// flat. Close the positions and switch live off first.
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
/// `apiRestrictions` endpoint first: only a verified trade-only key
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
    // One key per exchange, checked before Binance is asked anything.
    if let Some(existing) = vault.label_in_use(&input.exchange_id) {
        return Err(super::taken(&existing, &label));
    }
    let cred = ExchangeCredential {
        exchange_id: input.exchange_id.clone(),
        label,
        api_key: input.api_key.clone(),
        api_secret: secret_for(&vault, &input)?,
        passphrase: input.passphrase.clone().filter(|p| !p.is_empty()),
        permission: CredentialPermission::Unknown,
        added_at: now_millis(),
        updated_at: None,
        key_type: input.key_type,
        exchange_created_at: None,
        ip_restricted: None,
    };
    let cred = verified(&exchange, cred).await?;
    let ed25519 = cred.key_type == KeyType::Ed25519;
    vault.add(&vault_path(&app)?, cred)?;
    if ed25519 {
        vault.discard_ed25519();
    }
    vault.list()
}

/// Renews the key material of an existing (exchange, label): the same checks
/// as adding (verified permissions, withdraw- or transfer-enabled refused),
/// then an in-place swap. The old key stays until the new one is sealed.
#[tauri::command]
pub async fn vault_replace_credential(
    app: AppHandle,
    vault: State<'_, VaultManager>,
    exchange: State<'_, ExchangeManager>,
    input: AddCredentialInput,
) -> Result<Vec<CredentialMeta>, String> {
    refuse_while_live(&app)?;
    let cred = ExchangeCredential {
        exchange_id: input.exchange_id.clone(),
        label: input.label.clone(),
        api_key: input.api_key.clone(),
        api_secret: secret_for(&vault, &input)?,
        passphrase: input.passphrase.clone().filter(|p| !p.is_empty()),
        permission: CredentialPermission::Unknown,
        added_at: 0,
        updated_at: Some(now_millis()),
        key_type: input.key_type,
        exchange_created_at: None,
        ip_restricted: None,
    };
    let cred = verified(&exchange, cred).await?;
    let ed25519 = cred.key_type == KeyType::Ed25519;
    vault.replace(&vault_path(&app)?, cred)?;
    if ed25519 {
        vault.discard_ed25519();
    }
    vault.list()
}

/// Reads a stored key's permissions, IP restriction and creation date from
/// the exchange again and stores them: the key-age and IP warnings clear
/// once the user fixed them on the exchange. A key that has since gained
/// withdrawal or transfer permission (or lost futures) is marked unverified,
/// which keeps it off real money, and the reason is returned.
#[tauri::command]
pub async fn vault_recheck_credential(
    app: AppHandle,
    vault: State<'_, VaultManager>,
    exchange: State<'_, ExchangeManager>,
    exchange_id: String,
    label: String,
) -> Result<Vec<CredentialMeta>, String> {
    let cred = vault
        .credential(&exchange_id)
        .filter(|c| c.label == label)
        .ok_or_else(|| "credentialNotFound".to_string())?;
    let path = vault_path(&app)?;
    match check_key(&exchange, &cred).await {
        Ok(facts) => {
            vault.set_key_facts(&path, &exchange_id, &label, facts)?;
            vault.list()
        }
        Err(Checked::Refused(code)) => {
            let facts = KeyFacts { permission: CredentialPermission::Unknown, ip_restricted: None, exchange_created_at: None };
            vault.set_key_facts(&path, &exchange_id, &label, facts)?;
            Err(code)
        }
        Err(Checked::Unavailable(code)) => Err(code),
    }
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
/// a week and a month (30 days): a desk left running for days should not ask
/// for the password every half hour.
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

/// Why a key check did not produce facts. `Refused`: the exchange answered
/// and the key may not be stored (or, re-checked, may not stay verified).
/// `Unavailable`: no usable answer, nothing is decided.
enum Checked {
    Refused(String),
    Unavailable(String),
}

impl Checked {
    fn code(self) -> String {
        match self {
            Checked::Refused(c) | Checked::Unavailable(c) => c,
        }
    }
}

/// Runs the permission check on `cred` and returns it carrying the verified
/// facts, or the refusal code. Nothing unverified is ever stored.
async fn verified(exchange: &ExchangeManager, mut cred: ExchangeCredential) -> Result<ExchangeCredential, String> {
    let facts = check_key(exchange, &cred).await.map_err(Checked::code)?;
    cred.permission = facts.permission;
    cred.ip_restricted = facts.ip_restricted;
    cred.exchange_created_at = facts.exchange_created_at;
    Ok(cred)
}

/// Asks the exchange what this key may do. Binance: signed `apiRestrictions`
/// (HMAC or Ed25519, per the key). Bybit / OKX: the venue's own read through
/// ccxt. Any other exchange (Bitget, MEXC...) has no order path and no
/// verifier and is refused: its permissions could only be taken on the
/// user's word, and the vault holds verified trade-only keys.
async fn check_key(exchange: &ExchangeManager, cred: &ExchangeCredential) -> Result<KeyFacts, Checked> {
    if cred.exchange_id == "binance" {
        return match exchange.check_binance_permissions(&cred.api_key, cred.binance_signer()).await {
            Ok(perms) => judge_binance(perms).map_err(Checked::Refused),
            Err(e @ BinanceKeyCheckError::InvalidCredentials { .. }) => {
                Err(Checked::Unavailable(format!("keyRejected|{}", e.detail())))
            }
            // Binance unreachable: the permissions are unknown, and an unknown
            // key could be a withdraw key. Refuse with a retryable reason
            // rather than store it, so a network hiccup never lets one in.
            Err(_) => Err(Checked::Unavailable("keyCheckUnavailable".to_string())),
        };
    }
    if crate::exchange::venue::ccxt::is_rsa_secret(&cred.api_secret) {
        return Err(Checked::Refused("keyRsaUnsupported".to_string()));
    }
    match exchange.venue_key_permissions(cred).await {
        Ok((false, _)) => Err(Checked::Refused("keyNoFutures".to_string())),
        Ok((true, withdraw)) => refuse(withdraw, false)
            .map(|()| KeyFacts { permission: CredentialPermission::TradeOnly, ip_restricted: None, exchange_created_at: None })
            .map_err(Checked::Refused),
        // No order path, so no verifier.
        Err(BinanceKeyCheckError::Rejected { code: 0, .. }) if !crate::exchange::has_order_path(&cred.exchange_id) => {
            Err(Checked::Unavailable(format!("keyExchangeUnverified|{}", cred.exchange_id)))
        }
        Err(e @ BinanceKeyCheckError::InvalidCredentials { .. }) => Err(Checked::Unavailable(format!("keyRejected|{}", e.detail()))),
        Err(_) => Err(Checked::Unavailable("keyCheckUnavailable".to_string())),
    }
}

/// Binance's answer, judged. A key that cannot trade futures would fail at
/// the first live order or manual close; withdraw and transfer keys never
/// enter the vault. IP restriction and age are kept for the warnings.
fn judge_binance(p: BinancePermissions) -> Result<KeyFacts, String> {
    if !p.futures_enabled {
        return Err("keyNoFutures".to_string());
    }
    refuse(p.withdraw_enabled, p.transfer_enabled)?;
    Ok(KeyFacts {
        permission: CredentialPermission::TradeOnly,
        ip_restricted: p.ip_restricted,
        exchange_created_at: p.created_at,
    })
}

/// A key that can withdraw funds, or move them to another account by
/// internal or universal transfer, never enters the vault. Aleph Edge only
/// trades; a leaked withdraw key empties the account, and a leaked transfer
/// key can move funds to an account the attacker controls without a
/// withdrawal.
fn refuse(withdraw: bool, transfer: bool) -> Result<(), String> {
    if withdraw {
        Err("keyWithdrawEnabled".to_string())
    } else if transfer {
        Err("keyTransferEnabled".to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perms(withdraw: bool, transfer: bool, futures: bool) -> BinancePermissions {
        BinancePermissions {
            withdraw_enabled: withdraw,
            transfer_enabled: transfer,
            futures_enabled: futures,
            ip_restricted: Some(false),
            created_at: Some(42),
        }
    }

    #[test]
    fn a_withdraw_enabled_key_is_refused() {
        assert_eq!(refuse(true, false).unwrap_err(), "keyWithdrawEnabled");
        assert_eq!(refuse(true, true).unwrap_err(), "keyWithdrawEnabled", "withdraw is named first");
        assert_eq!(judge_binance(perms(true, false, true)).unwrap_err(), "keyWithdrawEnabled");
    }

    #[test]
    fn a_transfer_enabled_key_is_refused() {
        assert_eq!(refuse(false, true).unwrap_err(), "keyTransferEnabled");
        assert_eq!(judge_binance(perms(false, true, true)).unwrap_err(), "keyTransferEnabled");
        assert_eq!(judge_binance(perms(false, true, false)).unwrap_err(), "keyNoFutures");
    }

    #[test]
    fn a_trade_only_key_is_stored_with_its_ip_and_age_facts() {
        let f = judge_binance(perms(false, false, true)).unwrap();
        assert_eq!(f.permission, CredentialPermission::TradeOnly);
        assert_eq!((f.ip_restricted, f.exchange_created_at), (Some(false), Some(42)));
    }

    #[test]
    fn an_ed25519_key_takes_the_pending_seed_and_only_on_binance() {
        let vault = VaultManager::new();
        let input = |exchange_id: &str, key_type| AddCredentialInput {
            exchange_id: exchange_id.into(),
            label: "main".into(),
            api_key: "k".into(),
            api_secret: "pasted".into(),
            passphrase: None,
            key_type,
        };
        assert_eq!(secret_for(&vault, &input("binance", KeyType::Hmac)).unwrap(), "pasted");
        assert_eq!(secret_for(&vault, &input("binance", KeyType::Ed25519)).unwrap_err(), "ed25519NoPendingKey");
        assert_eq!(secret_for(&vault, &input("okx", KeyType::Ed25519)).unwrap_err(), "keyTypeUnsupported");
    }

    #[test]
    fn an_old_ui_payload_is_an_hmac_key_and_the_input_debug_is_redacted() {
        let input: AddCredentialInput = serde_json::from_str(
            r#"{"exchangeId":"binance","label":"m","apiKey":"KEY-1","apiSecret":"SEC-1","permission":"tradeOnly"}"#,
        )
        .unwrap();
        assert_eq!(input.key_type, KeyType::Hmac);
        let out = format!("{input:?}");
        assert!(!out.contains("KEY-1") && !out.contains("SEC-1"), "{out}");
        let ed: AddCredentialInput =
            serde_json::from_str(r#"{"exchangeId":"binance","label":"m","apiKey":"k","keyType":"ed25519"}"#).unwrap();
        assert_eq!((ed.key_type, ed.api_secret.as_str()), (KeyType::Ed25519, ""));
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
