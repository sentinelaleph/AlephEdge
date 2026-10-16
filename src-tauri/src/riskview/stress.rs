//! Portfolio aggregation (pure): BTC shock scenarios over the whole book,
//! the largest contributors, the BTC-explained share of the loss,
//! concentration, and the threshold checks.
//!
//! Paper and real money are two books, never summed. Capital in bots = the
//! capital each open exposure ties up (signal position capital, open cycle
//! budget; a live strategy bot at its real size). An exposure without a beta
//! is left out of a scenario and counted, never guessed.

use std::collections::BTreeMap;

use serde::Serialize;

use super::beta::BetaInfo;
use super::exposure::{Dir, Exposure, Source};
use crate::risk::model::ViewThresholds;

/// BTC moves the stress test applies, %.
pub const SHOCKS: [f64; 4] = [-10.0, -20.0, -30.0, 10.0];
/// The shock the threshold check and the what-if use.
pub const CHECK_SHOCK: f64 = -20.0;
/// How many contributors a scenario lists.
pub const TOP_N: usize = 5;
/// A check warns from this share of its threshold.
pub const WARN_FRACTION: f64 = 0.8;

/// One exposure's result in one scenario (or one replay window).
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Contribution {
    pub id: String,
    pub symbol: String,
    pub label: String,
    pub source: Source,
    /// Change from now, USDT.
    pub pnl: f64,
    pub exit: Option<&'static str>,
    /// Replay only: the coin's own path ("history") or beta x BTC ("beta").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basis: Option<&'static str>,
}

