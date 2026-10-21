//! Phone link: the layer that connects the desk to the remote control.
//!
//! The protocol and all of the decision logic live in the `aleph-desk-link`
//! crate and are tested there. The only job here is **wiring**: implementing
//! the `DeskControl` trait with Aleph Edge's real bot desk.
//!
//! The split is deliberate. Decision logic written here could not be tested
//! without bringing up a Tauri app, and in practice those tests would never
//! run.

pub mod commands;
pub mod health;
pub mod session;

use aleph_desk_link::DeskControl;
use aleph_link::{ack_code, AckReply, BotSelector, DeskStatus, SkipNote};
use std::collections::HashMap;
use tauri::{AppHandle, Manager};

use crate::bot::model::BotKind;
use crate::bot::strategy::engine as strategy_engine;
use crate::bot::strategy::StrategyManager;
use crate::bot::{engine, BotManager};
use crate::risk::RiskManager;
use crate::store::StoreManager;

/// The bridge that applies remote commands to the real desk.
pub struct DeskBridge {
    app: AppHandle,
}

impl DeskBridge {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    /// Maps a `BotSelector` to Aleph Edge's bot kinds.
    fn kinds(sel: BotSelector) -> Vec<BotKind> {
        match sel {
            BotSelector::All => crate::bot::ALL_KINDS.to_vec(),
            BotSelector::Futures => vec![BotKind::Futures],
            BotSelector::Spot => vec![BotKind::Spot],
            BotSelector::Pump => vec![BotKind::Pump],
        }
    }

    /// Maps a `BotKind` to the single-bot selector used in acknowledgements.
    fn selector(k: BotKind) -> BotSelector {
        match k {
            BotKind::Futures => BotSelector::Futures,
            BotKind::Spot => BotSelector::Spot,
            BotKind::Pump => BotSelector::Pump,
            // Never started from the phone (see `start`); "all" stops it.
            BotKind::Tradingview => BotSelector::All,
        }
    }

    /// DCA / Grid bots answer only to "all": a per-kind selector (futures,
    /// spot, pump) names a signal bot and must not touch them.
    fn includes_strategy(sel: BotSelector) -> bool {
        matches!(sel, BotSelector::All)
    }
}

impl DeskControl for DeskBridge {
    /// Stops opening new positions.
    ///
    /// `BotManager::stop` does exactly this: open positions stay managed and
    /// only NEW entries stop. The phone's "Stop" button maps here and does not
    /// touch open positions.
    async fn stop_opening(&mut self, bot: BotSelector) -> Result<AckReply, AckReply> {
        let bots = self.app.state::<BotManager>();
        let kinds = Self::kinds(bot);
        for k in &kinds {
            bots.stop(&self.app, *k);
        }
        if Self::includes_strategy(bot) {
            strategy_engine::stop_all(&self.app).await;
        }
        let open_now = bots.open_count() + self.app.state::<StrategyManager>().open_cycle_count();
        Ok(AckReply::stopped(open_now as u64))
    }

    /// Closes the open positions as well.
    ///
    /// New entries are stopped first, then `engine::force_close_kinds` (and,
    /// for "all", `strategy_engine::force_close_all`) is awaited. The order
    /// matters: if a bot could open a position while the close runs, "close
    /// all" could leave a new position behind it.
    ///
    /// The `await` matters too: the acknowledgement goes out only once the
    /// close is done.
    async fn close_all(&mut self, bot: BotSelector) -> Result<AckReply, AckReply> {
        let kinds = Self::kinds(bot);
        let open_now = |bots: &BotManager| kinds.iter().map(|k| bots.open_count_for(*k)).sum::<usize>();
        let before = {
            let bots = self.app.state::<BotManager>();
            for k in &kinds {
                bots.stop(&self.app, *k);
            }
            open_now(&bots)
        };

        // Only the selected bots' positions: "close spot" must not touch
        // futures positions.
        engine::force_close_kinds(&self.app, "remoteKill", &kinds).await;
        // "Close all" includes the DCA / Grid cycles. A live bot's real
        // position follows its simulated close on the next mirror pass
        // (`strategy_live::sync_all`).
        let (strategy_closed, strategy_left) = if Self::includes_strategy(bot) {
            strategy_engine::force_close_all(&self.app, "remoteKill").await
        } else {
            (0, 0)
        };

        let left = open_now(&self.app.state::<BotManager>()) + strategy_left as usize;
        let closed = before.saturating_sub(open_now(&self.app.state::<BotManager>())) + strategy_closed as usize;
        // Positions that could not be priced are left for the next close
        // round, and the acknowledgement SAYS so (`closed_partial`).
        Ok(AckReply::closed(closed as u64, left as u64))
    }

