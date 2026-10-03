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
            bots.stop(*k);
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
                bots.stop(*k);
            }
            acik(&bots)
        };

        // Yalnızca seçilen botların pozisyonları: "spot'u kapat" komutu
        // futures pozisyonlarına dokunmamalı.
        engine::force_close_kinds(&self.app, "remoteKill", &kinds).await;
        // "Hepsini kapat": DCA / Grid kağıt döngüleri de (gerçek emir yok).
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

    /// Telefonda gösterilecek durum.
    ///
    /// **Toplu winrate YOK.** Bir araç enstrüman satar, sicil satmaz. Yerine
    /// hayatta kalmayı belirleyen sayılar var: maruziyet ve bu pozisyonların
    /// kaç bağımsız bahis gibi davrandığı.
    fn status(&self) -> DeskStatus {
        let bots = self.app.state::<BotManager>();
        let st = bots.status();

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

        // Bugünün net sonucu, masaüstünün kendi defterinden. Gerçek para
        // varsa yalnızca gerçek işlemler, gerçek cüzdana oranla; yoksa
        // yalnızca simülasyon, beyan edilen bakiyeye oranla. İkisi asla
        // tek toplamda karışmaz (günlük durdurucuyla aynı kural).
        let bots = self.app.state::<BotManager>();
        let canli = crate::bot::model::LIVE_TRADING_ENABLED
            && bots.positions_snapshot().iter().any(|p| p.is_live());
        // DCA / Grid her zaman kağıt: günlük sonuçları (gerçekleşmiş ve
        // gerçekleşmemiş) yalnızca simülasyon toplamına eklenir.
        let strateji = self.app.state::<StrategyManager>();
        let strateji_gun = if canli {
            0.0
        } else {
            strateji.day_pnl_quote(crate::store::utc_day_start_ms())
        };
        let today_net_pct = self
            .app
            .path()
            .app_data_dir()
            .ok()
            .and_then(|dir| self.app.state::<StoreManager>().stats_for(&dir, canli).ok())
            .map(|s| {
                let balance = match bots.cached_equity() {
                    Some((cuzdan, _)) if canli => cuzdan - s.today_pnl_quote,
                    _ => self.app.state::<RiskManager>().balance(),
                };
                if balance > 0.0 {
                    (s.today_pnl_quote + strateji_gun) / balance * 100.0
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0);

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

        let open = (st.open_positions.len() + strateji.open_cycle_count()) as u32;
        let risk = self.app.state::<RiskManager>();
        let balance = risk.balance();
        // Maruziyet pozisyon büyüklüğüdür (notional), teminat değil: teminat
        // kaldıraç kadar küçük gösterir. Eski satırlarda notional yoksa
        // teminat × kaldıraç.
        let notional: f64 = st
            .open_positions
            .iter()
            .map(|p| if p.notional_usdt > 0.0 { p.notional_usdt } else { p.capital * f64::from(p.leverage) })
            .sum();
        let exposure_pct = if balance > 0.0 {
            notional / balance * 100.0
        } else {
            0.0
        };

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

/// `BotManager::start`'ın kararlı hata kodunu telefon onayına çevirir.
/// Bilinmeyen kod `start_failed` olur — hiçbir zaman sessiz bir başarı değil.
fn start_refusal(code: &str) -> AckReply {
    AckReply::new(match code {
        "botDailyLossTripped" => ack_code::KILL_SWITCH_TRIPPED,
        "botPumpNeedsAmbitious" => ack_code::PUMP_NEEDS_AMBITIOUS,
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
        assert_eq!(start_refusal("botPumpNeedsAmbitious").code, ack_code::PUMP_NEEDS_AMBITIOUS);
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
