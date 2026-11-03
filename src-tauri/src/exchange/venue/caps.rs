//! What each order venue can do, as data. The engine, the vault's key check,
//! the test-network harness and the UI all read this one table, so a venue
//! never looks more capable in one place than it is in another.
//!
//! A row here says what the CODE does on that venue. It does not say the
//! venue is safe for real money: that is `exchange::DRY_RUN_PASSED`, filled
//! only after the venue's own test-network dry run passed (a release
//! decision). Every row except Binance is "not yet verified on testnet".
//!
//! 15 venues (owner request, 22 Oct 2026): the 8 order venues in `VENUES`
//! and the 7 price-data / paper venues in `PAPER_VENUES` (owner choice B:
//! only venues that can reach live after a testnet dry run keep an order
//! module). Hyperliquid moved to `PAPER_VENUES` in 0.3.22 (owner decision,
//! 1 Nov 2026), see there. Two of the requested names were swapped, see
//! `SUBSTITUTIONS`.

use serde::Serialize;

/// The coin a venue's perpetuals are margined and settled in. Sentinel
/// signals are USDT pairs (`BTCUSDT`); on a USDC or USD venue the same base
/// coin's perpetual is traded, whose price differs by the USDT/USDC (or
/// USDT/USD) basis, usually well under 0.1%.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Margin {
    Usdt,
    Usdc,
    Usd,
}

impl Margin {
    pub fn code(self) -> &'static str {
        match self {
            Margin::Usdt => "USDT",
            Margin::Usdc => "USDC",
            Margin::Usd => "USD",
        }
    }
}

/// How the protective stop closes the position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StopKind {
    /// Binance `closePosition`: the venue closes whatever is open when it
    /// fires, so adds and partial closes need nothing.
    ClosesPosition,
    /// The venue has no position-closing stop the adapter can place on its
    /// own: a reduce-only stop-market for the full size. Every add or reduce
    /// the app makes re-syncs it to the new size (`resync_stops`). A position
    /// changed outside the app is not followed until the next app order.
    ReduceOnlyFullSize,
}

/// How the breakeven move reaches the venue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StopMove {
    /// Binance: a quantity stop beside the old one, then the old cancelled.
    StopBeside,
    /// The stop's level is changed in place (Bybit slot, OKX amend).
    InPlace,
    /// A new stop at the new level is placed and read back, then the old one
    /// is cancelled. There is never a moment without a stop; for a moment
    /// there are two, both reduce-only.
    PlaceThenCancel,
}

/// What the vault stores for a key on this venue. The fields reuse the
/// credential's `api_key` / `api_secret` / `passphrase` slots (all redacted
/// in Debug), so no vault format change is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyFields {
    KeySecret,
    /// OKX, Bitget, KuCoin: the key's passphrase too.
    KeySecretPassphrase,
}

/// How isolated margin is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Isolated {
    /// One call per symbol (`set_margin_mode`).
    SetMode,
    /// Sent with every order (OKX `tdMode`, HTX, WOO X).
    PerOrder,
    /// A leverage choice makes the position isolated (Gate).
    ViaLeverage,
    /// The venue has no per-position isolated margin (Deribit isolates by
    /// sub-account). The isolated step refuses, so a live entry is refused
    /// there even after a dry run.
    NotOffered,
}

/// How a client order id is written for the venue. The engine's ids are
/// `ae` + 32 hex (entries) or short ascii; each venue stores its own form,
/// and reconcile compares against that form (`venue_client_id`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientIdForm {
    /// Letters and digits, at most 32 (OKX's limit; Bybit and most others
    /// accept it unchanged).
    Alnum32,
    /// Gate: `t-` + at most 28 letters and digits.
    GatePrefixed,
    /// WOO X: an integer. 44 bits of a stable hash (well inside its field).
    Numeric44,
}

