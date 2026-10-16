//! Threshold crossings (pure): which checks crossed into danger for the
//! first time, so the Telegram line goes out once per crossing and the
//! in-app alert carries the time it started. A crossing re-arms only after
//! the value falls clearly back (below `REARM` x threshold), so a book
//! hovering at the line does not message every pass.

use std::collections::HashMap;

use super::stress::{Check, CheckKey, Level};

/// Share of the threshold the value must fall under to re-arm.
pub const REARM: f64 = 0.9;

#[derive(Default, Debug)]
pub struct Edge {
    /// (live book, check) -> when it crossed.
    crossed: HashMap<(bool, CheckKey), u64>,
}

impl Edge {
    /// Stamps `since_ms` on the book's danger checks and returns the ones
    /// that crossed in this pass.
    pub fn apply(&mut self, live: bool, checks: &mut [Check], now: u64) -> Vec<Check> {
        if checks.is_empty() {
            self.crossed.retain(|(l, _), _| *l != live);
            return Vec::new();
        }
        let mut fresh = Vec::new();
        for c in checks.iter_mut() {
            let key = (live, c.key);
            match c.level {
                Level::Danger => match self.crossed.get(&key) {
                    Some(at) => c.since_ms = Some(*at),
                    None => {
                        self.crossed.insert(key, now);
                        c.since_ms = Some(now);
                        fresh.push(c.clone());
                    }
                },
                _ => {
                    if c.value.is_some_and(|v| v < REARM * c.threshold) {
                        self.crossed.remove(&key);
                    }
                }
            }
        }
        fresh
    }
}

/// The Telegram line for a fresh crossing (English, like every Telegram
/// message of the app). `coin` / `side` / `loss` fill in the readout.
pub fn line(c: &Check, coin: Option<&str>, side: Option<&str>, loss: Option<f64>) -> String {
    let v = c.value.unwrap_or(0.0);
    match c.key {
        CheckKey::SameDirection => format!(
            "Portfolio risk: {v:.0}% of the capital in bots is {} (threshold {:.0}%).",
            side.unwrap_or("on one side"),
            c.threshold
        ),
        CheckKey::SingleCoin => format!(
            "Portfolio risk: {} holds {v:.0}% of the capital in bots (threshold {:.0}%).",
            coin.unwrap_or("one coin"),
            c.threshold
        ),
        CheckKey::Stress20 => match loss {
            Some(l) => format!(
                "Portfolio risk: BTC -20% would lose {l:.2} USDT, {v:.1}% of the capital in bots (threshold {:.0}%).",
                c.threshold
            ),
            None => format!("Portfolio risk: BTC -20% would lose {v:.1}% of the capital in bots (threshold {:.0}%).", c.threshold),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(key: CheckKey, value: f64, threshold: f64) -> Check {
        let level = if value > threshold {
            Level::Danger
        } else if value >= 0.8 * threshold {
            Level::Warn
        } else {
            Level::Ok
        };
        Check { key, value: Some(value), threshold, level, since_ms: None }
    }

    #[test]
    fn a_crossing_fires_once_and_rearms_only_below_ninety_percent() {
        let mut e = Edge::default();
        let mut c = vec![check(CheckKey::Stress20, 30.0, 25.0)];
        assert_eq!(e.apply(false, &mut c, 100).len(), 1);
        assert_eq!(c[0].since_ms, Some(100));
        // Still over: no new message, same start time.
        let mut c = vec![check(CheckKey::Stress20, 28.0, 25.0)];
        assert!(e.apply(false, &mut c, 200).is_empty());
        assert_eq!(c[0].since_ms, Some(100));
        // Dips to 24 (warn, above 22.5): not re-armed.
        e.apply(false, &mut [check(CheckKey::Stress20, 24.0, 25.0)], 300);
        assert!(e.apply(false, &mut [check(CheckKey::Stress20, 26.0, 25.0)], 400).is_empty());
        // Falls to 20: re-armed, the next crossing messages again.
        e.apply(false, &mut [check(CheckKey::Stress20, 20.0, 25.0)], 500);
        assert_eq!(e.apply(false, &mut [check(CheckKey::Stress20, 26.0, 25.0)], 600).len(), 1);
    }

    #[test]
    fn paper_and_real_books_cross_separately_and_an_empty_book_clears() {
        let mut e = Edge::default();
        assert_eq!(e.apply(false, &mut [check(CheckKey::SingleCoin, 50.0, 40.0)], 1).len(), 1);
        assert_eq!(e.apply(true, &mut [check(CheckKey::SingleCoin, 50.0, 40.0)], 2).len(), 1);
        e.apply(true, &mut [], 3);
        assert_eq!(e.apply(true, &mut [check(CheckKey::SingleCoin, 50.0, 40.0)], 4).len(), 1);
        assert!(e.apply(false, &mut [check(CheckKey::SingleCoin, 50.0, 40.0)], 5).is_empty());
    }

    #[test]
    fn lines_name_the_figures() {
        let c = check(CheckKey::Stress20, 31.2, 25.0);
        assert_eq!(
            line(&c, None, None, Some(123.456)),
            "Portfolio risk: BTC -20% would lose 123.46 USDT, 31.2% of the capital in bots (threshold 25%)."
        );
        let c = check(CheckKey::SingleCoin, 55.0, 40.0);
        assert!(line(&c, Some("SOL"), None, None).contains("SOL holds 55%"));
        let c = check(CheckKey::SameDirection, 92.0, 80.0);
        assert!(line(&c, None, Some("long"), None).contains("92% of the capital in bots is long"));
    }
}
