//! QR ile eşleştirme.
//!
//! # Neden bu kadar dar
//!
//! QR kodunun içinde eşleştirme anahtarı vardır. Yani ekranda duran bir QR,
//! **duran bir kimlik bilgisidir**: onu gören herkes o masayı durdurabilir,
//! açabilir, pozisyonlarını kapatabilir. Omuz üstünden bir fotoğraf yeter.
//!
//! Bu yüzden eşleştirme oturumu:
//!   - **kısa ömürlüdür** (`PAIRING_TTL_MS`),
//!   - **tek kullanımlıktır** — bir telefon eşleşince QR ölür,
//!   - ve süresi dolduğunda **görünür biçimde** ölür; ekranda geçerliymiş gibi
//!     durmaya devam etmez.
//!
//! Son madde, bu projenin tekrar tekrar yediği hata sınıfının aynısıdır:
//! görünmez biçimde geçersizleşen bir şey, geçerli sanılmaya devam eder.

use aleph_link::PairingKey;

/// Bir QR'ın geçerli kaldığı süre (ms). İki dakika, telefonu çıkarıp
/// okutmaya yeter; masanın başından kalkıp dönene kadar yaşamaz.
pub const PAIRING_TTL_MS: u64 = 120_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingError {
    /// QR'ın süresi dolmuş.
    Expired,
    /// Bu QR zaten bir telefonla eşleşmiş.
    AlreadyUsed,
    /// Okunan metin bu masaya ait değil ya da bozuk.
    Invalid,
}

/// Kararlı hata kodu: masaüstü arayüzü bunu `errors.*` altında kullanıcının
/// dilinde gösterir. Buraya Türkçe (ya da herhangi bir dilde) cümle yazmak,
/// sekiz dilli bir arayüzde tek dilli bir hata göstermek demekti.
impl std::fmt::Display for PairingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Expired => write!(f, "pairingExpired"),
            Self::AlreadyUsed => write!(f, "pairingUsed"),
            Self::Invalid => write!(f, "pairingInvalid"),
        }
    }
}

impl std::error::Error for PairingError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Pending,
    Used,
}

/// Masaüstünde açılan bir eşleştirme oturumu.
pub struct PairingSession {
    desk_id: String,
    relay_url: String,
    key: PairingKey,
    created_ms: u64,
    state: State,
}

impl PairingSession {
    pub fn begin(desk_id: impl Into<String>, relay_url: impl Into<String>, now_ms: u64) -> Self {
        Self {
            desk_id: desk_id.into(),
            relay_url: relay_url.into(),
            key: PairingKey::generate(),
            created_ms: now_ms,
            state: State::Pending,
        }
    }

    /// QR'a basılacak metin.
    ///
    /// `alephedge://pair?relay=<url>&desk=<id>&k=<anahtar>`
    ///
    /// Süresi dolmuş ya da kullanılmış bir oturum **metin üretmez**. Ekranda
    /// ölü bir QR bırakmak, kullanıcıya çalışmayan bir şeyi çalışıyor gibi
    /// göstermektir.
    pub fn qr_payload(&self, now_ms: u64) -> Result<String, PairingError> {
        self.check(now_ms)?;
        Ok(format!(
            "alephedge://pair?relay={}&desk={}&k={}",
            self.relay_url,
            self.desk_id,
            self.key.to_pairing_string()
        ))
    }

    fn check(&self, now_ms: u64) -> Result<(), PairingError> {
        if self.state == State::Used {
            return Err(PairingError::AlreadyUsed);
        }
        if now_ms.saturating_sub(self.created_ms) > PAIRING_TTL_MS {
            return Err(PairingError::Expired);
        }
        Ok(())
    }

    /// Kalan süre (ms). Arayüz bunu geri sayım olarak gösterir — kullanıcı
    /// QR'ın ne zaman öleceğini görmeli.
    pub fn remaining_ms(&self, now_ms: u64) -> u64 {
        PAIRING_TTL_MS.saturating_sub(now_ms.saturating_sub(self.created_ms))
    }

    /// Bir telefon eşleşti: oturumu kapatır ve kalıcı anahtarı verir.
    ///
    /// Masaüstü bunu, anahtarı ELİNDE TUTTUĞUNU kanıtlayan ilk doğrulanmış
    /// zarfı aldığında çağırır. Kamerayı tutan o olmadığı için "telefon QR'ı
    /// okudu" anını başka türlü bilemez.
    pub fn complete(&mut self, now_ms: u64) -> Result<PairingKey, PairingError> {
        self.check(now_ms)?;
        self.state = State::Used;
        Ok(self.key.clone())
    }

    /// Masanın KENDİ röle oturumu için anahtar.
    ///
    /// `complete` ile karıştırılmamalı: bu, eşleştirmenin karşı tarafa
    /// geçmesi değil, aynı masanın kendi bağlantısını kurması. Telefon röleye
    /// ancak masa zaten bağlıysa eklenebildiği için (`relay/hub.go`) masa,
    /// QR hâlâ ekrandayken bu anahtarla bağlanmak ZORUNDA — dolayısıyla burada
    /// QR harcanmaz. QR'ı harcayan tek şey karşı taraftan gelen imzalı bir
    /// zarftır.
    pub fn desk_session_key(&self) -> PairingKey {
        self.key.clone()
    }
}