/// One venue's row.
#[derive(Debug, Clone, Copy)]
pub struct VenueCaps {
    pub id: &'static str,
    /// Display name; the catalog carries the same one (tested).
    #[cfg_attr(not(test), allow(dead_code))]
    pub name: &'static str,
    pub margin: Margin,
    pub stop: StopKind,
    pub stop_move: StopMove,
    /// A take-profit order can be placed (the same mechanism as the stop).
    pub take_profit: bool,
    pub key_fields: KeyFields,
    pub isolated: Isolated,
    /// Hedge (two-sided) mode is read and refused. False: the venue has
    /// one-way positions only.
    pub hedge_mode_exists: bool,
    pub client_id: ClientIdForm,
    /// The test network ccxt reaches with `ALEPH_EDGE_VENUE_SANDBOX=1`, as a
    /// short description; None where the venue has none.
    pub testnet: Option<&'static str>,
}

const fn row(id: &'static str, name: &'static str) -> VenueCaps {
    VenueCaps {
        id,
        name,
        margin: Margin::Usdt,
        stop: StopKind::ReduceOnlyFullSize,
        stop_move: StopMove::PlaceThenCancel,
        take_profit: true,
        key_fields: KeyFields::KeySecret,
        isolated: Isolated::SetMode,
        hedge_mode_exists: true,
        client_id: ClientIdForm::Alnum32,
        testnet: None,
    }
}

/// Every order venue, Binance first. Order = display order.
pub const VENUES: [VenueCaps; 8] = [
    VenueCaps {
        stop: StopKind::ClosesPosition,
        stop_move: StopMove::StopBeside,
        testnet: Some("Binance Futures testnet (testnet.binancefuture.com)"),
        ..row("binance", "Binance")
    },
    VenueCaps {
        stop: StopKind::ClosesPosition,
        stop_move: StopMove::InPlace,
        testnet: Some("Bybit testnet (api-testnet.bybit.com)"),
        ..row("bybit", "Bybit")
    },
    VenueCaps {
        stop: StopKind::ClosesPosition,
        stop_move: StopMove::InPlace,
        key_fields: KeyFields::KeySecretPassphrase,
        isolated: Isolated::PerOrder,
        testnet: Some("OKX demo trading (same host, x-simulated-trading: 1)"),
        ..row("okx", "OKX")
    },
    VenueCaps {
        // ccxt places Bitget's stopLossPrice as planType=pos_loss: the whole
        // position, no size.
        stop: StopKind::ClosesPosition,
        key_fields: KeyFields::KeySecretPassphrase,
        testnet: Some("Bitget demo trading (demo API key, paptrading: 1)"),
        ..row("bitget", "Bitget")
    },
    VenueCaps {
        isolated: Isolated::ViaLeverage,
        client_id: ClientIdForm::GatePrefixed,
        testnet: Some("Gate testnet (api-testnet.gateapi.io, testnet API key)"),
        ..row("gate", "Gate")
    },
    VenueCaps {
        // closePosition=true: ccxt sends no quantity.
        stop: StopKind::ClosesPosition,
        testnet: Some("BingX VST demo (open-api-vst.bingx.com)"),
        ..row("bingx", "BingX")
    },
    VenueCaps {
        isolated: Isolated::PerOrder,
        client_id: ClientIdForm::Numeric44,
        testnet: Some("WOO X staging (api.staging.woox.io)"),
        ..row("woo", "WOO X")
    },
    VenueCaps {
        margin: Margin::Usdc,
        isolated: Isolated::NotOffered,
        hedge_mode_exists: false,
        testnet: Some("Deribit testnet (test.deribit.com)"),
        ..row("deribit", "Deribit")
    },
];

/// The requested venues kept as price data and paper trading only: no order
/// module, no key. Their prices come from the plain REST tickers in
/// `providers::CATALOG`. Why each has no order path:
/// - KuCoin Futures, HTX, Bitfinex: no test network, so no dry run, so
///   never live;
/// - Phemex, CoinEx: no API answer shows that a key is trade-only;
/// - Kraken Futures: that answer is outside ccxt, and its perpetuals are
///   USD-margined;
/// - Hyperliquid (0.3.22, owner decision 1 Nov 2026): its orders sign with
///   secp256k1, whose crate in ccxt (k256 0.13 on crypto-bigint 0.5.5) has
///   a timing advisory with no fix (RUSTSEC-2026-0387). Its price is the
///   public `info` endpoint, nothing signed.
pub const PAPER_VENUES: [(&str, &str, Margin); 7] = [
    ("kucoinfutures", "KuCoin Futures", Margin::Usdt),
    ("htx", "HTX", Margin::Usdt),
    ("phemex", "Phemex", Margin::Usdt),
    ("coinex", "CoinEx", Margin::Usdt),
    ("krakenfutures", "Kraken Futures", Margin::Usd),
    ("bitfinex", "Bitfinex", Margin::Usdt),
    ("hyperliquid", "Hyperliquid", Margin::Usdc),
];

