//! Closing real positions: the manual Close / Close all (user-invoked,
//! reduce-only) and the confirmed flatten the live book's closes share.
//!
//! Closing is never gated by the dry-run list (`orders::DRY_RUN_PASSED`):
//! whatever position exists on a keyed order venue can always be closed.
//!
//! A close is done only when the ACCOUNT reads flat. A venue can fill less
//! than it was asked to (OKX contract rounding sent one lot short before
//! 2026-10-08), and a close that left a remainder used to cancel the stop and
//! report success, leaving an unprotected position nobody tracked.

use std::future::Future;

use super::model::{BinanceKeyCheckError as E, FuturesPosition};
use super::providers::binance_parse::OrderFill;
use super::ExchangeManager;
use crate::vault::model::ExchangeCredential;

/// What the account says after a reduce-only close of `size`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CloseCheck {
    /// Flat (or only someone else's opposite exposure is left: a reduce-only
    /// order cannot flip a position).
    Done,
    /// Still holds this much on the closed side.
    Remainder(f64),
    /// The account could not be read and the fills did not cover the size.
    Unconfirmed,
}

/// `held` is the account's signed amount on the symbol after the close:
/// `Some(None)` = no row (flat), `None` = the account read failed.
pub fn close_check(long: bool, size: f64, executed: f64, held: Option<Option<f64>>) -> CloseCheck {
    match held {
        Some(Some(a)) if a != 0.0 && (a > 0.0) == long => CloseCheck::Remainder(a.abs()),
        Some(_) => CloseCheck::Done,
        None if executed >= size * (1.0 - 1e-9) => CloseCheck::Done,
        None => CloseCheck::Unconfirmed,
    }
}

/// One fill for the close legs: quantities summed, price volume-weighted.
pub fn combine_fills(fills: &[OrderFill]) -> Option<OrderFill> {
    let qty: f64 = fills.iter().map(|f| f.executed_qty).sum();
    if qty <= 0.0 {
        return None;
    }
    let value: f64 = fills.iter().map(|f| f.executed_qty * f.avg_price).sum();
    Some(OrderFill {
        order_id: fills.last()?.order_id,
        avg_price: value / qty,
        executed_qty: qty,
    })
}

/// Outcome of a Close all.
#[derive(Debug, Default)]
pub struct CloseAllReport {
    /// Symbols the account now reads flat on.
    pub closed: Vec<String>,
    /// Closed, but the stop / take-profit sweep failed (subset of `closed`).
    pub uncleaned: Vec<String>,
    /// Still open.
    pub failed: Vec<(String, E)>,
}

