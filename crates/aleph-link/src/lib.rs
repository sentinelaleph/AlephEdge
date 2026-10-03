//! Aleph Edge — masaüstü ile telefon arasındaki imzalı mesaj protokolü.
//!
//! # Neden tek bir crate
//!
//! Telefon, kullanıcının kendi bilgisayarında çalışan Aleph Edge'e bağlanır ve
//! ona komut gönderir: durumu göster, botu durdur, hepsini kapat. Aradaki yol
//! bizim işlettiğimiz bir röleden geçer, çünkü ev bilgisayarı NAT arkasındadır.
//!
//! **Röle mesajı taşır, üretemez.** Bunu sağlayan şey burasıdır: her komut,
//! eşleştirme sırasında kurulan ortak anahtarla imzalanır ve rölenin o anahtarı
//! yoktur.
//!
//! Bu mantığın İKİ uygulaması olsaydı — masaüstünde Rust, telefonda TypeScript —
//! ikisi zamanla ayrışırdı. Bu depoda aynı hata üç kez yandı: tokenize-hisse
//! izleyicisi kendi eski liste kopyasını taşıdı ve günlerce `leak=0` dedi; short
//! kill rule kendi allowlist kopyasını taşıdı, DGBUSDT'de yanlış alarm verdi ve
//! mandalını kilitledi; ve koruyucu-çıkış sayacı, kesen sorgunun ikinci bir
//! kopyası olacakken tek bir SQL sabitine bağlandı. Aynı hatayı bir de güvenlik
//! sınırında yapmamak için protokol tek yerde durur ve iki uç da onu linkler.
//! Tauri'yi seçmemizin asıl gerekçesi budur.

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Protokol sürümü. Uyuşmayan sürüm reddedilir — sessizce yorumlanmaz.
///
/// 1 → 2: zarfa `epoch` eklendi (bkz. `ReplayGuard`). Alan eklemek eski bir
/// eşin zarfını çözülemez yaptığı için sürüm de artırıldı; eksik alanı sessizce
/// sıfır saymak, iki ucun farklı protokolleri aynı sanması olurdu.
pub const PROTOCOL_VERSION: u8 = 2;

/// Bir zarfın kabul edileceği azami saat sapması (ms).
///
/// Telefon ile bilgisayarın saatleri birbirini tutmaz; 90 saniye, makul bir
/// sapmaya izin verirken kaydedilmiş bir zarfın günler sonra oynatılmasını
/// engeller. Tek başına yeterli değildir — asıl tekrar koruması `seq`.
pub const MAX_CLOCK_SKEW_MS: u64 = 90_000;

/// Eşleştirme anahtarı: telefon ile masaüstünün paylaştığı sır.
///
/// QR ile aktarılır: masaüstü ekranda gösterir, telefon kamerayla okur. Rölenin
/// bu anahtara erişimi yoktur ve olmamalıdır.
#[derive(Clone)]
pub struct PairingKey([u8; 32]);

impl PairingKey {
    pub fn from_bytes(b: [u8; 32]) -> Self {
        Self(b)
    }

    /// Yeni bir anahtar üretir (masaüstü, eşleştirme başlarken çağırır).
    pub fn generate() -> Self {
        use rand_core::RngCore;
        let mut b = [0u8; 32];
        rand_core::OsRng.fill_bytes(&mut b);
        Self(b)
    }

    /// QR'a basılacak metin.
    pub fn to_pairing_string(&self) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        URL_SAFE_NO_PAD.encode(self.0)
    }

    pub fn from_pairing_string(s: &str) -> Result<Self, LinkError> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let raw = URL_SAFE_NO_PAD
            .decode(s.trim())
            .map_err(|_| LinkError::BadPairingString)?;
        let b: [u8; 32] = raw.try_into().map_err(|_| LinkError::BadPairingString)?;
        Ok(Self(b))
    }
}

// Anahtar hiçbir log satırına, hiçbir hata mesajına düşmemeli.
impl std::fmt::Debug for PairingKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PairingKey(<gizli>)")
    }
}

/// Telefondan masaüstüne giden komutlar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Masanın anlık durumunu iste.
    Status,
    /// Yeni pozisyon açmayı durdur. Açık pozisyonlar çalışmaya devam eder.
    ///
    /// `CloseAll`'dan AYRI tutulur ve asla onunla birleştirilmez. Acil bir
    /// kontrolde "kill" kelimesi fazla belirsizdir: kullanıcı "durdur"a basıp
    /// açık pozisyonlarının da kapandığını sanırsa, ya da tersini sanırsa,
    /// ikisi de pahalıdır. Masaüstünde bu ayrım zaten var (`close_on_stop`).
    StopOpening { bot: BotSelector },
    /// Açık pozisyonları da kapat.
    CloseAll { bot: BotSelector },
    /// Botu tekrar başlat.
    Start { bot: BotSelector },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BotSelector {
    All,
    Futures,
    Spot,
    Pump,
}

