//! Foreign-trade check: fills on the user's Binance futures account that
//! this app did not make.
//!
//! The attack it is for: a trade-only API key leaks (the 3Commas pattern)
//! and someone wash-trades an illiquid coin against the user's balance. The
//! key cannot withdraw, so the money leaves through bad fills. The only
//! defence on this side is to notice fast and stop trading.
//!
//! How a fill is judged, per commission row (`GET /fapi/v1/income`,
//! `incomeType=COMMISSION`: one row per fill, carrying `symbol`, `tradeId`,
//! `time`):
//! 1. The fill's order: `GET /fapi/v1/userTrades?fromId=<tradeId>&limit=1`.
//! 2. Ours when that order id is one this app placed (`own_orders`), or
//!    the order one of its conditional stops / take-profits became.
//! 3. Else the order's client id (`GET /fapi/v1/order`): this app's entry
//!    and mirror shapes are ours (an entry whose answer was lost); Binance's
//!    own forced closes (liquidation, ADL, settlement) are not foreign.
//! 4. Else each of the app's unsettled conditional orders on the symbol is
//!    asked what it became (`GET /fapi/v1/algoOrder` → `actualOrderId`).
//! 5. Still unmatched: FOREIGN.
//!
//! Read-only: nothing here places, moves or cancels an order. Every request
//! goes to the base passed in, so tests run against a local fake.

use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};

use super::model::BinanceKeyCheckError as E;
use super::own_orders::OwnOrders;
use super::providers::binance_fills::parse_user_trades;
use super::providers::binance_http::{now_ms, signed_at};
use super::providers::binance_income as inc;
use super::providers::binance_requests::{algo_id_query, Side, ALGO_ORDER_PATH, ORDER_PATH, USER_TRADES_PATH};

/// Income rows can be written a little after the fill they describe; each
/// scan re-reads this far before the cursor (already judged rows are skipped).
pub const OVERLAP_MS: u64 = 10 * 60_000;
/// Binance keeps income history for three months; a cursor older than this
/// (the app was closed for months) restarts there.
pub const MAX_LOOKBACK_MS: u64 = 89 * 24 * 3_600_000;
/// Pages per scan (1000 rows each); the rest waits for the next scan.
const MAX_PAGES: usize = 10;

/// What the check remembers between scans (persisted).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScanCursor {
    /// Fills up to here are judged. None = never scanned: the first scan
    /// only sets it to now, so switching on raises no alarm for old trades.
    pub cursor_ms: Option<u64>,
    /// `symbol:tradeId` of rows judged inside the overlap window.
    pub seen: Vec<(String, u64)>,
}

/// A fill this app did not make.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignFill {
    pub symbol: String,
    pub trade_id: i64,
    /// 0 when Binance did not return the trade itself.
    pub order_id: i64,
    /// "BUY" / "SELL"; empty when unknown.
    pub side: String,
    pub qty: f64,
    pub price: f64,
    /// When the fill happened (Binance time, ms).
    pub time_ms: u64,
    /// When this app noticed it.
    pub detected_ms: u64,
}

/// One scan's result. `error` = the scan stopped at a row it could not
/// judge (network, rate limit); that row and the ones after it are read
/// again next time, so nothing is skipped.
#[derive(Debug, Default)]
pub struct ScanResult {
    pub foreign: Vec<ForeignFill>,
    pub judged: usize,
    pub error: Option<E>,
}

/// The signed Binance reads the check makes.
pub struct Reader<'a> {
    pub client: &'a Client,
    pub base: &'a str,
    pub key: &'a str,
    pub signer: crate::exchange::BinanceSigner<'a>,
}

impl Reader<'_> {
    async fn get(&self, path: &str, query: &str) -> Result<String, E> {
        signed_at(self.client, self.base, Method::GET, path, query, self.key, self.signer).await
    }
}

enum Verdict {
    Ours,
    /// Binance's own forced close: not the app's, not an intruder's.
    Exchange,
    Foreign(ForeignFill),
}