    async fn start(&mut self, bot: BotSelector) -> Result<AckReply, AckReply> {
        let bots = self.app.state::<BotManager>();
        if bots.kill_switch_tripped() {
            // A remote start while the daily loss stop is active would make
            // that stop meaningless.
            return Err(AckReply::new(ack_code::KILL_SWITCH_TRIPPED));
        }
        let mut started = Vec::new();
        // The TradingView bot executes the user's own alerts: started on the
        // desk only, never from the phone.
        for k in Self::kinds(bot).into_iter().filter(|k| *k != BotKind::Tradingview) {
            if bots.config_for(k).is_some() {
                bots.start(self.app.clone(), k).map_err(|e| start_refusal(&e))?;
                started.push(Self::selector(k));
            }
        }
        if started.is_empty() {
            return Err(AckReply::new(ack_code::NOTHING_TO_START));
        }
        Ok(AckReply::started(&started))
    }

    /// The desk's state for the phone. There is no win rate: the phone shows
    /// exposure and how many independent bets the open positions make.
    ///
    /// Every figure has one scope. While real money is in play (a live signal
    /// position or bot, or a live DCA / Grid bot) the open count, exposure and
    /// today's result cover real positions and real trades only, against the
    /// wallet. Otherwise they cover the simulation, against the declared
    /// balance. The two are never added together (the daily stop's rule).
    fn status(&self) -> DeskStatus {
        let bots = self.app.state::<BotManager>();
        let st = bots.status();
        let strategy = self.app.state::<StrategyManager>();
        let live = crate::bot::model::LIVE_TRADING_ENABLED && crate::bot::strategy_live::live_exposure(&self.app);

        let mut bots_running = Vec::new();
        if st.futures_running {
            bots_running.push("futures".to_string());
        }
        if st.spot_running {
            bots_running.push("spot".to_string());
        }
        if st.pump_running {
            bots_running.push("pump".to_string());
        }
        if st.tradingview_running {
            bots_running.push("tradingview".to_string());
        }
        bots_running.extend(strategy.running_kinds().into_iter().map(|k| k.as_str().to_string()));

        let mut positions: Vec<OpenExposure> = st
            .open_positions
            .iter()
            .map(|p| OpenExposure { notional: position_notional(p), live: p.is_live() })
            .collect();
        positions.extend(strategy.open_cycle_notionals().into_iter().map(|notional| OpenExposure { notional, live: false }));
        positions.extend(
            crate::bot::strategy_live::real_notionals(&self.app)
                .into_iter()
                .map(|notional| OpenExposure { notional, live: true }),
        );

        let day_start = crate::store::utc_day_start_ms();
        let day_quote = self
            .app
            .path()
            .app_data_dir()
            .ok()
            .and_then(|dir| self.app.state::<StoreManager>().stats_for(&dir, live).ok())
            .map(|s| {
                let strategy_today = if live {
                    crate::bot::strategy_live::day_realized_quote(&self.app, day_start)
                } else {
                    strategy.day_pnl_quote(day_start)
                };
                s.today_pnl_quote + strategy_today
            });
        let declared = self.app.state::<RiskManager>().balance();
        let wallet = if live { bots.cached_equity().map(|(w, _)| w) } else { None };
        let (open, exposure_pct) = exposure(&positions, live, wallet.unwrap_or(declared));
        let today_net_pct = day_quote.map_or(0.0, |q| day_net_pct(q, wallet, declared));

        // Skip notes are grouped by reason: on the phone a summary such as
        // "3 combo filter, 2 capital cap" helps, thirty separate lines do not.
        let mut by_reason: HashMap<String, u32> = HashMap::new();
        for s in &st.recent_skips {
            *by_reason.entry(s.reason.clone()).or_insert(0) += 1;
        }
        let mut skips_today: Vec<SkipNote> = by_reason
            .into_iter()
            .map(|(reason, count)| SkipNote { reason, count })
            .collect();
        skips_today.sort_by(|a, b| b.count.cmp(&a.count).then(a.reason.cmp(&b.reason)));

        DeskStatus {
            bots_running,
            open_positions: open,
            today_net_pct,
            skips_today,
            exposure_pct,
            effective_bets: effective_bets(open),
            kill_switch_tripped: st.kill_switch_tripped,
        }
    }
}

