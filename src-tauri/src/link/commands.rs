//! Eşleştirme ve canlı röle oturumu için Tauri komutları.
//!
//! Masaüstü bir QR gösterir, telefon okur. Anahtar QR'ın içindedir, yani
//! **ekranda duran bir QR duran bir kimlik bilgisidir** — bu yüzden oturum
//! kısa ömürlüdür ve süresi dolduğunda arayüz bunu göstermek zorundadır.
//!
//! # Ana makine ve kimlik buradan çözülür, arayüzden GELMEZ
//!
//! `app::endpoints` şu kuralı koyuyor: "arayüz hiçbir ana makine taşımaz."
//! Bu dosya bir süre o kuralın dışında kaldı — `desk_id` ve `relay_url`
//! parametre olarak geliyordu ve doğru çalışması, arayüzün onları Rust'tan
//! sorma nezaketine bağlıydı. Yani güvenlik sınırı webview'in içindeydi.
//! Artık ikisi de burada çözülür; parametre olarak verilemez.
//!
//! # Neden oturum QR ile birlikte başlar
//!
//! Telefon röleye bağlanabilsin diye masanın röleye ÖNCEDEN bağlanmış olması
//! gerekir (`relay/hub.go`: masa bağlı değilse telefon eklenemez). Bu yüzden
//! `link_begin_pairing` hem QR'ı üretir hem oturumu başlatır. Önceden oturumu
//! başlatan komut arayüzden hiç çağrılmıyordu ve sağlayamayacağı bir üyelik
//! token'ı istiyordu: QR ekranda geri sayıyor, karşılığında hiçbir oturum
//! açılmıyordu.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use aleph_desk_link::pairing::{PairingSession, PAIRING_TTL_MS};
use aleph_desk_link::DeskLink;
use aleph_link::PairingKey;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex as AsyncMutex;

use super::session::{self, LinkState, SharedLink};
use super::DeskBridge;
use crate::app::endpoints;
use crate::membership::MembershipManager;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Açık eşleştirme oturumu (aynı anda en fazla bir tane).
#[derive(Default)]
pub struct PairingState {
    pub(crate) session: Mutex<Option<PairingSession>>,
}

impl PairingState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Arayüzün gördüğü her şey.
///
/// Masa kimliği burada **yok**: arayüz onu ne kullanıyor ne de göndermesi
/// gerekiyor, ve bir yönlendirme adresini gereksiz yere webview'e taşımak
/// (oradan da log ve hata raporlarına) bedava değil.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingView {
    /// QR'a basılacak metin. Süresi dolduysa `None`.
    pub qr: Option<String>,
    /// Geri sayım — kullanıcı QR'ın ne zaman öleceğini görmeli.
    pub remaining_ms: u64,
    pub ttl_ms: u64,
    /// Süresi dolmuş ya da kullanılmışsa sebebi.
    pub expired_reason: Option<String>,
}

fn view(s: &PairingSession, now: u64) -> PairingView {
    match s.qr_payload(now) {
        Ok(qr) => PairingView {
            qr: Some(qr),
            remaining_ms: s.remaining_ms(now),
            ttl_ms: PAIRING_TTL_MS,
            expired_reason: None,
        },
        Err(e) => PairingView {
            qr: None,
            remaining_ms: 0,
            ttl_ms: PAIRING_TTL_MS,
            expired_reason: Some(e.to_string()),
        },
    }
}

