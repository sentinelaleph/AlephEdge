//! The Pine bot's decisions as pure functions over plain values: when a bar
//! is due, stale or missing, which mapped triggers fired, the resolution
//! order of spec section 6 (Close first, Buy + Sell conflict, flip, already
//! open), the exits from the bot and the script's inputs, the price guard,
//! the bot's limits and the halt after 3 failed runs. No clock, no network.

use crate::bot::model::{BotConfig, PineAction, PineSettings};
use crate::pine::{PineBar, PineErrorInfo, RunVerdict};
use crate::signal::model::Direction;

/// A bar is run this long after its close (the exchange has published it).
pub const RUN_DELAY_MS: u64 = 5_000;
/// The Pine phase's time budget per tick, across all symbols.
pub const PHASE_BUDGET_MS: u64 = 20_000;
/// Failed runs in a row after which a Live bot stops new entries.
pub const HALT_AFTER: u32 = 3;
/// At most one Telegram notice of a failed run per this window.
pub const NOTICE_EVERY_MS: u64 = 600_000;

pub const STALE: &str = "pineStale";
pub const BAR_MISSING: &str = "pineBarMissing";
pub const CONFLICT: &str = "pineConflict";
pub const ALREADY_OPEN: &str = "pineAlreadyOpen";
pub const NOTHING_TO_CLOSE: &str = "pineNothingToClose";
pub const PRICE_MOVED: &str = "pinePriceMoved";
pub const HALTED: &str = "pineHalted";

/// The action key and event id of one bar: `pine:<hash>:<symbol>:<close ms>`.
pub fn event_key(script_hash: &str, symbol: &str, bar_close_ms: u64) -> String {
    format!("pine:{script_hash}:{symbol}:{bar_close_ms}")
}

/// The close time (boundary, ms) of the newest bar that has closed at `now`.
/// Binance's futures bars of 15m / 1h / 4h / 1d start on these UTC multiples.
pub fn last_close(now_ms: u64, interval_ms: u64) -> u64 {
    now_ms - now_ms % interval_ms
}

/// The bar that closed at `close_ms` may run now.
pub fn due(now_ms: u64, close_ms: u64) -> bool {
    now_ms >= close_ms + RUN_DELAY_MS
}

/// More than the stale window has passed since the bar closed.
pub fn stale(now_ms: u64, close_ms: u64, stale_s: u32) -> bool {
    now_ms.saturating_sub(close_ms) > u64::from(stale_s) * 1000
}

/// The script runner: `pine::run_last_bar` in the app, a fake in tests.
pub type RunFn<'a> = &'a dyn Fn(&str, &[PineBar], &crate::pine::InputOverrides, &str, u64) -> Result<RunVerdict, PineErrorInfo>;

/// What one run of a symbol came to (before any order).
#[derive(Debug, Clone, PartialEq)]
pub enum RunOutcome {
    /// The newest closed bar is not the one that just closed: retried next
    /// tick within the stale window.
    BarMissing,
    Failed(PineErrorInfo),
    Ran(RunVerdict),
}

/// Checks the bars end on the bar that closed at `close_ms`, then runs the
/// script (`run` is `pine::run_last_bar` in the app, a fake in tests).
pub fn run_symbol(settings: &PineSettings, bars: &[PineBar], close_ms: u64, symbol: &str, run: RunFn) -> RunOutcome {
    let iv = settings.timeframe.interval_ms();
    if bars.last().map(|b| b.t + iv) != Some(close_ms) {
        return RunOutcome::BarMissing;
    }
    match run(&settings.script, bars, &settings.inputs, symbol, iv) {
        Ok(v) => RunOutcome::Ran(v),
        Err(e) => RunOutcome::Failed(e),
    }
}

/// The mapped triggers that fired on the last bar.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fired {
    pub buy: bool,
    pub sell: bool,
    pub close: bool,
    /// Every fired trigger with an action other than Ignore, in run order.
    pub ids: Vec<(String, PineAction)>,
}

impl Fired {
    pub fn any(&self) -> bool {
        self.buy || self.sell || self.close
    }
}

pub fn fired(ids: &[String], settings: &PineSettings) -> Fired {
    let mut f = Fired::default();
    for id in ids {
        let a = settings.action_of(id);
        match a {
            PineAction::Buy => f.buy = true,
            PineAction::Sell => f.sell = true,
            PineAction::Close => f.close = true,
            PineAction::Ignore => continue,
        }
        f.ids.push((id.clone(), a));
    }
    f
}

/// What one bar does on one symbol (spec section 6): `close` the bot's own
/// position first, then `open` one; `note` says why something did not happen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PinePlan {
    pub close: bool,
    pub open: Option<Direction>,
    pub note: Option<&'static str>,
}