/// Masaüstünden telefona giden mesajlar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "msg", rename_all = "snake_case")]
pub enum DeskMessage {
    /// Düzenli kalp atışı. Telefon bunun KESİLMESİNİ izler.
    ///
    /// Uygulamanın en değerli bildirimi "yeni sinyal" değil, "masan durdu"
    /// olduğu için bu mesaj protokolün en önemli parçasıdır. Bilgisayar uyursa,
    /// kapak kapanırsa ya da Windows güncelleme için yeniden başlarsa masa
    /// durur ve kullanıcı çalıştığını sanmaya devam eder.
    Heartbeat { at_ms: u64 },
    /// Durum yanıtı.
    Status(DeskStatus),
    /// Bir komutun UYGULANDIĞINA dair onay.
    ///
    /// Komut, bu onay gelmeden başarılı sayılmaz. Basıp yeşil tik gören ama
    /// aslında durmamış bir kullanıcı, hiç kill switch olmamasından daha kötü
    /// durumdadır.
    Ack {
        /// Onaylanan komutun `seq`'i.
        for_seq: u64,
        applied: bool,
        at_ms: u64,
        /// İngilizce yedek metin: `reply` alanını tanımayan eski telefonlar
        /// bunu gösterir. Yeni telefonlar `reply`'ı kendi dilinde çizer.
        detail: String,
        /// Sonucun yerelleştirilebilir biçimi: kararlı kod + sayısal
        /// parametreler. Eski bir masa göndermez (`None`); eski bir telefon
        /// alanı yok sayar — bu yüzden sürüm artırılmadı.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply: Option<AckReply>,
    },
}

/// Bir onayın kararlı kodu ve sayısal parametreleri.
///
/// # Neden metin değil
///
/// Masa onayı eskiden Türkçe bir cümle olarak yolluyordu ("başlatıldı: spot").
/// Telefon o cümleyi ne çevirebilir ne de içindeki sayıyı okuyabilirdi; masanın
/// dili telefonun diline dayatılıyordu. Kod + sayı, her ucun kendi dilinde
/// çizmesini sağlar.
///
/// Kablo biçimi düzdür: `{"code":"stopped","open":3}`. Kod bir `String`,
/// enum DEĞİL: yeni bir kod ekleyen masa, onu tanımayan bir telefonda onayın
/// tamamını çözülemez yapmamalı — telefon bilinmeyen kodda `detail`'e düşer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AckReply {
    pub code: String,
    #[serde(flatten)]
    pub params: std::collections::BTreeMap<String, i64>,
}

/// Onay kodları. Masa ve telefon aynı sabitleri kullanır.
pub mod ack_code {
    /// Yeni pozisyon açılmıyor; `open` açık pozisyon yönetilmeye devam ediyor.
    pub const STOPPED: &str = "stopped";
    /// `closed` pozisyon kapatıldı.
    pub const CLOSED: &str = "closed";
    /// `closed` kapatıldı, `pending` tanesi fiyatlanamadı; sonraki turda.
    pub const CLOSED_PARTIAL: &str = "closed_partial";
    /// Başlatıldı; `futures` / `spot` / `pump` = 1 başlayan botları işaretler.
    pub const STARTED: &str = "started";
    /// Günlük zarar durdurucusu devrede; ertesi UTC gününe kadar başlamaz.
    pub const KILL_SWITCH_TRIPPED: &str = "kill_switch_tripped";
    /// Başlatılacak yapılandırılmış bot yok.
    pub const NOTHING_TO_START: &str = "nothing_to_start";
    /// Pump botu yalnızca "ambitious" seviyesinde çalışır.
    pub const PUMP_NEEDS_AMBITIOUS: &str = "pump_needs_ambitious";
    /// Başlatma başka bir sebeple olmadı.
    pub const START_FAILED: &str = "start_failed";
}

impl AckReply {
    pub fn new(code: &str) -> Self {
        Self {
            code: code.to_string(),
            params: Default::default(),
        }
    }

    pub fn with(mut self, name: &str, value: i64) -> Self {
        self.params.insert(name.to_string(), value);
        self
    }

    pub fn param(&self, name: &str) -> i64 {
        self.params.get(name).copied().unwrap_or(0)
    }

    pub fn stopped(open: u64) -> Self {
        Self::new(ack_code::STOPPED).with("open", open as i64)
    }

    /// `pending == 0` ise düz "kapatıldı"; değilse kalanı SÖYLER — "hepsi
    /// kapandı" demek, fiyatlanamayan pozisyonu kullanıcıdan saklamak olurdu.
    pub fn closed(closed: u64, pending: u64) -> Self {
        if pending == 0 {
            Self::new(ack_code::CLOSED).with("closed", closed as i64)
        } else {
            Self::new(ack_code::CLOSED_PARTIAL)
                .with("closed", closed as i64)
                .with("pending", pending as i64)
        }
    }

