//! `DeskLink` birim testleri.
//!
//! Ayrı dosyada, çünkü `DeskControl` async'e çevrildikten sonra testlerin de
//! bir çalışma zamanına ihtiyacı oldu ve o yardımcıyı üretim kodunun içine
//! gömmek yerine burada tutmak daha okunur.

use super::*;
use aleph_link::{ack_code, SkipNote};

#[derive(Default)]
struct SahteMasa {
    durduruldu: Vec<BotSelector>,
    kapatildi: Vec<BotSelector>,
    baslatildi: Vec<BotSelector>,
    hata: Option<AckReply>,
}

impl DeskControl for SahteMasa {
    async fn stop_opening(&mut self, bot: BotSelector) -> Result<AckReply, AckReply> {
        if let Some(e) = &self.hata {
            return Err(e.clone());
        }
        self.durduruldu.push(bot);
        Ok(AckReply::stopped(8))
    }
    async fn close_all(&mut self, bot: BotSelector) -> Result<AckReply, AckReply> {
        if let Some(e) = &self.hata {
            return Err(e.clone());
        }
        self.kapatildi.push(bot);
        Ok(AckReply::closed(6, 0))
    }
    async fn start(&mut self, bot: BotSelector) -> Result<AckReply, AckReply> {
        if let Some(e) = &self.hata {
            return Err(e.clone());
        }
        self.baslatildi.push(bot);
        Ok(AckReply::started(&[bot]))
    }
    fn status(&self) -> DeskStatus {
        DeskStatus {
            bots_running: vec!["futures".into()],
            open_positions: 8,
            today_net_pct: -1.2,
            skips_today: vec![SkipNote {
                reason: "comboFiltered".into(),
                count: 3,
            }],
            exposure_pct: 14.0,
            effective_bets: 2.0,
            kill_switch_tripped: false,
        }
    }
}

fn key() -> PairingKey {
    PairingKey::from_bytes([3u8; 32])
}
const NOW: u64 = 1_786_700_000_000;
/// Telefonun oturum damgasi.
const TELEFON_EPOCH: u64 = 1_786_699_000_000;
/// Masanin oturum damgasi.
const MASA_EPOCH: u64 = 1_786_699_500_000;

fn telefon_zarfi(seq: u64, cmd: &Command) -> String {
    telefon_zarfi_epoch(TELEFON_EPOCH, seq, cmd)
}

fn telefon_zarfi_epoch(epoch: u64, seq: u64, cmd: &Command) -> String {
    serde_json::to_string(&seal(&key(), "desk-1", epoch, seq, NOW, cmd).unwrap()).unwrap()
}

fn masa(control: SahteMasa) -> DeskLink<SahteMasa> {
    DeskLink::new("desk-1", key(), control, MASA_EPOCH)
}

fn coz(env: &Envelope) -> DeskMessage {
    serde_json::from_str(&env.payload).unwrap()
}

fn isle(link: &mut DeskLink<SahteMasa>, raw: &str) -> Result<Vec<Envelope>, LinkError> {
    block_on(link.handle_raw(NOW, raw))
}

#[test]
fn her_komut_bir_onay_uretir() {
    let mut link = masa(SahteMasa::default());
    let out = isle(
        &mut link,
        &telefon_zarfi(
            1,
            &Command::StopOpening {
                bot: BotSelector::All,
            },
        ),
    )
    .unwrap();
    match coz(&out[0]) {
        DeskMessage::Ack {
            for_seq,
            applied,
            detail,
            reply,
            ..
        } => {
            assert_eq!(for_seq, 1);
            assert!(applied);
            // Kararli kod + sayi; eski telefonlar icin Ingilizce yedek metin.
            assert_eq!(reply, Some(AckReply::stopped(8)));
            assert_eq!(detail, AckReply::stopped(8).english());
        }
        other => panic!("onay bekleniyordu: {other:?}"),
    }
}