// `ScannedPairing` ve `parse_qr` ORTAK crate'e (aleph-link) taşındı: QR'ı hem
// masaüstü üretiyor hem telefon okuyor. Burada bırakılsaydı telefon ya bu
// masaüstü crate'ini çekmek ya da kendi kopyasını yazmak zorunda kalırdı — ki
// ikinci bir uygulama, bu depoda üç kez yanmış hatanın tohumudur.
pub use aleph_link::{parse_qr, ScannedPairing};

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_786_700_000_000;

    #[test]
    fn qr_metni_gidip_gelir() {
        let s = PairingSession::begin("desk-1", "wss://ribqa.com/link", NOW);
        let text = s.qr_payload(NOW).unwrap();
        let got = parse_qr(&text).unwrap();
        assert_eq!(got.desk_id, "desk-1");
        assert_eq!(got.relay_url, "wss://ribqa.com/link");
        // Okunan anahtar, masanin urettigiyle ayni olmali.
        let k = PairingKey::from_pairing_string(&got.key_str).unwrap();
        assert_eq!(k.to_pairing_string(), s.key.to_pairing_string());
    }

    /// Ekranda duran bir QR, duran bir kimlik bilgisidir. Suresi dolunca metin
    /// URETILMEZ — olu bir QR ekranda calisiyor gibi durmaz.
    #[test]
    fn suresi_dolan_qr_metin_uretmez() {
        let s = PairingSession::begin("desk-1", "wss://x", NOW);
        assert!(s.qr_payload(NOW + PAIRING_TTL_MS).is_ok());
        assert_eq!(
            s.qr_payload(NOW + PAIRING_TTL_MS + 1).unwrap_err(),
            PairingError::Expired
        );
    }

    #[test]
    fn suresi_dolan_qr_esleşemez() {
        let mut s = PairingSession::begin("desk-1", "wss://x", NOW);
        assert_eq!(
            s.complete(NOW + PAIRING_TTL_MS + 1).unwrap_err(),
            PairingError::Expired
        );
    }

    /// Tek kullanimlik: bir telefon eslesince QR olur. Ikinci bir telefonun
    /// ayni QR ile girebilmesi, fotografini cekenin de girebilmesi demektir.
    #[test]
    fn qr_tek_kullanimliktir() {
        let mut s = PairingSession::begin("desk-1", "wss://x", NOW);
        s.complete(NOW).unwrap();
        assert_eq!(s.complete(NOW).unwrap_err(), PairingError::AlreadyUsed);
        assert_eq!(s.qr_payload(NOW).unwrap_err(), PairingError::AlreadyUsed);
    }

    #[test]
    fn geri_sayim_dogru() {
        let s = PairingSession::begin("desk-1", "wss://x", NOW);
        assert_eq!(s.remaining_ms(NOW), PAIRING_TTL_MS);
        assert_eq!(s.remaining_ms(NOW + 30_000), PAIRING_TTL_MS - 30_000);
        assert_eq!(s.remaining_ms(NOW + PAIRING_TTL_MS * 2), 0);
    }

    #[test]
    fn bozuk_qr_reddedilir() {
        for kotu in [
            "",
            "https://example.com",
            "alephedge://pair?",
            "alephedge://pair?relay=x",
            "alephedge://pair?relay=x&desk=d",
            "alephedge://pair?relay=&desk=d&k=abc",
        ] {
            assert!(parse_qr(kotu).is_err(), "kabul edildi: {kotu:?}");
        }
    }

    /// Cozulemeyen bir anahtar, ESLESME aninda yakalanmali. Yoksa telefon
    /// "eslestim" der ve hata ilk komutta, yanlis yerde patlar.
    #[test]
    fn cozulemeyen_anahtar_esleşme_aninda_yakalanir() {
        let kotu = "alephedge://pair?relay=wss://x&desk=d1&k=bu-anahtar-degil";
        assert_eq!(
            parse_qr(kotu).unwrap_err(),
            aleph_link::LinkError::BadPairingString
        );
    }

    /// Bilinmeyen alan sessizce yutulmaz: ileride eklenen bir alani eski bir
    /// telefonun gormezden gelmesi, yarisi uygulanmis bir protokoldur.
    #[test]
    fn bilinmeyen_alan_sessizce_yutulmaz() {
        let s = PairingSession::begin("d1", "wss://x", NOW);
        let text = format!("{}&yeni_alan=1", s.qr_payload(NOW).unwrap());
        assert_eq!(
            parse_qr(&text).unwrap_err(),
            aleph_link::LinkError::BadPairingString
        );
    }

    /// Her oturum FARKLI bir anahtar uretmeli.
    #[test]
    fn her_oturum_farkli_anahtar_uretir() {
        let a = PairingSession::begin("d1", "wss://x", NOW);
        let b = PairingSession::begin("d1", "wss://x", NOW);
        assert_ne!(
            a.key.to_pairing_string(),
            b.key.to_pairing_string(),
            "iki oturum ayni anahtari uretti"
        );
    }

    #[test]
    fn hatalar_cevrilebilir_kararli_kodlardir() {
        // Arayüz bunları `errors.*` altında yerelleştirir; cümle değil kod.
        assert_eq!(PairingError::Expired.to_string(), "pairingExpired");
        assert_eq!(PairingError::AlreadyUsed.to_string(), "pairingUsed");
        assert_eq!(PairingError::Invalid.to_string(), "pairingInvalid");
    }
}
