//! What the notifier sends, as pure functions over a snapshot of real
//! sources: closed signal trades (store `trades`), closed DCA / Grid cycles
//! (store `strategy_cycles`), the bot desk's failure notes, the daily loss
//! stop, the strategy portfolio breaker and ended DCA / Grid bots.
//!
//! Every line states only what its source row says. A pass returns the events
//! plus the cursors to commit once they were sent; nothing here talks to the
//! network or the database.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::bot::model::SkipNote;
use crate::exchange::foreign_fill::ForeignFill;
use crate::store::model::TradeRecord;
use crate::store::strategy::ClosedCycleRow;

/// The in-app alert categories (src/app/alerts.ts) that have a real source
/// on the Rust side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    KillSwitch,
    BotError,
    StopLoss,
    DealClosed,
}

/// The user's per-category choice. All on by default.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Categories {
    pub kill_switch: bool,
    pub bot_error: bool,
    pub stop_loss: bool,
    pub deal_closed: bool,
}

impl Default for Categories {
    fn default() -> Self {
        Self { kill_switch: true, bot_error: true, stop_loss: true, deal_closed: true }
    }
}

impl Categories {
    fn on(&self, c: Category) -> bool {
        match c {
            Category::KillSwitch => self.kill_switch,
            Category::BotError => self.bot_error,
            Category::StopLoss => self.stop_loss,
            Category::DealClosed => self.deal_closed,
        }
    }

    /// An event goes out when ANY of its categories is on (a stop-loss close
    /// is both "stop loss" and "deal closed").
    pub fn allows(&self, cats: &[Category]) -> bool {
        cats.iter().any(|c| self.on(*c))
    }
}

/// Persisted read positions: what was already reported.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Cursors {
    /// Highest `trades.id` reported (ids only grow).
    pub trade_id: i64,
    /// Newest cycle `closed_at` reported (see `Watch::seen_cycles`).
    pub cycle_closed_at: u64,
    /// Newest failure note time reported (UNIX ms).
    pub skip_at_ms: u64,
    /// Newest foreign-trade alarm reported (`detected_ms`).
    pub foreign_at_ms: u64,
}

/// In-memory edge detectors. Seeded from the first snapshot, so a state that
/// already held when the app started (a trip from this morning) is not sent
/// again after a restart.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Watch {
    pub seeded: bool,
    pub kill: bool,
    pub breaker: bool,
    pub dead: HashSet<String>,
    /// `bot_id:seq` of the cycles in the current window already accounted
    /// for. A cycle is persisted when its bot's bar walk reaches it, so a
    /// later write can carry an earlier `closed_at`; a key set catches it
    /// where a time cursor alone would not.
    pub seen_cycles: HashSet<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeadBot {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub live: bool,
    pub reason: Option<String>,
}

/// Everything one pass reads.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub now_ms: u64,
    /// Real money is in play on the desk (live build and a live bot or
    /// position). Labels the events that have no mode of their own.
    pub desk_live: bool,
    /// Newest first (store order).
    pub trades: Vec<TradeRecord>,
    /// Paper cycles, newest first.
    pub cycles: Vec<ClosedCycleRow>,
    pub skips: Vec<SkipNote>,
    pub kill_tripped: bool,
    pub daily_loss_pct: f64,
    pub breaker_tripped: bool,
    pub breaker_dd_pct: f64,
    pub dead: Vec<DeadBot>,
    /// Unacknowledged fills on the Binance account this app did not make.
    pub foreign: Vec<ForeignFill>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub cats: Vec<Category>,
    pub live: bool,
    pub at: u64,
    pub text: String,
}

/// Up to this many events go out one message each; more become one message.
pub const BATCH_OVER: usize = 5;
const APP: &str = "Aleph Edge";

pub fn mode(live: bool) -> &'static str {
    if live {
        "LIVE"
    } else {
        "Paper"
    }
}

/// The first line of every message: the app and the money it is about.
pub fn header(live: bool) -> String {
    format!("{APP} · {}", mode(live))
}

fn cycle_key(c: &ClosedCycleRow) -> String {
    format!("{}:{}", c.bot_id, c.seq)
}

