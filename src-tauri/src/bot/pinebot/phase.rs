//! The tick's Pine phase (`bot::engine::tick`, after the close, kill-switch
//! and strategy phases, before the signal open phase): for a running Pine
//! bot, once per symbol and bar, RUN_DELAY_MS after the bar closed, fetch
//! the warm-up bars, check the newest closed bar is the one that just
//! closed, run the script off the async runtime and act on the mapped
//! triggers (`act`). At most PHASE_BUDGET_MS per tick; a symbol not reached
//! runs on the next tick, within the stale window.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::bot::model::{BotConfig, BotKind, PineSettings, TradeMode};
use crate::bot::BotManager;
use crate::pine::{PineBar, PineErrorInfo};
use crate::telegram::notify::Category;

use super::rules::{self, RunOutcome, BAR_MISSING, HALTED, HALT_AFTER, PHASE_BUDGET_MS, STALE};
use super::types::{FiredTrigger, PineRunError};
use super::{act, bars, pending, PineManager};

fn now_ms() -> u64 {
    crate::bot::engine::now_ms()
}

pub async fn pine_phase(app: &AppHandle) {
    let mgr = app.state::<PineManager>();
    let Ok(_one) = mgr.phase_lock.try_lock() else {
        return;
    };
    // Unknown real entries are looked up even while the bot is stopped.
    pending::resolve_pending_all(app).await;
    let bots = app.state::<BotManager>();
    let Some(cfg) = bots.running_config(BotKind::Pine) else {
        return;
    };
    let Some(p) = cfg.pine.clone() else {
        return;
    };
    if bots.kill_switch_tripped() || mgr.halted(app).is_some() {
        return;
    }
    let now = now_ms();
    let close = rules::last_close(now, p.timeframe.interval_ms());
    if !rules::due(now, close) {
        return;
    }
    ensure_titles(&mgr, &p).await;
    let started = Instant::now();
    for symbol in &p.symbols {
        if started.elapsed() >= Duration::from_millis(PHASE_BUDGET_MS) {
            break; // the rest run next tick, within the stale window
        }
        if mgr.is_done(&p.script_hash, symbol, close) {
            continue;
        }
        if rules::stale(now_ms(), close, p.stale_s) {
            mgr.mark_done(&p.script_hash, symbol, close);
            mgr.note(symbol, STALE);
            continue;
        }
        let fetched = bars::fetch(&mgr.client, symbol, p.timeframe, p.warmup_bars, now_ms()).await;
        let bars = match fetched {
            Ok(b) => b,
            Err(code) => {
                mgr.note(symbol, &code); // retried next tick
                continue;
            }
        };
        let Some(last) = bars.last().copied() else {
            mgr.note(symbol, BAR_MISSING);
            continue;
        };
        match run_blocking(&p, bars, close, symbol).await {
            RunOutcome::BarMissing => mgr.note(symbol, BAR_MISSING),
            RunOutcome::Failed(e) => {
                mgr.mark_done(&p.script_hash, symbol, close);
                on_failure(app, &cfg, &p, symbol, last.t, e).await;
            }
            RunOutcome::Ran(v) => {
                mgr.mark_done(&p.script_hash, symbol, close);
                mgr.lock().streak.record(true, false);
                let fired = rules::fired(&v.fired, &p);
                let shown = fired.ids.iter().map(|(id, a)| FiredTrigger { id: id.clone(), title: mgr.title(&p.script_hash, id), action: *a }).collect();
                mgr.ran(symbol, last.t, v.bars, v.ms, shown, now_ms());
                if fired.any() {
                    act::act(app, &cfg, &p, symbol, close, last, &v, &fired).await;
                }
            }
        }
    }
}

/// Runs one symbol off the async runtime, with a wall-time cap on top of the
/// runner's own: a run past it is a failed run.
async fn run_blocking(p: &PineSettings, bars: Vec<PineBar>, close: u64, symbol: &str) -> RunOutcome {
    let (settings, sym) = (p.clone(), symbol.to_string());
    let job = tokio::task::spawn_blocking(move || rules::run_symbol(&settings, &bars, close, &sym, &crate::pine::run_last_bar));
    let fail = |kind: &str, message: &str| RunOutcome::Failed(PineErrorInfo { kind: kind.into(), message: message.into(), line: 0, col: 0 });
    match tokio::time::timeout(Duration::from_millis(crate::pine::RUN_WALL_MS + 2_000), job).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(_)) => fail("runtime", "pineRunAborted"),
        Err(_) => fail("timeout", "pineRunTimeout"),
    }
}

/// A failed run: no order; the error on the card; on a Live bot a Telegram
/// notice (at most one per 10 minutes) and, at the third in a row, the halt.
async fn on_failure(app: &AppHandle, cfg: &BotConfig, p: &PineSettings, symbol: &str, bar_time: u64, e: PineErrorInfo) {
    let mgr = app.state::<PineManager>();
    let now = now_ms();
    let err = PineRunError { kind: e.kind.clone(), message: e.message.clone(), line: e.line, col: e.col, bar_time, at_ms: now };
    mgr.failed(symbol, err);
    let live = cfg.mode == TradeMode::Live;
    let (halt, notice) = {
        let mut g = mgr.lock();
        let halt = g.streak.record(false, live);
        let notice = live && rules::notice_due(g.last_notice_ms, now);
        if notice {
            g.last_notice_ms = Some(now);
        }
        (halt, notice)
    };
    if notice {
        let line = failure_line(&p.name, symbol, p.timeframe.as_str(), &e);
        crate::telegram::send_line_for(app, Category::BotError, &line, live).await;
    }
    if halt {
        let code = format!("{HALTED}|{HALT_AFTER}");
        mgr.set_halt(app, Some(code.clone()));
        let bots = app.state::<BotManager>();
        bots.stop(app, BotKind::Pine);
        bots.push_skip("—", HALTED, Some(HALT_AFTER.to_string()));
        let line = format!("Pine bot stopped new entries after {HALT_AFTER} failed runs in a row ({}). Open positions keep their stops.", p.name);
        crate::telegram::send_line_for(app, Category::BotError, &line, live).await;
    }
}

/// "Pine bot run failed: BTCUSDT 1h (Trend) · runtime: na in a loop (line 3)".
pub fn failure_line(name: &str, symbol: &str, tf: &str, e: &PineErrorInfo) -> String {
    let at = if e.line > 0 { format!(" (line {})", e.line) } else { String::new() };
    format!("Pine bot run failed: {symbol} {tf} ({name}) · {}: {}{at}", e.kind, e.message)
}

/// Reads the trigger titles of the script once per hash (for the card and
/// the results); a compile that fails leaves them empty.
async fn ensure_titles(mgr: &PineManager, p: &PineSettings) {
    if mgr.lock().titles.as_ref().is_some_and(|(h, _)| *h == p.script_hash) {
        return;
    }
    let source = p.script.clone();
    let info = tokio::task::spawn_blocking(move || crate::pine::compile(&source)).await.ok();
    let titles: HashMap<String, String> = info.map(|i| i.triggers.into_iter().map(|t| (t.id, t.title)).collect()).unwrap_or_default();
    mgr.lock().titles = Some((p.script_hash.clone(), titles));
}
