//! StrategyManager state rules without Tauri: restore, day P&L for the
//! kill switch, the portfolio breaker, views.

use super::cycle::{step_bar, CycleStart, CycleState};
use super::costs::FUTURES_SIM;
use super::driver::tests::{minute_bars, sample_bot};
use super::model::{BotRunState, MarketKind, StrategyParams};
use super::{Feed, StrategyManager};

fn open_cycle(bot: &super::model::StrategyBot, open: f64) -> CycleState {
    let StrategyParams::Dca(p) = &bot.cfg.params else { panic!() };
    let st = CycleStart {
        bot_id: &bot.id,
        seq: 1,
        side: bot.cfg.side,
        budget: bot.cfg.budget,
        leverage: 1.0,
        size_factor: 1.0,
        costs: FUTURES_SIM,
        max_duration_min: None,
    };
    let bars = minute_bars(&[(open, open, open, open)]);
    let mut c = CycleState::open_dca(&st, p, &bars[0]).unwrap();
    step_bar(&mut c, &bars[0], true);
    c
}

fn feed(px: f64) -> Feed {
    Feed {
        last_close_ms: 1,
        last_close: px,
        last_ok_ms: 1,
    }
}

#[test]
fn restore_stops_every_bot_but_keeps_open_cycles_managed() {
    let mgr = StrategyManager::new();
    let mut a = sample_bot();
    a.state = BotRunState::Armed;
    let mut b = sample_bot();
    b.id = "sb_bbbbbbbbbbbb".into();
    b.state = BotRunState::InCycle;
    let mut d = sample_bot();
    d.id = "sb_dddddddddddd".into();
    d.state = BotRunState::Dead;
    let cyc = open_cycle(&b, 100.0);
    mgr.load(vec![a.clone(), b.clone(), d.clone()], vec![cyc], Vec::new(), Vec::new());
    assert_eq!(mgr.bot(&a.id).unwrap().state, BotRunState::Stopped);
    assert_eq!(mgr.bot(&b.id).unwrap().state, BotRunState::Stopped, "not re-armed");
    assert_eq!(mgr.bot(&d.id).unwrap().state, BotRunState::Dead);
    assert!(mgr.has_open_cycle(&b.id));
    assert!(mgr.any_active(), "the open cycle keeps the engine running");
    assert_eq!(mgr.view(&b.id).unwrap().run_state, "inCycle");
    assert!(
        !mgr.view(&b.id).unwrap().accepting_new_cycles,
        "a stopped bot holding a cycle must not read as running"
    );
    assert!(mgr.view(&b.id).unwrap().paper);
}

#[test]
fn resume_puts_a_paper_bot_back_to_work() {
    let mgr = StrategyManager::new();
    let mut a = sample_bot();
    a.state = BotRunState::Armed;
    let mut b = sample_bot();
    b.id = "sb_bbbbbbbbbbbb".into();
    b.state = BotRunState::InCycle;
    let mut p = sample_bot();
    p.id = "sb_pppppppppppp".into();
    p.state = BotRunState::Paused;
    let mut d = sample_bot();
    d.id = "sb_dddddddddddd".into();
    d.state = BotRunState::Dead;
    let cyc = open_cycle(&b, 100.0);
    mgr.load(vec![a.clone(), b.clone(), p.clone(), d.clone()], vec![cyc], Vec::new(), Vec::new());
    for x in [&a, &b, &p, &d] {
        mgr.resume(&x.id, x.state);
    }
    assert_eq!(mgr.bot(&a.id).unwrap().state, BotRunState::Armed);
    assert_eq!(mgr.bot(&b.id).unwrap().state, BotRunState::InCycle);
    assert_eq!(mgr.bot(&p.id).unwrap().state, BotRunState::Paused);
    assert_eq!(mgr.bot(&d.id).unwrap().state, BotRunState::Dead, "a dead bot stays dead");
    assert!(mgr.view(&b.id).unwrap().accepting_new_cycles);
}

