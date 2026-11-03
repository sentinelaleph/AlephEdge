//! The entry shared by the bots that trade the user's own decisions: the
//! TradingView bot (an alert, `tv_trade`) and the Pine bot (a script's
//! trigger on a closed bar, `bot::pinebot`). One position per event, planned
//! through the New trade plan (`user_trade::plan_in`, `PlanFor::Alert`) and,
//! on a Live bot, opened with the signal bots' real entry
//! (`live::open_live_with`: market order, exchange stop and take-profit,
//! the pilot cap).
//!
//! What a Live entry needs, checked here at the open, every time: the bot's
//! stored mode Live AND its per-session arm (`cfg.live`, never restored),
//! the order path (`live_trading_allowed`), the terms that arm confirmed
//! still in force (`live_terms::same_terms`), then `open_live_with`'s own
//! checks. The daily stop, the risk level's caps and the exchange minimum
//! come from the shared plan. "Stop all live" disarms (cfg.live = false).
//!
//! The synthetic signal id is derived from the event id (`signal_id`), and
//! the entry's client id from it: an order whose answer was lost is never
//! sent again and is looked up by that client id (`resolve_pending`).

use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::exchange::providers::binance_parse::OrderFill;
use crate::exchange::providers::binance_requests::entry_client_id;
use crate::exchange::ExchangeManager;
use crate::risk::RiskManager;

use super::super::live_terms;
use super::super::model::{BotKind, OpenPosition, TakeProfitTarget, TradeMode};
use super::super::BotManager;
use super::live::{self, live_trading_allowed};
use super::tv_trade::{apply_fill, live_entry_refusal, pending_verdict, AlertOpen, AlertRefusal, Pending, ALERT_ID_PREFIX};
use super::user_trade::{self, Level, PlanFor, TradeRequest};
use super::{book, precheck};

/// Prefix of a Pine entry's synthetic id: a user trade (`manual-`), so the
/// precheck never asks Sentinel about it and History labels it the user's.
pub const PINE_ID_PREFIX: &str = "manual-pine-";

/// The codes one kind's entry refuses and notes with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindCodes {
    pub bot_required: &'static str,
    pub already_open: &'static str,
    pub opened: &'static str,
    pub late_fill: &'static str,
    pub id_prefix: &'static str,
}

/// `kind`'s codes; the TradingView bot's for every kind but Pine (the
/// TradingView entry kept its codes when it was generalised).
pub fn codes(kind: BotKind) -> KindCodes {
    match kind {
        BotKind::Pine => KindCodes {
            bot_required: "pineBotRequired",
            already_open: "pineAlreadyOpen",
            opened: "pineOpened",
            late_fill: "pineLateFillAdopted",
            id_prefix: PINE_ID_PREFIX,
        },
        _ => KindCodes {
            bot_required: "tvBotRequired",
            already_open: "tvAlreadyOpen",
            opened: "tvAlertOpened",
            late_fill: "tvLateFillAdopted",
            id_prefix: ALERT_ID_PREFIX,
        },
    }
}

/// `<prefix><24 hex of sha256(event id)>`: stable for one event, so the
/// client id (`ae` + its first 34 letters and digits) keeps the whole hash
/// (`manual-tv-` and `manual-pine-` both leave 24 hex inside the 34).
pub fn signal_id(kind: BotKind, event_id: &str) -> String {
    let digest = Sha256::digest(event_id.as_bytes());
    let hex: String = digest.iter().take(12).map(|b| format!("{b:02x}")).collect();
    format!("{}{hex}", codes(kind).id_prefix)
}