/// Yeni bir eşleştirme oturumu açar, röle oturumunu başlatır ve QR metnini
/// döner.
///
/// Var olan oturumun üzerine yazar: ekranda iki geçerli QR bulunması,
/// hangisinin canlı olduğunu bilinemez kılardı. Çalışan röle oturumu da yeni
/// anahtarla **yeniden** başlatılır — eski anahtarla bağlı kalmak, yeni QR'ı
/// okuyan telefonun hiçbir zaman cevap alamaması demekti.
#[tauri::command]
pub fn link_begin_pairing(
    app: AppHandle,
    state: State<PairingState>,
    runtime: State<LinkRuntime>,
) -> Result<PairingView, String> {
    // Röle her iki ucu da Sentinel üyeliğiyle doğrular. Giriş yapılmamışken QR
    // üretmek, hiçbir zaman bağlanamayacak bir kod göstermek olurdu — bu
    // paneldeki dürüstlük kuralının aynısı, bir adım öncesinde.
    if app.state::<MembershipManager>().access_token().is_none() {
        return Err("linkSignInFirst".into());
    }

    // Ana makine ve kimlik: tek yerden, arayüzden değil.
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "appDataDirUnavailable".to_string())?;
    let desk_id = endpoints::desk_id(&dir);
    let relay_url = endpoints::relay_url();

    let now = now_ms();
    let s = PairingSession::begin(desk_id.clone(), relay_url.clone(), now);
    let qr = s.qr_payload(now).map_err(|e| e.to_string())?;
    let remaining = s.remaining_ms(now);
    let key = s.desk_session_key();
    *state.session.lock().map_err(|_| "pairing mutex")? = Some(s);

    // QR'ın öleceği an anahtarın da son kullanma anıdır: o ana kadar hiçbir
    // telefon anahtarı kullanmazsa masa onunla imzalı zarfı artık kabul etmez.
    runtime.restart(&app, relay_url, desk_id, key, now + PAIRING_TTL_MS)?;

    Ok(PairingView {
        qr: Some(qr),
        remaining_ms: remaining,
        ttl_ms: PAIRING_TTL_MS,
        expired_reason: None,
    })
}

/// Açık oturumun anlık hâli.
///
/// Süresi dolan bir QR **metin döndürmez** ve sebebini söyler. Ölü bir QR'ı
/// ekranda çalışıyormuş gibi bırakmak, kullanıcıya olmayan bir şeyi
/// göstermektir.
#[tauri::command]
pub fn link_pairing_status(state: State<PairingState>) -> Result<Option<PairingView>, String> {
    let guard = state.session.lock().map_err(|_| "pairing mutex")?;
    let Some(s) = guard.as_ref() else {
        return Ok(None);
    };
    Ok(Some(view(s, now_ms())))
}

/// Açık oturumu iptal eder **ve röle oturumunu durdurur**.
///
/// İkincisi olmadan "iptal" bir yalandı: QR ekrandan kalkıyor, masa röleye
/// bağlı kalmaya devam ediyordu. Bu düğme yalnızca QR canlıyken görünür, yani
/// henüz hiçbir telefon o anahtarla konuşmamıştır (bkz. `session::spend_pairing`)
/// — çalışan bir bağlantıyı kesmez.
#[tauri::command]
pub async fn link_cancel_pairing(
    state: State<'_, PairingState>,
    runtime: State<'_, LinkRuntime>,
) -> Result<(), String> {
    {
        let mut guard = state.session.lock().map_err(|_| "pairing mutex")?;
        *guard = None;
    }
    runtime.stop().await
}

// ---------------------------------------------------------------------------
// Canlı oturum
// ---------------------------------------------------------------------------

/// Çalışan röle oturumunun paylaşılan durumu.
pub struct LinkRuntime {
    state: Arc<AsyncMutex<LinkState>>,
    /// Çalışan oturumun tutamağı. Tutulmasının sebebi yeniden eşleştirme:
    /// yeni bir QR yeni bir anahtar demek, ve eski anahtarla bağlı kalan bir
    /// oturum yeni telefona asla cevap veremez.
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    /// Çalışan oturumun bağlantı katmanı. İptal, görevi durdurmakla yetinmez:
    /// anahtarı da geçersiz kılar, ki durdurma gecikse bile o anahtarla
    /// imzalı hiçbir zarf uygulanmasın.
    link: Mutex<Option<SharedLink>>,
}

impl Default for LinkRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl LinkRuntime {
    pub fn new() -> Self {
        Self {
            state: Arc::new(AsyncMutex::new(LinkState::Unpaired)),
            task: Mutex::new(None),
            link: Mutex::new(None),
        }
    }

    /// Oturumu (varsa öncekini durdurarak) başlatır. `claim_deadline_ms`:
    /// bir telefonun anahtarı kullanması gereken son an (QR'ın ölümü).
    fn restart(
        &self,
        app: &AppHandle,
        relay_url: String,
        desk_id: String,
        key: PairingKey,
        claim_deadline_ms: u64,
    ) -> Result<(), String> {
        let mut task = self.task.lock().map_err(|_| "link mutex")?;
        if let Some(h) = task.take() {
            h.abort();
        }
        let mut slot = self.link.lock().map_err(|_| "link mutex")?;
        if let Some(old) = slot.take() {
            revoke_later(old);
        }
        // Oturum damgası: bu oturumun başladığı an. Giden sayaç her başlayışta
        // sıfırdan başlar, bu yüzden telefonun tekrar koruması yeniden
        // başlamayı saldırı sanıyordu ve masa sonsuza kadar "susmuş"
        // görünüyordu. Damga o ayrımı imzalı biçimde taşır — ayrıntı
        // `aleph_link::ReplayGuard`.
        let link: SharedLink = Arc::new(AsyncMutex::new(
            DeskLink::new(desk_id.clone(), key, DeskBridge::new(app.clone()), now_ms())
                .with_claim_deadline(claim_deadline_ms),
        ));
        *slot = Some(link.clone());
        *task = Some(tauri::async_runtime::spawn(session::run(
            app.clone(),
            relay_url,
            desk_id,
            link,
            self.state.clone(),
        )));
        Ok(())
    }

