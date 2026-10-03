//! Röle bağlantısının yeniden kurulma politikası.
//!
//! # Neden bu bir politika, bir `sleep` değil
//!
//! Masa, röleye giden bağlantıyı kaybettiğinde kullanıcı bunu **göremez** —
//! telefonunda "masan kapalı" yazar ama bilgisayarın başında böyle bir işaret
//! yoktur. Yani yeniden bağlanma davranışı, kullanıcının farkına varmadığı bir
//! yerde çalışır ve tam bu yüzden yazılı bir kural olmalıdır.
//!
//! İki hata sınıfı vardır ve **ikisine aynı şekilde davranmak** bu projenin
//! tekrar tekrar yediği hatadır:
//!
//! - **Geçici** (ağ yok, röle yeniden başlıyor): sonsuza kadar denenir. Pes
//!   eden bir masa, sessizce ölmüş bir masadır.
//! - **Kalıcı** (üyelik geçersiz, masa kimliği çakışıyor, protokol sürümü
//!   uyuşmuyor): denemek işe yaramaz. Sonsuza kadar denemek yalnızca röleyi
//!   yakar ve gerçek sorunu gizler — kullanıcı "bağlanıyor" görüntüsüne bakıp
//!   sorunun kendiliğinden geçmesini bekler.

use std::time::Duration;

/// İlk yeniden deneme gecikmesi.
pub const BACKOFF_START: Duration = Duration::from_secs(1);
/// Üst sınır. Bunun ötesinde beklemek, ağ geri geldiğinde masayı gereksiz
/// uzun süre kapalı tutar.
pub const BACKOFF_MAX: Duration = Duration::from_secs(60);

/// Bağlantının neden koptuğu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disconnect {
    /// Ağ yok, DNS çözülmedi, röle kapalı, soket düştü.
    Transient(String),
    /// Üyelik doğrulanmadı.
    Unauthorized,
    /// Bu masa kimliği zaten bağlı (başka bir makine ya da eski bir süreç).
    DeskConflict,
    /// Röle bizim konuşmadığımız bir sürümü konuşuyor.
    VersionMismatch,
    /// Eşleştirme QR'ı hiçbir telefon onu kullanmadan öldü: anahtar geçersiz
    /// (`DeskLink::key_live`). Yeniden bağlanmak işe yaramaz; yeni bir QR gerekir.
    PairingExpired,
}

impl Disconnect {
    /// Tekrar denemek anlamlı mı.
    fn retryable(&self) -> bool {
        matches!(self, Disconnect::Transient(_))
    }
}

/// Politikanın bir kopmadan sonra ne yapılacağına dair kararı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Şu kadar bekleyip tekrar dene.
    RetryIn(Duration),
    /// Denemeyi bırak ve kullanıcıya BU mesajı göster.
    ///
    /// Mesaj arayüzde görünür; "bağlanıyor…" diye dönen bir çark değil.
    Stop { reason: Disconnect, message: String },
}

/// Yeniden bağlanma durumu.
#[derive(Debug, Default)]
pub struct Reconnect {
    attempt: u32,
}

impl Reconnect {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bağlantı kurulduğunda çağrılır: sayaç sıfırlanır.
    ///
    /// Sıfırlamayı unutmak, uzun süre çalışan bir masanın bir kez kopunca
    /// doğrudan 60 saniye beklemesine yol açardı.
    pub fn connected(&mut self) {
        self.attempt = 0;
    }