/// Cursors and watch at "now": everything that exists is treated as seen, so
/// turning notifications on never sends a backlog.
pub fn reset(s: &Snapshot) -> (Cursors, Watch) {
    let cursors = Cursors {
        trade_id: s.trades.iter().map(|t| t.id).max().unwrap_or(0),
        cycle_closed_at: s.cycles.iter().map(|c| c.closed_at).max().unwrap_or(0),
        skip_at_ms: s.now_ms,
        foreign_at_ms: s.now_ms,
    };
    let watch = Watch {
        seeded: true,
        kill: s.kill_tripped,
        breaker: s.breaker_tripped,
        dead: s.dead.iter().map(|d| d.id.clone()).collect(),
        seen_cycles: s.cycles.iter().map(cycle_key).collect(),
    };
    (cursors, watch)
}

/// One pass: the events since `cur`, and the cursors / watch after them.
pub fn pass(s: &Snapshot, cur: &Cursors, watch: &Watch) -> (Vec<Event>, Cursors, Watch) {
    let mut w = watch.clone();
    if !w.seeded {
        // First pass of this process: transitions start from what holds now;
        // cycles at or before the persisted cursor count as reported.
        w = Watch {
            seeded: true,
            kill: s.kill_tripped,
            breaker: s.breaker_tripped,
            dead: s.dead.iter().map(|d| d.id.clone()).collect(),
            seen_cycles: s
                .cycles
                .iter()
                .filter(|c| c.closed_at <= cur.cycle_closed_at)
                .map(cycle_key)
                .collect(),
        };
    }
    let mut next = *cur;
    let mut out = Vec::new();

    if s.kill_tripped && !w.kill {
        out.push(Event {
            cats: vec![Category::KillSwitch],
            live: s.desk_live,
            at: s.now_ms,
            text: format!(
                "Daily loss stop tripped (limit {}%). Signal bots stopped until the next UTC day.",
                num(s.daily_loss_pct)
            ),
        });
    }
    w.kill = s.kill_tripped;
    if s.breaker_tripped && !w.breaker {
        out.push(Event {
            cats: vec![Category::KillSwitch],
            live: s.desk_live,
            at: s.now_ms,
            text: format!(
                "Portfolio breaker tripped (drawdown limit {}% of budget). DCA/Grid bots stopped.",
                num(s.breaker_dd_pct)
            ),
        });
    }
    w.breaker = s.breaker_tripped;

    // Fills this app did not make: always real money, always loud.
    let fresh: Vec<&ForeignFill> = s.foreign.iter().filter(|f| f.detected_ms > cur.foreign_at_ms).collect();
    if let Some(at) = fresh.iter().map(|f| f.detected_ms).max() {
        next.foreign_at_ms = at;
        out.push(Event { cats: vec![Category::KillSwitch], live: true, at, text: foreign_text(&fresh) });
    }

    for d in &s.dead {
        if w.dead.insert(d.id.clone()) {
            let why = d.reason.as_deref().map(exit_label).map(|r| format!(": {r}")).unwrap_or_default();
            out.push(Event {
                cats: vec![Category::BotError],
                live: d.live,
                at: s.now_ms,
                text: format!("{} bot \"{}\" ended{why}.", kind_label(&d.kind), d.name),
            });
        }
    }
    // A bot that left the dead state (archived, edited) may die again later.
    let dead_now: HashSet<&str> = s.dead.iter().map(|d| d.id.as_str()).collect();
    w.dead.retain(|id| dead_now.contains(id.as_str()));

    // Failure notes since the cursor, one event per reason.
    let mut groups: Vec<(&str, Vec<&SkipNote>)> = Vec::new();
    for n in s.skips.iter().filter(|n| n.at_ms > cur.skip_at_ms && is_bot_failure(&n.reason)) {
        match groups.iter_mut().find(|(r, _)| *r == n.reason) {
            Some((_, v)) => v.push(n),
            None => groups.push((n.reason.as_str(), vec![n])),
        }
        next.skip_at_ms = next.skip_at_ms.max(n.at_ms);
    }
    for (reason, notes) in groups {
        let latest = notes.iter().max_by_key(|n| n.at_ms).expect("non-empty group");
        let mut text = format!("Bot error: {reason}");
        if !latest.symbol.is_empty() && latest.symbol != "—" {
            text.push_str(&format!(" · {}", latest.symbol));
        }
        if notes.len() > 1 {
            text.push_str(&format!(" · {} times", notes.len()));
        }
        if let Some(d) = latest.detail.as_deref().filter(|d| !d.is_empty()) {
            text.push_str(&format!(" · {d}"));
        }
        out.push(Event {
            cats: vec![Category::BotError],
            live: reason.starts_with("live") || reason == "untrackedExchangePosition" || s.desk_live,
            at: latest.at_ms,
            text,
        });
    }

    for t in s.trades.iter().filter(|t| t.id > cur.trade_id) {
        next.trade_id = next.trade_id.max(t.id);
        let mut cats = vec![Category::DealClosed];
        if t.exit_reason == "sl" {
            cats.push(Category::StopLoss);
        }
        out.push(Event { cats, live: t.live, at: t.closed_at, text: trade_text(t) });
    }

    for c in &s.cycles {
        if !w.seen_cycles.insert(cycle_key(c)) {
            continue;
        }
        next.cycle_closed_at = next.cycle_closed_at.max(c.closed_at);
        let mut cats = vec![Category::DealClosed];
        if c.exit_reason.as_deref().is_some_and(is_cycle_stop) {
            cats.push(Category::StopLoss);
        }
        out.push(Event { cats, live: false, at: c.closed_at, text: cycle_text(c) });
    }
    // Keys that left the window cannot come back into it.
    let window: HashSet<String> = s.cycles.iter().map(cycle_key).collect();
    w.seen_cycles.retain(|k| window.contains(k));

    out.sort_by_key(|e| e.at);
    (out, next, w)
}