/// Judges every new commission row since the cursor. `extra_algos` are
/// conditional orders the book still holds (symbol, algo id), so a stop
/// placed before this check existed still counts as ours.
pub async fn scan(
    r: &Reader<'_>,
    cur: &mut ScanCursor,
    own: &OwnOrders,
    extra_algos: &[(String, i64)],
    now: u64,
) -> ScanResult {
    let mut out = ScanResult::default();
    let Some(cursor) = cur.cursor_ms else {
        cur.cursor_ms = Some(now);
        cur.seen.clear();
        return out;
    };
    let start = cursor.saturating_sub(OVERLAP_MS).max(now.saturating_sub(MAX_LOOKBACK_MS));
    let rows = match read_commissions(r, start, now).await {
        Ok(rows) => rows,
        Err(e) => {
            out.error = Some(e);
            return out;
        }
    };
    let (rows, complete) = rows;
    let mut reached = cursor;
    for row in rows {
        let key = format!("{}:{}", row.symbol, row.trade_id);
        if cur.seen.iter().any(|(k, _)| *k == key) {
            continue;
        }
        match judge(r, own, extra_algos, &row, now).await {
            Ok(v) => {
                if let Verdict::Foreign(f) = v {
                    out.foreign.push(f);
                }
                out.judged += 1;
                cur.seen.push((key, row.time));
                reached = reached.max(row.time);
            }
            Err(e) => {
                out.error = Some(e);
                break;
            }
        }
    }
    // A clean, complete read judged everything up to now; otherwise only up
    // to the last judged row.
    let next = if out.error.is_none() && complete { now.max(reached) } else { reached };
    cur.cursor_ms = Some(next);
    let floor = next.saturating_sub(OVERLAP_MS + 60_000);
    cur.seen.retain(|(_, t)| *t >= floor);
    out
}

/// Commission rows in [start, end], oldest first, paged. The flag says the
/// read reached `end` (false = more pages wait for the next scan).
async fn read_commissions(r: &Reader<'_>, start: u64, end: u64) -> Result<(Vec<inc::CommissionRow>, bool), E> {
    let mut all: Vec<inc::CommissionRow> = Vec::new();
    let mut from = start;
    for _ in 0..MAX_PAGES {
        let body = r.get(inc::INCOME_PATH, &inc::commission_query(from, end, now_ms())).await?;
        let page = inc::parse_commissions(&body).ok_or(E::Unknown)?;
        let full = page.len() >= inc::INCOME_PAGE;
        let last = page.last().map(|p| p.time);
        for row in page {
            if !all.iter().any(|a| a.symbol == row.symbol && a.trade_id == row.trade_id) {
                all.push(row);
            }
        }
        match (full, last) {
            // A full page ends at the newest row it holds; the next page
            // starts there (the shared millisecond is de-duplicated).
            (true, Some(t)) if t > from => from = t,
            _ => return Ok((all, true)),
        }
    }
    Ok((all, false))
}