pub fn paper_venue(id: &str) -> Option<(&'static str, &'static str, Margin)> {
    PAPER_VENUES.iter().copied().find(|v| v.0 == id)
}

/// Requested venues that were swapped, and why (README, report).
#[cfg_attr(not(test), allow(dead_code))]
pub const SUBSTITUTIONS: [(&str, &str, &str); 2] = [
    (
        "mexc",
        "deribit",
        "MEXC opens futures order placement by API only after KYB review (institutions); retail keys cannot place futures orders",
    ),
    ("bitmart", "bitfinex", "BitMart is not in the ccxt 4.5.85 Rust crate (no feature flag to enable)"),
];

pub fn caps(id: &str) -> Option<&'static VenueCaps> {
    VENUES.iter().find(|v| v.id == id)
}

/// `BTCUSDT` → the venue's unified perpetual symbol (`BTC/USDT:USDT`,
/// `BTC/USDC:USDC`, `BTC/USD:USD`). None for anything not a USDT pair.
pub fn unified_symbol_for(margin: Margin, id: &str) -> Option<String> {
    let base = id.strip_suffix("USDT")?;
    let m = margin.code();
    (!base.is_empty()).then(|| format!("{base}/{m}:{m}"))
}

/// FNV-1a 64 of `s` (stable across runs and builds).
fn fnv64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// The client id `id` is stored under on a venue of this form. Stable, and
/// idempotent (`f(f(x)) == f(x)`): ccxt or reconcile may map an id twice.
pub fn client_id_for(form: ClientIdForm, id: &str) -> String {
    let alnum = |n: usize| -> String { id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { 'x' }).take(n).collect() };
    match form {
        ClientIdForm::Alnum32 => alnum(32),
        ClientIdForm::GatePrefixed => {
            if let Some(rest) = id.strip_prefix("t-") {
                return format!("t-{}", rest.chars().filter(char::is_ascii_alphanumeric).take(28).collect::<String>());
            }
            format!("t-{}", alnum(28))
        }
        ClientIdForm::Numeric44 => {
            if !id.is_empty() && id.len() <= 14 && id.chars().all(|c| c.is_ascii_digit()) {
                return id.to_string();
            }
            ((fnv64(id) >> 20) % (1u64 << 44)).max(1).to_string()
        }
    }
}

/// Where a venue stands for real money, for the UI and README.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LiveState {
    /// The order path passed the venue's test-network dry run.
    Verified,
    /// Order path and key check exist; the dry run has not run yet. Paper,
    /// account read and closing work; live entries are refused.
    NotVerified,
    /// No order path (`PAPER_VENUES`): price data and paper trading only.
    PaperOnly,
}

/// What an order venue can do, for the UI (None on a paper venue).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderView {
    pub key_fields: KeyFields,
    pub stop: StopKind,
    pub stop_move: StopMove,
    pub isolated: Isolated,
    pub testnet: bool,
}

/// The secret-free venue view sent to the UI with the catalog.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VenueView {
    pub margin: &'static str,
    pub live: LiveState,
    pub orders: Option<OrderView>,
}

pub fn live_state(c: &VenueCaps, dry_run_passed: &[&str]) -> LiveState {
    if dry_run_passed.contains(&c.id) {
        LiveState::Verified
    } else {
        LiveState::NotVerified
    }
}