/// `isBotFailure` in src/lib/skipNotes.ts: a failure of the bot itself, not
/// a filter decision.
pub fn is_bot_failure(reason: &str) -> bool {
    (reason.starts_with("live")
        && ["Failed", "Unprotected", "Unconfirmed", "Mismatch", "Unavailable"]
            .iter()
            .any(|s| reason.ends_with(s)))
        || matches!(
            reason,
            "untrackedExchangePosition"
                | "storeWriteFailed"
                | "storeReadFailed"
                | "restoreRowUnreadable"
                | "restoreLiveBlocked"
                | "restoreConfigInvalid"
        )
}

/// `CYCLE_STOP_EXITS` in src/app/alerts.ts.
fn is_cycle_stop(reason: &str) -> bool {
    matches!(reason, "sl" | "liq" | "stop" | "ddstop" | "portfolioDd" | "dailyStop")
}

fn exit_label(code: &str) -> &str {
    match code {
        "tp" => "take profit",
        "sl" => "stop loss",
        "breakeven" => "breakeven",
        "horizon" | "timeout" => "time limit",
        "max_loss" => "max loss",
        "dailyStop" => "daily loss stop",
        "remoteKill" => "remote close",
        "manual" => "closed by hand",
        "exchangeClosed" => "closed on the exchange",
        "veto" => "Sentinel veto",
        "liq" => "liquidation",
        "stop" => "grid stop-out",
        "ddstop" => "drawdown stop",
        "portfolioDd" => "portfolio breaker",
        "trail" => "trailing take profit",
        "gridTp" => "grid take profit",
        "sizeDown" => "orders below the minimum",
        other => other,
    }
}

fn kind_label(kind: &str) -> &str {
    match kind {
        "dca" => "DCA",
        "grid" => "Grid",
        other => other,
    }
}

/// A number without trailing zeros (3, 2.5).
fn num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn signed(v: f64) -> String {
    let s = format!("{v:+.2}");
    if s == "-0.00" {
        "+0.00".into()
    } else {
        s
    }
}

pub fn trade_text(t: &TradeRecord) -> String {
    let side = if t.bot_kind == "spot" { "spot".to_string() } else { format!("{} {}", t.bot_kind, t.direction.as_str()) };
    format!(
        "Trade closed: {} {side} · {} · net {}% ({} USDT)",
        t.symbol,
        exit_label(&t.exit_reason),
        signed(t.pnl_pct),
        signed(t.pnl_quote)
    )
}

/// Up to this many foreign fills are listed one per line.
const FOREIGN_LINES: usize = 5;