    async fn stop(&self) -> Result<(), String> {
        {
            let mut task = self.task.lock().map_err(|_| "link mutex")?;
            if let Some(h) = task.take() {
                h.abort();
            }
        }
        // Görev iptali bir sonraki bekleme noktasında gerçekleşir; anahtar
        // ondan bağımsız olarak burada ölür.
        let old = self.link.lock().map_err(|_| "link mutex")?.take();
        if let Some(old) = old {
            old.lock().await.revoke();
        }
        // Durum, iptalden SONRA yazılır: aksi hâlde kopan oturumun son
        // "Retrying" yazısı ekranda kalabilirdi.
        *self.state.lock().await = LinkState::Unpaired;
        Ok(())
    }
}

/// Eski bir oturumun anahtarını, kilidi boşaldığında geçersiz kılar. Senkron
/// `restart` kilidi bekleyemez; iptal edilen görev kilidi bıraktığı anda
/// anahtar ölür.
fn revoke_later(link: SharedLink) {
    tauri::async_runtime::spawn(async move {
        link.lock().await.revoke();
    });
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum LinkStateView {
    Unpaired,
    Connecting,
    Connected,
    Retrying { in_secs: u64 },
    Stopped { message: String },
}

impl From<LinkState> for LinkStateView {
    fn from(s: LinkState) -> Self {
        match s {
            LinkState::Unpaired => Self::Unpaired,
            LinkState::Connecting => Self::Connecting,
            LinkState::Connected => Self::Connected,
            LinkState::Retrying { in_secs } => Self::Retrying { in_secs },
            LinkState::Stopped { message } => Self::Stopped { message },
        }
    }
}

/// Oturumun anlık durumu — arayüz bunu gösterir.
#[tauri::command]
pub async fn link_state(runtime: State<'_, LinkRuntime>) -> Result<LinkStateView, String> {
    Ok(runtime.state.lock().await.clone().into())
}

#[cfg(test)]
mod tests {
    /// S10: masa kimligi ve role adresi RUST'ta cozulur. Arayuzden parametre
    /// olarak alinsaydi, "hangi masayi baglariz" karari webview'in icinde olurdu
    /// — ve o kararin dogru olmasi, arayuzun degerleri Rust'tan sorma
    /// nezaketine kalirdi.
    ///
    /// Test, komutun IMZASINI okur: arayuzden gelen hicbir dize parametresi
    /// olmamali. Bir gun biri `desk_id` ya da `relay_url` parametresini geri
    /// koyarsa bu test kirilir.
    #[test]
    fn esleştirme_komutu_arayuzden_ana_makine_almaz() {
        let src = include_str!("commands.rs");
        let govde = &src[src
            .find("pub fn link_begin_pairing")
            .expect("komut bulunamadi")..];
        let imza = &govde[..govde.find(')').expect("imza kapanmiyor")];
        assert!(
            !imza.contains("String"),
            "komut arayuzden dize parametresi aliyor: {imza}"
        );
        // Ve degerler gercekten tek kaynaktan cozulmeli.
        assert!(src.contains("endpoints::desk_id(&dir)"));
        assert!(src.contains("endpoints::relay_url()"));
    }

    /// Masa kimligi arayuze de TASINMAZ: kullanilmadigi hâlde webview'e (ve
    /// oradan log/hata raporlarina) gecirmenin bir faydasi yok.
    #[test]
    fn gorunumde_masa_kimligi_yok() {
        let src = include_str!("commands.rs");
        let baslangic = src
            .find("pub struct PairingView {")
            .expect("PairingView bulunamadi");
        let govde = &src[baslangic..baslangic + src[baslangic..].find('}').unwrap()];
        assert!(
            !govde.contains("desk_id"),
            "PairingView masa kimligi tasiyor: {govde}"
        );
    }
}