    pub fn started(bots: &[BotSelector]) -> Self {
        let mut r = Self::new(ack_code::STARTED);
        for b in bots {
            if let Some(name) = b.started_param() {
                r = r.with(name, 1);
            }
        }
        r
    }

    /// İngilizce metin: eski telefonlar için `detail` alanı ve henüz dili
    /// olmayan telefon arayüzü bunu kullanır. Bilinmeyen kod, kodun kendisi
    /// olarak döner — hiçbir zaman boş değil.
    pub fn english(&self) -> String {
        let plural = |n: i64| if n == 1 { "" } else { "s" };
        match self.code.as_str() {
            ack_code::STOPPED => {
                let n = self.param("open");
                format!(
                    "not opening new positions; {n} open position{} still managed",
                    plural(n)
                )
            }
            ack_code::CLOSED => {
                let n = self.param("closed");
                format!("{n} position{} closed", plural(n))
            }
            ack_code::CLOSED_PARTIAL => {
                let (n, p) = (self.param("closed"), self.param("pending"));
                format!(
                    "{n} position{} closed, {p} could not be priced and will close next round",
                    plural(n)
                )
            }
            ack_code::STARTED => {
                let names: Vec<&str> = ["futures", "spot", "pump"]
                    .into_iter()
                    .filter(|b| self.param(b) == 1)
                    .collect();
                format!("started: {}", names.join(", "))
            }
            ack_code::KILL_SWITCH_TRIPPED => {
                "daily loss stop is active; cannot start until the next UTC day".into()
            }
            ack_code::NOTHING_TO_START => "no configured bot to start".into(),
            ack_code::PUMP_NEEDS_AMBITIOUS => "the pump bot runs only on the ambitious level".into(),
            ack_code::START_FAILED => "the bot could not be started".into(),
            other => other.to_string(),
        }
    }
}

impl BotSelector {
    /// `started` onayındaki parametre adı (`All` tek bir bot değildir).
    fn started_param(self) -> Option<&'static str> {
        match self {
            BotSelector::Futures => Some("futures"),
            BotSelector::Spot => Some("spot"),
            BotSelector::Pump => Some("pump"),
            BotSelector::All => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeskStatus {
    pub bots_running: Vec<String>,
    pub open_positions: u32,
    pub today_net_pct: f64,
    /// Bugün girilmeyen kurulumlar ve sebepleri — masaüstündeki atlama notları.
    pub skips_today: Vec<SkipNote>,
    /// Açık pozisyonların hesaba oranı (%). Winrate DEĞİL.
    pub exposure_pct: f64,
    /// Bu pozisyonlar kaç bağımsız bahis gibi davranıyor.
    pub effective_bets: f64,
    pub kill_switch_tripped: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkipNote {
    pub reason: String,
    pub count: u32,
}

/// Kablodan geçen imzalı zarf. Röle bunu OPAK olarak taşır.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u8,
    pub desk_id: String,
    /// Gönderenin OTURUM damgası — yeniden başlamayı tekrar saldırısından
    /// ayırır. Ayrıntı: `ReplayGuard`.
    pub epoch: u64,
    /// Gönderen başına, oturum içinde monoton artan sayaç.
    pub seq: u64,
    pub at_ms: u64,
    /// Serileştirilmiş yük (Command ya da DeskMessage).
    pub payload: String,
    /// Kanonik baytlar üzerinden HMAC-SHA256, base64.
    pub sig: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    BadPairingString,
    VersionMismatch {
        got: u8,
        want: u8,
    },
    BadSignature,
    /// Zarf, kabul edilen saat penceresinin dışında.
    Stale {
        at_ms: u64,
        now_ms: u64,
    },
    /// Zarf, bu göndericiden görülen son (epoch, seq) çiftinin gerisinde ya da
    /// aynısında — tekrar denemesi.
    Replay {
        epoch: u64,
        seq: u64,
        last_epoch: u64,
        last_seq: u64,
    },
    Malformed(String),
    /// Zarf başka bir masaya yazılmış (`desk_id` bu masanınki değil).
    WrongDesk,
    /// Eşleştirme anahtarı hiç kullanılmadan süresi doldu ya da eşleştirme
    /// iptal edildi: bu anahtarla imzalanmış hiçbir zarf artık kabul edilmez.
    KeyRevoked,
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadPairingString => write!(f, "eşleştirme dizesi geçersiz"),
            Self::VersionMismatch { got, want } => {
                write!(f, "protokol sürümü uyuşmuyor: {got}, beklenen {want}")
            }
            Self::BadSignature => write!(f, "imza doğrulanamadı"),
            Self::Stale { at_ms, now_ms } => {
                write!(f, "zarf penceresi dışında (zarf {at_ms}, şimdi {now_ms})")
            }
            Self::Replay {
                epoch,
                seq,
                last_epoch,
                last_seq,
            } => write!(
                f,
                "tekrar denemesi: gelen ({epoch}, {seq}), son görülen ({last_epoch}, {last_seq})"
            ),
            Self::Malformed(m) => write!(f, "bozuk zarf: {m}"),
            Self::WrongDesk => write!(f, "zarf başka bir masaya ait"),
            Self::KeyRevoked => write!(f, "eşleştirme anahtarı geçersiz kılındı"),
        }
    }
}