/// Uygulanamayan komut da onay dondurur — SESSIZ kalmaz. Telefon "denedim ve
/// olmadi" ile "hic cevap gelmedi"yi ayirt edebilmeli.
#[test]
fn uygulanamayan_komut_da_onay_dondurur() {
    let sahte_masa = SahteMasa {
        hata: Some(AckReply::new(ack_code::START_FAILED)),
        ..Default::default()
    };
    let mut link = masa(sahte_masa);
    let out = isle(
        &mut link,
        &telefon_zarfi(
            1,
            &Command::CloseAll {
                bot: BotSelector::All,
            },
        ),
    )
    .unwrap();
    match coz(&out[0]) {
        DeskMessage::Ack {
            applied, reply, ..
        } => {
            assert!(!applied, "basarisiz komut applied=true dondu");
            assert_eq!(reply.map(|r| r.code).as_deref(), Some(ack_code::START_FAILED));
        }
        other => panic!("onay bekleniyordu: {other:?}"),
    }
}

/// Acil bir kontrolde belirsizlik tehlikelidir: "durdur" acik pozisyonlara
/// DOKUNMAZ.
#[test]
fn durdur_acik_pozisyonlara_dokunmaz() {
    let mut link = masa(SahteMasa::default());
    isle(
        &mut link,
        &telefon_zarfi(
            1,
            &Command::StopOpening {
                bot: BotSelector::Futures,
            },
        ),
    )
    .unwrap();
    assert_eq!(link.control().durduruldu, vec![BotSelector::Futures]);
    assert!(
        link.control().kapatildi.is_empty(),
        "durdur komutu pozisyon kapatti"
    );
}

#[test]
fn hepsini_kapat_gercekten_kapatir() {
    let mut link = masa(SahteMasa::default());
    isle(
        &mut link,
        &telefon_zarfi(
            1,
            &Command::CloseAll {
                bot: BotSelector::All,
            },
        ),
    )
    .unwrap();
    assert_eq!(link.control().kapatildi, vec![BotSelector::All]);
}

/// Dogrulanmayan zarf ne bir eylem ne de bir YANIT uretir. Yanit uretseydi,
/// imzasiz zarf gonderen biri masanin varligini yoklayabilirdi.
#[test]
fn imzasi_tutmayan_zarf_ne_is_yapar_ne_cevap_verir() {
    let mut link = masa(SahteMasa::default());
    let baska = PairingKey::from_bytes([9u8; 32]);
    let env = seal(
        &baska,
        "desk-1",
        TELEFON_EPOCH,
        1,
        NOW,
        &Command::CloseAll {
            bot: BotSelector::All,
        },
    )
    .unwrap();
    let raw = serde_json::to_string(&env).unwrap();
    assert_eq!(isle(&mut link, &raw).unwrap_err(), LinkError::BadSignature);
    assert!(
        link.control().kapatildi.is_empty(),
        "imzasiz komut UYGULANDI"
    );
}

#[test]
fn tekrar_oynatilan_komut_ikinci_kez_uygulanmaz() {
    let mut link = masa(SahteMasa::default());
    let raw = telefon_zarfi(
        4,
        &Command::CloseAll {
            bot: BotSelector::All,
        },
    );
    isle(&mut link, &raw).unwrap();
    assert!(matches!(
        isle(&mut link, &raw).unwrap_err(),
        LinkError::Replay { .. }
    ));
    assert_eq!(
        link.control().kapatildi.len(),
        1,
        "ayni komut iki kez uygulandi"
    );
}

