//! Live readiness check (`live_preflight`): everything a real-money bot
//! depends on, read now, in one list. Read-only: it signs account reads with
//! the vaulted key and never places, moves or cancels an order.
//!
//! Gathering is async (`live_preflight`); judging is pure (`evaluate`) so
//! every verdict is unit-tested without a network.

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::bot::engine::btc_break::BtcRegime;
use crate::bot::model::LIVE_TRADING_ENABLED;
use crate::bot::strategy_live::{owns_symbol, StrategyLiveManager};
use crate::bot::BotManager;
use crate::exchange::model::{BinanceKeyCheckError as E, FuturesAccount};
use crate::exchange::ExchangeManager;
use crate::membership::model::MembershipHealth;
use crate::membership::MembershipManager;
use crate::signal::SignalManager;
use crate::vault::model::{CredentialPermission, VaultState};
use crate::vault::VaultManager;

/// Clock offsets above this are a warning: signing compensates, but a
/// drifting PC clock also skews every local timestamp.
const CLOCK_WARN_MS: i64 = 1_000;
/// Below this free margin a minimum-size order may already be refused.
const LOW_BALANCE_USDT: f64 = 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Ok,
    Warn,
    Fail,
}

/// One row. `id` names the check (i18n `preflight.checks.<id>`), `code` the
/// reason when it is not plain ok (`preflight.codes.<code>`), `value` a
/// number or list shown as-is.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub id: &'static str,
    pub verdict: Verdict,
    pub code: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preflight {
    pub checked_at_ms: u64,
    /// The exchange whose key and account were read.
    pub venue: String,
    pub checks: Vec<Check>,
    /// Worst verdict across the checks.
    pub overall: Verdict,
}

/// What was read; `None` where the read was skipped or failed upstream.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    /// Exchange the key and account rows read (`binance`, `bybit`, `okx`).
    pub venue: String,
    /// The ccxt venues talk to their test networks (`ALEPH_EDGE_VENUE_SANDBOX=1`).
    pub sandbox: bool,
    pub live_build: bool,
    pub production: bool,
    pub vault: Option<VaultState>,
    pub key: Option<CredentialPermission>,
    pub clock_offset_ms: Option<i64>,
    pub account: Option<Result<FuturesAccount, String>>,
    pub hedge_mode: Option<Result<bool, String>>,
    /// Symbols with a position on the account that no live bot of this app holds.
    pub foreign_positions: Vec<String>,
    pub kill_switch: bool,
    pub btc_regime: Option<BtcRegime>,
    pub membership: Option<MembershipHealth>,
    pub stream_live: bool,
    pub live_signal_bots: usize,
    pub live_strategy_bots: usize,
    pub halted_strategy_bots: usize,
}

fn check(id: &'static str, verdict: Verdict, code: Option<&str>, value: Option<String>) -> Check {
    Check { id, verdict, code: code.map(str::to_string), value }
}

pub fn evaluate(i: &Inputs) -> Vec<Check> {
    use Verdict::{Fail, Warn};
    let ok = Verdict::Ok;
    let mut out = Vec::new();
    out.push(if i.live_build {
        check("build", ok, None, None)
    } else {
        check("build", Fail, Some("buildOff"), None)
    });
    out.push(if i.production {
        check("endpoint", ok, Some("production"), None)
    } else {
        check("endpoint", Warn, Some("testnet"), None)
    });
    // Real money switches on only where the order path passed a test-network
    // dry run; in the sandbox the venue is being dry-run, which only warns.
    out.push(if crate::exchange::DRY_RUN_PASSED.contains(&i.venue.as_str()) {
        check("venue", ok, Some("venueDryRun"), None)
    } else if crate::exchange::live_venue_allowed_in(&i.venue, i.sandbox) {
        check("venue", Warn, Some("venueNotDryRun"), None)
    } else {
        check("venue", Fail, Some("venueNotDryRun"), None)
    });
    out.push(match (i.vault, i.key) {
        (_, Some(CredentialPermission::TradeOnly)) => check("key", ok, None, None),
        (_, Some(_)) => check("key", Fail, Some("keyNotVerified"), None),
        (Some(VaultState::Unlocked), None) => check("key", Fail, Some("noKey"), None),
        (Some(VaultState::Absent), None) => check("key", Fail, Some("noVault"), None),
        _ => check("key", Fail, Some("vaultLocked"), None),
    });
    let ms = |v: i64| Some(format!("{}{v} ms", if v > 0 { "+" } else { "" }));
    out.push(match i.clock_offset_ms {
        None => check("clock", Fail, Some("unreachable"), None),
        Some(o) if o.abs() > CLOCK_WARN_MS => check("clock", Warn, Some("clockDrift"), ms(o)),
        Some(o) => check("clock", ok, None, ms(o)),
    });
    out.push(match &i.account {
        None => check("account", Fail, Some("needsKey"), None),
        Some(Err(code)) => check("account", Fail, Some(code), None),
        Some(Ok(a)) => {
            let value = Some(format!("{:.2} / {:.2} USDT", a.available_balance, a.total_wallet_balance));
            if a.available_balance < LOW_BALANCE_USDT {
                check("account", Warn, Some("lowBalance"), value)
            } else {
                check("account", ok, None, value)
            }
        }
    });
    out.push(match &i.hedge_mode {
        None => check("positionMode", Fail, Some("needsKey"), None),
        Some(Err(code)) => check("positionMode", Fail, Some(code), None),
        Some(Ok(true)) => check("positionMode", Fail, Some("hedgeMode"), None),
        Some(Ok(false)) => check("positionMode", ok, Some("oneWay"), None),
    });
    if i.account.as_ref().is_some_and(|a| a.is_ok()) {
        out.push(if i.foreign_positions.is_empty() {
            check("positions", ok, None, Some("0".into()))
        } else {
            check("positions", Warn, Some("foreignPositions"), Some(i.foreign_positions.join(", ")))
        });
    }
    out.push(if i.kill_switch {
        check("killSwitch", Fail, Some("tripped"), None)
    } else {
        check("killSwitch", ok, None, None)
    });
    out.push(match i.btc_regime {
        Some(BtcRegime::Normal) => check("btcRegime", ok, Some("normal"), None),
        Some(BtcRegime::Break) => check("btcRegime", Warn, Some("break"), None),
        _ => check("btcRegime", Warn, Some("unknown"), None),
    });
    out.push(match i.membership {
        Some(MembershipHealth::Active) => check("membership", ok, None, None),
        Some(MembershipHealth::Expiring) => check("membership", Warn, Some("expiring"), None),
        _ => check("membership", Warn, Some("signedOut"), None),
    });
    out.push(if i.stream_live {
        check("stream", ok, None, None)
    } else {
        check("stream", Warn, Some("streamDown"), None)
    });
    let bots = Some(format!("{} + {}", i.live_signal_bots, i.live_strategy_bots));
    out.push(if i.halted_strategy_bots > 0 {
        check("liveBots", Fail, Some("halted"), Some(i.halted_strategy_bots.to_string()))
    } else {
        check("liveBots", ok, None, bots)
    });
    out
}

