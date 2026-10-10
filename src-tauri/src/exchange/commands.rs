//! Tauri command surface for exchange adapters.

use tauri::State;

use super::model::{BinanceKeyCheckError, ExchangeInfo, ExchangeStatus, FuturesAccount};
use super::ExchangeManager;
use crate::vault::model::ExchangeCredential;
use crate::vault::VaultManager;

/// The catalog of supported exchanges (id, name, futures support).
#[tauri::command]
pub fn exchange_list(exchange: State<ExchangeManager>) -> Vec<ExchangeInfo> {
    exchange.list()
}

/// Tradable USDT symbols of one exchange market (see ExchangeManager::symbols).
#[tauri::command]
pub async fn exchange_symbols(
    exchange: State<'_, ExchangeManager>,
    exchange_id: String,
    futures: bool,
) -> Result<Vec<String>, String> {
    Ok(exchange.symbols(&exchange_id, futures).await)
}

#[tauri::command]
pub async fn exchange_status(
    exchange: State<'_, ExchangeManager>,
    exchange_id: String,
    symbol: String,
) -> Result<ExchangeStatus, String> {
    Ok(exchange.status(&exchange_id, &symbol).await)
}

/// The connected exchange's USDT-M futures account (wallet balance + open
/// positions), read with the vaulted key. Binance-only today; the vault must be
/// unlocked and hold a key for the exchange. The secret is read in-process and
/// never leaves the device.
#[tauri::command]
pub async fn exchange_account(
    vault: State<'_, VaultManager>,
    exchange: State<'_, ExchangeManager>,
    exchange_id: String,
) -> Result<FuturesAccount, String> {
    let cred = binance_credential(&vault, &exchange_id)?;
    exchange
        .futures_account(&cred)
        .await
        .map_err(explain)
}

/// Manually closes ONE open futures position at market (reduce-only). This is a
/// user-initiated action on their REAL account — distinct from the simulation
/// bots — so it places a real order with the vaulted Trade-capable key. A
/// symbol that is already flat resolves successfully (nothing to do).
#[tauri::command]
pub async fn exchange_close_position(
    vault: State<'_, VaultManager>,
    exchange: State<'_, ExchangeManager>,
    exchange_id: String,
    symbol: String,
    confirmation: String,
) -> Result<(), String> {
    check_close_confirmation(&confirmation)?;
    let cred = binance_credential(&vault, &exchange_id)?;
    match exchange.close_position(&cred, &symbol).await {
        Ok(true) => Ok(()),
        // Closed; its stop / take-profit is still on the exchange.
        Ok(false) => Err(format!("closeCleanupFailed|{symbol}")),
        Err(e) => Err(explain(e)),
    }
}

/// Kill switch: closes EVERY open futures position at market, each one
/// attempted. Returns how many were closed; any position left open (or
/// closed with its orders left behind) is named in the error instead.
/// Real orders on the user's real account (see above).
#[tauri::command]
pub async fn exchange_close_all(
    vault: State<'_, VaultManager>,
    exchange: State<'_, ExchangeManager>,
    exchange_id: String,
    confirmation: String,
) -> Result<usize, String> {
    check_close_confirmation(&confirmation)?;
    let cred = binance_credential(&vault, &exchange_id)?;
    let report = exchange.close_all(&cred).await.map_err(explain)?;
    super::close::close_all_verdict(&report)
}

/// The word the close dialogs make the user type. Checked here as well, so a
/// stray or scripted invoke cannot place a real close order on its own.
pub const CLOSE_CONFIRMATION: &str = "CLOSE";

fn check_close_confirmation(confirmation: &str) -> Result<(), String> {
    if confirmation.trim() == CLOSE_CONFIRMATION {
        Ok(())
    } else {
        Err("closeConfirmRequired".to_string())
    }
}

/// Resolves the vaulted Binance credential, rejecting non-Binance ids (the only
/// exchange with a signed account/order path today) and a missing key.
///
/// Returns the credential itself rather than `(String, String)` of its secrets.
/// The tuple version moved the key and secret out of the credential, which left
/// two owned `String`s behind with nothing to wipe them — the credential whose
/// `Drop` would have done it had been emptied. Callers borrow instead, and the
/// credential is wiped when it leaves the command.
fn binance_credential(
    vault: &VaultManager,
    exchange_id: &str,
) -> Result<ExchangeCredential, String> {
    if !super::has_order_path(exchange_id) {
        return Err("exchangeBinanceOnly".to_string());
    }
    vault
        .credential(exchange_id)
        .ok_or_else(|| "noVaultedKey".to_string())
}

/// Maps a Binance call error to a stable code the UI localizes (`errors.*`);
/// Binance's own code and text ride along as detail.
fn explain(e: BinanceKeyCheckError) -> String {
    match e {
        BinanceKeyCheckError::InvalidCredentials => "binanceKeyRejected".to_string(),
        BinanceKeyCheckError::NetworkUnavailable => "binanceUnreachable".to_string(),
        BinanceKeyCheckError::Rejected { code, msg } => format!("binanceRefused|{code}: {msg}"),
        BinanceKeyCheckError::RateLimited => "binanceRateLimited".to_string(),
        BinanceKeyCheckError::Unknown => "binanceUnconfirmed".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_close_without_the_typed_word_is_refused() {
        assert_eq!(check_close_confirmation(""), Err("closeConfirmRequired".to_string()));
        assert_eq!(check_close_confirmation("close"), Err("closeConfirmRequired".to_string()));
        assert_eq!(check_close_confirmation(" CLOSE "), Ok(()));
    }

    #[test]
    fn binance_failures_are_codes_with_binance_text_as_detail() {
        assert_eq!(explain(BinanceKeyCheckError::InvalidCredentials), "binanceKeyRejected");
        assert_eq!(explain(BinanceKeyCheckError::NetworkUnavailable), "binanceUnreachable");
        assert_eq!(
            explain(BinanceKeyCheckError::Rejected {
                code: -2019,
                msg: "Margin is insufficient.".into()
            }),
            "binanceRefused|-2019: Margin is insufficient."
        );
        assert_eq!(explain(BinanceKeyCheckError::RateLimited), "binanceRateLimited");
        assert_eq!(explain(BinanceKeyCheckError::Unknown), "binanceUnconfirmed");
    }
}
