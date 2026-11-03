//! Pine entries through the entry generalised from the TradingView bot
//! (`user_entry`): the pre-network gates with the Pine bot's codes, the
//! TradingView bot's codes unchanged, and the Live gating.

use crate::bot::engine::tv_trade::live_entry_refusal;
use crate::bot::engine::user_trade::{alert_refusal, TradeRequest, BOT_KIND};
use crate::bot::live_terms::signal_terms;
use crate::bot::model::{BotConfig, BotKind, TradeMode};
use crate::risk::model::RiskLevel;
use crate::signal::model::Direction;

use super::tests::saved;

fn req(kind: BotKind) -> TradeRequest {
    TradeRequest {
        kind,
        symbol: "btcusdt".into(),
        direction: Direction::Long,
        notional_usdt: Some(50.0),
        take_profit: 104.0,
        stop_loss: 98.0,
        mode: TradeMode::Paper,
        confirmation: None,
        confirmed_margin_usdt: None,
    }
}

#[test]
fn pine_entries_pass_the_generalised_entry_gates() {
    let pine = saved(TradeMode::Paper, false);
    assert_eq!(alert_refusal(Some(&pine), false, &req(BotKind::Pine), false), Ok("BTCUSDT".to_string()));
    assert_eq!(alert_refusal(None, false, &req(BotKind::Pine), false), Err("pineBotRequired".into()));
    assert_eq!(alert_refusal(Some(&pine), false, &req(BotKind::Pine), true), Err("pineAlreadyOpen".into()));
    assert_eq!(alert_refusal(Some(&pine), true, &req(BotKind::Pine), false), Err("botDailyLossTripped".into()));
    assert_eq!(alert_refusal(Some(&pine), false, &req(BotKind::Tradingview), false), Err(BOT_KIND.into()), "the bot must be the request's kind");
    // The TradingView bot's codes are unchanged.
    let tv = BotConfig::new_default(BotKind::Tradingview, "binance");
    assert_eq!(alert_refusal(None, false, &req(BotKind::Tradingview), false), Err("tvBotRequired".into()));
    assert_eq!(alert_refusal(Some(&tv), false, &req(BotKind::Tradingview), true), Err("tvAlreadyOpen".into()));
    assert_eq!(alert_refusal(Some(&tv), false, &req(BotKind::Futures), false), Err(BOT_KIND.into()));
    // Live gating: stored mode Live, armed this session, an order venue, the armed terms.
    let limits = RiskLevel::Balanced.limits();
    let armed = saved(TradeMode::Live, true);
    let terms = signal_terms(&armed, &limits);
    assert_eq!(live_entry_refusal(&armed, true, Some(&terms), &terms), Ok(()));
    assert_eq!(live_entry_refusal(&saved(TradeMode::Paper, false), true, Some(&terms), &terms), Err("tradeBotIsPaper".into()));
    assert_eq!(live_entry_refusal(&saved(TradeMode::Live, false), true, Some(&terms), &terms), Err("liveArmRequired".into()));
    assert_eq!(live_entry_refusal(&armed, false, Some(&terms), &terms), Err("liveFuturesBinanceOnly".into()));
    let mut moved = terms.clone();
    moved.margin_at_risk_usdt += 10.0;
    assert!(live_entry_refusal(&armed, true, Some(&terms), &moved).unwrap_err().starts_with("liveTermsChanged|"));
    // Not armed this session: never live, in any mode or venue.
    for venue in ["binance", "bybit", "okx"] {
        let mut c = saved(TradeMode::Live, false);
        c.exchange_id = venue.into();
        assert!(!crate::bot::engine::live::live_trading_allowed_in(&c, BotKind::Pine, false));
    }
    assert!(crate::bot::engine::live::live_trading_allowed_in(&armed, BotKind::Pine, false));
}

#[test]
fn the_telegram_lines_name_the_action_the_bot_and_the_code() {
    use super::pending::{live_open_line, refusal_line};
    use super::types::PineResult;
    let r = PineResult {
        symbol: "BTCUSDT".into(),
        timeframe: "1h".into(),
        bot_name: "Trend".into(),
        side: Some("long".into()),
        outcome: "pinePriceMoved|0.80%".into(),
        ..Default::default()
    };
    assert_eq!(refusal_line(&r), "Pine bot action not executed: buy BTCUSDT 1h (Trend) · pinePriceMoved|0.80%");
    let open = PineResult { filled_qty: Some(0.002), fill_price: Some(65010.5), outcome: "opened".into(), ..r.clone() };
    assert_eq!(live_open_line(&open), "Pine LIVE open: buy BTCUSDT 0.002 at 65010.5 (Trend)");
    let close = PineResult { side: None, outcome: "pineNothingToClose".into(), ..r };
    assert_eq!(refusal_line(&close), "Pine bot action not executed: close BTCUSDT 1h (Trend) · pineNothingToClose");
}

// The real runner (QuickJS) through the bot's own path: the bars end on the
// bar just closed, the mapped alert fires on the last bar, and the stop is
// read from the script's input under the bot's floor.
#[test]
fn a_real_script_run_feeds_the_bots_rules() {
    use super::rules::{exits, fired, run_symbol, RunOutcome};
    use crate::bot::model::{PineAction, PineSettings};
    use crate::pine::PineBar;
    const H: u64 = 3_600_000;
    let close = 1_761_897_600_000u64;
    let bars: Vec<PineBar> =
        (0..300u64).map(|i| PineBar { t: close - (300 - i) * H, o: 100.0, h: 101.0, l: 99.0, c: 100.0 + (i % 7) as f64, v: 10.0 }).collect();
    let mut p = PineSettings {
        script: "//@version=6\nindicator(\"t\")\nstop = input.float(1.5, \"Stop %\")\nalertcondition(close > 0, \"Up\")\n".into(),
        symbols: vec!["BTCUSDT".into()],
        stop_input: Some("Stop %".into()),
        ..Default::default()
    };
    p.triggers.insert("alert#0".into(), PineAction::Buy);
    let v = match run_symbol(&p, &bars, close, "BTCUSDT", &crate::pine::run_last_bar) {
        RunOutcome::Ran(v) => v,
        other => panic!("{other:?}"),
    };
    assert_eq!(v.bars, 300);
    let f = fired(&v.fired, &p);
    assert!(f.buy && !f.sell && !f.close, "{:?}", v.fired);
    assert_eq!(exits(&p, &v.numeric_inputs), Ok((1.5, 4.0)), "the script's tighter stop under the bot's 2%");
    assert_eq!(run_symbol(&p, &bars[..299], close, "BTCUSDT", &crate::pine::run_last_bar), RunOutcome::BarMissing);
}
