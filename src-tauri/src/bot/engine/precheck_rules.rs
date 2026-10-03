//! The PRD §5.4 precheck decisions, pure: skip type, the signal-validity,
//! funding, depth and risk-budget verdicts, and the final/transient split.
//!
//! Split out of `precheck.rs` (2026-09-22) so the headless paper runner
//! compiles THIS file — the exact rules the desktop applies — without Tauri or
//! any manager. `precheck.rs` keeps the fetching (`run`) and re-exports all of
//! it; code in either program reaches these items as `super::precheck::…`.

use crate::risk::model::RiskLimits;
use crate::signal::model::{Direction, OpenStatus, Signal};
use crate::signal::time::parse_rfc3339_ms;

use super::super::model::BotConfig;

/// Why a signal was not traded. `key` maps to an i18n string; `detail` is a
/// preformatted value the UI interpolates.
#[derive(Debug)]
pub struct Skip {
    pub key: &'static str,
    pub detail: Option<String>,
}

impl Skip {
    pub(super) fn new(key: &'static str) -> Self {
        Self { key, detail: None }
    }
    pub(super) fn with(key: &'static str, detail: String) -> Self {
        Self {
            key,
            detail: Some(detail),
        }
    }
}

/// Numbers a passing precheck hands to the fill (context captured at open).
pub struct PrecheckContext {
    pub fr_at_open: Option<f64>,
    pub ld_at_open: Option<f64>,
}

/// Position counts + simulated balance the risk-budget checks need.
pub struct Budget {
    /// Open positions across ALL bots (global concurrent ceiling).
    pub global_open: usize,
    /// Open positions held by THIS bot (per-bot ceiling).
    pub bot_open: usize,
    /// Simulated account equity — base for the % caps.
    pub balance: f64,
    /// The SIZED notional of the candidate position (engine/sizing.rs). In
    /// risk mode it is not capital × leverage, so the depth check must see
    /// the real size.
    pub notional: f64,
}

/// Step 2: the signal is still valid — not invalidated server-side, not
/// vetoed by the BTC-regime guard (2026-09-18 — the veto ALSO closes any open
/// position, see `engine::book::close_on_veto`; this only blocks a
/// not-yet-filled entry), not expired.
pub fn signal_still_valid(
    invalidated: bool,
    regime_vetoed: bool,
    expires_at: &str,
    now_ms: u64,
) -> Result<(), Skip> {
    if invalidated {
        return Err(Skip::new("invalidated"));
    }
    if regime_vetoed {
        return Err(Skip::new("vetoedByRegime"));
    }
    if expired(expires_at, now_ms) {
        return Err(Skip::new("expired"));
    }
    Ok(())
}

/// Step 2b (desktop): Sentinel has not already recorded the signal's outcome.
/// A resolved signal is never entered; one whose status cannot be confirmed
/// is refused when `strict` (always for a live bot). The paper runner's arms
/// keep their pre-registered rules and do not call this.
pub fn judge_still_open(status: OpenStatus, strict: bool) -> Result<(), Skip> {
    match status {
        OpenStatus::Open => Ok(()),
        OpenStatus::Resolved => Err(Skip::new("signalResolved")),
        OpenStatus::Unknown if strict => Err(Skip::new("signalStatusUnknown")),
        OpenStatus::Unknown => Ok(()),
    }
}

/// Strict data policy (Cautious/Calm): missing FR/LD data = no trade.
pub fn strict_data(limits: &RiskLimits) -> bool {
    limits.fr_threshold_pct <= 0.05
}

/// Step 3 (futures-market bots only): skip when funding runs against the
/// position beyond the level's threshold. `fetched` is the funding rate in
/// PERCENT per interval: Ok(Some) = covered, Ok(None) = feed has no row for
/// the symbol, Err = feed unreachable or no session. Returns `fr_at_open`.
// The level-policy form, used by the paper runner; the desktop passes
// strictness explicitly (live bots are always strict).
#[allow(dead_code)]
pub fn judge_funding(
    limits: &RiskLimits,
    direction: Direction,
    fetched: Result<Option<f64>, ()>,
) -> Result<Option<f64>, Skip> {
    judge_funding_with(limits, strict_data(limits), direction, fetched)
}