/// One book (paper or real) under one BTC move or replay window.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BookResult {
    /// Net change of the included exposures, USDT (negative = loss).
    pub pnl: f64,
    /// `pnl` as % of the book's capital in bots.
    pub pct_of_capital: Option<f64>,
    /// Of the losses, the share BTC's direction explains (loss-weighted
    /// mean of each coin's R^2 to BTC), %. None without losses.
    pub btc_share_pct: Option<f64>,
    /// The largest losses first (at most TOP_N); gains are not listed.
    pub top: Vec<Contribution>,
    pub included: u32,
    /// Exposures without a beta (or a path): left out of `pnl`.
    pub excluded: u32,
    /// Replay: included exposures that followed beta x BTC (no own history).
    pub estimated: u32,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Scenario {
    pub btc_move_pct: f64,
    pub book: BookResult,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Concentration {
    /// Share of capital on the dominant side, %.
    pub same_direction_pct: Option<f64>,
    pub dominant: Option<Dir>,
    pub top_coin: Option<String>,
    pub top_coin_pct: Option<f64>,
    /// Capital-weighted mean beta of the exposures that have one.
    pub avg_beta: Option<f64>,
    pub with_beta: u32,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    Ok,
    Warn,
    Danger,
    Unavailable,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum CheckKey {
    SameDirection,
    SingleCoin,
    Stress20,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub key: CheckKey,
    pub value: Option<f64>,
    pub threshold: f64,
    pub level: Level,
    /// When this book first crossed the threshold in this run (UNIX ms).
    pub since_ms: Option<u64>,
}

/// One book's full stress readout.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BookStress {
    pub live: bool,
    pub exposures: u32,
    pub capital: f64,
    pub scenarios: Vec<Scenario>,
    pub concentration: Concentration,
    pub checks: Vec<Check>,
}

/// Builds a book result from per-exposure outcomes (None = excluded).
pub fn aggregate(capital: f64, rows: Vec<(Contribution, Option<f64>)>, excluded: u32) -> BookResult {
    let pnl: f64 = rows.iter().map(|(c, _)| c.pnl).sum();
    let losses: f64 = rows.iter().map(|(c, _)| (-c.pnl).max(0.0)).sum();
    let explained: f64 = rows
        .iter()
        .map(|(c, r2)| (-c.pnl).max(0.0) * r2.unwrap_or(0.0))
        .sum();
    let included = rows.len() as u32;
    let estimated = rows.iter().filter(|(c, _)| c.basis == Some("beta")).count() as u32;
    let mut top: Vec<Contribution> = rows.into_iter().map(|(c, _)| c).filter(|c| c.pnl < 0.0).collect();
    top.sort_by(|a, b| a.pnl.total_cmp(&b.pnl));
    top.truncate(TOP_N);
    BookResult {
        pnl,
        pct_of_capital: (capital > 0.0 && included > 0).then(|| pnl / capital * 100.0),
        btc_share_pct: (losses > 1e-9).then(|| explained / losses * 100.0),
        top,
        included,
        excluded,
        estimated,
    }
}

pub fn contribution(e: &Exposure, pnl: f64, exit: Option<&'static str>, basis: Option<&'static str>) -> Contribution {
    Contribution {
        id: e.id.clone(),
        symbol: e.symbol.clone(),
        label: e.label.clone(),
        source: e.source,
        pnl,
        exit,
        basis,
    }
}

/// The book under a straight BTC move of `btc_move_pct` %.
pub fn scenario(book: &[(&Exposure, Option<BetaInfo>)], btc_move_pct: f64, t0: u64) -> BookResult {
    let capital = book.iter().map(|(e, _)| e.capital).sum();
    let mut rows = Vec::new();
    let mut excluded = 0;
    for (e, beta) in book {
        match beta {
            Some(b) => {
                let out = e.shock(b.beta, btc_move_pct, t0);
                rows.push((contribution(e, out.pnl, out.exit, None), Some(b.r2)));
            }
            None => excluded += 1,
        }
    }
    aggregate(capital, rows, excluded)
}

/// Base asset of a USDT pair ("SOLUSDT" -> "SOL").
pub fn base_of(symbol: &str) -> &str {
    symbol.strip_suffix("USDT").filter(|b| !b.is_empty()).unwrap_or(symbol)
}

pub fn concentration(book: &[(&Exposure, Option<BetaInfo>)]) -> Concentration {
    let total: f64 = book.iter().map(|(e, _)| e.capital).sum();
    if total <= 0.0 {
        return Concentration {
            same_direction_pct: None,
            dominant: None,
            top_coin: None,
            top_coin_pct: None,
            avg_beta: None,
            with_beta: 0,
        };
    }
    let side = |d: Dir| book.iter().filter(|(e, _)| e.direction == d).map(|(e, _)| e.capital).sum::<f64>();
    let (long, short) = (side(Dir::Long), side(Dir::Short));
    let (dominant, dom_cap) = if long >= short { (Dir::Long, long) } else { (Dir::Short, short) };
    let mut by_coin: BTreeMap<&str, f64> = BTreeMap::new();
    for (e, _) in book {
        *by_coin.entry(base_of(&e.symbol)).or_default() += e.capital;
    }
    let (coin, coin_cap) = by_coin
        .iter()
        .max_by(|a, b| a.1.total_cmp(b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(k, v)| (k.to_string(), *v))
        .unwrap_or_default();
    let (mut wsum, mut wcap, mut with_beta) = (0.0, 0.0, 0u32);
    for (e, b) in book {
        if let Some(b) = b {
            wsum += b.beta * e.capital;
            wcap += e.capital;
            with_beta += 1;
        }
    }
    Concentration {
        same_direction_pct: (dom_cap > 0.0).then(|| dom_cap / total * 100.0),
        dominant: (dom_cap > 0.0).then_some(dominant),
        top_coin: Some(coin),
        top_coin_pct: Some(coin_cap / total * 100.0),
        avg_beta: (wcap > 0.0).then(|| wsum / wcap),
        with_beta,
    }
}

fn level(value: Option<f64>, threshold: f64) -> Level {
    match value {
        None => Level::Unavailable,
        Some(v) if v > threshold => Level::Danger,
        Some(v) if v >= WARN_FRACTION * threshold => Level::Warn,
        Some(_) => Level::Ok,
    }
}

/// The three checks of a non-empty book. The stress value is the loss at
/// BTC -20% as % of capital (0 when that scenario gains); unavailable when
/// any exposure had no beta, since a partial sum would understate it.
pub fn checks(c: &Concentration, stress20: Option<&BookResult>, t: &ViewThresholds) -> Vec<Check> {
    let stress = stress20
        .filter(|r| r.excluded == 0)
        .and_then(|r| r.pct_of_capital)
        .map(|p| (-p).max(0.0));
    [
        (CheckKey::SameDirection, c.same_direction_pct, t.same_direction_pct),
        (CheckKey::SingleCoin, c.top_coin_pct, t.single_coin_pct),
        (CheckKey::Stress20, stress, t.stress20_pct),
    ]
    .into_iter()
    .map(|(key, value, threshold)| Check { key, value, threshold, level: level(value, threshold), since_ms: None })
    .collect()
}

/// One book's readout: every shock, concentration and the checks.
pub fn book_stress(book: &[(&Exposure, Option<BetaInfo>)], live: bool, t: &ViewThresholds, t0: u64) -> BookStress {
    let scenarios: Vec<Scenario> = SHOCKS
        .iter()
        .map(|m| Scenario { btc_move_pct: *m, book: scenario(book, *m, t0) })
        .collect();
    let concentration = concentration(book);
    let checks = if book.is_empty() {
        Vec::new()
    } else {
        let s20 = scenarios.iter().find(|s| s.btc_move_pct == CHECK_SHOCK).map(|s| &s.book);
        checks(&concentration, s20, t)
    };
    BookStress {
        live,
        exposures: book.len() as u32,
        capital: book.iter().map(|(e, _)| e.capital).sum(),
        scenarios,
        concentration,
        checks,
    }
}

#[cfg(test)]
mod tests {
    use super::super::exposure::tests::{cycle_exposure, dca_cycle, long_position, pos_exposure};
    use super::*;
    use crate::bot::strategy::model::MarketKind;

    fn beta(b: f64, r2: f64) -> Option<BetaInfo> {
        Some(BetaInfo { beta: b, raw: b, r2, samples: 500, clamped: false })
    }

    #[test]
    fn a_scenario_sums_losses_ranks_contributors_and_splits_the_btc_share() {
        let a = pos_exposure(long_position(50.0, 200.0, 1.0), 100.0);
        let mut b = pos_exposure(long_position(50.0, 200.0, 1.0), 100.0);
        b.id = "b".into();
        b.symbol = "ETHUSDT".into();
        let mut c = pos_exposure(long_position(50.0, 200.0, 1.0), 100.0);
        c.id = "c".into();
        let book = vec![(&a, beta(1.0, 1.0)), (&b, beta(2.0, 0.5)), (&c, None)];
        let r = scenario(&book, -10.0, 0);
        // a: -10, b: -20, c excluded.
        assert!((r.pnl - (-30.0)).abs() < 1e-9, "{r:?}");
        assert_eq!((r.included, r.excluded), (2, 1));
        assert_eq!(r.top[0].id, "b");
        // Capital 300 (excluded exposures still hold capital).
        assert!((r.pct_of_capital.unwrap() - (-10.0)).abs() < 1e-9);
        // (10 * 1.0 + 20 * 0.5) / 30.
        assert!((r.btc_share_pct.unwrap() - 200.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn concentration_reads_direction_coin_and_beta() {
        let a = pos_exposure(long_position(50.0, 200.0, 1.0), 100.0);
        let mut s = cycle_exposure(dca_cycle(None, 1.0, MarketKind::Spot), 100.0);
        s.direction = Dir::Short;
        s.symbol = "BTCUSDT".into();
        let book = vec![(&a, beta(2.0, 1.0)), (&s, beta(1.0, 1.0))];
        let c = concentration(&book);
        // a: 100 long (SOL), s: 400 short (BTC).
        assert_eq!(c.dominant, Some(Dir::Short));
        assert!((c.same_direction_pct.unwrap() - 80.0).abs() < 1e-9);
        assert_eq!(c.top_coin.as_deref(), Some("BTC"));
        assert!((c.top_coin_pct.unwrap() - 80.0).abs() < 1e-9);
        assert!((c.avg_beta.unwrap() - (2.0 * 100.0 + 400.0) / 500.0).abs() < 1e-9);
    }

    #[test]
    fn checks_warn_near_and_danger_past_the_threshold() {
        let t = ViewThresholds::default();
        let conc = Concentration {
            same_direction_pct: Some(100.0),
            dominant: Some(Dir::Long),
            top_coin: Some("SOL".into()),
            top_coin_pct: Some(33.0),
            avg_beta: Some(1.4),
            with_beta: 3,
        };
        let stress = BookResult { pnl: -21.0, pct_of_capital: Some(-21.0), btc_share_pct: None, top: vec![], included: 3, excluded: 0, estimated: 0 };
        let c = checks(&conc, Some(&stress), &t);
        assert_eq!(c[0].level, Level::Danger, "100% > 80%");
        assert_eq!(c[1].level, Level::Warn, "33% >= 0.8 x 40%");
        assert_eq!(c[2].level, Level::Warn, "21% >= 20%");
        assert_eq!(c[2].value, Some(21.0));
        // An exposure without beta makes the stress check unavailable.
        let partial = BookResult { excluded: 1, ..stress };
        assert_eq!(checks(&conc, Some(&partial), &t)[2].level, Level::Unavailable);
    }

    #[test]
    fn an_empty_book_has_no_checks_and_no_percentages() {
        let s = book_stress(&[], false, &ViewThresholds::default(), 0);
        assert!(s.checks.is_empty());
        assert_eq!(s.scenarios.len(), 4);
        assert_eq!(s.scenarios[0].book.pct_of_capital, None);
        assert_eq!(s.concentration.same_direction_pct, None);
    }
}
