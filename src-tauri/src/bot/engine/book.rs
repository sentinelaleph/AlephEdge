//! The position book's side effects: managing open positions each tick,
//! recording closes, and persisting open positions to SQLite so a restart
//! does not orphan them.

use tauri::{AppHandle, Manager};

use crate::exchange::ExchangeManager;
use crate::signal::model::RegimeVetoEvent;
use crate::signal::SignalManager;
use crate::store::model::TradeRecord;
use crate::store::StoreManager;

use super::super::model::{BotKind, OpenPosition, LIVE_TRADING_ENABLED};
use super::super::BotManager;
use super::record::stamp_veto_reason;
use super::{close_price, live_close, live_manage, live_price, management, now_ms};

// The trade records are pure and live in `record.rs` (shared with the
// headless paper runner); re-exported so `book::trade_record` keeps its path.
pub use super::record::{trade_record, veto_trade_record};

/// Runs the management plan for every open position at the live price.
/// Invalidation never closes (measured −163 pts); it is noted once.
pub async fn close_phase(app: &AppHandle) {
    let bots = app.state::<BotManager>();
    let exchange = app.state::<ExchangeManager>();
    let signals = app.state::<SignalManager>();

    for mut pos in bots.positions_snapshot() {
        // Backstop for Sentinel's veto: the SSE handler closes at receipt,
        // but a veto learned by backfill, or whose close failed or found the
        // position claimed, is closed here, retried every tick until it holds.
        if let Some(veto) = signals.regime_veto(&pos.signal_id) {
            if bots.claim_open(&pos.signal_id, pos.bot_kind) {
                close_one_on_veto(app, &exchange, &bots, &pos, &veto).await;
                bots.closing.release(&pos.signal_id, pos.bot_kind);
            }
            continue;
        }
        // A real position with no exchange stop is never held: flatten it.
        if pos.is_live() && pos.unprotected {
            close_now(app, &pos.signal_id, pos.bot_kind, "unprotected").await;
            continue;
        }
        let Some(price) = live_price(&exchange, &pos.exchange_id, &pos.symbol, pos.bot_kind).await
        else {
            continue;
        };
        let max_loss = bots.config_for(pos.bot_kind).and_then(|c| c.max_loss_pct);
        let invalidated = signals.is_invalidated(&pos.signal_id);
        let had_partial = pos.partial_price.is_some();
        let step = management::close_step(&mut pos, price, now_ms(), invalidated, max_loss);

        if step.note_invalidation {
            bots.skip_once(
                pos.bot_kind,
                &pos.signal_id,
                &pos.symbol,
                "invalidatedHeld",
                None,
            );
        }
        // A veto delivered between the snapshot above and here may already be
        // closing (or have closed) this exact position (see close_claim.rs) —
        // back off rather than record a second close on it. A live position
        // being managed takes the claim too: moving its exchange stop while a
        // veto flattens it would leave a stop on a flat symbol.
        let claimed = step.needs_claim(pos.is_live());
        if claimed && !bots.claim_open(&pos.signal_id, pos.bot_kind) {
            continue;
        }
        match step.exit {
            // A LIVE position must be flat on the exchange before the book
            // says so, and is recorded on real fills; on failure it stays
            // open and retries next tick.
            Some(exit) if pos.is_live() => {
                match live_close::close_live(app, &pos, exit.reason, exit.price).await {
                    Ok(record) => close_with_record(app, &pos, &record),
                    Err(key) => {
                        bots.skip_once(pos.bot_kind, &pos.signal_id, &pos.symbol, key, None);
                        if step.changed {
                            save_position(app, &pos);
                        }
                    }
                }
            }
            Some(exit) => close_position(app, &pos, exit.price, exit.reason),
            None => {
                let mut changed = step.changed;
                if pos.is_live() {
                    // Mirror the plan onto the exchange: a partial just banked
                    // reduces the real quantity; an armed breakeven moves the
                    // real stop (retried each tick until it holds).
                    if !had_partial && pos.partial_price.is_some() {
                        live_manage::execute_partial(app, &mut pos).await;
                    }
                    changed |= live_manage::sync_breakeven(app, &mut pos).await;
                }
                if changed {
                    save_position(app, &pos);
                }
            }
        }
        if claimed {
            bots.closing.release(&pos.signal_id, pos.bot_kind);
        }
    }
}