/// Opens one event's position on `kind`'s bot (the TradingView or the Pine
/// bot): Paper on a Paper bot, a real order on a Live, armed one. `levels`
/// are the event's own (a percent is put at the real fill and re-checked
/// there).
pub async fn open(app: &AppHandle, kind: BotKind, req: &TradeRequest, event_id: &str, levels: (Level, Level)) -> Result<AlertOpen, AlertRefusal> {
    let c = codes(kind);
    if !kind.is_user_driven() || req.kind != kind {
        return Err(user_trade::BOT_KIND.to_string().into());
    }
    let bots = app.state::<BotManager>();
    let cfg = bots.config_for(kind).ok_or_else(|| c.bot_required.to_string())?;
    let id = signal_id(kind, event_id);
    let live = cfg.mode == TradeMode::Live;
    let limits = app.state::<RiskManager>().limits();
    if live {
        // Checked before any network call, and again by open_live_with.
        live_entry_refusal(&cfg, live_trading_allowed(&cfg, kind), bots.armed_terms(kind).as_ref(), &live_terms::signal_terms(&cfg, &limits))?;
    } else if cfg.live {
        return Err(super::manual::PAPER_ONLY.to_string().into());
    }
    let req = TradeRequest { mode: cfg.mode, ..req.clone() };
    let (mut pos, _) = user_trade::plan_in(app, &req, PlanFor::Alert { id: &id }).await?;
    let requested_qty = pos.qty;
    if !live {
        if !book::open_position_if_free(app, pos.clone()) {
            return Err(c.already_open.to_string().into());
        }
        bots.push_skip(&pos.symbol, c.opened, None);
        bots.ensure_loop(app.clone());
        return Ok(AlertOpen { pos, requested_qty });
    }
    let leverage = precheck::effective_leverage(&cfg, &limits);
    let tcfg = user_trade::trade_config(&cfg, &req, leverage);
    let sig = user_trade::synthetic_signal(&req, &pos.symbol, pos.entry, super::now_ms(), pos.signal_id.clone());
    let opened = live::open_live_with(app, &tcfg, &sig, pos.entry, pos.notional_usdt, leverage, TakeProfitTarget::Tp1, Some(levels)).await;
    let open = match opened {
        Ok(o) => o,
        Err(r) => {
            return Err(AlertRefusal { code: r.key.to_string(), outcome_unknown: r.outcome_unknown, planned: Some(pos) });
        }
    };
    bots.pilot_used_saved(app, kind);
    apply_fill(&mut pos, open.entry, open.qty, Some(open.entry_order_id), open.stop_algo_id, open.tp_algo_id, open.tp.tp, open.sl);
    book::open_position(app, pos.clone());
    bots.push_skip(&pos.symbol, c.opened, Some("live".to_string()));
    bots.ensure_loop(app.clone());
    // The daily stop or "Stop all live" can land while the order was on the
    // wire; their close-all ran on a book without this position.
    if bots.kill_switch_tripped() || !bots.config_for(kind).is_some_and(|c| c.live) {
        book::close_now(app, &pos.signal_id, kind, "stopped").await;
    }
    Ok(AlertOpen { pos, requested_qty })
}

