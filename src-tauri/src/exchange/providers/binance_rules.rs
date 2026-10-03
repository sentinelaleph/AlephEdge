//! A symbol's trading rules from the public exchangeInfo endpoint: LOT_SIZE
//! (quantity step + minimum) and PRICE_FILTER (tick size).
//!
//! Both matter for a real order. An OPEN or PARTIAL quantity is computed by
//! us, so it must sit on the step or Binance answers 400 — indistinguishable
//! from a bad key. The tick matters for trigger prices: Sentinel levels carry
//! more decimals than most ticks (a 2.5×ATR stop is not a round number), and
//! an over-precise `triggerPrice` is rejected (-1111), which on the stop
//! would flatten every entry the moment it opened.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LotStep {
    pub step: f64,
    pub min_qty: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SymbolRules {
    pub lot: LotStep,
    pub tick: f64,
    /// MARKET_LOT_SIZE maxQty: the largest single MARKET order. None when the
    /// filter is absent. A close above it is sent in parts.
    pub market_max_qty: Option<f64>,
    /// MIN_NOTIONAL `notional` (USDT); 0 when absent. An entry below it is
    /// refused up front instead of being rejected by Binance.
    pub min_notional: f64,
    /// `status == "TRADING"` (absent status reads as trading). A SETTLING,
    /// PENDING_TRADING or delisted symbol refuses new positions for good, so
    /// the entry is judged once instead of retried every tick.
    pub trading: bool,
}

#[derive(Deserialize)]
struct ExchangeInfo {
    symbols: Vec<SymbolInfo>,
}

#[derive(Deserialize)]
struct SymbolInfo {
    symbol: String,
    #[serde(default)]
    status: Option<String>,
    filters: Vec<SymbolFilter>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SymbolFilter {
    filter_type: String,
    step_size: Option<String>,
    min_qty: Option<String>,
    max_qty: Option<String>,
    tick_size: Option<String>,
    notional: Option<String>,
}

fn num(v: &Option<String>) -> f64 {
    v.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0.0)
}

/// Parses one symbol's rules. None for an unknown symbol, a missing filter or
/// a non-positive step/tick — never a guessed value: a guessed step becomes
/// a rejected order or a wrong position size.
pub fn parse_symbol_rules(body: &str, symbol: &str) -> Option<SymbolRules> {
    let info: ExchangeInfo = serde_json::from_str(body).ok()?;
    let sym = info.symbols.iter().find(|s| s.symbol == symbol)?;
    let filter = |t: &str| sym.filters.iter().find(|f| f.filter_type == t);
    let lot = filter("LOT_SIZE")?;
    let price = filter("PRICE_FILTER")?;
    let rules = SymbolRules {
        lot: LotStep {
            step: num(&lot.step_size),
            min_qty: num(&lot.min_qty),
        },
        tick: num(&price.tick_size),
        market_max_qty: filter("MARKET_LOT_SIZE")
            .map(|f| num(&f.max_qty))
            .filter(|q| *q > 0.0),
        min_notional: filter("MIN_NOTIONAL").map_or(0.0, |f| num(&f.notional)),
        trading: sym.status.as_deref().map_or(true, |s| s == "TRADING"),
    };
    (rules.lot.step > 0.0 && rules.tick > 0.0).then_some(rules)
}

/// Floors `qty` to the step. Flooring, never rounding up: rounding up would
/// order more than the capital the user allotted. 0.0 when the result sits
/// below the exchange minimum — "cannot trade", never "trade something tiny".
pub fn quantize_qty(qty: f64, lot: LotStep) -> f64 {
    if qty <= 0.0 || lot.step <= 0.0 {
        return 0.0;
    }
    // The epsilon guards the classic f64 trap: 2.3 / 0.1 evaluates to
    // 22.999999999999996, and a bare floor would knock an EXACT-step quantity
    // down a whole step. 1e-9 only rescues representation noise.
    let steps = (qty / lot.step + 1e-9).floor();
    let q = ((steps * lot.step) * 1e8).round() / 1e8;
    if q < lot.min_qty {
        return 0.0;
    }
    q
}

/// A reduce-only close of `qty` cut into MARKET orders no larger than
/// `market_max` (each on the step). One part when there is no cap.
pub fn close_parts(qty: f64, market_max: Option<f64>, lot: LotStep) -> Vec<f64> {
    let Some(max) = market_max.map(|m| quantize_qty(m, lot)).filter(|m| *m > 0.0) else {
        return vec![qty];
    };
    let mut parts = Vec::new();
    let mut left = qty;
    while left > max && parts.len() < 50 {
        parts.push(max);
        left = ((left - max) * 1e8).round() / 1e8;
    }
    if left > 0.0 {
        parts.push(left);
    }
    parts
}