#[test]
fn strategy_day_pnl_counts_unrealised_loss() {
    let mgr = StrategyManager::new();
    let bot = sample_bot();
    let cyc = open_cycle(&bot, 100.0);
    mgr.load(vec![bot.clone()], vec![cyc], Vec::new(), vec![((MarketKind::Futures, bot.cfg.symbol.clone()), feed(100.0))]);
    let today = 86_400_000;
    assert_eq!(mgr.day_pnl_quote(today), 0.0, "no snapshot yet: no stale base");
    mgr.roll_day(today, None);
    // price falls 60% on a 100-notional base order: an unrealised loss
    mgr.set_feed(MarketKind::Futures, &bot.cfg.symbol, feed(40.0));
    let pnl = mgr.day_pnl_quote(today);
    assert!(pnl < -55.0, "{pnl}");
    // a later day starts from that equity again
    assert_eq!(mgr.day_pnl_quote(today + 86_400_000), 0.0);
}

#[test]
fn portfolio_breaker_peak_trip_and_rearm() {
    let mgr = StrategyManager::new();
    let bot = sample_bot();
    mgr.load(vec![bot], Vec::new(), Vec::new(), Vec::new());
    assert!(!mgr.breaker_check(50.0, 1000.0));
    assert_eq!(mgr.portfolio_peak(), 50.0);
    assert!(!mgr.breaker_check(-99.0, 1000.0), "14.9% of budget below the peak");
    assert!(mgr.breaker_check(-100.0, 1000.0), "15% below the peak trips");
    let day = 5 * 86_400_000;
    mgr.trip_breaker(day);
    assert!(mgr.portfolio_tripped());
    assert_eq!(mgr.rearm_breaker(day), Err("portfolioDdTripped"), "not the same UTC day");
    assert!(mgr.portfolio_tripped(), "holds until re-armed");
    // persisted state survives a restart
    let restored = StrategyManager::new();
    restored.set_risk(mgr.risk(), mgr.tripped_day(), mgr.portfolio_peak());
    assert!(restored.portfolio_tripped());
    assert_eq!(restored.rearm_breaker(day + 86_400_000), Ok(()));
    assert!(!restored.portfolio_tripped());
}

#[test]
fn stop_all_halts_new_cycles_only() {
    let mgr = StrategyManager::new();
    let mut bot = sample_bot();
    let cyc = open_cycle(&bot, 100.0);
    mgr.load(vec![bot.clone()], vec![cyc], Vec::new(), Vec::new());
    bot.state = BotRunState::Armed;
    mgr.put_bot(bot.clone());
    assert_eq!(mgr.stop_all().len(), 1);
    assert_eq!(mgr.bot(&bot.id).unwrap().state, BotRunState::Stopped);
    assert!(mgr.has_open_cycle(&bot.id));
    assert_eq!(mgr.reserved_budget(None), 1000.0, "an open cycle keeps its reservation");
}

#[test]
fn notes_are_capped_and_deduplicated_per_bot() {
    let mgr = StrategyManager::new();
    mgr.note("b1", "X", "btcBreak", None, 1);
    mgr.note("b1", "X", "btcBreak", None, 2);
    assert_eq!(mgr.notes().len(), 1);
    mgr.clear_last_note("b1");
    mgr.note("b1", "X", "btcBreak", None, 3);
    assert_eq!(mgr.notes().len(), 2);
    for i in 0..60 {
        mgr.note("", "X", "k", Some(i.to_string()), i);
    }
    assert_eq!(mgr.notes().len(), 50);
}

#[test]
fn bot_ids_have_the_documented_shape() {
    let id = super::new_bot_id();
    assert_eq!(id.len(), 15);
    assert!(id.starts_with("sb_"));
    assert!(id[3..].chars().all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c)));
    assert_ne!(super::new_bot_id(), id);
}

/// Two bots, one inside the breaker (`a`), one switched out (`x`).
fn covered_and_exempt() -> (StrategyManager, super::model::StrategyBot, super::model::StrategyBot) {
    let mgr = StrategyManager::new();
    let mut a = sample_bot();
    a.state = BotRunState::Armed;
    let mut x = sample_bot();
    x.id = "sb_xxxxxxxxxxxx".into();
    x.cfg.symbol = "OTHERUSDT".into();
    x.cfg.portfolio_breaker = false;
    x.state = BotRunState::Armed;
    mgr.load(vec![a.clone(), x.clone()], Vec::new(), Vec::new(), Vec::new());
    // load() restores every bot Stopped; re-arm both.
    mgr.put_bot(a.clone());
    mgr.put_bot(x.clone());
    (mgr, a, x)
}