async fn judge(
    r: &Reader<'_>,
    own: &OwnOrders,
    extra_algos: &[(String, i64)],
    row: &inc::CommissionRow,
    now: u64,
) -> Result<Verdict, E> {
    let symbol = row.symbol.as_str();
    // 1. The order behind the fill.
    let body = r.get(USER_TRADES_PATH, &inc::trade_by_id_query(symbol, row.trade_id, now_ms())).await?;
    let trade = parse_user_trades(&body)
        .ok_or(E::Unknown)?
        .into_iter()
        .find(|t| t.id == row.trade_id);
    let Some(trade) = trade else {
        // The commission proves a fill happened; with no order to compare,
        // it is not provably ours.
        return Ok(Verdict::Foreign(ForeignFill {
            symbol: symbol.to_string(),
            trade_id: row.trade_id,
            order_id: 0,
            side: String::new(),
            qty: 0.0,
            price: 0.0,
            time_ms: row.time,
            detected_ms: now,
        }));
    };
    // 2. An order this app placed, or one its stops became.
    if own.snapshot().owns_order(symbol, trade.order_id) {
        return Ok(Verdict::Ours);
    }
    // 3. The client id: an entry whose answer was lost, or Binance's own close.
    match r.get(ORDER_PATH, &inc::order_by_id_query(symbol, trade.order_id, now_ms())).await {
        Ok(body) => match inc::parse_client_order_id(&body).map(|c| inc::client_id_owner(&c)) {
            Some(inc::ClientIdOwner::App) => {
                own.record_order(symbol, trade.order_id, now);
                return Ok(Verdict::Ours);
            }
            Some(inc::ClientIdOwner::Exchange) => return Ok(Verdict::Exchange),
            _ => {}
        },
        // -2013: Binance no longer finds the order; the algo check decides.
        Err(E::Rejected { code: -2013, .. }) => {}
        Err(e) => return Err(e),
    }
    // 4. What the app's unsettled conditional orders on the symbol became.
    let mut algos = own.snapshot().open_algos(symbol);
    for (s, id) in extra_algos {
        if s == symbol && !algos.contains(id) && !own.snapshot().orders.iter().any(|o| o.id == *id && o.done) {
            algos.push(*id);
        }
    }
    for algo_id in algos {
        let body = match r.get(ALGO_ORDER_PATH, &algo_id_query(algo_id, now_ms())).await {
            Ok(b) => b,
            // Gone from Binance's history: it can no longer be asked.
            Err(E::Rejected { code: -2013, .. }) => {
                own.settle_algo(symbol, algo_id, None, now);
                continue;
            }
            Err(e) => return Err(e),
        };
        let Some(outcome) = inc::parse_algo_outcome(&body) else { continue };
        if let Some(actual) = outcome.actual_order_id {
            own.settle_algo(symbol, algo_id, Some(actual), now);
            if actual == trade.order_id {
                return Ok(Verdict::Ours);
            }
        } else if inc::algo_ended_untriggered(&outcome) {
            own.settle_algo(symbol, algo_id, None, now);
        }
    }
    // 5. Nobody here made it.
    Ok(Verdict::Foreign(ForeignFill {
        symbol: symbol.to_string(),
        trade_id: row.trade_id,
        order_id: trade.order_id,
        side: match trade.side {
            Side::Buy => "BUY".into(),
            Side::Sell => "SELL".into(),
        },
        qty: trade.qty,
        price: trade.price,
        time_ms: row.time,
        detected_ms: now,
    }))
}

#[cfg(test)]
mod tests {
    //! Against a scripted HTTP server on 127.0.0.1: nothing reaches Binance,
    //! and the check only ever sends GETs.
    use super::*;
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// path -> (status, body), matched on the path plus the query keys that
    /// pick the row (fromId, orderId, algoId). Every request is logged.
    struct Fake {
        routes: HashMap<String, (u16, String)>,
        log: Mutex<Vec<(String, String)>>,
    }

    fn route_key(path_q: &str) -> String {
        let (path, q) = path_q.split_once('?').unwrap_or((path_q, ""));
        let pick: Vec<&str> = q
            .split('&')
            .filter(|kv| ["fromId=", "orderId=", "algoId=", "symbol="].iter().any(|p| kv.starts_with(p)))
            .collect();
        if pick.is_empty() {
            path.to_string()
        } else {
            format!("{path}?{}", pick.join("&"))
        }
    }