impl std::error::Error for LinkError {}

/// İmzalanan kanonik baytlar.
///
/// Alanlar sabit sırayla ve ayraçla birleştirilir. JSON serileştirmesine
/// GÜVENİLMEZ: alan sırası ya da boşluk değişirse imza sessizce bozulurdu ve
/// hata "imza tutmadı" diye görünüp asıl sebebi saklardı.
fn canonical_bytes(
    v: u8,
    desk_id: &str,
    epoch: u64,
    seq: u64,
    at_ms: u64,
    payload: &str,
) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&[v]);
    b.push(0x1f);
    b.extend_from_slice(desk_id.as_bytes());
    b.push(0x1f);
    // Epoch de imzanın içindedir: dışında kalsaydı araya giren biri epoch'u
    // büyütüp kaydettiği bir zarfı "yeni oturum" gibi geçirebilirdi.
    b.extend_from_slice(&epoch.to_be_bytes());
    b.push(0x1f);
    b.extend_from_slice(&seq.to_be_bytes());
    b.push(0x1f);
    b.extend_from_slice(&at_ms.to_be_bytes());
    b.push(0x1f);
    b.extend_from_slice(payload.as_bytes());
    b
}

fn sign(key: &PairingKey, bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let mut mac = HmacSha256::new_from_slice(&key.0).expect("HMAC her anahtar boyunu kabul eder");
    mac.update(bytes);
    STANDARD.encode(mac.finalize().into_bytes())
}

/// Bir yükü imzalı zarfa koyar.
///
/// `epoch`, gönderenin BU oturumunun damgası; `seq` o oturum içindeki sayaç.
/// İkisinin birlikte taşınması, yeniden başlayan bir ucun tekrar saldırganı
/// sanılmaması içindir (bkz. `ReplayGuard`).
pub fn seal<T: Serialize>(
    key: &PairingKey,
    desk_id: &str,
    epoch: u64,
    seq: u64,
    at_ms: u64,
    payload: &T,
) -> Result<Envelope, LinkError> {
    let payload =
        serde_json::to_string(payload).map_err(|e| LinkError::Malformed(e.to_string()))?;
    let sig = sign(
        key,
        &canonical_bytes(PROTOCOL_VERSION, desk_id, epoch, seq, at_ms, &payload),
    );
    Ok(Envelope {
        v: PROTOCOL_VERSION,
        desk_id: desk_id.to_string(),
        epoch,
        seq,
        at_ms,
        payload,
        sig,
    })
}

/// Alıcı tarafın tekrar (replay) durumu. Gönderen başına tutulur.
///
/// # Neden sayaç tek başına yetmedi
///
/// İki uç da sayacı sıfırdan başlatıyor ve hiçbiri onu diske yazmıyordu. Masa
/// yeniden başlayınca (Windows güncellemesi, kapak, çökme) gönderdiği her kalp
/// atışı `seq <= last` olduğu için TEKRAR sayılıyor, telefon da sonsuza kadar
/// "masan sustu" gösteriyordu: protokol, kalp atışının yakalamak için var
/// olduğu arızayı kendi üretiyordu. Telefon yeniden başlarsa daha kötüsü
/// oluyordu — masa "hepsini kapat" dahil her komutu düşürüyordu.
///
/// # Neden sayacı diske yazmak değil, epoch
///
/// Alıcı, yeniden başlayan bir eşi tekrar saldırganından **ayırt edemez**. O
/// hâlde ayrımı alıcının tahminine bırakmak yerine göndericinin imzalı olarak
/// BEYAN etmesi gerekir: `epoch` kanonik baytların içindedir, yani anahtarı
/// olmayan biri yeni bir epoch üretemez. Diske yazılan bir sayaç ise aynı
/// arızayı geri getirmeye açıktı — dosya silinir ya da bozulursa (taşınan
/// profil, yeniden kurulum, telefonun uygulama verisini temizlemesi) sayaç
/// sıfırlanır ve hat bu kez kurtulma yolu olmadan kilitlenirdi; ayrıca iki saf
/// protokol crate'ine dosya G/Ç'si sokmak gerekirdi.
///
/// Kural: epoch yalnızca İLERİ gider. Kaydedilmiş bir zarfın epoch'u daima
/// geçmişte kaldığı için eski bir oturumu yeniden oynatmak hâlâ imkânsız.
/// Epoch, gönderenin oturum başlangıç zamanıdır; yani buradaki saat varsayımı,
/// `MAX_CLOCK_SKEW_MS`'in zaten yaptığı varsayımdan fazlası değil.
#[derive(Debug, Default, Clone)]
pub struct ReplayGuard {
    /// Kabul edilen son (epoch, seq).
    last: Option<(u64, u64)>,
}