/// Closes one open position now at the live price (`reason` is the exit
/// reason). Takes the close claim; a position already closing or gone is
/// left alone. A live close that fails stays open and retries next tick.
pub(crate) async fn close_now(app: &AppHandle, signal_id: &str, kind: BotKind, reason: &str) {
    let bots = app.state::<BotManager>();
    let exchange = app.state::<ExchangeManager>();
    let Some(pos) = bots
        .positions_snapshot()
        .into_iter()
        .find(|p| p.signal_id == signal_id && p.bot_kind == kind)
    else {
        return;
    };
    let Some(price) = close_price(&exchange, &pos).await else {
        return;
    };
    if !bots.claim_open(signal_id, kind) {
        return;
    }
    if pos.is_live() {
        match live_close::close_live(app, &pos, reason, price).await {
            Ok(record) => close_with_record(app, &pos, &record),
            Err(key) => bots.skip_once(kind, signal_id, &pos.symbol, key, None),
        }
    } else {
        close_position(app, &pos, price, reason);
    }
    bots.closing.release(signal_id, kind);
}

/// Closes the given bots' open positions at the live price with a fixed
/// reason (daily stop, remote kill); a remote "close all" for one bot must not
/// close the others'. Unpriceable positions are left for the next tick.
pub(crate) async fn force_close_kinds(app: &AppHandle, reason: &str, kinds: &[BotKind]) {
    let bots = app.state::<BotManager>();
    let exchange = app.state::<ExchangeManager>();
    for pos in bots
        .positions_snapshot()
        .into_iter()
        .filter(|p| kinds.contains(&p.bot_kind))
    {
        let Some(price) = close_price(&exchange, &pos).await else {
            continue;
        };
        if !bots.claim_open(&pos.signal_id, pos.bot_kind) {
            continue; // a veto is already closing (or closed) this position
        }
        if !pos.is_live() {
            close_position(app, &pos, price, reason);
        } else {
            match live_close::close_live(app, &pos, reason, price).await {
                Ok(record) => close_with_record(app, &pos, &record),
                Err(key) => bots.push_skip(&pos.symbol, key, None),
            }
        }
        bots.closing.release(&pos.signal_id, pos.bot_kind);
    }
}

/// Closes every open position (any bot kind) held on `veto.signal_id`, right
/// now, at the live price, exit reason "veto" — the owner's 2026-09-18
/// decision: once Sentinel vetoes a signal because BTC's regime turned
/// against it, a bot holding that position must protect itself immediately
/// rather than wait out the management plan. Shares the close claim with
/// `close_phase`/`force_close_kinds` so this can never race the tick loop into
/// a double close; an already-claimed (or already-closed) position is simply
/// skipped — a duplicate SSE delivery is therefore a safe no-op. Pushes one
/// desk notice naming how many positions it actually closed.
pub async fn close_on_veto(app: &AppHandle, veto: &RegimeVetoEvent) {
    let bots = app.state::<BotManager>();
    let exchange = app.state::<ExchangeManager>();
    let mut closed = 0u32;
    for pos in bots
        .positions_snapshot()
        .into_iter()
        .filter(|p| p.signal_id == veto.signal_id)
    {
        if !bots.claim_open(&pos.signal_id, pos.bot_kind) {
            continue;
        }
        if close_one_on_veto(app, &exchange, &bots, &pos, veto).await {
            closed += 1;
        }
        bots.closing.release(&pos.signal_id, pos.bot_kind);
    }
    if closed > 0 {
        bots.push_skip(
            &veto.symbol,
            "vetoClosed",
            Some(format!("{closed}|{}", crate::signal::veto_note_reason(veto))),
        );
    }
}