/// Tries EVERY position, whatever happened to the one before. A Close all
/// that stopped at the first failure left the rest open after the user typed
/// CLOSE. `close` answers Ok(true) closed and swept, Ok(false) closed with
/// its orders left behind, Err still open.
pub async fn close_each<'a, F, Fut>(positions: &'a [FuturesPosition], mut close: F) -> CloseAllReport
where
    F: FnMut(&'a FuturesPosition) -> Fut,
    Fut: Future<Output = Result<bool, E>>,
{
    let mut report = CloseAllReport::default();
    for pos in positions {
        match close(pos).await {
            Ok(swept) => {
                report.closed.push(pos.symbol.clone());
                if !swept {
                    report.uncleaned.push(pos.symbol.clone());
                }
            }
            Err(e) => report.failed.push((pos.symbol.clone(), e)),
        }
    }
    report
}

/// The Close all command's answer: the count when everything closed and was
/// swept; otherwise a code the UI localizes with the symbols concerned:
/// `closeAllPartial` (still open) before `closeCleanupFailed` (closed, its
/// stop / take-profit left on the exchange).
pub fn close_all_verdict(report: &CloseAllReport) -> Result<usize, String> {
    if !report.failed.is_empty() {
        let open: Vec<&str> = report.failed.iter().map(|(s, _)| s.as_str()).collect();
        return Err(format!("closeAllPartial|{}", open.join(", ")));
    }
    if !report.uncleaned.is_empty() {
        return Err(format!("closeCleanupFailed|{}", report.uncleaned.join(", ")));
    }
    Ok(report.closed.len())
}

impl ExchangeManager {
    /// Closes the open position on `symbol` at market (reduce-only), sized
    /// from a fresh account read, never from a stale UI snapshot. Already
    /// flat is success. Ok(false): closed, but its stop / take-profit could
    /// not be cancelled.
    pub async fn close_position(&self, cred: &ExchangeCredential, symbol: &str) -> Result<bool, E> {
        let account = self.futures_account(cred).await?;
        match account.positions.iter().find(|p| p.symbol == symbol) {
            Some(pos) => self.close_one(cred, pos).await,
            None => Ok(true),
        }
    }

    /// Kill switch: closes EVERY open futures position at market, each one
    /// attempted (see `close_each`).
    pub async fn close_all(&self, cred: &ExchangeCredential) -> Result<CloseAllReport, E> {
        let account = self.futures_account(cred).await?;
        Ok(close_each(&account.positions, |pos| self.close_one(cred, pos)).await)
    }

    /// Flattens `pos` (SELL a long, BUY a short, reduce-only), confirmed on
    /// the account, then clears the symbol's stop and take-profit, which would
    /// otherwise outlive the position and act on the next one. A failed sweep
    /// does not undo the close: Ok(false).
    async fn close_one(&self, cred: &ExchangeCredential, pos: &FuturesPosition) -> Result<bool, E> {
        // One-way mode is assumed. A hedge-mode position is named instead of
        // being sent a one-way order. A mode that cannot be read does not
        // block (Bybit's read is per symbol and was missing in ccxt): a
        // reduce-only order can only shrink a position, and the venue refuses
        // it if the mode does not match.
        if let Ok(true) = self.hedge_mode(cred, &pos.symbol).await {
            return Err(E::Rejected {
                code: 0,
                msg: "the account is in Hedge Mode; close this position on the exchange".into(),
            });
        }
        let long = pos.position_amt > 0.0;
        self.flatten(cred, &pos.symbol, long, pos.position_amt.abs()).await?;
        Ok(self.cancel_symbol_orders(cred, &pos.symbol).await.is_ok())
    }

    /// Reduce-only close of `size` held on side `long`, confirmed flat on the
    /// account; a remainder gets one more reduce. Never cancels orders: the
    /// stop must stay while anything is left, so callers sweep only after Ok.
    /// The fill is volume-weighted over the legs.
    pub async fn flatten(&self, cred: &ExchangeCredential, symbol: &str, long: bool, size: f64) -> Result<OrderFill, E> {
        let mut fills = vec![self.reduce_market_all(cred, symbol, long, size).await?];
        loop {
            let executed: f64 = fills.iter().map(|f| f.executed_qty).sum();
            let held = self
                .futures_account(cred)
                .await
                .ok()
                .map(|a| a.positions.iter().find(|p| p.symbol == symbol).map(|p| p.position_amt));
            match close_check(long, size, executed, held) {
                CloseCheck::Done => return Ok(combine_fills(&fills).unwrap_or(fills[0])),
                CloseCheck::Remainder(rest) if fills.len() < 2 => {
                    fills.push(self.reduce_market_all(cred, symbol, long, rest).await?);
                }
                _ => {
                    return Err(E::Rejected {
                        code: 0,
                        msg: "the position is not flat after the close".into(),
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(symbol: &str, amt: f64) -> FuturesPosition {
        FuturesPosition { symbol: symbol.into(), position_amt: amt, entry_price: 100.0, unrealized_pnl: 0.0 }
    }

    #[test]
    fn a_close_is_done_only_when_the_account_reads_flat() {
        use CloseCheck::*;
        assert_eq!(close_check(true, 4.3, 4.2, Some(None)), Done, "no row: flat");
        assert_eq!(close_check(true, 4.3, 4.2, Some(Some(0.0))), Done);
        // OKX one lot short: 4.2 of 4.3 filled, 0.1 still held.
        assert_eq!(close_check(true, 4.3, 4.2, Some(Some(0.1))), Remainder(0.1));
        assert_eq!(close_check(false, 2.0, 1.0, Some(Some(-1.0))), Remainder(1.0));
        // Opposite exposure is not ours: a reduce-only order cannot flip.
        assert_eq!(close_check(true, 2.0, 2.0, Some(Some(-1.0))), Done);
        // Unreadable account: the fills decide.
        assert_eq!(close_check(true, 2.0, 2.0, None), Done);
        assert_eq!(close_check(true, 2.0, 1.9, None), Unconfirmed);
    }

    #[test]
    fn close_legs_combine_volume_weighted() {
        let a = OrderFill { order_id: 1, avg_price: 100.0, executed_qty: 3.0 };
        let b = OrderFill { order_id: 2, avg_price: 104.0, executed_qty: 1.0 };
        let c = combine_fills(&[a, b]).expect("filled");
        assert_eq!((c.order_id, c.executed_qty), (2, 4.0));
        assert!((c.avg_price - 101.0).abs() < 1e-12);
        assert!(combine_fills(&[]).is_none());
    }

    #[test]
    fn close_all_tries_every_position_after_a_failure() {
        let positions = [pos("BTCUSDT", 1.0), pos("ETHUSDT", -2.0), pos("SOLUSDT", 3.0)];
        let mut tried = Vec::new();
        let report = tauri::async_runtime::block_on(close_each(&positions, |p| {
            tried.push(p.symbol.clone());
            let out = match p.symbol.as_str() {
                // The first one fails outright (OKX: cancel sweep unsupported
                // used to fail the close and stop the loop here).
                "BTCUSDT" => Err(E::Rejected { code: 0, msg: "refused".into() }),
                "ETHUSDT" => Ok(false),
                _ => Ok(true),
            };
            std::future::ready(out)
        }));
        assert_eq!(tried, ["BTCUSDT", "ETHUSDT", "SOLUSDT"], "every position attempted");
        assert_eq!(report.closed, ["ETHUSDT", "SOLUSDT"]);
        assert_eq!(report.uncleaned, ["ETHUSDT"]);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(close_all_verdict(&report), Err("closeAllPartial|BTCUSDT".to_string()));
    }

    #[test]
    fn close_all_verdict_names_what_is_left() {
        let clean = CloseAllReport { closed: vec!["A".into(), "B".into()], ..Default::default() };
        assert_eq!(close_all_verdict(&clean), Ok(2));
        let swept_not = CloseAllReport { closed: vec!["A".into()], uncleaned: vec!["A".into()], failed: vec![] };
        assert_eq!(close_all_verdict(&swept_not), Err("closeCleanupFailed|A".to_string()));
        assert_eq!(close_all_verdict(&CloseAllReport::default()), Ok(0));
    }
}
