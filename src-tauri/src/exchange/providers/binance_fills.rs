//! The account's real fills (`GET /fapi/v1/userTrades`) → what one position
//! actually paid and received.
//!
//! This is the measurement the real-money test exists for: execution versus
//! the Sentinel ledger. The ticker price the app saw when it decided to close
//! is not an exit; the exchange's fills are. It also covers the exits the app
//! never sent — a stop or take-profit that fired on Binance while the app was
//! closed has fills here and nowhere else.

use serde::Deserialize;

use super::binance_requests::Side;

#[derive(Debug, Clone, PartialEq)]
pub struct UserTrade {
    pub id: i64,
    pub order_id: i64,
    pub side: Side,
    pub price: f64,
    pub qty: f64,
    pub commission: f64,
    pub commission_asset: String,
    pub time: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTrade {
    id: i64,
    order_id: i64,
    side: String,
    price: String,
    qty: String,
    commission: String,
    commission_asset: String,
    time: u64,
}

/// Parses the trade list. One malformed row fails the whole parse: a
/// silently dropped fill would misstate the exit price or the fees.
pub fn parse_user_trades(body: &str) -> Option<Vec<UserTrade>> {
    let raw: Vec<RawTrade> = serde_json::from_str(body).ok()?;
    raw.into_iter()
        .map(|r| {
            Some(UserTrade {
                id: r.id,
                order_id: r.order_id,
                side: match r.side.as_str() {
                    "BUY" => Side::Buy,
                    "SELL" => Side::Sell,
                    _ => return None,
                },
                price: r.price.parse().ok()?,
                qty: r.qty.parse().ok()?,
                commission: r.commission.parse().ok()?,
                commission_asset: r.commission_asset,
                time: r.time,
            })
        })
        .collect()
}

/// One position's real round trip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillSummary {
    /// Volume-weighted entry price and total entered quantity.
    pub entry_price: f64,
    pub entry_qty: f64,
    /// Volume-weighted exit across EVERY closing fill (partial legs included),
    /// so a partial-then-TP trade is blended exactly as it happened.
    pub exit_price: f64,
    pub exit_qty: f64,
    /// Total commission in USDT, entry and exits. None when any fill paid
    /// its fee in another asset (BNB discount): an understated fee is worse
    /// than an unknown one.
    pub commission_usdt: Option<f64>,
}

/// Folds the symbol's trades into one position's round trip.
///
/// The window starts at the entry order's first fill (when its id is known
/// and present), which drops a previous position's closing fills. Opening
/// fills count until the first closing fill; closing fills count until the
/// position is flat. Anything after that belongs to a later position. None
/// when there is no entry or no exit yet.
pub fn summarize_fills(
    trades: &[UserTrade],
    long: bool,
    entry_order_id: Option<i64>,
) -> Option<FillSummary> {
    let mut sorted: Vec<&UserTrade> = trades.iter().collect();
    sorted.sort_by_key(|t| (t.time, t.id));
    let (open, close) = (Side::opening(long), Side::closing(long));
    let start = entry_order_id
        .and_then(|id| {
            sorted
                .iter()
                .filter(|t| t.order_id == id)
                .map(|t| t.time)
                .min()
        })
        .or_else(|| sorted.iter().find(|t| t.side == open).map(|t| t.time))?;

    let (mut in_qty, mut in_notional, mut out_qty, mut out_notional) = (0.0, 0.0, 0.0, 0.0);
    let (mut fees, mut fees_known) = (0.0, true);
    for t in sorted.into_iter().filter(|t| t.time >= start) {
        if t.side == open {
            if out_qty > 0.0 {
                break; // re-entry after exits: a different position.
            }
            in_qty += t.qty;
            in_notional += t.price * t.qty;
        } else if t.side == close && in_qty > 0.0 {
            out_qty += t.qty;
            out_notional += t.price * t.qty;
        } else {
            continue;
        }
        fees += t.commission;
        fees_known &= t.commission_asset == "USDT";
        // Flat (within f64 noise of the step): the round trip is complete.
        if out_qty >= in_qty - 1e-9 {
            break;
        }
    }
    (in_qty > 0.0 && out_qty > 0.0).then(|| FillSummary {
        entry_price: in_notional / in_qty,
        entry_qty: in_qty,
        exit_price: out_notional / out_qty,
        exit_qty: out_qty,
        commission_usdt: fees_known.then_some(fees),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shaped like Binance's documented userTrades response (not captured
    // from an account): a SHORT entered in two fills, a partial buy-back,
    // the take-profit fill, plus a previous position's close before it and a
    // new position's entry after it — both must be excluded.
    const BODY: &str = include_str!("../../../tests/fixtures/binance_user_trades_short_2legs.json");

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn short_round_trip_vwap_and_commission() {
        let trades = parse_user_trades(BODY).expect("fixture parses");
        assert_eq!(trades.len(), 6);
        let s = summarize_fills(&trades, false, Some(1000)).expect("round trip");
        assert!(
            close(s.entry_price, 150.2) && close(s.entry_qty, 10.0),
            "{s:?}"
        );
        assert!(
            close(s.exit_price, 147.0) && close(s.exit_qty, 10.0),
            "{s:?}"
        );
        assert!(close(
            s.commission_usdt.unwrap(),
            0.36 + 0.2408 + 0.296 + 0.292
        ));
    }

    #[test]
    fn unknown_entry_id_falls_back_to_first_opening_fill() {
        let trades = parse_user_trades(BODY).unwrap();
        let s = summarize_fills(&trades, false, None).unwrap();
        assert!(close(s.entry_price, 150.2) && close(s.exit_price, 147.0));
    }

    #[test]
    fn non_usdt_fee_is_unknown_not_understated() {
        let mut trades = parse_user_trades(BODY).unwrap();
        trades[4].commission_asset = "BNB".into();
        let s = summarize_fills(&trades, false, Some(1000)).unwrap();
        assert_eq!(s.commission_usdt, None);
    }

    #[test]
    fn open_position_has_no_summary() {
        let trades = parse_user_trades(BODY).unwrap();
        let entry_only: Vec<_> = trades.into_iter().filter(|t| t.order_id == 1000).collect();
        assert_eq!(summarize_fills(&entry_only, false, Some(1000)), None);
        assert_eq!(
            parse_user_trades(r#"[{"id":1}]"#),
            None,
            "malformed row fails whole parse"
        );
    }
}