    /// Kopma sonrası kararı verir.
    pub fn on_disconnect(&mut self, why: &Disconnect) -> Next {
        if !why.retryable() {
            return Next::Stop {
                reason: why.clone(),
                // Kararlı kod; arayüz `errors.*` altında, kullanıcının
                // dilinde ve ne yapacağını söyleyerek gösterir.
                message: match why {
                    Disconnect::Unauthorized => "linkUnauthorized".into(),
                    Disconnect::DeskConflict => "linkDeskConflict".into(),
                    Disconnect::VersionMismatch => "linkVersionMismatch".into(),
                    Disconnect::PairingExpired => "pairingExpired".into(),
                    Disconnect::Transient(_) => unreachable!(),
                },
            };
        }

        // Üstel geri çekilme, üst sınırda sabitlenir. Rastgelelik YOK: tek bir
        // masa için sürü etkisi diye bir şey olmadığı gibi, deterministik
        // olması davranışı test edilebilir kılıyor.
        let secs = BACKOFF_START
            .as_secs()
            .saturating_mul(1u64 << self.attempt.min(6));
        self.attempt = self.attempt.saturating_add(1);
        Next::RetryIn(Duration::from_secs(secs.min(BACKOFF_MAX.as_secs())))
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gecici() -> Disconnect {
        Disconnect::Transient("ag yok".into())
    }

    /// Pes eden bir masa, sessizce olmus bir masadir: gecici hata sonsuza
    /// kadar denenir.
    #[test]
    fn gecici_hata_sonsuza_kadar_denenir() {
        let mut r = Reconnect::new();
        for _ in 0..200 {
            match r.on_disconnect(&gecici()) {
                Next::RetryIn(_) => {}
                Next::Stop { .. } => panic!("gecici hatada pes etti"),
            }
        }
    }

    #[test]
    fn geri_cekilme_ustel_ve_sinirli() {
        let mut r = Reconnect::new();
        let mut gorulen = Vec::new();
        for _ in 0..10 {
            if let Next::RetryIn(d) = r.on_disconnect(&gecici()) {
                gorulen.push(d.as_secs());
            }
        }
        assert_eq!(&gorulen[..7], &[1, 2, 4, 8, 16, 32, 60]);
        // Ust sinirda sabitlenir, sonsuza kadar buyumez.
        assert!(gorulen.iter().all(|&s| s <= BACKOFF_MAX.as_secs()));
    }

    /// Sifirlamayi unutmak, uzun sure calisan bir masanin bir kez kopunca
    /// dogrudan 60 saniye beklemesine yol acardi.
    #[test]
    fn baglanti_kurulunca_sayac_sifirlanir() {
        let mut r = Reconnect::new();
        for _ in 0..5 {
            r.on_disconnect(&gecici());
        }
        r.connected();
        assert_eq!(r.attempt(), 0);
        assert_eq!(r.on_disconnect(&gecici()), Next::RetryIn(BACKOFF_START));
    }

    /// Kalici hatada sonsuza kadar denemek roleyi yakar ve GERCEK sorunu
    /// gizler: kullanici "baglaniyor" carkina bakip beklemeye devam eder.
    #[test]
    fn kalici_hatalar_denenmez_ve_sebebini_soyler() {
        for why in [
            Disconnect::Unauthorized,
            Disconnect::DeskConflict,
            Disconnect::VersionMismatch,
            Disconnect::PairingExpired,
        ] {
            let mut r = Reconnect::new();
            match r.on_disconnect(&why) {
                Next::Stop { reason, message } => {
                    assert_eq!(reason, why);
                    assert!(!message.is_empty(), "sebep bos mesajla dondu: {why:?}");
                    // Her sebebin kendi kararlı kodu var; ne yapılacağını
                    // söyleyen metin arayüzün çevirisinde (errors.link*).
                    let beklenen = match why {
                        Disconnect::Unauthorized => "linkUnauthorized",
                        Disconnect::DeskConflict => "linkDeskConflict",
                        Disconnect::PairingExpired => "pairingExpired",
                        _ => "linkVersionMismatch",
                    };
                    assert_eq!(message, beklenen);
                }
                Next::RetryIn(_) => panic!("kalici hata tekrar denendi: {why:?}"),
            }
        }
    }

    /// Masa cakismasi OZELLIKLE tekrar denenmemeli: iki surec sirayla
    /// baglanmaya calisirsa ikisi de calismaz ve kullanici hicbirini gormez.
    #[test]
    fn masa_cakismasi_tekrar_denenmez() {
        let mut r = Reconnect::new();
        assert!(matches!(
            r.on_disconnect(&Disconnect::DeskConflict),
            Next::Stop { .. }
        ));
    }
}
