//! Pure bot driver: one closed bar at a time, decides whether a new cycle
//! starts (start condition, restart policy, gates, size-down), walks the
//! open cycle, applies the per-bot drawdown stop, and keeps the accounting.
//! The engine feeds it bars and persists what it returns; tests feed it
//! bars directly (no network, no Tauri).

use super::accounting;
use super::costs::CostModel;
use super::cycle::{step_bar, CycleStart, CycleState};
use super::model::{
    Bar, BotRunState, CrossDir, ExitReason, Fill, StartCondition, StrategyBot, StrategyParams,
};

/// A gap between consecutive bars longer than this writes a note.
pub const DATA_GAP_NOTE_MS: u64 = 5 * 60_000;
/// A grid whose start is outside its stop band retries once an hour.
pub const GRID_RETRY_MS: u64 = 3_600_000;

/// One bar-close equity sample of a bot with an open (or just closed) cycle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EquityPoint {
    pub ts: u64,
    pub equity: f64,
    pub margin: f64,
    pub reserved: f64,
}

#[derive(Debug, Default)]
pub struct BarOutcome {
    /// Fills of this bar, tagged with their cycle seq.
    pub events: Vec<(u32, Fill)>,
    /// Seq of a cycle opened on this bar.
    pub opened: Option<u32>,
    /// A cycle that closed on this bar (final state).
    pub closed: Option<CycleState>,
    /// (note key, detail).
    pub notes: Vec<(&'static str, Option<String>)>,
    pub equity: Option<EquityPoint>,
}

/// Whether the bot may open new cycles at all (Armed or running a cycle).
pub fn is_running(state: BotRunState) -> bool {
    matches!(state, BotRunState::Armed | BotRunState::InCycle | BotRunState::Paused)
}

fn crossed(start: &StartCondition, bar: &Bar) -> bool {
    match start {
        StartCondition::Immediately => true,
        StartCondition::PriceCross { price, direction } => match direction {
            CrossDir::Up => bar.h >= *price,
            CrossDir::Down => bar.l <= *price,
        },
    }
}

/// Tries to open a cycle on `bar`. `gate` = a reason new cycles are held
/// (kill switch, breaker, membership, BTC break, stale price).
fn try_start(
    bot: &mut StrategyBot,
    bar: &Bar,
    gate: Option<&'static str>,
    out: &mut BarOutcome,
) -> Option<CycleState> {
    let cfg = bot.cfg.clone();
    if !matches!(cfg.start, StartCondition::Immediately) && !bot.start_triggered {
        // The cross is known at this bar's close: the cycle opens next bar.
        if crossed(&cfg.start, bar) {
            bot.start_triggered = true;
            bot.next_decision_ms = bar.end_ms();
        }
        return None;
    }
    if let Some(end) = cfg.restart.end_at_ms {
        if bar.open_ms >= end {
            bot.state = BotRunState::Stopped;
            out.notes.push(("endReached", None));
            return None;
        }
    }
    if let Some(max) = cfg.restart.max_cycles {
        if bot.chain_cycles >= max {
            bot.state = BotRunState::Stopped;
            out.notes.push(("maxCyclesReached", None));
            return None;
        }
    }
    if let Some(reason) = gate {
        out.notes.push((reason, None));
        return None;
    }
    if let Some((lo, hi)) = cfg.restart.price_band {
        if bar.o < lo || bar.o > hi {
            out.notes.push(("priceBandWait", None));
            return None;
        }
    }
    // The per-bot drawdown stop is measured from the lifetime equity peak.
    // A bot already at or beyond it (stopped by it, then started again)
    // would open a cycle only for the bar-close check to close it at once,
    // paying the entry and exit fees every time: hold it instead.
    if let Some(pct) = cfg.max_drawdown_pct.filter(|p| *p > 0.0) {
        let eq = accounting::equity(cfg.budget, bot.realized_quote, 0.0);
        if bot.peak_equity.max(eq) - eq >= pct / 100.0 * cfg.budget {
            bot.state = BotRunState::Stopped;
            out.notes.push(("ddStop", None));
            return None;
        }
    }
    let scale = accounting::size_down(cfg.budget, bot.realized_quote);
    // Losses shrink every order by `scale`. Below 1% of the budget, or once
    // the smallest order falls under the exchange minimum (a real venue
    // would refuse it; paper must not fill it), the bot is done.
    let below_min = super::validate::smallest_order_notional(&cfg) * scale
        < super::limits::MIN_ORDER_NOTIONAL;
    if accounting::is_dead(scale) || below_min {
        bot.state = BotRunState::Dead;
        bot.dead_reason = Some(ExitReason::SizeDown);
        out.notes.push((if accounting::is_dead(scale) { "sizeDownDead" } else { "sizeDownBelowMin" }, None));
        return None;
    }
    let st = CycleStart {
        bot_id: &bot.id,
        seq: bot.cycles_done + 1,
        side: cfg.side,
        budget: cfg.budget,
        leverage: f64::from(cfg.leverage.max(1)),
        size_factor: scale,
        costs: CostModel::for_market(cfg.market),
        max_duration_min: cfg.params.max_duration_min(),
    };
    let opened = match &cfg.params {
        StrategyParams::Dca(p) => CycleState::open_dca(&st, p, bar),
        StrategyParams::Grid(p) => CycleState::open_grid(&st, p, bar),
    };
    match opened {
        // An absolute-range grid passed its liquidation check at the range's
        // mid; at this start its worst-case inventory would liquidate inside
        // the stop band. Held like an out-of-band start (retried hourly).
        Ok(_) if matches!(&cfg.params, StrategyParams::Grid(p) if !super::validate::grid_liq_clear_at(&cfg, p, bar.o)) => {
            bot.next_decision_ms = bar.open_ms + GRID_RETRY_MS;
            out.notes.push(("gridLiqInsideBand", None));
            None
        }
        Ok(c) => {
            bot.chain_cycles += 1;
            bot.state = BotRunState::InCycle;
            out.opened = Some(c.core.seq);
            Some(c)
        }
        Err("gridStartOutsideBand") => {
            bot.next_decision_ms = bar.open_ms + GRID_RETRY_MS;
            out.notes.push(("gridStartOutsideBand", None));
            None
        }
        Err(code) => {
            bot.state = BotRunState::Stopped;
            out.notes.push((code, None));
            None
        }
    }
}

/// Settles a closed cycle into the bot and applies the restart policy.
fn settle(bot: &mut StrategyBot, c: &CycleState, bar: &Bar, out: &mut BarOutcome) {
    bot.realized_quote += c.core.cash;
    bot.cycles_done += 1;
    let exit = c.core.exit.unwrap_or(ExitReason::Manual);
    let policy = &bot.cfg.restart;
    let running = is_running(bot.state);
    bot.state = if bot.state == BotRunState::Dead {
        BotRunState::Dead
    } else if exit == ExitReason::Liq && !policy.after_liquidation {
        bot.dead_reason = Some(ExitReason::Liq);
        BotRunState::Dead
    } else if exit == ExitReason::Ddstop {
        out.notes.push(("ddStop", None));
        BotRunState::Stopped
    } else if !running
        || matches!(
            exit,
            ExitReason::Manual
                | ExitReason::DailyStop
                | ExitReason::PortfolioDd
                | ExitReason::RemoteKill
                | ExitReason::End
        )
        // A stop-loss exit restarts only when the policy allows it.
        || (exit.is_stop() && !policy.after_stop)
    {
        BotRunState::Stopped
    } else if policy.max_cycles.is_some_and(|m| bot.chain_cycles >= m) {
        out.notes.push(("maxCyclesReached", None));
        BotRunState::Stopped
    } else {
        if policy.cooldown_min > 0 {
            out.notes.push(("cooldown", Some(policy.cooldown_min.to_string())));
        }
        BotRunState::Armed
    };
    bot.next_decision_ms = bar.end_ms() + u64::from(policy.cooldown_min) * 60_000;
}

/// One closed bar for one bot.
pub fn on_bar(
    bot: &mut StrategyBot,
    cycle: &mut Option<CycleState>,
    bar: &Bar,
    gate: Option<&'static str>,
) -> BarOutcome {
    let mut out = BarOutcome::default();
    let mut first = false;
    if cycle.is_none() {
        if !is_running(bot.state) {
            return out;
        }
        if bar.open_ms < bot.next_decision_ms {
            // A cooldown (or an hourly grid retry) is idle life of a running
            // bot: it counts towards utilisation like any other idle bar.
            if bot.cycles_done > 0 {
                accounting::add_bar(&mut bot.util, 0.0, 0.0);
            }
            return out;
        }
        match try_start(bot, bar, gate, &mut out) {
            Some(c) => {
                *cycle = Some(c);
                first = true;
            }
            None => {
                // An idle running bot after its first cycle still counts
                // towards utilisation (cooldowns included).
                if bot.cycles_done > 0 && is_running(bot.state) {
                    accounting::add_bar(&mut bot.util, 0.0, 0.0);
                }
                return out;
            }
        }
    }
    let Some(c) = cycle.as_mut() else {
        return out;
    };
    if !first && c.core.last_bar_close_ms > 0 && bar.open_ms > c.core.last_bar_close_ms + 1 + DATA_GAP_NOTE_MS {
        out.notes.push(("strategyDataGap", Some(((bar.open_ms - c.core.last_bar_close_ms - 1) / 60_000).to_string())));
    }
    let fund_unknown_before = c.core.funding_unknown;
    let seq = c.core.seq;
    c.core.dd_floor = bot.cfg.max_drawdown_pct.filter(|p| *p > 0.0).map(|pct| {
        bot.peak_equity - pct / 100.0 * bot.cfg.budget - bot.cfg.budget - bot.realized_quote
    });
    out.events
        .extend(step_bar(c, bar, first).into_iter().map(|f| (seq, f)));
    if c.core.funding_unknown && !fund_unknown_before {
        out.notes.push(("fundingUnknown", None));
    }
    // Per-bot drawdown stop on bar-close MTM.
    if c.is_open() {
        if let Some(pct) = bot.cfg.max_drawdown_pct.filter(|p| *p > 0.0) {
            let eq = accounting::equity(bot.cfg.budget, bot.realized_quote, c.mtm(bar.c));
            let peak = bot.peak_equity.max(eq);
            if peak - eq >= pct / 100.0 * bot.cfg.budget {
                c.core.now_ms = bar.close_ms;
                c.close_market(bar.c, ExitReason::Ddstop);
                out.events
                    .extend(c.drain_events().into_iter().map(|f| (seq, f)));
            }
        }
    }
    let margin = if c.is_open() { c.margin() } else { 0.0 };
    let reserved = if c.is_open() { c.committed() } else { 0.0 };
    accounting::add_bar(&mut bot.util, margin, reserved);
    if !c.is_open() {
        let closed = cycle.take().expect("cycle present");
        settle(bot, &closed, bar, &mut out);
        out.closed = Some(closed);
    }
    let open_mtm = cycle.as_ref().map(|c| c.mtm(bar.c)).unwrap_or(0.0);
    let eq = accounting::equity(bot.cfg.budget, bot.realized_quote, open_mtm);
    accounting::mark(bot, eq);
    out.equity = Some(EquityPoint {
        ts: bar.close_ms,
        equity: eq,
        margin,
        reserved,
    });
    out
}

/// Closes an open cycle now at `ref_px` (manual, kill switch, breaker,
/// remote kill), settles it and stops the bot. Returns the fills.
pub fn force_close(
    bot: &mut StrategyBot,
    cycle: &mut Option<CycleState>,
    ref_px: f64,
    now_ms: u64,
    reason: ExitReason,
) -> Option<(CycleState, Vec<Fill>)> {
    let c = cycle.as_mut()?;
    c.core.now_ms = now_ms;
    c.close_market(ref_px, reason);
    let fills = c.drain_events();
    let closed = cycle.take()?;
    bot.realized_quote += closed.core.cash;
    bot.cycles_done += 1;
    if bot.state != BotRunState::Dead {
        bot.state = BotRunState::Stopped;
    }
    let eq = accounting::equity(bot.cfg.budget, bot.realized_quote, 0.0);
    accounting::mark(bot, eq);
    Some((closed, fills))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::bot::strategy::model::{
        DcaParams, MarketKind, RestartPolicy, Side, StrategyConfig, Utilisation,
    };

    pub(crate) fn dca_params() -> DcaParams {
        DcaParams {
            base_order: Some(100.0),
            safety_order: Some(100.0),
            base_weight: None,
            safety_weight: None,
            max_so: 2,
            so_step_pct: 2.0,
            step_scale: 1.0,
            volume_scale: 1.0,
            tp_pct: 2.0,
            trailing_pct: None,
            sl_pct: None,
            max_duration_min: None,
        }
    }

    pub(crate) fn sample_bot() -> StrategyBot {
        let cfg = StrategyConfig {
            schema_version: 1,
            name: "t".into(),
            exchange_id: "binance".into(),
            market: MarketKind::Futures,
            symbol: "SYNUSDT".into(),
            side: Side::Long,
            budget: 1000.0,
            leverage: 1,
            start: StartCondition::Immediately,
            restart: RestartPolicy::default(),
            max_drawdown_pct: None,
            pause_on_btc_break: true,
            portfolio_breaker: true,
            params: StrategyParams::Dca(dca_params()),
            preset_id: None,
        };
        StrategyBot {
            id: "sb_abcdefghijkl".into(),
            cfg,
            state: BotRunState::Armed,
            created_at: 0,
            cycles_done: 0,
            realized_quote: 0.0,
            peak_equity: 1000.0,
            max_dd_quote: 0.0,
            dead_reason: None,
            next_decision_ms: 0,
            chain_cycles: 0,
            start_triggered: false,
            util: Utilisation::default(),
            archived_at: None,
        }
    }

    pub(crate) fn minute_bars(ohlc: &[(f64, f64, f64, f64)]) -> Vec<Bar> {
        ohlc.iter()
            .enumerate()
            .map(|(i, (o, h, l, c))| Bar {
                open_ms: 1_700_000_000_000 - (1_700_000_000_000 % 60_000) + i as u64 * 60_000,
                close_ms: 1_700_000_000_000 - (1_700_000_000_000 % 60_000) + i as u64 * 60_000 + 59_999,
                o: *o,
                h: *h,
                l: *l,
                c: *c,
                funding_rate: None,
                funding_unknown: false,
            })
            .collect()
    }

    /// A wavy series that drives several DCA cycles (SO fills and TPs).
    pub(crate) fn wave() -> Vec<Bar> {
        let mut v = Vec::new();
        let mut px = 100.0_f64;
        for i in 0..400 {
            let drift = ((i as f64) / 17.0).sin() * 0.9;
            let o = px;
            let c = (px + drift).max(1.0);
            let h = o.max(c) + 0.4;
            let l = o.min(c) - 0.4;
            v.push((o, h, l, c));
            px = c;
        }
        minute_bars(&v)
    }

    fn run(bot: &mut StrategyBot, cycle: &mut Option<CycleState>, bars: &[Bar]) -> Vec<(u32, String, f64)> {
        let mut log = Vec::new();
        for b in bars {
            let out = on_bar(bot, cycle, b, None);
            for (seq, f) in out.events {
                log.push((seq, f.client_id, f.price));
            }
        }
        log
    }

    #[test]
    fn restart_mid_stream_is_identical_to_an_uninterrupted_run() {
        let bars = wave();
        let mut a = sample_bot();
        let mut ca = None;
        let full = run(&mut a, &mut ca, &bars);
        assert!(a.cycles_done >= 2, "wave should complete cycles: {}", a.cycles_done);

        let mut b = sample_bot();
        let mut cb = None;
        let mut part = run(&mut b, &mut cb, &bars[..173]);
        // Simulated app restart: everything goes through its stored form.
        let b_json = serde_json::to_string(&b).unwrap();
        let c_json = serde_json::to_string(&cb).unwrap();
        let mut b2: StrategyBot = serde_json::from_str(&b_json).unwrap();
        let mut cb2: Option<CycleState> = serde_json::from_str(&c_json).unwrap();
        part.extend(run(&mut b2, &mut cb2, &bars[173..]));
        assert_eq!(full, part, "no double or lost fill across a restart");
        assert_eq!(a.realized_quote, b2.realized_quote);
        assert_eq!(a.cycles_done, b2.cycles_done);
    }

    #[test]
    fn a_stopped_bot_opens_nothing_but_manages_its_open_cycle() {
        let bars = wave();
        let mut bot = sample_bot();
        let mut cyc = None;
        on_bar(&mut bot, &mut cyc, &bars[0], None);
        assert!(cyc.is_some());
        bot.state = BotRunState::Stopped;
        let mut closed = false;
        for b in &bars[1..] {
            let out = on_bar(&mut bot, &mut cyc, b, None);
            closed |= out.closed.is_some();
            if closed {
                assert!(cyc.is_none());
                assert_eq!(bot.state, BotRunState::Stopped);
            }
        }
        assert!(closed);
        assert_eq!(bot.cycles_done, 1, "never re-armed after the stop");
    }

    #[test]
    fn gate_holds_new_cycles_with_a_note() {
        let bars = wave();
        let mut bot = sample_bot();
        let mut cyc = None;
        let out = on_bar(&mut bot, &mut cyc, &bars[0], Some("btcBreak"));
        assert!(cyc.is_none());
        assert_eq!(out.notes[0].0, "btcBreak");
    }

    #[test]
    fn cooldown_and_max_cycles_are_respected() {
        let bars = wave();
        let mut bot = sample_bot();
        bot.cfg.restart.max_cycles = Some(1);
        let mut cyc = None;
        run(&mut bot, &mut cyc, &bars);
        assert_eq!(bot.cycles_done, 1);
        assert_eq!(bot.state, BotRunState::Stopped);

        let mut bot = sample_bot();
        bot.cfg.restart.cooldown_min = 30;
        let mut cyc = None;
        let mut opens = Vec::new();
        let mut last_close = None;
        for b in &bars {
            let out = on_bar(&mut bot, &mut cyc, b, None);
            if out.opened.is_some() {
                if let Some(t) = last_close {
                    assert!(b.open_ms >= t + 30 * 60_000, "cooldown violated");
                }
                opens.push(b.open_ms);
            }
            if out.closed.is_some() {
                last_close = Some(b.end_ms());
            }
        }
        assert!(opens.len() >= 2);
    }

    #[test]
    fn price_cross_opens_on_the_bar_after_the_cross() {
        let bars = wave();
        let mut bot = sample_bot();
        let target = bars[5].l + 0.01;
        bot.cfg.start = StartCondition::PriceCross {
            price: target,
            direction: CrossDir::Down,
        };
        let mut cyc = None;
        let mut opened_at = None;
        for (i, b) in bars.iter().enumerate() {
            if on_bar(&mut bot, &mut cyc, b, None).opened.is_some() {
                opened_at = Some(i);
                break;
            }
        }
        let first_cross = bars.iter().position(|b| b.l <= target).unwrap();
        assert_eq!(opened_at, Some(first_cross + 1));
    }

    #[test]
    fn drawdown_stop_closes_and_stops_the_bot() {
        let mut bot = sample_bot();
        bot.cfg.max_drawdown_pct = Some(1.0);
        bot.cfg.params = StrategyParams::Dca(DcaParams {
            max_so: 0,
            safety_order: None,
            ..dca_params()
        });
        let bars = minute_bars(&[
            (100.0, 100.1, 99.9, 100.0),
            (100.0, 100.0, 80.0, 81.0),
            (81.0, 82.0, 80.0, 81.0),
        ]);
        let mut cyc = None;
        on_bar(&mut bot, &mut cyc, &bars[0], None);
        let out = on_bar(&mut bot, &mut cyc, &bars[1], None);
        let closed = out.closed.expect("closed by the dd stop");
        assert_eq!(closed.core.exit, Some(ExitReason::Ddstop));
        assert_eq!(bot.state, BotRunState::Stopped);
        assert!(out.notes.iter().any(|n| n.0 == "ddStop"));
    }

    // Owner's 4h backtest 3 Oct: a 25% drawdown stop closed a cycle at -29.4%
    // of budget, because it was only checked at the bar close.
    #[test]
    fn drawdown_stop_fills_at_its_level_inside_a_long_bar() {
        let mut bot = sample_bot();
        bot.cfg.max_drawdown_pct = Some(10.0);
        bot.cfg.params = StrategyParams::Dca(DcaParams {
            base_order: Some(1000.0),
            max_so: 0,
            safety_order: None,
            ..dca_params()
        });
        let bars = minute_bars(&[(100.0, 100.1, 99.9, 100.0), (100.0, 100.0, 50.0, 55.0)]);
        let mut cyc = None;
        on_bar(&mut bot, &mut cyc, &bars[0], None);
        let out = on_bar(&mut bot, &mut cyc, &bars[1], None);
        let closed = out.closed.expect("closed by the dd stop");
        assert_eq!(closed.core.exit, Some(ExitReason::Ddstop));
        // 10% of 1000 plus fees and slippage, nowhere near the -450 at the close.
        let loss = -closed.core.cash;
        assert!((100.0..103.0).contains(&loss), "loss {loss}");
        assert_eq!(bot.state, BotRunState::Stopped);
    }

    #[test]
    fn cooldown_bars_count_towards_utilisation() {
        // accounting::Utilisation::bars = "bars of life, cooldowns included".
        let bars = wave();
        let mut bot = sample_bot();
        bot.cfg.restart.cooldown_min = 30;
        let mut cyc = None;
        let mut first_open = None;
        for (i, b) in bars.iter().enumerate() {
            if on_bar(&mut bot, &mut cyc, b, None).opened.is_some() && first_open.is_none() {
                first_open = Some(i);
            }
        }
        assert!(bot.cycles_done >= 2);
        let life = (bars.len() - first_open.unwrap()) as f64;
        assert_eq!(bot.util.bars, life, "every bar from the first start is a bar of life");
    }

    #[test]
    fn a_sized_down_order_below_the_exchange_minimum_ends_the_bot_with_a_reason() {
        let mut bot = sample_bot();
        bot.cfg.params = StrategyParams::Dca(DcaParams {
            max_so: 0,
            safety_order: None,
            ..dca_params()
        });
        let full = crate::bot::strategy::validate::smallest_order_notional(&bot.cfg);
        assert!(full >= crate::bot::strategy::limits::MIN_ORDER_NOTIONAL);
        // losses leave a size factor that puts the smallest order just under 5
        let factor = (crate::bot::strategy::limits::MIN_ORDER_NOTIONAL * 0.99) / full;
        bot.realized_quote = -(1.0 - factor) * bot.cfg.budget;
        bot.next_decision_ms = 0;
        let bars = minute_bars(&[(100.0, 100.1, 99.9, 100.0)]);
        let mut cyc = None;
        let out = on_bar(&mut bot, &mut cyc, &bars[0], None);
        assert!(cyc.is_none() && out.opened.is_none(), "no sub-minimum order is filled");
        assert_eq!(bot.state, BotRunState::Dead);
        assert_eq!(bot.dead_reason, Some(ExitReason::SizeDown));
        assert_eq!(ExitReason::parse("sizeDown"), Some(ExitReason::SizeDown));
    }

    #[test]
    fn a_restart_beyond_the_drawdown_stop_opens_no_fee_burning_cycle() {
        let mut bot = sample_bot();
        bot.cfg.max_drawdown_pct = Some(1.0);
        bot.cfg.params = StrategyParams::Dca(DcaParams {
            max_so: 0,
            safety_order: None,
            ..dca_params()
        });
        let bars = minute_bars(&[
            (100.0, 100.1, 99.9, 100.0),
            (100.0, 100.0, 80.0, 81.0),
            (81.0, 82.0, 80.0, 81.0),
            (81.0, 82.0, 80.0, 81.0),
        ]);
        let mut cyc = None;
        on_bar(&mut bot, &mut cyc, &bars[0], None);
        on_bar(&mut bot, &mut cyc, &bars[1], None);
        assert_eq!(bot.state, BotRunState::Stopped);
        let realized = bot.realized_quote;
        let done = bot.cycles_done;
        // the user presses Start (strategy_start)
        bot.state = BotRunState::Armed;
        bot.next_decision_ms = 0;
        bot.chain_cycles = 0;
        let out = on_bar(&mut bot, &mut cyc, &bars[2], None);
        assert!(out.opened.is_none() && out.closed.is_none(), "no open-and-close in one bar");
        assert!(cyc.is_none());
        assert_eq!(bot.realized_quote, realized, "no fees paid");
        assert_eq!(bot.cycles_done, done);
        assert_eq!(bot.state, BotRunState::Stopped);
        assert!(out.notes.iter().any(|n| n.0 == "ddStop"));
        // a raised stop lets it run again
        bot.cfg.max_drawdown_pct = Some(50.0);
        bot.state = BotRunState::Armed;
        assert!(on_bar(&mut bot, &mut cyc, &bars[3], None).opened.is_some());
    }

    #[test]
    fn liquidation_kills_the_bot_by_default() {
        let mut bot = sample_bot();
        bot.cfg.side = Side::Short;
        bot.cfg.leverage = 10;
        bot.cfg.params = StrategyParams::Dca(DcaParams {
            base_order: Some(5000.0),
            safety_order: None,
            max_so: 0,
            tp_pct: 5.0,
            ..dca_params()
        });
        let bars = minute_bars(&[
            (100.0, 100.5, 99.6, 100.1),
            (100.1, 112.0, 100.0, 111.0),
            (111.0, 111.0, 111.0, 111.0),
        ]);
        let mut cyc = None;
        for b in &bars {
            on_bar(&mut bot, &mut cyc, b, None);
        }
        assert_eq!(bot.state, BotRunState::Dead);
        assert_eq!(bot.dead_reason, Some(ExitReason::Liq));
        assert!((bot.realized_quote + 502.5).abs() < 1e-9);
        assert_eq!(bot.cycles_done, 1);
    }

    #[test]
    fn an_absolute_grid_is_held_where_its_liquidation_enters_the_band() {
        use crate::bot::strategy::model::{GridParams, GridRange, Spacing};
        let mut bot = sample_bot();
        bot.cfg.leverage = 10;
        bot.cfg.params = StrategyParams::Grid(GridParams {
            range: GridRange::Absolute { lower: 90.0, upper: 110.0 },
            n_grids: 10,
            spacing: Spacing::Arith,
            stop_out_pct: Some(3.0),
            trailing_up: false,
            trail_up_limit: None,
            take_profit_pct: None,
            max_duration_min: None,
        });
        // passes at the range mid (validation), not at a start near the top
        assert_eq!(crate::bot::strategy::validate::validate(&bot.cfg, 10), Ok(()));
        let mut cyc = None;
        let top = minute_bars(&[(109.0, 109.2, 108.8, 109.0)]);
        let out = on_bar(&mut bot, &mut cyc, &top[0], None);
        assert!(cyc.is_none());
        assert!(out.notes.iter().any(|n| n.0 == "gridLiqInsideBand"));
        assert_eq!(bot.next_decision_ms, top[0].open_ms + GRID_RETRY_MS);
        let mut bot2 = bot.clone();
        bot2.next_decision_ms = 0;
        let mid = minute_bars(&[(99.5, 99.7, 99.3, 99.5)]);
        on_bar(&mut bot2, &mut cyc, &mid[0], None);
        assert!(cyc.is_some(), "the same grid opens near its mid");
    }

    #[test]
    fn a_grid_drawdown_stop_fills_inside_the_bar_too() {
        use crate::bot::strategy::model::{GridParams, GridRange, Spacing};
        let mut bot = sample_bot();
        bot.cfg.max_drawdown_pct = Some(10.0);
        bot.cfg.params = StrategyParams::Grid(GridParams {
            range: GridRange::Absolute { lower: 80.0, upper: 120.0 },
            n_grids: 10,
            spacing: Spacing::Arith,
            stop_out_pct: None,
            trailing_up: false,
            trail_up_limit: None,
            take_profit_pct: None,
            max_duration_min: None,
        });
        let bars = minute_bars(&[(100.0, 100.1, 99.9, 100.0), (100.0, 100.0, 40.0, 45.0)]);
        let mut cyc = None;
        on_bar(&mut bot, &mut cyc, &bars[0], None);
        assert!(cyc.is_some());
        let out = on_bar(&mut bot, &mut cyc, &bars[1], None);
        let closed = out.closed.expect("closed by the dd stop");
        assert_eq!(closed.core.exit, Some(ExitReason::Ddstop));
        let loss = -bot.realized_quote;
        assert!((95.0..110.0).contains(&loss), "loss {loss}");
        assert_eq!(bot.state, BotRunState::Stopped);
    }

    #[test]
    fn force_close_settles_at_market_and_stops() {
        let bars = wave();
        let mut bot = sample_bot();
        let mut cyc = None;
        on_bar(&mut bot, &mut cyc, &bars[0], None);
        let (closed, fills) =
            force_close(&mut bot, &mut cyc, 100.0, bars[0].close_ms, ExitReason::Manual).unwrap();
        assert_eq!(closed.core.exit, Some(ExitReason::Manual));
        assert_eq!(fills.len(), 1);
        assert_eq!(bot.state, BotRunState::Stopped);
        assert!(cyc.is_none());
    }
}