impl ReplayGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Yalnızca kabul edilen zarflarda ilerletilir.
    fn accept(&mut self, epoch: u64, seq: u64) -> Result<(), LinkError> {
        if let Some((last_epoch, last_seq)) = self.last {
            // Yeni bir oturum (epoch büyük) sayacı sıfırdan başlatabilir; aynı
            // oturum içinde sayaç ilerlemek ZORUNDA; eski bir oturum hiçbir
            // koşulda kabul edilmez.
            let ileri = epoch > last_epoch || (epoch == last_epoch && seq > last_seq);
            if !ileri {
                return Err(LinkError::Replay {
                    epoch,
                    seq,
                    last_epoch,
                    last_seq,
                });
            }
        }
        self.last = Some((epoch, seq));
        Ok(())
    }

    pub fn last_seq(&self) -> Option<u64> {
        self.last.map(|(_, seq)| seq)
    }

    pub fn last_epoch(&self) -> Option<u64> {
        self.last.map(|(epoch, _)| epoch)
    }
}

/// Zarfı açar: sürümü, imzayı, saat penceresini ve tekrar sayacını doğrular.
///
/// Sıra önemlidir — imza doğrulanmadan hiçbir alana güvenilmez.
pub fn open<T: for<'de> Deserialize<'de>>(
    key: &PairingKey,
    guard: &mut ReplayGuard,
    now_ms: u64,
    env: &Envelope,
) -> Result<T, LinkError> {
    if env.v != PROTOCOL_VERSION {
        return Err(LinkError::VersionMismatch {
            got: env.v,
            want: PROTOCOL_VERSION,
        });
    }

    let expected = sign(
        key,
        &canonical_bytes(
            env.v,
            &env.desk_id,
            env.epoch,
            env.seq,
            env.at_ms,
            &env.payload,
        ),
    );
    // Sabit zamanlı karşılaştırma: `==` imzanın ne kadarının tuttuğunu zamanla
    // sızdırır.
    if expected.as_bytes().ct_eq(env.sig.as_bytes()).unwrap_u8() != 1 {
        return Err(LinkError::BadSignature);
    }

    let skew = now_ms.abs_diff(env.at_ms);
    if skew > MAX_CLOCK_SKEW_MS {
        return Err(LinkError::Stale {
            at_ms: env.at_ms,
            now_ms,
        });
    }

    // Yük, sayaç ilerletilmeden ÖNCE çözülür. İki uç aynı anahtarla imzaladığı
    // için masanın kendi kalp atışı da geçerli bir imza taşır; röle onu masaya
    // geri yansıtırsa imza tutar, ama yük bir `Command` değildir. Sayaç önce
    // ilerletilseydi, yansıtılan zarfın (masa epoch'u) sayacı telefonun
    // epoch'unun ötesine taşıması mümkündü ve telefonun sonraki HER komutu —
    // "hepsini kapat" dahil — tekrar diye düşerdi. Yani röle, imza üretemeden
    // kill switch'i susturabilirdi. Yanlış yöndeki bir zarf artık `Malformed`
    // ile reddedilir ve sayaca dokunmaz.
    let payload: T =
        serde_json::from_str(&env.payload).map_err(|e| LinkError::Malformed(e.to_string()))?;

    guard.accept(env.epoch, env.seq)?;

    Ok(payload)
}

/// Telefonun okuduğu QR metnini çözer.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedPairing {
    pub relay_url: String,
    pub desk_id: String,
    pub key_str: String,
}