#[test]
fn breaker_counts_only_the_bots_inside_it() {
    let (mgr, mut a, mut x) = covered_and_exempt();
    x.realized_quote = -900.0;
    mgr.put_bot(x.clone());
    assert_eq!(mgr.breaker_pnl(), 0.0, "the exempt bot's loss is not counted");
    assert_eq!(mgr.breaker_budget(), 1000.0, "the exempt bot's budget is not counted");
    assert_eq!(mgr.breaker_coverage(), (1, 2));
    assert!(!mgr.breaker_check(mgr.breaker_pnl(), mgr.breaker_budget()));
    a.realized_quote = -150.0;
    mgr.put_bot(a.clone());
    assert_eq!(mgr.breaker_pnl(), -150.0);
    assert!(mgr.breaker_check(mgr.breaker_pnl(), mgr.breaker_budget()), "15% of the covered budget trips");
}

#[test]
fn a_trip_stops_closes_and_holds_only_the_bots_inside_it() {
    let (mgr, a, x) = covered_and_exempt();
    let day = 3 * 86_400_000;
    mgr.trip_breaker(day);
    // the engine's trip path: stop these, then close these
    let stopped: Vec<_> = mgr.stop_breaker_bots().into_iter().map(|b| b.id).collect();
    assert_eq!(stopped, vec![a.id.clone()]);
    assert_eq!(mgr.breaker_bot_ids(), vec![a.id.clone()], "the exempt bot is not closed");
    assert_eq!(mgr.bot(&a.id).unwrap().state, BotRunState::Stopped);
    assert_eq!(mgr.bot(&x.id).unwrap().state, BotRunState::Armed, "the exempt bot runs on");
    assert!(mgr.breaker_holds(&mgr.bot(&a.id).unwrap()));
    assert!(!mgr.breaker_holds(&mgr.bot(&x.id).unwrap()), "the exempt bot may start");
    // the kill switch still covers every bot
    assert_eq!(mgr.stop_all().len(), 1, "x was still running");
    assert_eq!(mgr.bot(&x.id).unwrap().state, BotRunState::Stopped);
}

#[test]
fn breaker_never_trips_when_every_bot_is_exempt() {
    let mgr = StrategyManager::new();
    let mut a = sample_bot();
    a.cfg.portfolio_breaker = false;
    a.realized_quote = -999.0;
    mgr.load(vec![a], Vec::new(), Vec::new(), Vec::new());
    assert_eq!(mgr.breaker_budget(), 0.0);
    assert_eq!(mgr.breaker_coverage(), (0, 1));
    assert!(!mgr.breaker_check(mgr.breaker_pnl(), mgr.breaker_budget()));
    assert!(!mgr.breaker_check(-1e9, mgr.breaker_budget()), "a 0 budget never trips");
}

#[test]
fn rearm_restarts_the_peak_at_the_covered_pnl() {
    let (mgr, mut a, mut x) = covered_and_exempt();
    a.realized_quote = -200.0;
    x.realized_quote = 500.0;
    mgr.put_bot(a);
    mgr.put_bot(x);
    mgr.trip_breaker(86_400_000);
    assert_eq!(mgr.rearm_breaker(2 * 86_400_000), Ok(()));
    assert_eq!(mgr.portfolio_peak(), -200.0, "the exempt bot's profit is not in the peak");
}

#[test]
fn switching_a_bot_keeps_the_breaker_drawdown() {
    // strategy_update shifts the peak by the bot's P&L when the flag moves.
    let (mgr, _, mut x) = covered_and_exempt();
    x.realized_quote = -120.0;
    mgr.put_bot(x.clone());
    mgr.breaker_check(mgr.breaker_pnl(), mgr.breaker_budget());
    let dd_before = mgr.portfolio_peak() - mgr.breaker_pnl();
    let pnl = mgr.bot_pnl(&x.id);
    x.cfg.portfolio_breaker = true;
    mgr.put_bot(x.clone());
    mgr.shift_peak(pnl);
    assert_eq!(mgr.portfolio_peak() - mgr.breaker_pnl(), dd_before);
    assert!(!mgr.breaker_check(mgr.breaker_pnl(), mgr.breaker_budget()), "joining is not a drawdown");
}