/// `judge_funding` with the missing-data policy explicit: a live bot always
/// runs strict (never trade real money on an unknown funding rate).
pub fn judge_funding_with(
    limits: &RiskLimits,
    strict: bool,
    direction: Direction,
    fetched: Result<Option<f64>, ()>,
) -> Result<Option<f64>, Skip> {
    match fetched {
        Ok(Some(rate)) => {
            let against = match direction {
                Direction::Long => rate > limits.fr_threshold_pct,
                Direction::Short => rate < -limits.fr_threshold_pct,
            };
            if against {
                return Err(Skip::with("frAgainst", format!("{rate:+.3}%")));
            }
            Ok(Some(rate))
        }
        Ok(None) | Err(_) if strict => Err(Skip::new("frUnavailable")),
        Ok(None) | Err(_) => Ok(None),
    }
}

/// Step 4: liquidity depth. `notional` must stay under the level's share of
/// the ±1% book. `fetched` = (bid, ask) depth in USD; Err = feed unreachable
/// or no session. Thin book = slippage risk = skip. Returns `ld_at_open`.
// The level-policy form, used by the paper runner; the desktop passes
// strictness explicitly (live bots are always strict).
#[allow(dead_code)]
pub fn judge_depth(
    limits: &RiskLimits,
    notional: f64,
    fetched: Result<(f64, f64), ()>,
) -> Result<Option<f64>, Skip> {
    judge_depth_with(limits, strict_data(limits), notional, fetched)
}

/// `judge_depth` with the missing-data policy explicit (see
/// `judge_funding_with`). An empty book is too thin, not "no limit".
pub fn judge_depth_with(
    limits: &RiskLimits,
    strict: bool,
    notional: f64,
    fetched: Result<(f64, f64), ()>,
) -> Result<Option<f64>, Skip> {
    match fetched {
        Ok((bid, ask)) => {
            let book = bid + ask;
            if book.is_nan() || book <= 0.0 || notional > book * limits.max_depth_share_pct / 100.0 {
                return Err(Skip::with("ldTooThin", format!("${book:.0}")));
            }
            Ok(Some(book))
        }
        Err(_) if strict => Err(Skip::new("ldUnavailable")),
        Err(_) => Ok(None),
    }
}

/// Step 5: the risk budget. Three independent ceilings.
pub fn judge_budget(limits: &RiskLimits, cfg: &BotConfig, budget: &Budget) -> Result<(), Skip> {
    //    (a) global concurrent positions across all bots,
    let global_max = usize::from(limits.max_concurrent_positions);
    if budget.global_open >= global_max {
        return Err(Skip::with(
            "maxPositions",
            format!("{}/{global_max}", budget.global_open),
        ));
    }
    //    (b) this bot's own position count,
    let bot_max = usize::from(cfg.max_positions);
    if budget.bot_open >= bot_max {
        return Err(Skip::with(
            "botMaxPositions",
            format!("{}/{bot_max}", budget.bot_open),
        ));
    }
    //    (c) per-position capital as a % of the simulated balance.
    let cap = budget.balance * limits.max_capital_pct / 100.0;
    if budget.balance > 0.0 && cfg.capital > cap {
        return Err(Skip::with("capitalCap", format!("{cap:.0}")));
    }
    Ok(())
}

/// Leverage the fill actually uses: spot is always 1x; futures-market bots
/// (Futures, Pump) use the bot's setting clamped by the risk level.
pub fn effective_leverage(cfg: &BotConfig, limits: &RiskLimits) -> u8 {
    if cfg.kind.uses_futures_market() {
        cfg.leverage.clamp(1, limits.max_leverage)
    } else {
        1
    }
}