pub fn foreign_text(fills: &[&ForeignFill]) -> String {
    let mut text = format!(
        "{} on your Binance account that this app did not make.",
        if fills.len() == 1 { "A trade".to_string() } else { format!("{} trades", fills.len()) }
    );
    for f in fills.iter().take(FOREIGN_LINES) {
        let what = if f.side.is_empty() {
            String::new()
        } else {
            format!(" {} {} @ {}", f.side, num_plain(f.qty), num_plain(f.price))
        };
        text.push_str(&format!("
{}{what} (trade {})", f.symbol, f.trade_id));
    }
    if fills.len() > FOREIGN_LINES {
        text.push_str(&format!("
… and {} more", fills.len() - FOREIGN_LINES));
    }
    text.push_str(
        "
New entries stopped on every bot. Open positions untouched.
Not you: delete this API key on Binance now. You: press \"That was me\" on the Risk page.",
    );
    text
}

/// A quantity or price as Binance writes it (up to 8 decimals, no padding).
fn num_plain(v: f64) -> String {
    let s = format!("{v:.8}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn cycle_text(c: &ClosedCycleRow) -> String {
    let why = c.exit_reason.as_deref().map(exit_label).unwrap_or("closed");
    let pct = c.pnl_pct_budget.map(|p| format!(", {}% of budget", signed(p))).unwrap_or_default();
    format!(
        "{} cycle #{} closed: \"{}\" {} {} · {why} · net {} USDT{pct}",
        kind_label(&c.kind),
        c.seq,
        c.bot_name,
        c.symbol,
        c.side,
        signed(c.pnl_quote)
    )
}

/// The messages for one pass: one per event, or a single summary message
/// when there are more than `BATCH_OVER`.
pub fn compose(events: &[Event]) -> Vec<String> {
    if events.len() <= BATCH_OVER {
        return events.iter().map(|e| format!("{}\n{}", header(e.live), e.text)).collect();
    }
    let mut msg = format!("{APP} · {} events", events.len());
    for e in events {
        msg.push_str(&format!("\n{} · {}", mode(e.live), e.text));
    }
    vec![msg]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::model::Direction;

    fn trade(id: i64, reason: &str, live: bool) -> TradeRecord {
        TradeRecord {
            id,
            signal_id: format!("s{id}"),
            bot_kind: "futures".into(),
            exchange_id: "binance".into(),
            symbol: "BTCUSDT".into(),
            direction: Direction::Long,
            entry: 100.0,
            exit: 99.0,
            leverage: 3,
            capital: 100.0,
            pnl_pct: -1.25,
            pnl_quote: -3.1,
            exit_reason: reason.into(),
            fr_at_open: None,
            ld_at_open: None,
            opened_at: 1,
            closed_at: 1_000 + id as u64,
            unlevered_net_pct: None,
            live,
            fill_entry: None,
            fill_exit: None,
            commission_usdt: None,
            sizing_mode: "fixed".into(),
            risk_pct: None,
            notional_usdt: None,
            effective_leverage: None,
            risk_capped: false,
            pnl_usdt: -3.1,
            pnl_pct_of_capital: -1.25,
            veto_reason_code: None,
            veto_reason_text: None,
            tp_target: None,
            tp_fallback_from: None,
            manual: false,
        }
    }

    fn cycle(bot: &str, seq: u32, closed_at: u64, reason: &str) -> ClosedCycleRow {
        ClosedCycleRow {
            bot_id: bot.into(),
            bot_name: "Night DCA".into(),
            kind: "dca".into(),
            market: "spot".into(),
            symbol: "ETHUSDT".into(),
            side: "long".into(),
            seq,
            opened_at: 0,
            closed_at,
            exit_reason: Some(reason.into()),
            pnl_quote: 1.2,
            pnl_pct_budget: Some(0.6),
            archived: false,
        }
    }

    fn skip(at_ms: u64, reason: &str) -> SkipNote {
        SkipNote { at_ms, symbol: "SOLUSDT".into(), reason: reason.into(), detail: None }
    }

    #[test]
    fn turning_on_sends_no_backlog() {
        let s = Snapshot {
            now_ms: 5_000,
            trades: vec![trade(9, "tp", false), trade(8, "sl", false)],
            cycles: vec![cycle("a", 2, 4_000, "tp")],
            skips: vec![skip(4_500, "liveOrderFailed")],
            kill_tripped: true,
            dead: vec![DeadBot { id: "d".into(), name: "x".into(), kind: "grid".into(), live: false, reason: None }],
            ..Default::default()
        };
        let (cur, w) = reset(&s);
        assert_eq!(cur, Cursors { trade_id: 9, cycle_closed_at: 4_000, skip_at_ms: 5_000, foreign_at_ms: 5_000 });
        let (events, next, _) = pass(&s, &cur, &w);
        assert!(events.is_empty(), "{events:?}");
        assert_eq!(next, cur);
    }

    #[test]
    fn new_rows_after_the_cursor_are_reported_once() {
        let mut s = Snapshot { now_ms: 5_000, trades: vec![trade(9, "tp", false)], ..Default::default() };
        let (cur, w) = reset(&s);
        s.now_ms = 6_000;
        s.trades.insert(0, trade(10, "sl", true));
        s.cycles = vec![cycle("a", 1, 5_500, "liq")];
        s.skips = vec![skip(5_600, "liveSlFailed"), skip(5_700, "liveSlFailed"), skip(5_800, "capitalCap")];
        let (events, next, w2) = pass(&s, &cur, &w);
        assert_eq!(events.len(), 3, "{events:?}");
        assert_eq!(next.trade_id, 10);
        assert_eq!(next.skip_at_ms, 5_700, "a filter note does not move the failure cursor");
        let trade_ev = events.iter().find(|e| e.text.starts_with("Trade closed")).unwrap();
        assert_eq!(trade_ev.cats, vec![Category::DealClosed, Category::StopLoss]);
        assert!(trade_ev.live);
        let cyc = events.iter().find(|e| e.text.starts_with("DCA cycle")).unwrap();
        assert_eq!(cyc.cats, vec![Category::DealClosed, Category::StopLoss], "liquidation is a stop exit");
        assert!(!cyc.live);
        let err = events.iter().find(|e| e.text.starts_with("Bot error")).unwrap();
        assert_eq!(err.text, "Bot error: liveSlFailed · SOLUSDT · 2 times");
        assert!(err.live);
        // The same snapshot again: nothing new.
        let (again, _, _) = pass(&s, &next, &w2);
        assert!(again.is_empty(), "{again:?}");
    }

    #[test]
    fn a_cycle_written_late_with_an_earlier_close_is_still_reported() {
        let mut s = Snapshot { cycles: vec![cycle("a", 1, 9_000, "tp")], ..Default::default() };
        let (cur, w) = reset(&s);
        s.cycles.push(cycle("b", 4, 8_000, "tp"));
        let (events, next, _) = pass(&s, &cur, &w);
        assert_eq!(events.len(), 1);
        assert_eq!(next.cycle_closed_at, 9_000);
    }

    #[test]
    fn after_a_restart_cycles_up_to_the_cursor_are_not_resent() {
        let s = Snapshot { cycles: vec![cycle("a", 2, 9_000, "tp"), cycle("a", 1, 7_000, "tp")], ..Default::default() };
        let cur = Cursors { cycle_closed_at: 7_000, ..Default::default() };
        let (events, _, w) = pass(&s, &cur, &Watch::default());
        assert_eq!(events.len(), 1);
        assert!(events[0].text.contains("#2"));
        assert!(w.seeded);
    }

    #[test]
    fn trips_are_sent_on_the_transition_only() {
        let mut s = Snapshot { daily_loss_pct: 3.0, breaker_dd_pct: 12.5, ..Default::default() };
        let (cur, w) = reset(&s);
        s.kill_tripped = true;
        let (ev, cur, w) = pass(&s, &cur, &w);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].text, "Daily loss stop tripped (limit 3%). Signal bots stopped until the next UTC day.");
        assert_eq!(ev[0].cats, vec![Category::KillSwitch]);
        let (ev, cur, w) = pass(&s, &cur, &w);
        assert!(ev.is_empty(), "still tripped: not sent again");
        s.breaker_tripped = true;
        let (ev, _, _) = pass(&s, &cur, &w);
        assert_eq!(ev[0].text, "Portfolio breaker tripped (drawdown limit 12.5% of budget). DCA/Grid bots stopped.");
    }

    #[test]
    fn a_foreign_trade_is_sent_once_as_a_kill_switch_event() {
        let f = |trade_id, detected_ms| ForeignFill {
            symbol: "XUSDT".into(),
            trade_id,
            order_id: 9,
            side: "BUY".into(),
            qty: 50_000.0,
            price: 0.0123,
            time_ms: 1,
            detected_ms,
        };
        let mut s = Snapshot { now_ms: 1_000, ..Default::default() };
        let (cur, w) = reset(&s);
        s.foreign = vec![f(4, 900)];
        let (ev, cur, w) = pass(&s, &cur, &w);
        assert!(ev.is_empty(), "noticed before notifications were on: no backlog");
        s.foreign.push(f(5, 2_000));
        let (ev, cur, w) = pass(&s, &cur, &w);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].cats, vec![Category::KillSwitch]);
        assert!(ev[0].live);
        assert!(ev[0].text.starts_with("A trade on your Binance account that this app did not make."), "{}", ev[0].text);
        assert!(ev[0].text.contains("XUSDT BUY 50000 @ 0.0123 (trade 5)"), "{}", ev[0].text);
        assert!(ev[0].text.contains("New entries stopped on every bot. Open positions untouched."));
        assert!(!ev[0].text.contains("trade 4"));
        let (ev, _, _) = pass(&s, &cur, &w);
        assert!(ev.is_empty(), "sent once");
        assert!(!Categories { kill_switch: false, ..Default::default() }.allows(&[Category::KillSwitch]));
    }

    #[test]
    fn a_trip_that_held_at_startup_is_not_sent() {
        let s = Snapshot { kill_tripped: true, breaker_tripped: true, ..Default::default() };
        let (ev, _, _) = pass(&s, &Cursors::default(), &Watch::default());
        assert!(ev.is_empty());
    }

    #[test]
    fn an_ended_bot_is_reported_once() {
        let mut s = Snapshot::default();
        let (cur, w) = reset(&s);
        s.dead = vec![DeadBot { id: "g1".into(), name: "Range".into(), kind: "grid".into(), live: false, reason: Some("liq".into()) }];
        let (ev, cur, w) = pass(&s, &cur, &w);
        assert_eq!(ev[0].text, "Grid bot \"Range\" ended: liquidation.");
        assert_eq!(ev[0].cats, vec![Category::BotError]);
        let (ev, _, _) = pass(&s, &cur, &w);
        assert!(ev.is_empty());
    }

    #[test]
    fn failure_reasons_match_the_ui_rule() {
        for r in ["liveOrderFailed", "liveSlUnprotected", "liveFillsUnavailable", "untrackedExchangePosition", "storeWriteFailed", "restoreConfigInvalid"] {
            assert!(is_bot_failure(r), "{r}");
        }
        for r in ["capitalCap", "maxPositions", "comboFilter", "liveEntry", "botNotResumed"] {
            assert!(!is_bot_failure(r), "{r}");
        }
    }

    #[test]
    fn categories_filter_events() {
        let mut c = Categories::default();
        assert!(c.allows(&[Category::DealClosed]));
        c.deal_closed = false;
        assert!(!c.allows(&[Category::DealClosed]));
        assert!(c.allows(&[Category::DealClosed, Category::StopLoss]), "a stop-loss close still goes out");
        c.stop_loss = false;
        assert!(!c.allows(&[Category::DealClosed, Category::StopLoss]));
        assert!(c.allows(&[Category::KillSwitch]));
    }

    #[test]
    fn messages_name_the_app_and_the_money() {
        let t = trade(1, "sl", false);
        assert_eq!(trade_text(&t), "Trade closed: BTCUSDT futures long · stop loss · net -1.25% (-3.10 USDT)");
        let ev = |live| Event { cats: vec![Category::DealClosed], live, at: 0, text: "x".into() };
        assert_eq!(compose(&[ev(false)]), vec!["Aleph Edge · Paper\nx".to_string()]);
        assert_eq!(compose(&[ev(true)]), vec!["Aleph Edge · LIVE\nx".to_string()]);
        assert_eq!(
            cycle_text(&cycle("a", 3, 0, "tp")),
            "DCA cycle #3 closed: \"Night DCA\" ETHUSDT long · take profit · net +1.20 USDT, +0.60% of budget"
        );
        let mut spot = trade(2, "tp", false);
        spot.bot_kind = "spot".into();
        assert!(trade_text(&spot).starts_with("Trade closed: BTCUSDT spot · take profit"));
    }

    #[test]
    fn more_than_five_events_become_one_message() {
        let evs: Vec<Event> = (0..7)
            .map(|i| Event { cats: vec![Category::DealClosed], live: i == 0, at: i, text: format!("e{i}") })
            .collect();
        let msgs = compose(&evs);
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].starts_with("Aleph Edge · 7 events\nLIVE · e0\nPaper · e1"));
        assert_eq!(compose(&evs[..5]).len(), 5);
    }
}