/// First Close (the bot's own position on the symbol); then, with Buy and
/// Sell both, nothing opens (`pineConflict`); else Buy or Sell opens, or
/// flips an opposite position (close, then open); the same side already
/// open does nothing (`pineAlreadyOpen`).
pub fn plan(f: &Fired, own: Option<Direction>) -> PinePlan {
    let want = match (f.buy, f.sell) {
        (true, true) => return PinePlan { close: f.close && own.is_some(), open: None, note: Some(CONFLICT) },
        (true, false) => Some(Direction::Long),
        (false, true) => Some(Direction::Short),
        (false, false) => None,
    };
    let Some(dir) = want else {
        return match (f.close, own) {
            (true, Some(_)) => PinePlan { close: true, open: None, note: None },
            (true, None) => PinePlan { close: false, open: None, note: Some(NOTHING_TO_CLOSE) },
            (false, _) => PinePlan { close: false, open: None, note: None },
        };
    };
    match own {
        // Close fired with it: the position is closed first, then reopened.
        Some(d) if d == dir && !f.close => PinePlan { close: false, open: None, note: Some(ALREADY_OPEN) },
        Some(_) => PinePlan { close: true, open: Some(dir), note: None },
        None => PinePlan { close: false, open: Some(dir), note: None },
    }
}

/// The stop and target percents of one entry: the bot's own, or the
/// script's named `input.float` values. The bot's stop is the floor: with
/// both, the tighter one is used, so the script can never widen it.
pub fn exits(p: &PineSettings, numeric_inputs: &[(String, f64)]) -> Result<(f64, f64), String> {
    use crate::bot::model::pine_settings::{PINE_STOP_MAX_PCT, PINE_STOP_MIN_PCT, PINE_TARGET_MAX_PCT, PINE_TARGET_MIN_PCT};
    let read = |title: &Option<String>, lo: f64, hi: f64| -> Result<Option<f64>, String> {
        let Some(t) = title.as_deref().map(str::trim).filter(|t| !t.is_empty()) else {
            return Ok(None);
        };
        match numeric_inputs.iter().find(|(n, _)| n == t) {
            Some((_, v)) if v.is_finite() && (lo..=hi).contains(v) => Ok(Some(*v)),
            Some(_) => Err(format!("pineInputLevelInvalid|{t}")),
            None => Err(format!("pineInputNotFound|{t}")),
        }
    };
    let script_stop = read(&p.stop_input, PINE_STOP_MIN_PCT, PINE_STOP_MAX_PCT)?;
    let script_target = read(&p.target_input, PINE_TARGET_MIN_PCT, PINE_TARGET_MAX_PCT)?;
    let stop = match (p.stop_pct, script_stop) {
        (Some(b), Some(s)) => b.min(s),
        (b, s) => b.or(s).ok_or_else(|| "pineStopRequired".to_string())?,
    };
    let target = script_target.or(p.take_profit_pct).ok_or_else(|| "pineTargetRequired".to_string())?;
    Ok((stop, target))
}

/// The price guard: the live price more than `guard_pct` from the bar's
/// close refuses (`pinePriceMoved|0.73%`).
pub fn price_refusal(bar_close: f64, live: f64, guard_pct: f64) -> Result<(), String> {
    if !(bar_close.is_finite() && bar_close > 0.0 && live.is_finite() && live > 0.0) {
        return Err("priceUnavailable".to_string());
    }
    let moved = (live - bar_close).abs() / bar_close * 100.0;
    if moved > guard_pct + 1e-9 {
        return Err(format!("{PRICE_MOVED}|{moved:.2}%"));
    }
    Ok(())
}

/// What the Pine bot holds and has done.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Usage {
    pub open: usize,
    pub margin_usdt: f64,
    pub opens_today: u32,
}

/// The bot's own limits on one entry of `size` notional: the daily entry
/// cap, the position cap and the budget (margin = notional / leverage).
pub fn limit_refusal(cfg: &BotConfig, p: &PineSettings, size: f64, leverage: u8, u: Usage) -> Result<(), String> {
    if u.opens_today >= p.max_opens_per_day {
        return Err(format!("pineDailyOpensFull|{}", p.max_opens_per_day));
    }
    if u.open >= usize::from(cfg.max_positions) {
        return Err(format!("pinePositionsFull|{}", cfg.max_positions));
    }
    let margin = size / f64::from(leverage.max(1));
    if u.margin_usdt + margin > p.budget_usdt + 0.005 {
        return Err(format!("pineBudgetFull|{:.2}", (p.budget_usdt - u.margin_usdt).max(0.0)));
    }
    Ok(())
}

/// Failed runs in a row. A good run resets it; on a Live bot the third
/// failure in a row halts new entries (`pineHalted`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FailStreak(pub u32);

impl FailStreak {
    /// Records one run; `true` when this run halts the bot.
    pub fn record(&mut self, ok: bool, live: bool) -> bool {
        if ok {
            self.0 = 0;
            return false;
        }
        self.0 += 1;
        live && self.0 == HALT_AFTER
    }
}

/// Whether a failure notice may go out now (1 per `NOTICE_EVERY_MS`).
pub fn notice_due(last_ms: Option<u64>, now_ms: u64) -> bool {
    last_ms.is_none_or(|t| now_ms.saturating_sub(t) >= NOTICE_EVERY_MS)
}

/// The fill against the bar's close, in %, positive when worse for the
/// trade (a long filled above, a short below).
pub fn slippage_pct(direction: Direction, fill: f64, bar_close: f64) -> Option<f64> {
    crate::tvlink::tasks::slippage_pct(direction, fill, bar_close)
}