/// A Pump signal (the "supersport on an empty highway" thesis): it needs the
/// full structure, not just a pump reading. Confluence `source` values are
/// Sentinel's lowercase engine IDs (verified: "pump", "msb", "choch", "ob",
/// "fvg"), so match exactly — `contains` would false-match "ob" against the
/// "obv" indicator. Required, all three:
///   1. `pump`  — the pump/dump engine is firing (the accelerating car),
///   2. `msb` OR `choch` — a confirmed structure shift (the highway direction),
///   3. `ob` OR `fvg` — a valid entry zone (the seatbelt: enter at the right
///      spot, not mid-run — critical on leveraged futures).
pub fn is_pump_signal(sig: &Signal) -> bool {
    let has = |ids: &[&str]| {
        sig.confluence
            .iter()
            .any(|c| c.score > 0.0 && ids.iter().any(|id| c.source.eq_ignore_ascii_case(id)))
    };
    has(&["pump", "pump_dump", "pumpdump"]) && has(&["msb", "choch"]) && has(&["ob", "fvg"])
}

/// Unparseable expiry is treated as expired (fail-safe, never enter blind).
fn expired(expires_at: &str, now_ms: u64) -> bool {
    parse_rfc3339_ms(expires_at).map_or(true, |ts| ts <= now_ms)
}

/// Skip keys that are a FINAL verdict on a signal for a bot: a static
/// property of the signal or bot settings, or a lifecycle end. Every other
/// key (feeds down, caps full, funding/depth, regime) is transient and the
/// signal retries until it expires — marking those judged made a momentary
/// condition refuse the signal forever.
pub fn is_final(key: &str) -> bool {
    matches!(
        key,
        "spotLongOnly"
            | "belowConfidence"
            | "directionFiltered"
            | "symbolFiltered"
            | "comboFiltered"
            | "engineFiltered"
            | "spotOnlyExchange"
            | "notPumpSignal"
            | "incoherentGeometry"
            | "fillCrossedLevel"
            | "invalidated"
            | "vetoedByRegime"
            | "expired"
            | "tooOld"
            // Live entry failures are final: retrying every 4s tick would
            // re-send real orders, and a failed stop already flattened once —
            // a retry loop would pay fees to open and flatten repeatedly.
            | "liveOrderFailed"
            | "liveProtectiveStopFailed"
            | "liveQtyBelowMinimum"
            | "liveStopBeyondLiquidation"
            | "liveOrderUnconfirmed"
            // Binance refused the setup for good (see live::setup_failure_skip).
            | "liveLeverageRejected"
            | "liveMarginTypeRejected"
            | "liveSymbolNotTrading"
            | "sizingInvalidStop"
            // The user's take-profit target cannot apply to this signal.
            | "tpCustomInvalid"
            | "tpNotBeyondFill"
    )
}

#[cfg(test)]
mod data_policy_tests {
    use super::*;
    use crate::risk::model::RiskLevel;

    #[test]
    fn an_empty_book_is_too_thin_not_unlimited() {
        let l = RiskLevel::Balanced.limits();
        assert_eq!(judge_depth_with(&l, false, 100.0, Ok((0.0, 0.0))).unwrap_err().key, "ldTooThin");
        assert!(judge_depth_with(&l, false, 100.0, Ok((50_000.0, 50_000.0))).is_ok());
    }

    #[test]
    fn a_strict_bot_refuses_missing_data_on_any_level() {
        let l = RiskLevel::Greedy.limits();
        assert!(judge_depth_with(&l, false, 100.0, Err(())).unwrap().is_none(), "lenient passes");
        assert_eq!(judge_depth_with(&l, true, 100.0, Err(())).unwrap_err().key, "ldUnavailable");
        assert_eq!(
            judge_funding_with(&l, true, Direction::Long, Ok(None)).unwrap_err().key,
            "frUnavailable"
        );
    }

    #[test]
    fn a_resolved_signal_is_never_entered_and_an_unknown_one_only_leniently() {
        assert!(judge_still_open(OpenStatus::Open, true).is_ok());
        assert_eq!(judge_still_open(OpenStatus::Resolved, false).unwrap_err().key, "signalResolved");
        assert_eq!(judge_still_open(OpenStatus::Unknown, true).unwrap_err().key, "signalStatusUnknown");
        assert!(judge_still_open(OpenStatus::Unknown, false).is_ok(), "paper, lenient level");
    }
}
