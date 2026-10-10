//! Closed-trade records, pure: the NET record a close persists and the
//! veto-stamped variant. Split out of `book.rs` (2026-09-22) so the headless
//! paper runner writes byte-for-byte the same `trades` rows as the desktop.

use crate::signal::model::RegimeVetoEvent;
use crate::store::model::TradeRecord;

use super::super::model::OpenPosition;
use super::pnl;

/// The trade record for a position closed by a veto: identical to a normal
/// close (`trade_record`) but stamped with Sentinel's reason so the desk can
/// show WHY, not just that it happened. Pure — no I/O — so it is unit-tested
/// directly rather than through the async close path above.
pub fn veto_trade_record(
    pos: &OpenPosition,
    price: f64,
    veto: &RegimeVetoEvent,
    closed_at: u64,
) -> TradeRecord {
    let mut record = trade_record(pos, price, "veto", closed_at);
    stamp_veto_reason(&mut record, veto);
    record
}

pub fn stamp_veto_reason(record: &mut TradeRecord, veto: &RegimeVetoEvent) {
    record.veto_reason_code = Some(veto.reason_code.clone());
    record.veto_reason_text = Some(veto.reason_text.clone());
}

/// The persisted NET trade record (fees included, partial-blended), plus
/// the ledger-parity `unlevered_net_pct`.
pub fn trade_record(pos: &OpenPosition, exit: f64, reason: &str, closed_at: u64) -> TradeRecord {
    let pnl_pct = if reason == super::management::EXIT_LIQUIDATION {
        pnl::liquidation_pnl_pct(pos)
    } else {
        pnl::unrealized_net_pnl_pct(pos, exit)
    };
    TradeRecord {
        id: 0,
        signal_id: pos.signal_id.clone(),
        bot_kind: pos.bot_kind.as_str().to_string(),
        exchange_id: pos.exchange_id.clone(),
        symbol: pos.symbol.clone(),
        direction: pos.direction.clone(),
        entry: pos.entry,
        exit,
        leverage: pos.leverage,
        capital: pos.capital,
        pnl_pct,
        pnl_quote: pos.capital * pnl_pct / 100.0,
        exit_reason: reason.to_string(),
        fr_at_open: pos.fr_at_open,
        ld_at_open: pos.ld_at_open,
        opened_at: pos.opened_at,
        closed_at,
        unlevered_net_pct: Some(pnl::unlevered_net_pct(pos, exit)),
        live: false,
        fill_entry: None,
        fill_exit: None,
        commission_usdt: None,
        sizing_mode: pos.sizing_mode.as_str().to_string(),
        risk_pct: pos.risk_pct,
        notional_usdt: (pos.notional_usdt > 0.0).then_some(pos.notional_usdt),
        effective_leverage: Some(super::sizing::leverage_of(pos)),
        risk_capped: pos.risk_capped,
        pnl_usdt: pos.capital * pnl_pct / 100.0,
        pnl_pct_of_capital: pnl_pct,
        veto_reason_code: None,
        veto_reason_text: None,
        tp_target: Some(pos.tp_target.clone()),
        tp_fallback_from: pos.tp_fallback_from.clone(),
        manual: pos.manual,
    }
}
