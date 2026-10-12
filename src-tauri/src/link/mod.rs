//! Telefon bağlantısı: masayı uzaktan kumandaya bağlayan katman.
//!
//! Protokolün ve karar mantığının tamamı `aleph-desk-link` crate'inde ve orada
//! test edilmiş durumda. Burada yapılan tek şey **kablolama**: `DeskControl`
//! arayüzünü Aleph Edge'in gerçek bot masasıyla doldurmak.
//!
//! Bu ayrım kasıtlı. Karar mantığı burada yazılsaydı, testleri bir Tauri
//! uygulaması ayağa kaldırmadan koşturmak mümkün olmazdı ve pratikte hiç
//! koşmazdı.

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

/// Uzaktan komutları gerçek masaya uygulayan köprü.
pub struct DeskBridge {
    app: AppHandle,
}

impl DeskBridge {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    /// `BotSelector`'ı Aleph Edge'in bot türlerine çevirir.
    fn kinds(sel: BotSelector) -> Vec<BotKind> {
        match sel {
            BotSelector::All => vec![BotKind::Futures, BotKind::Spot, BotKind::Pump],
            BotSelector::Futures => vec![BotKind::Futures],
            BotSelector::Spot => vec![BotKind::Spot],
            BotSelector::Pump => vec![BotKind::Pump],
        }
    }

    /// `BotKind`'ı onaydaki tek bot seçicisine çevirir.
    fn selector(k: BotKind) -> BotSelector {
        match k {
            BotKind::Futures => BotSelector::Futures,
            BotKind::Spot => BotSelector::Spot,
            BotKind::Pump => BotSelector::Pump,
        }
    }

    /// DCA / Grid bots answer only to "all": a per-kind selector (futures,
    /// spot, pump) names a signal bot and must not touch them.
    fn includes_strategy(sel: BotSelector) -> bool {
        matches!(sel, BotSelector::All)
    }
}

impl DeskControl for DeskBridge {
    /// Yeni pozisyon açmayı durdurur.
    ///
    /// `BotManager::stop` tam olarak bunu yapar: açık pozisyonlar yönetilmeye
    /// devam eder, yalnızca YENİ girişler durur. Telefondaki "Durdur" düğmesi
    /// buraya bağlıdır ve açık pozisyonlara dokunmaz.
    async fn stop_opening(&mut self, bot: BotSelector) -> Result<AckReply, AckReply> {
        let bots = self.app.state::<BotManager>();
        let kinds = Self::kinds(bot);
        for k in &kinds {
            bots.stop(&self.app, *k);
        }
        if Self::includes_strategy(bot) {
            strategy_engine::stop_all(&self.app).await;
        }
        let acik = bots.open_count() + self.app.state::<StrategyManager>().open_cycle_count();
        Ok(AckReply::stopped(acik as u64))
    }

    /// Açık pozisyonları da kapatır.
    ///
    /// Önce yeni girişler durdurulur, sonra `force_close_all` beklenir. Sıra
    /// önemli: kapatma sürerken bot yeni pozisyon açabilseydi, "hepsini kapat"
    /// komutu kendi arkasından yeni pozisyon bırakabilirdi.
    ///
    /// `await` de önemli — onay ancak kapatma bittikten sonra gider.
    async fn close_all(&mut self, bot: BotSelector) -> Result<AckReply, AckReply> {
        let kinds = Self::kinds(bot);
        let acik = |bots: &BotManager| kinds.iter().map(|k| bots.open_count_for(*k)).sum::<usize>();
        let oncesi = {
            let bots = self.app.state::<BotManager>();
            for k in &kinds {
                bots.stop(&self.app, *k);
            }
            acik(&bots)
        };

        // Yalnızca seçilen botların pozisyonları: "spot'u kapat" komutu
        // futures pozisyonlarına dokunmamalı.
        engine::force_close_kinds(&self.app, "remoteKill", &kinds).await;
        // "Close all" includes the DCA / Grid cycles. A live bot's real
        // position follows its simulated close on the next mirror pass
        // (`strategy_live::sync_all`).
        let (strateji_kapanan, strateji_kalan) = if Self::includes_strategy(bot) {
            strategy_engine::force_close_all(&self.app, "remoteKill").await
        } else {
            (0, 0)
        };

        let kalan = acik(&self.app.state::<BotManager>()) + strateji_kalan as usize;
        let kapanan = oncesi.saturating_sub(acik(&self.app.state::<BotManager>())) + strateji_kapanan as usize;
        // Fiyatlanamayan pozisyonlar bir sonraki kapanış turuna bırakılır.
        // Bunu SÖYLEMEK zorundayız: "hepsi kapandı" demek, kalan pozisyonu
        // kullanıcıdan saklamak olurdu (`closed_partial`).
        Ok(AckReply::closed(kapanan as u64, kalan as u64))
    }