fn worst(checks: &[Check]) -> Verdict {
    if checks.iter().any(|c| c.verdict == Verdict::Fail) {
        Verdict::Fail
    } else if checks.iter().any(|c| c.verdict == Verdict::Warn) {
        Verdict::Warn
    } else {
        Verdict::Ok
    }
}

/// Same stable codes as the exchange commands (`errors.*` in the UI).
fn code(e: E) -> String {
    match e {
        E::InvalidCredentials => "binanceKeyRejected".into(),
        E::NetworkUnavailable => "binanceUnreachable".into(),
        E::Rejected { code, .. } => format!("binanceRefused{code}"),
        E::RateLimited => "binanceRateLimited".into(),
        E::Unknown => "binanceUnconfirmed".into(),
    }
}

#[tauri::command]
pub async fn live_preflight(app: AppHandle, venue: Option<String>) -> Result<Preflight, String> {
    let venue = venue.unwrap_or_else(|| "binance".into());
    if !crate::exchange::has_order_path(&venue) {
        return Err("venueUnsupported".into());
    }
    let exchange = app.state::<ExchangeManager>();
    let vault = app.state::<VaultManager>();
    let bots = app.state::<BotManager>();
    let status = bots.status();
    let (live_strategy_bots, halted_strategy_bots) = app.state::<StrategyLiveManager>().counts();
    let sandbox = crate::exchange::venue::ccxt::sandbox_from_env();
    let production = if venue == "binance" {
        crate::app::endpoints::binance_is_production()
    } else {
        !sandbox
    };
    let mut i = Inputs {
        venue: venue.clone(),
        sandbox,
        live_build: LIVE_TRADING_ENABLED,
        production,
        vault: crate::vault::vault_path(&app).ok().map(|p| vault.status(&p).state),
        kill_switch: status.kill_switch_tripped,
        btc_regime: Some(bots.btc_regime()),
        membership: Some(app.state::<MembershipManager>().health_state()),
        stream_live: app.state::<SignalManager>().health().connected,
        live_signal_bots: [&status.futures, &status.spot, &status.pump]
            .iter()
            .filter(|c| c.as_ref().is_some_and(|c| c.live))
            .count(),
        live_strategy_bots,
        halted_strategy_bots,
        ..Inputs::default()
    };
    i.clock_offset_ms = exchange.sync_clock().await;
    let cred = vault.credential(&venue);
    i.key = cred.as_ref().map(|c| c.permission);
    if let Some(cred) = cred.as_ref() {
        let account = exchange.futures_account(cred).await;
        if let Ok(acc) = &account {
            let own_signal: Vec<&str> = status
                .open_positions
                .iter()
                .filter(|p| p.is_live())
                .map(|p| p.symbol.as_str())
                .collect();
            i.foreign_positions = acc
                .positions
                .iter()
                .filter(|p| p.position_amt != 0.0)
                .map(|p| p.symbol.clone())
                .filter(|s| !own_signal.contains(&s.as_str()) && !owns_symbol(&app, s))
                .collect();
        }
        i.account = Some(account.map_err(code));
        // Bybit sets the mode per symbol; BTCUSDT stands for the account.
        i.hedge_mode = Some(exchange.hedge_mode(cred, "BTCUSDT").await.map_err(code));
    }
    let checks = evaluate(&i);
    Ok(Preflight {
        checked_at_ms: crate::bot::engine::now_ms(),
        venue,
        overall: worst(&checks),
        checks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> Inputs {
        Inputs {
            venue: "binance".into(),
            sandbox: false,
            live_build: true,
            production: true,
            vault: Some(VaultState::Unlocked),
            key: Some(CredentialPermission::TradeOnly),
            clock_offset_ms: Some(-40),
            account: Some(Ok(FuturesAccount {
                total_wallet_balance: 500.0,
                available_balance: 480.0,
                total_unrealized_pnl: 0.0,
                positions: vec![],
            })),
            hedge_mode: Some(Ok(false)),
            foreign_positions: vec![],
            kill_switch: false,
            btc_regime: Some(BtcRegime::Normal),
            membership: Some(MembershipHealth::Active),
            stream_live: true,
            live_signal_bots: 1,
            live_strategy_bots: 2,
            halted_strategy_bots: 0,
        }
    }

    fn get<'a>(c: &'a [Check], id: &str) -> &'a Check {
        c.iter().find(|c| c.id == id).expect(id)
    }

    #[test]
    fn a_ready_desk_is_all_ok() {
        let c = evaluate(&ready());
        assert_eq!(worst(&c), Verdict::Ok, "{c:?}");
        assert_eq!(get(&c, "account").value.as_deref(), Some("480.00 / 500.00 USDT"));
        assert_eq!(get(&c, "clock").value.as_deref(), Some("-40 ms"));
        assert_eq!(get(&c, "liveBots").value.as_deref(), Some("1 + 2"));
    }

    #[test]
    fn blockers_fail_and_soft_issues_warn() {
        let mut i = ready();
        i.key = Some(CredentialPermission::Unknown);
        i.hedge_mode = Some(Ok(true));
        i.kill_switch = true;
        i.halted_strategy_bots = 1;
        let c = evaluate(&i);
        for id in ["key", "positionMode", "killSwitch", "liveBots"] {
            assert_eq!(get(&c, id).verdict, Verdict::Fail, "{id}");
        }
        let mut i = ready();
        i.production = false;
        i.clock_offset_ms = Some(2_500);
        i.foreign_positions = vec!["ETHUSDT".into()];
        i.btc_regime = Some(BtcRegime::Break);
        i.stream_live = false;
        let c = evaluate(&i);
        assert_eq!(worst(&c), Verdict::Warn, "{c:?}");
        assert_eq!(get(&c, "positions").value.as_deref(), Some("ETHUSDT"));
        assert_eq!(get(&c, "clock").value.as_deref(), Some("+2500 ms"));
    }

    #[test]
    fn a_missing_key_names_why_and_skips_account_rows() {
        let mut i = ready();
        i.key = None;
        i.account = None;
        i.hedge_mode = None;
        i.vault = Some(VaultState::Locked);
        let c = evaluate(&i);
        assert_eq!(get(&c, "key").code.as_deref(), Some("vaultLocked"));
        assert_eq!(get(&c, "account").code.as_deref(), Some("needsKey"));
        assert!(c.iter().all(|c| c.id != "positions"));
        let mut i = ready();
        i.account = Some(Err("binanceRefused-2015".into()));
        assert_eq!(get(&evaluate(&i), "account").code.as_deref(), Some("binanceRefused-2015"));
    }

    #[test]
    fn a_venue_without_a_dry_run_blocks_and_the_sandbox_dry_run_warns() {
        assert_eq!(get(&evaluate(&ready()), "venue").verdict, Verdict::Ok);
        for venue in ["bybit", "okx"] {
            let mut i = ready();
            i.venue = venue.into();
            let c = evaluate(&i);
            assert_eq!(get(&c, "venue").code.as_deref(), Some("venueNotDryRun"), "{venue}");
            assert_eq!(get(&c, "venue").verdict, Verdict::Fail, "{venue}");
            assert_eq!(worst(&c), Verdict::Fail, "{venue}");
            // Against the venues' test networks the dry run itself may run.
            i.sandbox = true;
            i.production = false;
            let c = evaluate(&i);
            assert_eq!(get(&c, "venue").verdict, Verdict::Warn, "{venue}");
            assert_eq!(worst(&c), Verdict::Warn, "{venue}");
        }
        // No order path (Bitget since 2026-10-09): the sandbox does not help.
        let mut i = ready();
        (i.venue, i.sandbox, i.production) = ("bitget".into(), true, false);
        assert_eq!(get(&evaluate(&i), "venue").verdict, Verdict::Fail);
    }

    #[test]
    fn low_balance_and_unreachable_clock() {
        let mut i = ready();
        if let Some(Ok(a)) = i.account.as_mut() {
            a.available_balance = 20.0;
        }
        i.clock_offset_ms = None;
        let c = evaluate(&i);
        assert_eq!(get(&c, "account").verdict, Verdict::Warn);
        assert_eq!(get(&c, "clock").verdict, Verdict::Fail);
    }
}