/// Snaps a trigger price to the nearest tick. Nearest, not floor: a floor
/// would move a long's stop AWAY from entry but a short's TOWARD it — one
/// rule for both directions keeps the error symmetric and under half a tick.
pub fn quantize_price(price: f64, tick: f64) -> f64 {
    if price <= 0.0 || tick <= 0.0 {
        return 0.0;
    }
    (((price / tick).round() * tick) * 1e8).round() / 1e8
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = r#"{"symbols":[{"symbol":"SOLUSDT","filters":[
        {"filterType":"PRICE_FILTER","minPrice":"0.4200","maxPrice":"6857","tickSize":"0.0100"},
        {"filterType":"LOT_SIZE","stepSize":"1","minQty":"1","maxQty":"1000000"},
        {"filterType":"MARKET_LOT_SIZE","stepSize":"1","minQty":"1","maxQty":"4000"}]}]}"#;

    #[test]
    fn parses_step_min_and_tick() {
        let r = parse_symbol_rules(INFO, "SOLUSDT").unwrap();
        assert_eq!(
            r.lot,
            LotStep {
                step: 1.0,
                min_qty: 1.0
            }
        );
        assert_eq!(r.tick, 0.01);
        assert!(
            parse_symbol_rules(INFO, "ETHUSDT").is_none(),
            "unknown symbol is not guessed"
        );
    }

    #[test]
    fn floors_to_step_never_rounds_up() {
        let lot = LotStep {
            step: 0.1,
            min_qty: 0.1,
        };
        assert_eq!(quantize_qty(2.39, lot), 2.3);
        assert_eq!(quantize_qty(2.30001, lot), 2.3);
        // 2.3/0.1 is 22.999999999999996 in f64; without the guard an EXACT
        // step quantity would floor down a whole step to 2.2.
        assert_eq!(quantize_qty(2.3, lot), 2.3);
    }

    #[test]
    fn below_minimum_is_zero_not_tiny() {
        let lot = LotStep {
            step: 0.001,
            min_qty: 0.01,
        };
        assert_eq!(quantize_qty(0.009, lot), 0.0);
        assert_eq!(quantize_qty(0.0, lot), 0.0);
        assert_eq!(quantize_qty(-1.0, lot), 0.0);
        let whole = LotStep {
            step: 1.0,
            min_qty: 1.0,
        };
        assert_eq!(quantize_qty(17.9, whole), 17.0);
        assert_eq!(quantize_qty(0.9, whole), 0.0);
    }

    #[test]
    fn prices_snap_to_nearest_tick() {
        assert_eq!(quantize_price(142.34567, 0.01), 142.35);
        assert_eq!(quantize_price(142.3412, 0.01), 142.34);
        assert_eq!(quantize_price(0.248713, 0.00001), 0.24871);
        assert_eq!(quantize_price(64_123.47, 0.1), 64_123.5);
        assert_eq!(quantize_price(-1.0, 0.01), 0.0);
    }

    #[test]
    fn reads_the_market_order_cap_and_min_notional() {
        let r = parse_symbol_rules(INFO, "SOLUSDT").unwrap();
        assert_eq!(r.market_max_qty, Some(4000.0));
        assert_eq!(r.min_notional, 0.0, "absent filter");
        let with_min = INFO.replace(
            r#"{"filterType":"MARKET_LOT_SIZE""#,
            r#"{"filterType":"MIN_NOTIONAL","notional":"5"},{"filterType":"MARKET_LOT_SIZE""#,
        );
        assert_eq!(parse_symbol_rules(&with_min, "SOLUSDT").unwrap().min_notional, 5.0);
    }

    // Live, public, unsigned: the exact request `ExchangeManager::symbol_rules`
    // sends, parsed against Binance's real payload. Run by hand:
    // `cargo test --lib real_exchange_info -- --ignored`.
    #[test]
    #[ignore]
    fn real_exchange_info_parses_for_btcusdt() {
        let body = tauri::async_runtime::block_on(async {
            reqwest::get("https://fapi.binance.com/fapi/v1/exchangeInfo?symbol=BTCUSDT")
                .await
                .expect("reachable")
                .text()
                .await
                .expect("body")
        });
        let r = parse_symbol_rules(&body, "BTCUSDT").expect("BTCUSDT rules");
        assert!(r.lot.step > 0.0 && r.lot.min_qty > 0.0 && r.tick > 0.0, "{r:?}");
        assert!(r.min_notional > 0.0, "futures MIN_NOTIONAL.notional read: {r:?}");
        assert!(r.market_max_qty.is_some(), "MARKET_LOT_SIZE read: {r:?}");
    }

    #[test]
    fn a_symbol_not_trading_is_flagged() {
        assert!(parse_symbol_rules(INFO, "SOLUSDT").unwrap().trading, "no status: trading");
        let with = |status: &str| {
            INFO.replace(r#""symbol":"SOLUSDT","#, &format!(r#""symbol":"SOLUSDT","status":"{status}","#))
        };
        assert!(parse_symbol_rules(&with("TRADING"), "SOLUSDT").unwrap().trading);
        for status in ["SETTLING", "PENDING_TRADING", "CLOSE", "DELIVERING"] {
            let r = parse_symbol_rules(&with(status), "SOLUSDT").expect("rules still parse");
            assert!(!r.trading, "{status}");
        }
    }

    #[test]
    fn close_parts_respect_the_cap_and_add_up() {
        let lot = LotStep { step: 1.0, min_qty: 1.0 };
        assert_eq!(close_parts(9500.0, Some(4000.0), lot), vec![4000.0, 4000.0, 1500.0]);
        assert_eq!(close_parts(300.0, Some(4000.0), lot), vec![300.0]);
        assert_eq!(close_parts(300.0, None, lot), vec![300.0]);
    }
}