/// One position's veto close. `false` on a real failure (unpriceable, or the
/// live close didn't confirm) — the position stays open and the ordinary
/// tick loop keeps managing it, exactly like any other retried close.
async fn close_one_on_veto(
    app: &AppHandle,
    exchange: &ExchangeManager,
    bots: &BotManager,
    pos: &OpenPosition,
    veto: &RegimeVetoEvent,
) -> bool {
    let Some(price) = close_price(exchange, pos).await else {
        return false;
    };
    if pos.is_live() {
        match live_close::close_live(app, pos, "veto", price).await {
            Ok(mut record) => {
                stamp_veto_reason(&mut record, veto);
                close_with_record(app, pos, &record);
                true
            }
            Err(key) => {
                bots.skip_once(pos.bot_kind, &pos.signal_id, &pos.symbol, key, None);
                false
            }
        }
    } else {
        let record = veto_trade_record(pos, price, veto, now_ms());
        close_with_record(app, pos, &record);
        true
    }
}

/// Adds a freshly opened position to memory and the store.
pub fn open_position(app: &AppHandle, pos: OpenPosition) {
    persist_position(app, &pos);
    app.state::<BotManager>().add_position(pos);
}

/// Records a simulated close at `exit`, then removes the position.
fn close_position(app: &AppHandle, pos: &OpenPosition, exit: f64, reason: &str) {
    close_with_record(app, pos, &trade_record(pos, exit, reason, now_ms()));
}

/// Persists `record`, then removes the position from memory and the store.
pub(crate) fn close_with_record(app: &AppHandle, pos: &OpenPosition, record: &TradeRecord) {
    let bots = app.state::<BotManager>();
    // The position is already flat (simulated or on the exchange), so it
    // leaves the book either way; a storage failure is surfaced instead of
    // swallowed, because the kill switch reads today's loss from that table.
    match app.path().app_data_dir() {
        Ok(dir) => {
            let store = app.state::<StoreManager>();
            if let Err(e) = store.record_trade(&dir, record) {
                bots.push_skip(&pos.symbol, "storeWriteFailed", Some(e));
            }
            if let Err(e) = store.delete_position(&dir, &pos.signal_id, pos.bot_kind.as_str()) {
                bots.push_skip(&pos.symbol, "storeWriteFailed", Some(e));
            }
        }
        Err(e) => bots.push_skip(&pos.symbol, "storeWriteFailed", Some(e.to_string())),
    }
    bots.remove_position(&pos.signal_id, pos.bot_kind);
}

/// Persists management state for a position STILL in the book; one a
/// concurrent close removed is not written back (it would return on restart).
fn save_position(app: &AppHandle, pos: &OpenPosition) {
    if !app.state::<BotManager>().update_position(pos) {
        return;
    }
    persist_position(app, pos);
}

fn persist_position(app: &AppHandle, pos: &OpenPosition) {
    let bots = app.state::<BotManager>();
    let (dir, body) = match (app.path().app_data_dir(), serde_json::to_string(pos)) {
        (Ok(dir), Ok(body)) => (dir, body),
        (Err(e), _) => return bots.push_skip(&pos.symbol, "storeWriteFailed", Some(e.to_string())),
        (_, Err(e)) => return bots.push_skip(&pos.symbol, "storeWriteFailed", Some(e.to_string())),
    };
    if let Err(e) = app.state::<StoreManager>().upsert_position(
        &dir,
        &pos.signal_id,
        pos.bot_kind.as_str(),
        &body,
        now_ms(),
    ) {
        bots.push_skip(&pos.symbol, "storeWriteFailed", Some(e));
    }
}

/// Positions persisted by a previous run. Rows that no longer parse are
/// skipped rather than failing the whole restore.
pub fn load_positions(app: &AppHandle) -> Vec<OpenPosition> {
    let Ok(dir) = app.path().app_data_dir() else {
        return Vec::new();
    };
    app.state::<StoreManager>()
        .load_positions(&dir)
        .unwrap_or_default()
        .iter()
        .filter_map(|body| serde_json::from_str::<OpenPosition>(body).ok())
        // Positions are persisted as raw JSON, so `live` arrives from a file
        // any local process can edit. Drop it at the boundary while the master
        // switch is off: a tampered row then cannot even enter the book as
        // live, on top of the `is_live()` gate every path reads (2026-09-20).
        .map(|mut pos| {
            if !LIVE_TRADING_ENABLED {
                pos.live = false;
            }
            pos
        })
        .collect()
}