    fn serve(fake: Arc<Fake>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let fake = fake.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() {
                        return;
                    }
                    let mut parts = line.split_whitespace();
                    let method = parts.next().unwrap_or_default().to_string();
                    let path = parts.next().unwrap_or_default().to_string();
                    loop {
                        let mut h = String::new();
                        if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                            break;
                        }
                    }
                    fake.log.lock().unwrap().push((method, path.clone()));
                    let (status, out) = if path.starts_with("/fapi/v1/time") {
                        (200, format!(r#"{{"serverTime":{}}}"#, super::super::providers::binance_http::now_ms()))
                    } else {
                        fake.routes
                            .get(&route_key(&path))
                            .cloned()
                            .unwrap_or((400, r#"{"code":-2013,"msg":"Order does not exist."}"#.into()))
                    };
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                        out.len()
                    );
                    let _ = stream.write_all(resp.as_bytes());
                });
            }
        });
        format!("http://{addr}")
    }

    fn run<T>(f: impl std::future::Future<Output = T>) -> T {
        tauri::async_runtime::block_on(f)
    }

    fn trade(id: i64, order_id: i64, side: &str) -> String {
        format!(
            r#"[{{"id":{id},"orderId":{order_id},"side":"{side}","price":"0.0123","qty":"50000","commission":"0.3","commissionAsset":"USDT","time":1000,"symbol":"XUSDT","buyer":true,"maker":false,"realizedPnl":"0"}}]"#
        )
    }

    fn commission(symbol: &str, trade_id: i64, time: u64) -> String {
        format!(r#"{{"symbol":"{symbol}","incomeType":"COMMISSION","income":"-0.3","asset":"USDT","info":"","time":{time},"tranId":1,"tradeId":"{trade_id}"}}"#)
    }

    #[test]
    fn the_first_scan_only_sets_the_cursor() {
        let fake = Arc::new(Fake { routes: HashMap::new(), log: Mutex::new(vec![]) });
        let base = serve(fake.clone());
        let client = Client::new();
        let r = Reader { client: &client, base: &base, key: "k", signer: crate::exchange::BinanceSigner::Hmac("s") };
        let mut cur = ScanCursor::default();
        let res = run(scan(&r, &mut cur, &OwnOrders::default(), &[], 5_000_000));
        assert!(res.foreign.is_empty() && res.error.is_none());
        assert_eq!(cur.cursor_ms, Some(5_000_000));
        assert!(fake.log.lock().unwrap().is_empty(), "no backlog read on the first run");
    }

    #[test]
    fn own_orders_stops_and_entries_pass_and_an_intruders_fill_is_foreign() {
        let now = 10_000_000u64;
        let own = OwnOrders::default();
        own.record_order("ETHUSDT", 501, now - 5_000); // a close the app sent
        own.record_algo("ETHUSDT", 9_001, now - 9_000); // the app's stop
        let mut routes = HashMap::new();
        routes.insert(
            "/fapi/v1/income".to_string(),
            (
                200,
                format!(
                    "[{},{},{},{},{}]",
                    commission("ETHUSDT", 1, now - 4_000),
                    commission("ETHUSDT", 2, now - 3_000),
                    commission("SOLUSDT", 3, now - 2_000),
                    commission("XUSDT", 4, now - 1_000),
                    commission("BTCUSDT", 5, now - 500)
                ),
            ),
        );
        // 1: the app's own close.
        routes.insert("/fapi/v1/userTrades?symbol=ETHUSDT&fromId=1".into(), (200, trade(1, 501, "SELL")));
        // 2: the app's stop fired and became order 777.
        routes.insert("/fapi/v1/userTrades?symbol=ETHUSDT&fromId=2".into(), (200, trade(2, 777, "SELL")));
        routes.insert("/fapi/v1/order?symbol=ETHUSDT&orderId=777".into(), (200, r#"{"orderId":777,"clientOrderId":"x8Kd9a"}"#.into()));
        routes.insert("/fapi/v1/algoOrder?algoId=9001".into(), (200, r#"{"algoId":9001,"algoStatus":"FINISHED","actualOrderId":"777"}"#.into()));
        // 3: an entry whose answer was lost: known only by its client id.
        routes.insert("/fapi/v1/userTrades?symbol=SOLUSDT&fromId=3".into(), (200, trade(3, 600, "BUY")));
        routes.insert(
            "/fapi/v1/order?symbol=SOLUSDT&orderId=600".into(),
            (200, r#"{"orderId":600,"clientOrderId":"aec3393031884949a7b006a3d50e2d3566"}"#.into()),
        );
        // 4: someone else's wash trade in an illiquid coin.
        routes.insert("/fapi/v1/userTrades?symbol=XUSDT&fromId=4".into(), (200, trade(4, 31337, "BUY")));
        routes.insert("/fapi/v1/order?symbol=XUSDT&orderId=31337".into(), (200, r#"{"orderId":31337,"clientOrderId":"web_Zq81"}"#.into()));
        // 5: Binance liquidated a position: not the app's, not foreign.
        routes.insert("/fapi/v1/userTrades?symbol=BTCUSDT&fromId=5".into(), (200, trade(5, 42, "SELL")));
        routes.insert("/fapi/v1/order?symbol=BTCUSDT&orderId=42".into(), (200, r#"{"orderId":42,"clientOrderId":"autoclose-1792000000"}"#.into()));
        let fake = Arc::new(Fake { routes, log: Mutex::new(vec![]) });
        let base = serve(fake.clone());
        let client = Client::new();
        let r = Reader { client: &client, base: &base, key: "k", signer: crate::exchange::BinanceSigner::Hmac("s") };
        let mut cur = ScanCursor { cursor_ms: Some(now - 60_000), seen: vec![] };

        let res = run(scan(&r, &mut cur, &own, &[], now));
        assert!(res.error.is_none(), "{:?}", res.error);
        assert_eq!(res.judged, 5);
        assert_eq!(res.foreign.len(), 1, "{:?}", res.foreign);
        let f = &res.foreign[0];
        assert_eq!((f.symbol.as_str(), f.trade_id, f.order_id, f.side.as_str()), ("XUSDT", 4, 31337, "BUY"));
        assert_eq!((f.qty, f.price, f.detected_ms), (50_000.0, 0.0123, now));
        assert_eq!(cur.cursor_ms, Some(now));
        // The stop and the lost entry are remembered: no second lookup.
        let book = own.snapshot();
        assert!(book.owns_order("ETHUSDT", 777) && book.owns_order("SOLUSDT", 600));
        assert!(fake.log.lock().unwrap().iter().all(|(m, _)| m == "GET"), "read-only");

        // The next scan re-reads the overlap and judges nothing twice.
        let before = fake.log.lock().unwrap().len();
        let res = run(scan(&r, &mut cur, &own, &[], now + 60_000));
        assert!(res.foreign.is_empty() && res.judged == 0 && res.error.is_none());
        assert_eq!(fake.log.lock().unwrap().len() - before, 1, "only the income page");
    }

    #[test]
    fn a_stop_from_the_book_counts_and_an_unreadable_row_waits_for_the_next_scan() {
        let now = 20_000_000u64;
        let own = OwnOrders::default();
        let mut routes = HashMap::new();
        routes.insert(
            "/fapi/v1/income".to_string(),
            (200, format!("[{},{}]", commission("ETHUSDT", 7, now - 3_000), commission("ETHUSDT", 8, now - 1_000))),
        );
        // 7: a stop placed by an older version, known only from the book.
        routes.insert("/fapi/v1/userTrades?symbol=ETHUSDT&fromId=7".into(), (200, trade(7, 900, "SELL")));
        routes.insert("/fapi/v1/order?symbol=ETHUSDT&orderId=900".into(), (200, r#"{"orderId":900,"clientOrderId":"q1"}"#.into()));
        routes.insert("/fapi/v1/algoOrder?algoId=55".into(), (200, r#"{"algoId":55,"algoStatus":"FINISHED","actualOrderId":"900"}"#.into()));
        // 8: Binance errors on the lookup (500): judged next time, not skipped.
        routes.insert("/fapi/v1/userTrades?symbol=ETHUSDT&fromId=8".into(), (500, "".into()));
        let fake = Arc::new(Fake { routes, log: Mutex::new(vec![]) });
        let base = serve(fake);
        let client = Client::new();
        let r = Reader { client: &client, base: &base, key: "k", signer: crate::exchange::BinanceSigner::Hmac("s") };
        let mut cur = ScanCursor { cursor_ms: Some(now - 60_000), seen: vec![] };
        let res = run(scan(&r, &mut cur, &own, &[("ETHUSDT".into(), 55)], now));
        assert!(res.foreign.is_empty());
        assert_eq!(res.judged, 1);
        assert!(matches!(res.error, Some(E::Unknown)));
        assert_eq!(cur.cursor_ms, Some(now - 3_000), "stops at the last judged row");
        assert!(own.snapshot().owns_order("ETHUSDT", 900));
    }

    #[test]
    fn an_error_body_from_income_is_an_error_not_an_empty_list() {
        let mut routes = HashMap::new();
        routes.insert("/fapi/v1/income".to_string(), (401, r#"{"code":-2015,"msg":"Invalid API-key"}"#.into()));
        let fake = Arc::new(Fake { routes, log: Mutex::new(vec![]) });
        let base = serve(fake);
        let client = Client::new();
        let r = Reader { client: &client, base: &base, key: "k", signer: crate::exchange::BinanceSigner::Hmac("s") };
        let mut cur = ScanCursor { cursor_ms: Some(1_000), seen: vec![] };
        let res = run(scan(&r, &mut cur, &OwnOrders::default(), &[], 2_000_000));
        assert!(matches!(res.error, Some(E::InvalidCredentials { .. })));
        assert_eq!(cur.cursor_ms, Some(1_000), "the cursor does not move");
    }
}