    async fn start(&mut self, bot: BotSelector) -> Result<AckReply, AckReply> {
        let bots = self.app.state::<BotManager>();
        if bots.kill_switch_tripped() {
            // Günlük zarar durdurucusu devredeyken uzaktan başlatmaya izin
            // vermek, o durdurucuyu anlamsız kılardı.
            return Err(AckReply::new(ack_code::KILL_SWITCH_TRIPPED));
        }
        let mut basladi = Vec::new();
        for k in Self::kinds(bot) {
            if bots.config_for(k).is_some() {
                bots.start(self.app.clone(), k).map_err(|e| start_refusal(&e))?;
                basladi.push(Self::selector(k));
            }
        }
        if basladi.is_empty() {
            return Err(AckReply::new(ack_code::NOTHING_TO_START));
        }
        Ok(AckReply::started(&basladi))
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

        // Atlama notları sebebe göre toplanır: telefonda otuz ayrı satır değil,
        // "3 kombo filtresi, 2 sermaye tavanı" gibi bir özet işe yarar.
        let mut sayac: HashMap<String, u32> = HashMap::new();
        for s in &st.recent_skips {
            *sayac.entry(s.reason.clone()).or_insert(0) += 1;
        }
        let mut skips_today: Vec<SkipNote> = sayac
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

/// `BotManager::start`'ın kararlı hata kodunu telefon onayına çevirir.
/// Bilinmeyen kod `start_failed` olur — hiçbir zaman sessiz bir başarı değil.
fn start_refusal(code: &str) -> AckReply {
    AckReply::new(match code {
        "botDailyLossTripped" => ack_code::KILL_SWITCH_TRIPPED,
        "botNotConfigured" => ack_code::NOTHING_TO_START,
        _ => ack_code::START_FAILED,
    })
}

/// Açık pozisyonların kaç BAĞIMSIZ bahis gibi davrandığı.
///
/// Sentinel'in kendi defterinde ölçülen gün-içi korelasyon ρ≈0.18. Tasarım
/// etkisi `DEFF = 1 + (n-1)ρ`, etkin sayı `n / DEFF`. 8 pozisyon **3,5**
/// bağımsız bahis eder; 40 pozisyon **5,0** — ki bu, Sentinel'in bağımsız
/// ölçümü olan "günde ~48 sinyal ≈ 5 bağımsız bahis" ile örtüşüyor.
///
/// Bu sayı telefonda winrate'in YERİNE durur. 2026-07-29'da 40 pozisyon aynı
/// anda açıldı ve −45,7pp'ye mal oldu — kullanıcının ekranında görmesi gereken
/// şey buydu, bir başarı oranı değil.
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

    /// Korelasyonlu bir defterde pozisyon sayisi ile bagimsiz bahis sayisi
    /// AYNI SEY DEGILDIR; telefonda gosterilen sayi bu farki tasimali.
    #[test]
    fn etkin_bahis_korelasyonu_yansitir() {
        assert_eq!(effective_bets(0), 0.0);
        assert_eq!(effective_bets(1), 1.0);
        // 8 pozisyon, 8 bahis DEGIL: 8 / (1 + 7x0.18) = 3.5.
        assert_eq!(effective_bets(8), 3.5);
        // 2026-07-29: 40 pozisyon acildi ve -45.7pp'ye mal oldu. Sentinel'in
        // kendi olcumu "gunde ~48 sinyal ~ 5 bagimsiz bahis" diyor; formul
        // bunu bagimsiz olarak yeniden uretiyor.
        assert_eq!(effective_bets(40), 5.0);
    }

    /// Pozisyon sayisi arttikca etkin sayi artmali ama ORANTILI degil.
    #[test]
    fn etkin_bahis_artan_ama_dogrusal_degil() {
        let a = effective_bets(4);
        let b = effective_bets(8);
        assert!(b > a, "artmadi");
        assert!(b < a * 2.0, "dogrusal artti: {a} -> {b}");
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

    /// Strateji botları (DCA/Grid) yalnızca "hepsi" komutuna uyar.
    #[test]
    fn strateji_yalnizca_hepsi_secicisinde() {
        assert!(DeskBridge::includes_strategy(BotSelector::All));
        assert!(!DeskBridge::includes_strategy(BotSelector::Futures));
        assert!(!DeskBridge::includes_strategy(BotSelector::Spot));
        assert!(!DeskBridge::includes_strategy(BotSelector::Pump));
    }

    /// Başlatma reddi telefona KOD olarak gider; masanın dili değil.
    #[test]
    fn baslatma_reddi_kararli_koda_cevrilir() {
        assert_eq!(start_refusal("botDailyLossTripped").code, ack_code::KILL_SWITCH_TRIPPED);
        assert_eq!(start_refusal("botNotConfigured").code, ack_code::NOTHING_TO_START);
        assert_eq!(start_refusal("somethingNew").code, ack_code::START_FAILED);
    }

    /// Uzaktan komut yanıtlarında sabit Türkçe metin kalmamalı: telefon
    /// kodu kendi dilinde çizer.
    #[test]
    fn uzak_komut_yanitlari_metin_tasimaz() {
        let src = include_str!("mod.rs");
        let govde = &src[..src.find("#[cfg(test)]").unwrap()];
        for kalinti in ["başlatıldı", "kapatıldı", "açılmıyor", "durdurucusu devrede"] {
            assert!(
                !govde.contains(&format!("\"{kalinti}")) && !govde.contains(&format!("{kalinti}\"")),
                "sabit metin kaldi: {kalinti}"
            );
        }
    }

    /// `started` onayı, gerçekten başlayan bot türlerini adlandırır.
    #[test]
    fn baslayan_tur_secici_ile_gidip_gelir() {
        for k in [BotKind::Futures, BotKind::Spot, BotKind::Pump] {
            assert_eq!(DeskBridge::kinds(DeskBridge::selector(k)), vec![k]);
        }
    }

    #[test]
    fn bot_secici_dogru_esleniyor() {
        assert_eq!(DeskBridge::kinds(BotSelector::All).len(), 3);
        assert_eq!(DeskBridge::kinds(BotSelector::Spot), vec![BotKind::Spot]);
        assert_eq!(
            DeskBridge::kinds(BotSelector::Futures),
            vec![BotKind::Futures]
        );
    }
}