/// Looks up one pending entry of `kind` (`planned`, sent with the client id
/// of its synthetic id). A late fill is protected at the event's levels from
/// its fill and booked on that bot. `Filled` means the position is now in
/// the book (or the stop could not be placed and it was flattened: then
/// `Failed`).
pub async fn resolve_pending(app: &AppHandle, kind: BotKind, planned: &OpenPosition, levels: (Level, Level), age_ms: u64) -> Pending {
    let bots = app.state::<BotManager>();
    if bots.has_position(&planned.signal_id, kind) {
        return Pending::Filled(OrderFill { order_id: planned.entry_order_id.unwrap_or(0), avg_price: planned.entry, executed_qty: planned.qty });
    }
    let Some(cred) = app.state::<crate::vault::VaultManager>().credential(&planned.exchange_id) else {
        return Pending::Wait;
    };
    let exchange = app.state::<ExchangeManager>();
    let client_id = entry_client_id(&planned.signal_id);
    let res = exchange.order_by_client_id(&cred, &planned.symbol, &client_id).await;
    let not_found = matches!(&res, Err(crate::exchange::model::BinanceKeyCheckError::Rejected { code: -2013, .. }));
    let verdict = pending_verdict(live::unknown_entry_lookup(res), not_found, age_ms);
    let Pending::Filled(fill) = verdict else {
        return verdict;
    };
    let Ok(rules) = exchange.symbol_rules(&cred, &planned.symbol).await else {
        return Pending::Wait;
    };
    let cfg = bots.config_for(kind);
    let leverage = planned.leverage.max(1);
    let fee = user_trade::round_trip_fee(kind);
    let mut sig = user_trade::synthetic_signal(
        &TradeRequest {
            kind,
            symbol: planned.symbol.clone(),
            direction: planned.direction,
            notional_usdt: Some(planned.notional_usdt),
            take_profit: planned.tp,
            stop_loss: planned.sl,
            mode: TradeMode::Live,
            confirmation: None,
            confirmed_margin_usdt: None,
        },
        &planned.symbol,
        fill.avg_price,
        super::now_ms(),
        planned.signal_id.clone(),
    );
    let long = planned.direction == crate::signal::model::Direction::Long;
    match live::levels_after_fill(planned.direction, fill.avg_price, levels, leverage, fee) {
        Ok((tp, sl)) => (sig.tp, sig.sl) = (vec![tp], sl),
        Err(_) => {
            return if live::flatten_confirmed(&exchange, &cred, &planned.symbol, long, fill.executed_qty).await {
                Pending::Failed
            } else {
                bots.push_skip(&planned.symbol, live::NOTE_UNPROTECTED, None);
                Pending::Wait
            };
        }
    }
    match live::protect_fill(&exchange, &bots, &cred, &sig, TakeProfitTarget::Tp1, &fill, fill.avg_price, rules.tick).await {
        Ok(p) => {
            let mut pos = planned.clone();
            apply_fill(&mut pos, fill.avg_price, fill.executed_qty, Some(fill.order_id), p.stop_algo_id, p.tp_algo_id, p.tp.tp, sig.sl);
            book::open_position(app, pos);
            bots.pilot_used_saved(app, kind);
            bots.push_skip(&planned.symbol, codes(kind).late_fill, None);
            bots.ensure_loop(app.clone());
            // A bot stopped meanwhile does not keep it.
            if bots.kill_switch_tripped() || !cfg.is_some_and(|c| c.live) {
                book::close_now(app, &planned.signal_id, kind, "stopped").await;
            }
            Pending::Filled(fill)
        }
        // The stop failed and the emergency flatten confirmed flat.
        Err(_) => Pending::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client_id(kind: BotKind, event_id: &str) -> String {
        entry_client_id(&signal_id(kind, event_id))
    }

    #[test]
    fn both_kinds_derive_stable_user_trade_ids_that_fit_binance() {
        for (kind, prefix, client) in [(BotKind::Tradingview, "manual-tv-", "aemanualtv"), (BotKind::Pine, "manual-pine-", "aemanualpine")] {
            let ev = "pine:ab12:BTCUSDT:1761900000000";
            let id = signal_id(kind, ev);
            assert_eq!(id, signal_id(kind, ev), "stable across restarts");
            assert!(id.starts_with(prefix) && id.len() == prefix.len() + 24, "{id}");
            assert!(user_trade::is_user_trade_id(&id), "a user trade: never looked up at Sentinel");
            let c = client_id(kind, ev);
            assert!(c.starts_with(client) && c.len() <= 36 && c.chars().all(|x| x.is_ascii_alphanumeric()), "{c}");
            // The whole 24-hex hash survives the client id's 34-character cut.
            assert_eq!(&c[2..], id.replace('-', ""));
            assert_ne!(c, client_id(kind, "pine:ab12:BTCUSDT:1761903600000"));
        }
        // The TradingView entry's ids did not change with the generalisation.
        assert_eq!(signal_id(BotKind::Tradingview, "41"), super::super::tv_trade::alert_signal_id("41"));
        assert_eq!(client_id(BotKind::Tradingview, "41"), super::super::tv_trade::alert_client_id("41"));
        assert_ne!(signal_id(BotKind::Pine, "41"), signal_id(BotKind::Tradingview, "41"));
    }

    #[test]
    fn each_kind_refuses_and_notes_with_its_own_codes() {
        assert_eq!(codes(BotKind::Tradingview).already_open, "tvAlreadyOpen");
        assert_eq!(codes(BotKind::Tradingview).bot_required, "tvBotRequired");
        assert_eq!(codes(BotKind::Pine).already_open, "pineAlreadyOpen");
        assert_eq!(codes(BotKind::Pine).bot_required, "pineBotRequired");
        assert_eq!(codes(BotKind::Pine).opened, "pineOpened");
    }
}