/// One open position as the phone counts it.
#[derive(Clone, Copy, Debug)]
struct OpenExposure {
    notional: f64,
    live: bool,
}

/// Exposure is position size (notional), not margin: margin understates it by
/// the leverage. A row without a notional falls back to margin x leverage.
fn position_notional(p: &crate::bot::model::OpenPosition) -> f64 {
    if p.notional_usdt > 0.0 {
        p.notional_usdt
    } else {
        p.capital * f64::from(p.leverage)
    }
}

/// Open count and exposure (% of `balance`) over one set of positions: the
/// real ones while real money is in play, the simulated ones otherwise.
fn exposure(positions: &[OpenExposure], live: bool, balance: f64) -> (u32, f64) {
    let (count, notional) = positions
        .iter()
        .filter(|p| p.live == live)
        .fold((0u32, 0.0), |(n, sum), p| (n + 1, sum + p.notional));
    (count, pct_of(notional, balance))
}

/// Today's result as % of the start-of-day balance: the wallet less today's
/// realised result when real money is in play, the declared balance otherwise.
fn day_net_pct(day_quote: f64, wallet: Option<f64>, declared: f64) -> f64 {
    let start = wallet.map_or(declared, |w| w - day_quote);
    pct_of(day_quote, start)
}

fn pct_of(value: f64, balance: f64) -> f64 {
    if balance > 0.0 {
        value / balance * 100.0
    } else {
        0.0
    }
}

/// Maps `BotManager::start`'s stable error code to a phone acknowledgement.
/// An unknown code becomes `start_failed`, never a silent success.
fn start_refusal(code: &str) -> AckReply {
    AckReply::new(match code {
        "botDailyLossTripped" => ack_code::KILL_SWITCH_TRIPPED,
        "botNotConfigured" => ack_code::NOTHING_TO_START,
        _ => ack_code::START_FAILED,
    })
}

