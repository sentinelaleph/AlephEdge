//! Masaüstü tarafının bağlantı katmanı: gelen komutu doğrular, uygular, ve
//! **her zaman** bir onay döner.
//!
//! # Neden ayrı bir crate
//!
//! Aleph Edge'in kendisi sürüm kontrolsüz bir ağaçta duruyor. Bu katmanı orada
//! tasarlamak, tasarım hatasını geri alınamaz bir yere yazmak olurdu. Burada
//! gerçek uygulamadan bağımsız, `DeskControl` arayüzü üzerinden tam test
//! edilir; entegrasyon sonra bir kablolama işine iner.
//!
//! # Değişmezler
//!
//! 1. **Her komut bir onay üretir.** Uygulanmadıysa `applied: false` ve sebebi.
//!    Onaysız bir komut, telefonda yeşil tik gösterilemeyeceği anlamına gelir;
//!    basıp durduğunu sanan ama durmamış bir kullanıcı, hiç kill switch
//!    olmamasından daha kötü durumdadır.
//! 2. **Durdur ile hepsini-kapat asla birleşmez.** Acil bir kontrolde
//!    belirsizlik pahalıdır.
//! 3. **Doğrulanmayan zarf hiçbir şey yapmaz** ve sayaç ilerletmez.

pub mod pairing;
pub mod session;

use aleph_link::{
    open, seal, AckReply, BotSelector, Command, DeskMessage, DeskStatus, Envelope, LinkError,
    PairingKey, ReplayGuard,
};

/// Masanın gerçek kontrolü.
///
/// Metotlar **async**, çünkü açık pozisyonları kapatmak borsaya gider ve
/// zaman alır. Senkron olsalardı, gerçek uygulama işi arka plana atıp hemen
/// dönmek zorunda kalırdı — yani onay, kapatma BİTMEDEN gönderilirdi. Erken
/// bir onay, telefonda "6 pozisyon kapatıldı" yazarken pozisyonların hâlâ
/// açık olması demektir; bu katmanın engellemek için var olduğu hatanın ta
/// kendisi.
///
/// Sonuçlar cümle değil `AckReply`: kararlı kod + sayılar. Telefon onu kendi
/// dilinde çizer; masanın dili telefona dayatılmaz.
pub trait DeskControl {
    /// Yeni pozisyon açmayı durdurur. Açık pozisyonlara DOKUNMAZ.
    fn stop_opening(
        &mut self,
        bot: BotSelector,
    ) -> impl std::future::Future<Output = Result<AckReply, AckReply>>;
    /// Açık pozisyonları da kapatır. İş BİTTİKTEN sonra döner.
    fn close_all(
        &mut self,
        bot: BotSelector,
    ) -> impl std::future::Future<Output = Result<AckReply, AckReply>>;
    /// Botu başlatır.
    fn start(
        &mut self,
        bot: BotSelector,
    ) -> impl std::future::Future<Output = Result<AckReply, AckReply>>;
    /// Anlık durum.
    fn status(&self) -> DeskStatus;
}

/// Kalp atışı aralığı (ms). Röle üç atış kaçırınca masayı susmuş sayar.
pub const HEARTBEAT_EVERY_MS: u64 = 30_000;

pub struct DeskLink<C: DeskControl> {
    desk_id: String,
    key: PairingKey,
    guard: ReplayGuard,
    /// Bu oturumun damgası. `out_seq` her yeniden başlayışta sıfırdan başladığı
    /// için sayaç tek başına yetmiyordu: telefon, yeniden başlayan masanın
    /// atışlarını tekrar sanıp sonsuza kadar "masan sustu" gösteriyordu. Epoch
    /// o ayrımı imzalı biçimde taşır — ayrıntı `aleph_link::ReplayGuard`.
    epoch: u64,
    /// Giden mesajların sayacı. Telefon tarafı bunu kendi tekrar korumasında
    /// kullanır.
    out_seq: u64,
    control: C,
    last_beat_ms: u64,
    /// Bir telefonun anahtarı ELİNDE TUTTUĞUNU bu ana kadar kanıtlaması
    /// gerekir (QR'ın ölüm anı). Kanıtlamazsa anahtar ölür: QR ekrandan
    /// kalktığı hâlde masanın o anahtarla imzalı komutu kabul etmeye devam
    /// etmesi, QR'ın fotoğrafını çeken birine süresiz bir kapı bırakmaktı.
    claim_deadline_ms: Option<u64>,
    /// Doğrulanmış bir telefon zarfı geldi mi.
    claimed: bool,
    /// Anahtar geçersiz kılındı (süre doldu ya da iptal). Geri dönüşü yok.
    revoked: bool,
}

impl<C: DeskControl> DeskLink<C> {
    /// `session_epoch`, bu oturumun damgası: üretimde oturumun başladığı
    /// duvar saati (ms). Yalnızca ileri gitmesi gerekir; geriye giden bir
    /// epoch, telefonun bu masayı yeniden reddetmesi demektir.
    pub fn new(
        desk_id: impl Into<String>,
        key: PairingKey,
        control: C,
        session_epoch: u64,
    ) -> Self {
        Self {
            desk_id: desk_id.into(),
            key,
            guard: ReplayGuard::new(),
            epoch: session_epoch,
            out_seq: 0,
            control,
            last_beat_ms: 0,
            claim_deadline_ms: None,
            claimed: false,
            revoked: false,
        }
    }

    /// Anahtarın, hiçbir telefon onu kullanmazsa öleceği an (ms). Eşleştirme
    /// QR'ı ile kurulan oturumlar bunu QR'ın son geçerlilik anına bağlar.
    pub fn with_claim_deadline(mut self, deadline_ms: u64) -> Self {
        self.claim_deadline_ms = Some(deadline_ms);
        self
    }