/// S8: TELEFON yeniden baslarsa sayaci sifirdan baslar. Epoch olmadan masa
/// "hepsini kapat" dahil her komutu tekrar sanip dusururdu — yani kill switch,
/// tam olarak kullanilmasi gereken anda calismazdi.
#[test]
fn yeniden_baslayan_telefonun_komutu_dusurulmez() {
    let mut link = masa(SahteMasa::default());

    // Ilk oturum: iki komut.
    isle(&mut link, &telefon_zarfi(1, &Command::Status)).unwrap();
    isle(&mut link, &telefon_zarfi(2, &Command::Status)).unwrap();

    // Telefon yeniden basladi: yeni epoch, sayac yeniden 1.
    let yeni = telefon_zarfi_epoch(
        TELEFON_EPOCH + 1,
        1,
        &Command::CloseAll {
            bot: BotSelector::All,
        },
    );
    isle(&mut link, &yeni).expect("yeniden baslayan telefonun komutu dusuruldu");
    assert_eq!(link.control().kapatildi, vec![BotSelector::All]);

    // Ama gercek bir tekrar hala reddedilir: ayni zarf ikinci kez uygulanmaz.
    assert!(matches!(
        isle(&mut link, &yeni).unwrap_err(),
        LinkError::Replay { .. }
    ));
    assert_eq!(
        link.control().kapatildi.len(),
        1,
        "ayni komut iki kez uygulandi"
    );

    // Ve ESKI oturumdan kaydedilmis bir zarf gecmez.
    let eski = telefon_zarfi_epoch(
        TELEFON_EPOCH,
        99,
        &Command::CloseAll {
            bot: BotSelector::All,
        },
    );
    assert!(matches!(
        isle(&mut link, &eski).unwrap_err(),
        LinkError::Replay { .. }
    ));
    assert_eq!(link.control().kapatildi.len(), 1);
}

#[test]
fn durum_sorgusu_durum_mesaji_dondurur() {
    let mut link = masa(SahteMasa::default());
    let out = isle(&mut link, &telefon_zarfi(1, &Command::Status)).unwrap();
    match coz(&out[0]) {
        DeskMessage::Status(st) => {
            assert_eq!(st.open_positions, 8);
            assert_eq!(st.effective_bets, 2.0);
        }
        other => panic!("durum bekleniyordu: {other:?}"),
    }
}

#[test]
fn kalp_atisi_erken_gonderilmez_zamaninda_gonderilir() {
    let mut link = masa(SahteMasa::default());
    assert!(link.heartbeat_due(NOW).is_some(), "ilk atis gonderilmeli");
    assert!(
        link.heartbeat_due(NOW + 1_000).is_none(),
        "1sn sonra tekrar atis gonderildi"
    );
    assert!(link.heartbeat_due(NOW + HEARTBEAT_EVERY_MS).is_some());
}

/// Giden sayac her mesajda ilerlemeli, yoksa TELEFONUN tekrar korumasi
/// mesajlari dusururdu.
#[test]
fn giden_sayac_her_mesajda_ilerler() {
    let mut link = masa(SahteMasa::default());
    let a = link.heartbeat_due(NOW).unwrap();
    let b = isle(&mut link, &telefon_zarfi(1, &Command::Status))
        .unwrap()
        .remove(0);
    let c = link.heartbeat_due(NOW + HEARTBEAT_EVERY_MS).unwrap();
    assert!(a.seq < b.seq && b.seq < c.seq);
}

#[test]
fn masanin_yaniti_telefon_tarafinda_dogrulanir() {
    let mut link = masa(SahteMasa::default());
    let out = isle(&mut link, &telefon_zarfi(1, &Command::Status)).unwrap();
    let mut telefon_guard = ReplayGuard::new();
    let msg: DeskMessage = open(&key(), &mut telefon_guard, NOW, &out[0]).unwrap();
    assert!(matches!(msg, DeskMessage::Status(_)));
}

/// Bir masanin baslatilmasi da onay uretmeli.
#[test]
fn baslatma_onay_uretir() {
    let mut link = masa(SahteMasa::default());
    let out = isle(
        &mut link,
        &telefon_zarfi(
            1,
            &Command::Start {
                bot: BotSelector::Spot,
            },
        ),
    )
    .unwrap();
    assert!(matches!(
        coz(&out[0]),
        DeskMessage::Ack { applied: true, .. }
    ));
    assert_eq!(link.control().baslatildi, vec![BotSelector::Spot]);
}

/// Role, masanin kendi imzali kalp atisini masaya geri yansitabilir (iki uc
/// ayni anahtari kullanir). Masa epoch'u telefonunkinden buyuk oldugu icin,
/// yansima sayaci ilerletseydi telefonun sonraki "hepsini kapat" komutu tekrar
/// diye duserdi: imza uretemeyen role kill switch'i susturmus olurdu.
#[test]
fn yansitilan_kalp_atisi_kill_switchi_susturamaz() {
    let mut link = masa(SahteMasa::default());
    let beat = link.heartbeat_due(NOW).expect("ilk atis");
    let yansima = serde_json::to_string(&beat).unwrap();
    assert!(isle(&mut link, &yansima).is_err(), "yansima kabul edildi");

    let out = isle(
        &mut link,
        &telefon_zarfi(
            1,
            &Command::CloseAll {
                bot: BotSelector::All,
            },
        ),
    )
    .expect("telefonun komutu reddedildi");
    assert_eq!(out.len(), 1);
    assert_eq!(link.control().kapatildi, vec![BotSelector::All]);
}