/// How many INDEPENDENT bets the open positions behave like.
///
/// ρ ≈ 0.18 is the intraday correlation between Sentinel signals. The design
/// effect is `DEFF = 1 + (n-1)ρ` and the effective count `n / DEFF`: 8
/// positions make **3.5** independent bets and 40 make **5.0**, which agrees
/// with Sentinel's separate estimate of "~48 signals a day ≈ 5 independent
/// bets".
///
/// On the phone this number stands IN PLACE of a win rate: forty positions
/// opened together are about five bets, and that concentration is what the
/// user needs to see.
fn effective_bets(n: u32) -> f64 {
    const RHO: f64 = 0.18;
    if n == 0 {
        return 0.0;
    }
    let n = f64::from(n);
    let deff = 1.0 + (n - 1.0) * RHO;
    (n / deff * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In a correlated book the position count and the independent-bet count
    /// are NOT THE SAME; the number shown on the phone must carry the gap.
    #[test]
    fn effective_bets_reflect_correlation() {
        assert_eq!(effective_bets(0), 0.0);
        assert_eq!(effective_bets(1), 1.0);
        // 8 positions are NOT 8 bets: 8 / (1 + 7x0.18) = 3.5.
        assert_eq!(effective_bets(8), 3.5);
        // Sentinel's separate estimate is "~48 signals a day ~ 5 independent
        // bets"; the formula reproduces it independently.
        assert_eq!(effective_bets(40), 5.0);
    }

    /// The effective count grows with the position count, but NOT
    /// proportionally.
    #[test]
    fn effective_bets_grow_but_not_linearly() {
        let a = effective_bets(4);
        let b = effective_bets(8);
        assert!(b > a, "did not grow");
        assert!(b < a * 2.0, "grew linearly: {a} -> {b}");
    }

    fn pos(notional: f64, live: bool) -> OpenExposure {
        OpenExposure { notional, live }
    }

    /// A live DCA cycle with no signal positions is real exposure.
    #[test]
    fn live_strategy_only_counts_its_real_position() {
        let p = [pos(500.0, true), pos(500.0, false)]; // real mirror + its simulated cycle
        assert_eq!(exposure(&p, true, 1000.0), (1, 50.0));
    }

    /// A live signal position: the paper DCA cycle beside it is left out.
    #[test]
    fn live_signal_only_leaves_paper_out() {
        let p = [pos(300.0, true), pos(200.0, false)];
        assert_eq!(exposure(&p, true, 1000.0), (1, 30.0));
    }

    /// Mixed book: the open count and the exposure cover the same positions,
    /// real ones in live mode and simulated ones on paper.
    #[test]
    fn mixed_book_never_adds_real_and_paper() {
        let p = [pos(300.0, true), pos(200.0, true), pos(100.0, false), pos(400.0, false)];
        assert_eq!(exposure(&p, true, 1000.0), (2, 50.0));
        assert_eq!(exposure(&p, false, 1000.0), (2, 50.0));
        assert_eq!(exposure(&p, true, 0.0), (2, 0.0));
    }

    /// Live: today's result against the start-of-day wallet. Paper: against
    /// the declared balance.
    #[test]
    fn day_result_uses_the_scope_balance() {
        // Wallet 990 after a -10 day started at 1000.
        assert!((day_net_pct(-10.0, Some(990.0), 5000.0) - -1.0).abs() < 1e-9);
        assert!((day_net_pct(-10.0, None, 500.0) - -2.0).abs() < 1e-9);
        assert_eq!(day_net_pct(-10.0, None, 0.0), 0.0);
    }

    /// Strategy bots (DCA/Grid) answer only to "all".
    #[test]
    fn strategy_bots_answer_only_to_all() {
        assert!(DeskBridge::includes_strategy(BotSelector::All));
        assert!(!DeskBridge::includes_strategy(BotSelector::Futures));
        assert!(!DeskBridge::includes_strategy(BotSelector::Spot));
        assert!(!DeskBridge::includes_strategy(BotSelector::Pump));
    }

    /// A start refusal reaches the phone as a CODE, not in the desk's language.
    #[test]
    fn start_refusal_maps_to_a_stable_code() {
        assert_eq!(start_refusal("botDailyLossTripped").code, ack_code::KILL_SWITCH_TRIPPED);
        assert_eq!(start_refusal("botNotConfigured").code, ack_code::NOTHING_TO_START);
        assert_eq!(start_refusal("somethingNew").code, ack_code::START_FAILED);
    }

    /// Remote command replies carry no fixed Turkish text: the phone renders
    /// the code in its own language. The search words are Turkish on purpose;
    /// they are the reply literals this test keeps out of the module body.
    #[test]
    fn remote_command_replies_carry_no_fixed_text() {
        let src = include_str!("mod.rs");
        let body = &src[..src.find("#[cfg(test)]").unwrap()];
        for leftover in ["başlatıldı", "kapatıldı", "açılmıyor", "durdurucusu devrede"] {
            assert!(
                !body.contains(&format!("\"{leftover}")) && !body.contains(&format!("{leftover}\"")),
                "fixed text remains: {leftover}"
            );
        }
    }

    /// The `started` acknowledgement names the bot kinds that really started.
    #[test]
    fn started_kind_round_trips_through_the_selector() {
        for k in [BotKind::Futures, BotKind::Spot, BotKind::Pump] {
            assert_eq!(DeskBridge::kinds(DeskBridge::selector(k)), vec![k]);
        }
    }

    #[test]
    fn bot_selector_maps_to_kinds() {
        assert_eq!(DeskBridge::kinds(BotSelector::All).len(), 4, "a remote stop-all reaches the TradingView bot too");
        assert_eq!(DeskBridge::kinds(BotSelector::Spot), vec![BotKind::Spot]);
        assert_eq!(
            DeskBridge::kinds(BotSelector::Futures),
            vec![BotKind::Futures]
        );
    }
}