pub fn parse_qr(text: &str) -> Result<ScannedPairing, LinkError> {
    let rest = text
        .strip_prefix("alephedge://pair?")
        .ok_or(LinkError::BadPairingString)?;
    let (mut relay, mut desk, mut k) = (None, None, None);
    for part in rest.split('&') {
        let (name, value) = part.split_once('=').ok_or(LinkError::BadPairingString)?;
        match name {
            "relay" => relay = Some(value.to_string()),
            "desk" => desk = Some(value.to_string()),
            "k" => k = Some(value.to_string()),
            // Bilinmeyen alan sessizce YUTULMAZ: ileride eklenen bir alanı
            // eski bir telefonun görmezden gelmesi, yarısı uygulanmış bir
            // protokol demektir.
            _ => return Err(LinkError::BadPairingString),
        }
    }
    match (relay, desk, k) {
        (Some(relay_url), Some(desk_id), Some(key_str))
            if !relay_url.is_empty() && !desk_id.is_empty() && !key_str.is_empty() =>
        {
            // Anahtarın gerçekten çözülebildiğini BURADA doğrula; bozuk bir
            // anahtarla "eşleştim" deyip ilk komutta patlamak, hatayı yanlış
            // yere taşır.
            PairingKey::from_pairing_string(&key_str).map_err(|_| LinkError::BadPairingString)?;
            Ok(ScannedPairing {
                relay_url,
                desk_id,
                key_str,
            })
        }
        _ => Err(LinkError::BadPairingString),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> PairingKey {
        PairingKey::from_bytes([7u8; 32])
    }

    const NOW: u64 = 1_786_700_000_000;
    /// Gönderenin oturum damgası. Üretimde oturumun başladığı ms.
    const EPOCH: u64 = 1_786_699_000_000;

    #[test]
    fn gidip_gelen_komut_aynen_cozulur() {
        let k = key();
        let cmd = Command::CloseAll {
            bot: BotSelector::Futures,
        };
        let env = seal(&k, "desk-1", EPOCH, 1, NOW, &cmd).unwrap();
        let mut g = ReplayGuard::new();
        let got: Command = open(&k, &mut g, NOW, &env).unwrap();
        assert_eq!(got, cmd);
    }

    /// Rölenin var oluş sebebine karşı test: röle mesajı TAŞIR, ÜRETEMEZ.
    #[test]
    fn role_komut_uyduramaz() {
        let desk_key = key();
        let role_key = PairingKey::from_bytes([9u8; 32]); // rölenin bildiği hiçbir şey

        let sahte = seal(
            &role_key,
            "desk-1",
            EPOCH,
            1,
            NOW,
            &Command::CloseAll {
                bot: BotSelector::All,
            },
        )
        .unwrap();

        let mut g = ReplayGuard::new();
        let r: Result<Command, _> = open(&desk_key, &mut g, NOW, &sahte);
        assert_eq!(r.unwrap_err(), LinkError::BadSignature);
        // Reddedilen zarf sayacı İLERLETMEMELİ, yoksa saldırgan sayaç yakarak
        // meşru komutları düşürebilirdi.
        assert_eq!(g.last_seq(), None);
    }

    #[test]
    fn yukun_tek_biti_degisse_imza_tutmaz() {
        let k = key();
        let mut env = seal(&k, "desk-1", EPOCH, 1, NOW, &Command::Status).unwrap();
        // "Durumu göster"i "hepsini kapat"a çevirme denemesi.
        env.payload = serde_json::to_string(&Command::CloseAll {
            bot: BotSelector::All,
        })
        .unwrap();
        let mut g = ReplayGuard::new();
        let r: Result<Command, _> = open(&k, &mut g, NOW, &env);
        assert_eq!(r.unwrap_err(), LinkError::BadSignature);
    }

    #[test]
    fn desk_id_degistirilirse_imza_tutmaz() {
        let k = key();
        let mut env = seal(&k, "desk-1", EPOCH, 1, NOW, &Command::Status).unwrap();
        // Komutu BASKA birinin masasına yönlendirme denemesi.
        env.desk_id = "desk-2".into();
        let mut g = ReplayGuard::new();
        let r: Result<Command, _> = open(&k, &mut g, NOW, &env);
        assert_eq!(r.unwrap_err(), LinkError::BadSignature);
    }

    #[test]
    fn kaydedilmis_zarf_tekrar_oynatilamaz() {
        let k = key();
        let env = seal(
            &k,
            "desk-1",
            EPOCH,
            5,
            NOW,
            &Command::StopOpening {
                bot: BotSelector::All,
            },
        )
        .unwrap();
        let mut g = ReplayGuard::new();
        let _: Command = open(&k, &mut g, NOW, &env).unwrap();

        // Ayni zarf ikinci kez: gecerli imza, gecerli saat, ama eski sayac.
        let r: Result<Command, _> = open(&k, &mut g, NOW, &env);
        assert_eq!(
            r.unwrap_err(),
            LinkError::Replay {
                epoch: EPOCH,
                seq: 5,
                last_epoch: EPOCH,
                last_seq: 5,
            }
        );
    }

    #[test]
    fn eski_bir_zarf_gunler_sonra_oynatilamaz() {
        let k = key();
        let env = seal(&k, "desk-1", EPOCH, 1, NOW, &Command::Status).unwrap();
        let mut g = ReplayGuard::new();
        let sonra = NOW + 48 * 3600 * 1000;
        let r: Result<Command, _> = open(&k, &mut g, sonra, &env);
        assert!(matches!(r.unwrap_err(), LinkError::Stale { .. }));
    }

    #[test]
    fn makul_saat_sapmasi_kabul_edilir() {
        let k = key();
        let env = seal(&k, "desk-1", EPOCH, 1, NOW, &Command::Status).unwrap();
        let mut g = ReplayGuard::new();
        // Telefonun saati 60 sn ileri.
        let r: Result<Command, _> = open(&k, &mut g, NOW - 60_000, &env);
        assert!(r.is_ok(), "makul sapma reddedilmemeli: {r:?}");
    }

    #[test]
    fn surum_uyusmazligi_sessizce_yorumlanmaz() {
        let k = key();
        let mut env = seal(&k, "desk-1", EPOCH, 1, NOW, &Command::Status).unwrap();
        env.v = PROTOCOL_VERSION + 1;
        let mut g = ReplayGuard::new();
        let r: Result<Command, _> = open(&k, &mut g, NOW, &env);
        assert_eq!(
            r.unwrap_err(),
            LinkError::VersionMismatch {
                got: PROTOCOL_VERSION + 1,
                want: PROTOCOL_VERSION
            }
        );
    }

    /// Acil bir kontrolde belirsizlik tehlikelidir. "Durdur" ile "hepsini kapat"
    /// AYRI komutlardır ve biri digerine seri hale gelemez.
    #[test]
    fn durdur_ve_hepsini_kapat_ayri_komutlardir() {
        let stop = serde_json::to_string(&Command::StopOpening {
            bot: BotSelector::All,
        })
        .unwrap();
        let close = serde_json::to_string(&Command::CloseAll {
            bot: BotSelector::All,
        })
        .unwrap();
        assert_ne!(stop, close);
        assert!(stop.contains("stop_opening"), "{stop}");
        assert!(close.contains("close_all"), "{close}");
    }

    #[test]
    fn esleştirme_anahtari_gidip_gelir() {
        let k = PairingKey::generate();
        let s = k.to_pairing_string();
        let k2 = PairingKey::from_pairing_string(&s).unwrap();
        assert_eq!(k.0, k2.0);
        // QR'a basilabilecek kadar kisa olmali.
        assert!(s.len() < 64, "eşleştirme dizesi {} karakter", s.len());
    }

    #[test]
    fn anahtar_log_satirina_dusmez() {
        let k = PairingKey::from_bytes([42u8; 32]);
        let s = format!("{k:?}");
        assert!(!s.contains("42"), "anahtar Debug ciktisina sizdi: {s}");
        assert!(s.contains("gizli"));
    }

    /// S8: masa yeniden baslarsa sayaci sifirdan baslar. Epoch olmadan bu, her
    /// kalp atisinin "tekrar" diye reddedilmesi ve telefonun sonsuza kadar
    /// "masan sustu" gostermesi demekti — protokol, kalp atisinin yakalamak
    /// icin var oldugu arizayi kendi uretiyordu.
    ///
    /// Yeniden baslayan es KABUL edilir; ama gercek bir tekrar hala reddedilir.
    #[test]
    fn yeniden_baslayan_es_kabul_edilir_gercek_tekrar_reddedilir() {
        let k = key();
        let mut g = ReplayGuard::new();

        // Ilk oturum: iki atis.
        let a = seal(&k, "desk-1", EPOCH, 1, NOW, &Command::Status).unwrap();
        let b = seal(&k, "desk-1", EPOCH, 2, NOW, &Command::Status).unwrap();
        let _: Command = open(&k, &mut g, NOW, &a).unwrap();
        let _: Command = open(&k, &mut g, NOW, &b).unwrap();

        // Masa yeniden basladi: yeni epoch, sayac yeniden 1.
        let yeni_epoch = EPOCH + 1;
        let c = seal(&k, "desk-1", yeni_epoch, 1, NOW, &Command::Status).unwrap();
        let r: Result<Command, _> = open(&k, &mut g, NOW, &c);
        assert!(r.is_ok(), "yeniden baslayan es reddedildi: {r:?}");

        // Ayni zarf ikinci kez: hala tekrar.
        assert!(matches!(
            open::<Command>(&k, &mut g, NOW, &c).unwrap_err(),
            LinkError::Replay { .. }
        ));

        // Ve ONCEKI oturumdan kaydedilmis bir zarf: epoch geride, reddedilir.
        let eski = seal(&k, "desk-1", EPOCH, 99, NOW, &Command::Status).unwrap();
        assert!(
            matches!(
                open::<Command>(&k, &mut g, NOW, &eski).unwrap_err(),
                LinkError::Replay { .. }
            ),
            "eski oturumun zarfi kabul edildi"
        );
    }

    /// Epoch imzanin ICINDE: disinda kalsaydi araya giren biri epoch'u buyutup
    /// kaydettigi bir zarfi "yeni oturum" gibi gecirebilirdi.
    #[test]
    fn epoch_kurcalanirsa_imza_tutmaz() {
        let k = key();
        let mut env = seal(&k, "desk-1", EPOCH, 1, NOW, &Command::Status).unwrap();
        env.epoch = EPOCH + 5_000;
        let mut g = ReplayGuard::new();
        let r: Result<Command, _> = open(&k, &mut g, NOW, &env);
        assert_eq!(r.unwrap_err(), LinkError::BadSignature);
    }

    /// Iki uc ayni anahtarla imzalar. Role, masanin kendi kalp atisini masaya
    /// geri yansitirsa imza tutar; yansitilan zarf sayaci ILERLETMEMELI, yoksa
    /// masa epoch'u telefonunkinden buyukse telefonun sonraki her komutu
    /// (hepsini kapat dahil) "tekrar" diye duserdi.
    #[test]
    fn yansitilan_masa_zarfi_sayaci_ilerletmez() {
        let k = key();
        let mut desk_guard = ReplayGuard::new();
        // Masa telefondan SONRA basladi: masa epoch'u buyuk.
        let desk_epoch = EPOCH + 10_000;
        let beat = seal(
            &k,
            "desk-1",
            desk_epoch,
            1,
            NOW,
            &DeskMessage::Heartbeat { at_ms: NOW },
        )
        .unwrap();
        let r: Result<Command, _> = open(&k, &mut desk_guard, NOW, &beat);
        assert!(matches!(r.unwrap_err(), LinkError::Malformed(_)));
        assert_eq!(desk_guard.last_epoch(), None, "yansima sayaci ilerletti");

        // Telefonun gercek komutu hala gecmeli.
        let close = seal(
            &k,
            "desk-1",
            EPOCH,
            1,
            NOW,
            &Command::CloseAll {
                bot: BotSelector::All,
            },
        )
        .unwrap();
        let got: Command = open(&k, &mut desk_guard, NOW, &close).unwrap();
        assert_eq!(
            got,
            Command::CloseAll {
                bot: BotSelector::All
            }
        );
    }

    #[test]
    fn kalp_atisi_ve_onay_gidip_gelir() {
        let k = key();
        let msg = DeskMessage::Ack {
            for_seq: 5,
            applied: true,
            at_ms: NOW,
            detail: AckReply::closed(6, 0).english(),
            reply: Some(AckReply::closed(6, 0)),
        };
        let env = seal(&k, "desk-1", EPOCH, 1, NOW, &msg).unwrap();
        let mut g = ReplayGuard::new();
        let got: DeskMessage = open(&k, &mut g, NOW, &env).unwrap();
        assert_eq!(got, msg);
    }

    /// Kablo biçimi düz ve sayısal: telefon kodu ve sayıyı okur, cümleyi değil.
    #[test]
    fn onay_kodu_duz_ve_sayisal_serilesir() {
        let v = serde_json::to_value(AckReply::stopped(3)).unwrap();
        assert_eq!(v, serde_json::json!({"code": "stopped", "open": 3}));
        let back: AckReply = serde_json::from_value(v).unwrap();
        assert_eq!(back, AckReply::stopped(3));
    }

    /// Eski bir masa `reply` göndermez; yeni telefon yine çözer (None).
    /// Eski bir telefon `reply`'ı tanımaz; alan yok sayılır, `detail` kalır.
    #[test]
    fn onay_kodu_eski_uclarla_uyumlu() {
        let eski = r#"{"msg":"ack","for_seq":1,"applied":true,"at_ms":5,"detail":"x"}"#;
        match serde_json::from_str::<DeskMessage>(eski).unwrap() {
            DeskMessage::Ack { reply, detail, .. } => {
                assert_eq!(reply, None);
                assert_eq!(detail, "x");
            }
            other => panic!("{other:?}"),
        }

        #[derive(Deserialize)]
        #[serde(tag = "msg", rename_all = "snake_case")]
        #[allow(dead_code)]
        enum EskiMesaj {
            Ack { for_seq: u64, applied: bool, at_ms: u64, detail: String },
        }
        let yeni = serde_json::to_string(&DeskMessage::Ack {
            for_seq: 1,
            applied: true,
            at_ms: 5,
            detail: AckReply::stopped(2).english(),
            reply: Some(AckReply::stopped(2)),
        })
        .unwrap();
        let EskiMesaj::Ack { detail, .. } = serde_json::from_str(&yeni).unwrap();
        assert!(detail.contains('2'), "{detail}");
    }

    #[test]
    fn onay_ingilizce_metni() {
        assert_eq!(
            AckReply::stopped(1).english(),
            "not opening new positions; 1 open position still managed"
        );
        assert_eq!(AckReply::closed(6, 0).english(), "6 positions closed");
        assert_eq!(
            AckReply::closed(4, 2).english(),
            "4 positions closed, 2 could not be priced and will close next round"
        );
        assert_eq!(AckReply::closed(4, 2).code, ack_code::CLOSED_PARTIAL);
        assert_eq!(
            AckReply::started(&[BotSelector::Futures, BotSelector::Pump]).english(),
            "started: futures, pump"
        );
        // Bilinmeyen kod boş metin üretmez.
        assert_eq!(AckReply::new("future_code").english(), "future_code");
    }

    /// Onay kodu imzalı yükün İÇİNDE: kurcalanırsa imza tutmaz.
    #[test]
    fn onay_kodu_imzanin_icinde() {
        let k = key();
        let msg = DeskMessage::Ack {
            for_seq: 1,
            applied: true,
            at_ms: NOW,
            detail: AckReply::stopped(8).english(),
            reply: Some(AckReply::stopped(8)),
        };
        let mut env = seal(&k, "desk-1", EPOCH, 1, NOW, &msg).unwrap();
        env.payload = env.payload.replace("\"open\":8", "\"open\":0");
        let mut g = ReplayGuard::new();
        let r: Result<DeskMessage, _> = open(&k, &mut g, NOW, &env);
        assert_eq!(r.unwrap_err(), LinkError::BadSignature);
    }
}