    /// Anahtarı kalıcı olarak geçersiz kılar (eşleştirme iptal edildi).
    /// Bundan sonra hiçbir zarf kabul edilmez, kalp atışı da gönderilmez.
    pub fn revoke(&mut self) {
        self.revoked = true;
    }

    /// Anahtar hâlâ geçerli mi. Süresi hiç kullanılmadan dolduysa burada
    /// kalıcı olarak ölür — sonradan gelen bir zarf onu diriltemez.
    pub fn key_live(&mut self, now_ms: u64) -> bool {
        if !self.revoked && !self.claimed {
            if let Some(deadline) = self.claim_deadline_ms {
                if now_ms > deadline {
                    self.revoked = true;
                }
            }
        }
        !self.revoked
    }

    fn next_seq(&mut self) -> u64 {
        self.out_seq += 1;
        self.out_seq
    }

    fn send(&mut self, now_ms: u64, msg: &DeskMessage) -> Envelope {
        let seq = self.next_seq();
        seal(
            &self.key,
            &self.desk_id.clone(),
            self.epoch,
            seq,
            now_ms,
            msg,
        )
        .expect("DeskMessage her zaman serileşir")
    }

    /// Röleden gelen ham zarfı işler.
    ///
    /// Dönüş: telefona gönderilecek zarf(lar). Doğrulama başarısızsa **boş** —
    /// yani doğrulanmayan bir zarf ne bir eylem ne de bir yanıt üretir.
    /// Yanıt üretseydi, imzasız zarf gönderen biri masanın varlığını ve
    /// durumunu yoklayabilirdi.
    pub async fn handle_raw(&mut self, now_ms: u64, raw: &str) -> Result<Vec<Envelope>, LinkError> {
        if !self.key_live(now_ms) {
            return Err(LinkError::KeyRevoked);
        }
        let env: Envelope =
            serde_json::from_str(raw).map_err(|e| LinkError::Malformed(e.to_string()))?;
        // Başka bir masaya yazılmış zarf: imzası tutsa bile (aynı anahtar iki
        // masada kullanılmışsa) bu masa onu uygulamaz. Sayaçtan ÖNCE bakılır.
        if env.desk_id != self.desk_id {
            return Err(LinkError::WrongDesk);
        }
        let cmd: Command = open(&self.key, &mut self.guard, now_ms, &env)?;
        self.claimed = true;
        Ok(vec![self.apply(now_ms, env.seq, cmd).await])
    }

    /// Doğrulanmış bir komutu uygular ve onayı üretir.
    async fn apply(&mut self, now_ms: u64, for_seq: u64, cmd: Command) -> Envelope {
        let (applied, reply) = match cmd {
            Command::Status => {
                let st = self.control.status();
                // Durum sorgusu ayrı bir mesaj tipiyle döner; yine de onaysız
                // bırakılmaz, çünkü telefon "sordum, cevap gelmedi" ile
                // "sordum, masa cevapladı" arasını ayırabilmeli.
                return self.send(now_ms, &DeskMessage::Status(st));
            }
            // `.await` burada kritik: onay, iş BİTTİKTEN sonra gönderilir.
            // Erken bir onay, telefonda "6 pozisyon kapatıldı" yazarken
            // pozisyonların hâlâ açık olması demektir.
            Command::StopOpening { bot } => match self.control.stop_opening(bot).await {
                Ok(d) => (true, d),
                Err(e) => (false, e),
            },
            Command::CloseAll { bot } => match self.control.close_all(bot).await {
                Ok(d) => (true, d),
                Err(e) => (false, e),
            },
            Command::Start { bot } => match self.control.start(bot).await {
                Ok(d) => (true, d),
                Err(e) => (false, e),
            },
        };
        self.send(
            now_ms,
            &DeskMessage::Ack {
                for_seq,
                applied,
                at_ms: now_ms,
                // Kodu tanımayan eski telefonlar için İngilizce yedek.
                detail: reply.english(),
                reply: Some(reply),
            },
        )
    }

    /// Kalp atışı zamanı geldiyse bir zarf üretir.
    ///
    /// Zamanı gelmediyse `None` döner — röle zaten taşınan HER mesajı canlılık
    /// işareti sayar, bu yüzden yoğun bir hatta ayrıca atış göndermek gereksiz
    /// trafiktir.
    pub fn heartbeat_due(&mut self, now_ms: u64) -> Option<Envelope> {
        // Ölü bir anahtarla atış göndermek, telefona olmayan bir hattı canlı
        // göstermek olurdu.
        if !self.key_live(now_ms) {
            return None;
        }
        if now_ms.saturating_sub(self.last_beat_ms) < HEARTBEAT_EVERY_MS {
            return None;
        }
        self.last_beat_ms = now_ms;
        Some(self.send(now_ms, &DeskMessage::Heartbeat { at_ms: now_ms }))
    }

    pub fn control(&self) -> &C {
        &self.control
    }
}

/// Bagimliliksiz bir `block_on`.
///
/// `DeskControl` async'e cevrilince testlerin (ve senkron bir baglamdan
/// cagiran kodun) bir calisma zamanina ihtiyaci oldu. Bu yardimci, gercek G/C
/// yapmayan gelecekler icin yeterlidir; Aleph Edge kendi calisma zamanini
/// (`tauri::async_runtime`) kullanir.
pub fn block_on<F: std::future::Future>(mut fut: F) -> F::Output {
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    // SAFETY: `fut` bu fonksiyondan disari tasinmiyor.
    let mut fut = unsafe { Pin::new_unchecked(&mut fut) };
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::hint::spin_loop(),
        }
    }
}

#[cfg(test)]
mod tests;