/// Zarf baska bir masaya yazilmissa (ayni anahtar iki masada kullanilmis
/// olsa bile) bu masa onu UYGULAMAZ ve sayaci ilerletmez.
#[test]
fn baska_masanin_zarfi_reddedilir() {
    let mut link = masa(SahteMasa::default());
    let env = seal(
        &key(),
        "desk-2",
        TELEFON_EPOCH,
        1,
        NOW,
        &Command::CloseAll {
            bot: BotSelector::All,
        },
    )
    .unwrap();
    let raw = serde_json::to_string(&env).unwrap();
    assert_eq!(isle(&mut link, &raw).unwrap_err(), LinkError::WrongDesk);
    assert!(link.control().kapatildi.is_empty(), "baska masanin komutu uygulandi");

    // Sayac ilerlemedi: ayni seq ile dogru masaya yazilmis komut gecer.
    isle(
        &mut link,
        &telefon_zarfi(
            1,
            &Command::CloseAll {
                bot: BotSelector::All,
            },
        ),
    )
    .expect("dogru masanin komutu reddedildi");
    assert_eq!(link.control().kapatildi, vec![BotSelector::All]);
}

/// Hic kullanilmadan suresi dolan QR anahtari OLUR: QR ekrandan kalktiktan
/// sonra fotografini ceken biri o anahtarla masayi yonetememeli.
#[test]
fn kullanilmadan_suresi_dolan_anahtar_olur() {
    let son = NOW + 120_000;
    let mut link = masa(SahteMasa::default()).with_claim_deadline(son);
    assert!(link.key_live(son));

    let komut = |seq| {
        let env = seal(
            &key(),
            "desk-1",
            TELEFON_EPOCH,
            seq,
            son + 1,
            &Command::CloseAll {
                bot: BotSelector::All,
            },
        )
        .unwrap();
        serde_json::to_string(&env).unwrap()
    };
    assert_eq!(
        block_on(link.handle_raw(son + 1, &komut(1))).unwrap_err(),
        LinkError::KeyRevoked
    );
    assert!(link.control().kapatildi.is_empty(), "olu anahtarla komut uygulandi");
    // Olu anahtar kalp atisi da gondermez.
    assert!(link.heartbeat_due(son + 1).is_none());
    assert!(!link.key_live(son + 1));
}

/// Suresi icinde kullanilan anahtar, QR'in olumunden sonra da yasar: bu,
/// eslesmis telefonun kalici hattidir.
#[test]
fn suresinde_kullanilan_anahtar_yasamaya_devam_eder() {
    let son = NOW + 120_000;
    let mut link = masa(SahteMasa::default()).with_claim_deadline(son);
    isle(&mut link, &telefon_zarfi(1, &Command::Status)).unwrap();

    let sonra = son + 3_600_000;
    let env = seal(&key(), "desk-1", TELEFON_EPOCH, 2, sonra, &Command::Status).unwrap();
    let out = block_on(link.handle_raw(sonra, &serde_json::to_string(&env).unwrap()))
        .expect("eslesmis telefonun komutu reddedildi");
    assert_eq!(out.len(), 1);
    assert!(link.heartbeat_due(sonra).is_some());
}

/// Iptal: anahtar aninda olur, eslesme olmus olsa bile.
#[test]
fn iptal_edilen_anahtar_hicbir_zarfi_kabul_etmez() {
    let mut link = masa(SahteMasa::default());
    link.revoke();
    assert_eq!(
        isle(&mut link, &telefon_zarfi(1, &Command::Status)).unwrap_err(),
        LinkError::KeyRevoked
    );
    assert!(link.heartbeat_due(NOW).is_none());
}