/// The view of venue `id`: an order venue, a paper venue, or None for an
/// exchange outside the 15 (MEXC, the regional spot exchanges).
pub fn view_of(id: &str, dry_run_passed: &[&str]) -> Option<VenueView> {
    if let Some(c) = caps(id) {
        return Some(VenueView {
            margin: c.margin.code(),
            live: live_state(c, dry_run_passed),
            orders: Some(OrderView { key_fields: c.key_fields, stop: c.stop, stop_move: c.stop_move, isolated: c.isolated, testnet: c.testnet.is_some() }),
        });
    }
    paper_venue(id).map(|(_, _, margin)| VenueView { margin: margin.code(), live: LiveState::PaperOnly, orders: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifteen_venues_eight_with_an_order_path() {
        assert_eq!(VENUES.len(), 8);
        assert_eq!(VENUES[0].id, "binance");
        let mut ids: Vec<_> = VENUES.iter().map(|v| v.id).chain(PAPER_VENUES.iter().map(|v| v.0)).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 15, "no venue is both");
        for (from, to, why) in SUBSTITUTIONS {
            assert!(caps(from).is_none() && paper_venue(from).is_none(), "{from} was swapped out");
            assert!(caps(to).is_some() || paper_venue(to).is_some(), "{to} is in");
            assert!(!why.is_empty());
        }
        // Every order venue can reach live: it has a test network for its dry run.
        for v in VENUES {
            assert!(v.testnet.is_some(), "{}", v.id);
        }
    }

    #[test]
    fn only_binance_reads_verified_today() {
        let passed = crate::exchange::DRY_RUN_PASSED;
        for v in VENUES {
            let want = if v.id == "binance" { LiveState::Verified } else { LiveState::NotVerified };
            assert_eq!(view_of(v.id, &passed).unwrap().live, want, "{}", v.id);
            assert!(view_of(v.id, &passed).unwrap().orders.is_some());
        }
        for (id, _, _) in PAPER_VENUES {
            let p = view_of(id, &passed).unwrap();
            assert_eq!((p.live, p.orders.is_none()), (LiveState::PaperOnly, true), "{id}");
        }
        assert!(view_of("mexc", &passed).is_none());
        assert_eq!(view_of("krakenfutures", &passed).unwrap().margin, "USD");
        // 0.3.22: Hyperliquid is price data / paper only (RUSTSEC-2026-0387).
        assert!(caps("hyperliquid").is_none());
        let hl = view_of("hyperliquid", &passed).unwrap();
        assert_eq!((hl.live, hl.margin, hl.orders.is_none()), (LiveState::PaperOnly, "USDC", true));
    }

    #[test]
    fn symbols_follow_the_margin_coin() {
        assert_eq!(unified_symbol_for(Margin::Usdt, "BTCUSDT").as_deref(), Some("BTC/USDT:USDT"));
        assert_eq!(unified_symbol_for(Margin::Usdc, "ETHUSDT").as_deref(), Some("ETH/USDC:USDC"));
        assert_eq!(unified_symbol_for(Margin::Usd, "BTCUSDT").as_deref(), Some("BTC/USD:USD"));
        assert_eq!(unified_symbol_for(Margin::Usdc, "BTCUSDC"), None);
        assert_eq!(unified_symbol_for(Margin::Usdt, "USDT"), None);
    }

    #[test]
    fn client_ids_fit_each_form_and_are_idempotent() {
        let entry = "ae5ded71932335436cba179bd2b5f8e87e";
        let ids = [entry, "aa1759700000123", "astest71759700000", "as12345678-3-2", "ap17597000001230"];
        for form in [ClientIdForm::Alnum32, ClientIdForm::GatePrefixed, ClientIdForm::Numeric44] {
            for id in ids {
                let once = client_id_for(form, id);
                assert_eq!(client_id_for(form, &once), once, "{form:?} {id}");
                match form {
                    ClientIdForm::Alnum32 => assert!(once.len() <= 32 && once.chars().all(|c| c.is_ascii_alphanumeric())),
                    ClientIdForm::GatePrefixed => assert!(once.starts_with("t-") && once.len() <= 30),
                    ClientIdForm::Numeric44 => {
                        let n: u64 = once.parse().unwrap();
                        assert!(n > 0 && n < (1 << 44));
                    }
                }
            }
        }
        // Different ids stay different.
        assert_ne!(client_id_for(ClientIdForm::Numeric44, "aa1"), client_id_for(ClientIdForm::Numeric44, "aa2"));
    }
}
