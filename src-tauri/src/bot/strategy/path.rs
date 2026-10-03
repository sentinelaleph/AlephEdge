//! Pure intrabar walker shared by the paper engine and any backtest: the
//! research simulator's conservative rule, at whatever bar resolution the
//! caller feeds (1m live bars, 1h research bars).
//!
//! 1. A bar is walked O -> X1 -> X2 -> C with monotone legs. C > O: low
//!    first; C < O: high first; C == O: the extreme that hurts the position
//!    held at the open first (long: low, short: high, flat: low).
//! 2. The move from the previous close to this open is a gap leg.
//! 3. Protective exits win: when walking the harmful extreme first triggers a
//!    stop, stop-out or liquidation, that walk is committed.

use std::cmp::Ordering;

use super::model::Bar;

/// Four path points O, X1, X2, C.
pub type Path4 = [f64; 4];

/// (natural path, harmful-extreme-first path). `exposure`: +1 long, -1
/// short, 0 flat, at the bar open.
pub fn bar_paths(o: f64, h: f64, l: f64, c: f64, exposure: i8) -> (Path4, Path4) {
    let lo_first = [o, l, h, c];
    let hi_first = [o, h, l, c];
    // Rule 1 of the header, one arm per case.
    let high_first = match c.partial_cmp(&o) {
        // C > O: low first.
        Some(Ordering::Greater) => false,
        // C < O: high first.
        Some(Ordering::Less) => true,
        // C == O: the extreme that hurts the position held at the open
        // first: high for a short, low for a long or flat.
        _ => exposure < 0,
    };
    let nat = if high_first { hi_first } else { lo_first };
    let adv = if exposure > 0 {
        lo_first
    } else if exposure < 0 {
        hi_first
    } else {
        nat
    };
    (nat, adv)
}

/// The jump from the previous close to this open; none on the first bar of
/// a cycle or when there is no move.
pub fn gap_leg(prev_close: Option<f64>, open: f64) -> Option<(f64, f64)> {
    match prev_close {
        Some(pc) if pc != open => Some((pc, open)),
        _ => None,
    }
}

/// A cycle that can be walked through a bar.
pub trait Walker: Clone {
    fn exposure(&self) -> i8;
    /// Whether any protective exit (stop, stop-out, liquidation) exists.
    fn can_protect(&self) -> bool;
    fn is_open(&self) -> bool;
    /// Closed by a stop, stop-out or liquidation.
    fn closed_protectively(&self) -> bool;
    /// One monotone leg; `gap` marks the previous-close-to-open jump.
    fn seg(&mut self, from: f64, to: f64, gap: bool);
    /// Funding of a timestamp inside `bar`, on the position held just after
    /// the open.
    fn funding(&mut self, bar: &Bar, open: f64);
}

fn walk_path<W: Walker>(w: &mut W, bar: &Bar, path: Path4, prev_close: Option<f64>) {
    if let Some((a, b)) = gap_leg(prev_close, path[0]) {
        w.seg(a, b, true);
    }
    if w.is_open() {
        w.funding(bar, path[0]);
    }
    for (a, b) in [(path[0], path[1]), (path[1], path[2]), (path[2], path[3])] {
        if !w.is_open() {
            break;
        }
        w.seg(a, b, false);
    }
}

/// Walks one closed bar with the protective-first overlay. `prev_close` is
/// `None` on the cycle's start bar.
pub fn walk_bar<W: Walker>(w: &mut W, bar: &Bar, prev_close: Option<f64>) {
    let (nat, adv) = bar_paths(bar.o, bar.h, bar.l, bar.c, w.exposure());
    if nat != adv && w.can_protect() {
        let mut trial = w.clone();
        walk_path(&mut trial, bar, adv, prev_close);
        if !trial.is_open() && trial.closed_protectively() {
            *w = trial;
            return;
        }
    }
    walk_path(w, bar, nat, prev_close);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar_legs(o: f64, h: f64, l: f64, c: f64, exposure: i8) -> Path4 {
        bar_paths(o, h, l, c, exposure).0
    }

    #[test]
    fn leg_order_by_bar_direction_and_exposure() {
        assert_eq!(bar_legs(10.0, 12.0, 9.0, 11.0, 1), [10.0, 9.0, 12.0, 11.0]);
        assert_eq!(bar_legs(10.0, 12.0, 9.0, 9.5, 1), [10.0, 12.0, 9.0, 9.5]);
        // Bar direction alone decides when C != O, whatever is held.
        assert_eq!(bar_legs(10.0, 12.0, 9.0, 11.0, -1), [10.0, 9.0, 12.0, 11.0]);
        assert_eq!(bar_legs(10.0, 12.0, 9.0, 9.5, -1), [10.0, 12.0, 9.0, 9.5]);
        assert_eq!(bar_legs(10.0, 12.0, 9.0, 9.5, 0), [10.0, 12.0, 9.0, 9.5]);
        // C == O: the harmful extreme first
        assert_eq!(bar_legs(10.0, 12.0, 9.0, 10.0, 1), [10.0, 9.0, 12.0, 10.0]);
        assert_eq!(bar_legs(10.0, 12.0, 9.0, 10.0, -1), [10.0, 12.0, 9.0, 10.0]);
        assert_eq!(bar_legs(10.0, 12.0, 9.0, 10.0, 0), [10.0, 9.0, 12.0, 10.0]);
    }

    #[test]
    fn adverse_path_follows_exposure() {
        let (nat, adv) = bar_paths(10.0, 12.0, 9.0, 11.0, -1);
        assert_eq!(nat, [10.0, 9.0, 12.0, 11.0]);
        assert_eq!(adv, [10.0, 12.0, 9.0, 11.0]);
        let (nat, adv) = bar_paths(10.0, 12.0, 9.0, 11.0, 0);
        assert_eq!(nat, adv);
    }

    #[test]
    fn gap_leg_only_on_a_move() {
        assert_eq!(gap_leg(None, 10.0), None);
        assert_eq!(gap_leg(Some(10.0), 10.0), None);
        assert_eq!(gap_leg(Some(9.0), 10.0), Some((9.0, 10.0)));
    }
}
