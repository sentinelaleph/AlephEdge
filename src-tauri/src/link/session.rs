//! Röleye giden canlı oturum.
//!
//! Bu dosya `DeskBridge`'i çalıştıran parçadır: röleye **giden** bir bağlantı
//! açar (ev bilgisayarı NAT arkasında olduğu için gelen bağlantı beklenemez),
//! zarfları `DeskLink`'e verir, onayları geri yollar ve kalp atışı gönderir.
//!
//! Karar mantığı burada değil: doğrulama, onay ve tekrar koruması
//! `aleph-desk-link`'te ve orada test edilmiş. Burada yalnızca soket, zamanlama
//! ve yeniden bağlanma var.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aleph_desk_link::session::{Disconnect, Next, Reconnect};
use aleph_desk_link::{DeskLink, HEARTBEAT_EVERY_MS};
use futures_util::{SinkExt, StreamExt};
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

use super::DeskBridge;
use crate::membership::MembershipManager;

/// Masa kimliğinin taşındığı başlık (`relay/server.go`: `DeskIDHeader`).
///
/// Sorgu dizesinde DEĞİL: kimlik, vekillerin ve erişim loglarının en kolay
/// sızdırdığı yerde durmayı gerektirmeyen bir yönlendirme adresi.
const DESK_ID_HEADER: &str = "Aleph-Desk";

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Oturumun dışarıdan görünen durumu — arayüz bunu gösterir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    /// Eşleştirilmiş bir telefon yok.
    Unpaired,
    Connecting,
    Connected,
    /// Bağlantı koptu, tekrar denenecek.
    Retrying {
        in_secs: u64,
    },
    /// Denenmeyecek. Kullanıcıya BU mesaj gösterilir.
    Stopped {
        message: String,
    },
}

/// Masanın canlı bağlantı katmanı; `LinkRuntime` de bir kopyasını tutar ki
/// iptalde anahtarı geçersiz kılabilsin.
pub type SharedLink = Arc<Mutex<DeskLink<DeskBridge>>>;

/// Anahtarın hâlâ geçerli olup olmadığına bakma aralığı. QR'ın ölümüyle
/// bağlantının kesilmesi arasındaki en uzun gecikme budur.
const KEY_CHECK_EVERY: Duration = Duration::from_secs(1);

/// Röle oturumunu sonsuza kadar sürdürür.
///
/// `Stopped` durumuna düşmedikçe döngü bitmez — pes eden bir masa, sessizce
/// ölmüş bir masadır ve kullanıcı bunu bilgisayarın başında göremez.
///
/// `link` çağıran tarafta kurulur (`LinkRuntime::restart`): oturum damgası ve
/// anahtarın son kullanma anı orada bağlanır.
pub async fn run(
    app: AppHandle,
    relay_url: String,
    desk_id: String,
    link: SharedLink,
    state: Arc<Mutex<LinkState>>,
) {
    let mut backoff = Reconnect::new();

    // One refresh-and-retry per run of 401s. The relay checks the same
    // Sentinel access token every other route does, and that token lives two
    // hours: the first reconnect after it expired used to land on
    // `Unauthorized`, a PERMANENT stop ("sign in again") although a refresh
    // would have cured it. Only a 401 that survives a refreshed token stops.
    let mut auth_retry_used = false;

    loop {
        // Hiç kullanılmadan ölen bir QR anahtarıyla yeniden bağlanmak, ölü bir
        // anahtarı röleye taşımaktır: dur ve sebebini söyle.
        if !link.lock().await.key_live(now_ms()) {
            if let Next::Stop { message, .. } = backoff.on_disconnect(&Disconnect::PairingExpired) {
                *state.lock().await = LinkState::Stopped { message };
            }
            return;
        }

        *state.lock().await = LinkState::Connecting;

        let membership = app.state::<MembershipManager>();
        let result = match membership.fresh_access_token().await {
            Some(token) => {
                let r = connect_once(&app, &relay_url, &desk_id, &token, &link, &state).await;
                if matches!(r, Err(Disconnect::Unauthorized)) {
                    let refreshed = !auth_retry_used
                        && membership.refresh_after_unauthorized(&token).await;
                    match unauthorized_next(auth_retry_used, refreshed) {
                        AuthRetry::Retry => {
                            auth_retry_used = true;
                            Err(Disconnect::Transient("access token refreshed".into()))
                        }
                        AuthRetry::Stop => r,
                    }
                } else {
                    auth_retry_used = false;
                    r
                }
            }
            None => Err(Disconnect::Unauthorized),
        };

        match result {
            Ok(()) => {
                // Temiz kapanış da bir kopmadır; yeniden bağlanılır.
                backoff.connected();
            }
            Err(why) => match backoff.on_disconnect(&why) {
                Next::Stop { message, .. } => {
                    *state.lock().await = LinkState::Stopped { message };
                    return;
                }
                Next::RetryIn(d) => {
                    *state.lock().await = LinkState::Retrying {
                        in_secs: d.as_secs(),
                    };
                    tokio::time::sleep(d).await;
                    continue;
                }
            },
        }

        // Başarılı bir oturumdan sonra kısa bir soluk; sıkı döngü olmasın.
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AuthRetry {
    Retry,
    Stop,
}

/// What a relay 401 means: retry once when a NEW access token is in place,
/// otherwise it is the permanent "sign in again" stop.
fn unauthorized_next(auth_retry_used: bool, refreshed: bool) -> AuthRetry {
    if !auth_retry_used && refreshed {
        AuthRetry::Retry
    } else {
        AuthRetry::Stop
    }
}

async fn connect_once(
    app: &AppHandle,
    relay_url: &str,
    desk_id: &str,
    token: &str,
    link: &SharedLink,
    state: &Arc<Mutex<LinkState>>,
) -> Result<(), Disconnect> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    // Token her denemede TAZE alınır (`run`: `fresh_access_token`), oturum
    // boyunca taşınmaz. Taşınsaydı erişim token'ı süresi dolduktan sonraki
    // ilk yeniden bağlanma "üyelik doğrulanamadı" diye kalıcı olarak durur,
    // kullanıcı da masasını kapanmış sanmadan önce hiçbir şey görmezdi.

    let url = format!("{relay_url}/link/desk");
    let mut req = url
        .into_client_request()
        .map_err(|e| Disconnect::Transient(e.to_string()))?;
    req.headers_mut().insert(
        "Authorization",
        format!("Bearer {token}")
            .parse()
            .map_err(|_| Disconnect::Unauthorized)?,
    );
    req.headers_mut().insert(
        DESK_ID_HEADER,
        desk_id.parse().map_err(|_| Disconnect::Unauthorized)?,
    );

    let (ws, res) = tokio_tungstenite::connect_async(req)
        .await
        .map_err(classify)?;
    // Röle 401 dönerse el sıkışma zaten başarısız olur; yine de açıkça
    // kontrol etmek, sebebi doğru sınıflandırmayı garantiler.
    if res.status() == 401 {
        return Err(Disconnect::Unauthorized);
    }

    *state.lock().await = LinkState::Connected;
    let (mut tx, mut rx) = ws.split();

    let mut beat = tokio::time::interval(Duration::from_millis(HEARTBEAT_EVERY_MS));
    let mut key_check = tokio::time::interval(KEY_CHECK_EVERY);
    // İlk tick anında gelir; masanın bağlandığını röleye hemen bildirir.
    loop {
        tokio::select! {
            _ = key_check.tick() => {
                // QR hiç kullanılmadan öldüyse (ya da eşleştirme iptal
                // edildiyse) bağlantı burada kesilir; röle oturumu ölü bir
                // anahtarla açık kalmaz.
                if !link.lock().await.key_live(now_ms()) {
                    return Err(Disconnect::PairingExpired);
                }
            }
            _ = beat.tick() => {
                let env = { link.lock().await.heartbeat_due(now_ms()) };
                if let Some(env) = env {
                    let raw = serde_json::to_string(&env)
                        .map_err(|e| Disconnect::Transient(e.to_string()))?;
                    tx.send(Message::Text(raw.into()))
                        .await
                        .map_err(classify)?;
                }
            }
            msg = rx.next() => {
                let Some(msg) = msg else { return Ok(()) };
                let msg = msg.map_err(classify)?;
                let Message::Text(raw) = msg else { continue };

                // Doğrulama, uygulama ve onay üretimi burada DEĞİL — hepsi
                // DeskLink'te ve test altında.
                let out = { link.lock().await.handle_raw(now_ms(), &raw).await };
                match out {
                    Ok(envs) => {
                        // Doğrulanmış bir zarf geldiyse bir telefon anahtarı
                        // GERÇEKTEN ele geçirmiş demektir: QR harcanır.
                        spend_pairing(app);
                        for env in envs {
                            let raw = serde_json::to_string(&env)
                                .map_err(|e| Disconnect::Transient(e.to_string()))?;
                            tx.send(Message::Text(raw.into()))
                                .await
                                .map_err(classify)?;
                        }
                    }
                    Err(e) => {
                        // Doğrulanmayan zarf: sessizce yutulmaz, loglanır; ama
                        // yanıt da üretilmez, yoksa imzasız zarf gönderen biri
                        // masanın varlığını yoklayabilirdi.
                        eprintln!("link: zarf reddedildi: {e}");
                    }
                }
            }
        }
    }
}

/// Bir telefon anahtarı kullandı: QR harcanır.
///
/// # Neden tek kullanımlık BURADA işaretlenir
///
/// Eşleştirme oturumu kendini "tek kullanımlık" diye tanımlıyordu ama masaüstü
/// bir telefonun QR'ı okuduğunu GÖREMEZ — kamerayı o tutmuyor. İşaret, oturumu
/// başlatan komutta atılıyordu; o komut hiç çağrılmadığı için pratikte hiç
/// atılmıyordu, ve atılsaydı QR daha gösterilmeden ölürdü.
///
/// Masanın bilebileceği tek an burası: doğrulanmış bir zarf, gönderenin
/// anahtarı ELİNDE TUTTUĞUNU kanıtlar. O andan sonra QR ekranda ölür ve
/// sebebini söyler.
///
/// Bu, aynı QR'ı 120 saniye içinde okumuş ikinci bir telefonu tek başına
/// engellemez — o yüzden asıl sınır rölede: bir eşleştirme başına bir telefon
/// (`relay/hub.go`: `MaxPhonesPerDesk`) ve telefonun masanın SAHİBİ olma
/// zorunluluğu.
fn spend_pairing(app: &AppHandle) {
    let state = app.state::<super::commands::PairingState>();
    let mut guard = match state.session.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    if let Some(s) = guard.as_mut() {
        // Zaten harcanmışsa dönen hata haber değil: her zarfta bir kez daha
        // denenir ve ilk seferden sonra hiçbir şey değişmez.
        let _ = s.complete(now_ms());
    }
}

/// Soket hatasını yeniden bağlanma politikasının anladığı sınıfa çevirir.
///
/// Varsayılan **geçici**: bilinmeyen bir hatayı kalıcı saymak, masayı
/// gereksiz yere kalıcı olarak kapatırdı. Yanlış yönde hata yapmak burada daha
/// pahalı.
fn classify(e: tokio_tungstenite::tungstenite::Error) -> Disconnect {
    use tokio_tungstenite::tungstenite::Error as E;
    match &e {
        E::Http(r) if r.status() == 401 || r.status() == 403 => Disconnect::Unauthorized,
        E::Http(r) if r.status() == 409 => Disconnect::DeskConflict,
        E::Protocol(_) => Disconnect::Transient(e.to_string()),
        _ => Disconnect::Transient(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bilinmeyen bir hatayi KALICI saymak, masayi gereksiz yere kapatirdi.
    /// Varsayilan gecici olmali.
    #[test]
    fn bilinmeyen_hata_gecici_sayilir() {
        let e = tokio_tungstenite::tungstenite::Error::ConnectionClosed;
        assert!(matches!(classify(e), Disconnect::Transient(_)));
    }

    /// Erişim token'ı iki saatte ölür. Süresi dolduktan sonraki ilk 401,
    /// tazelenmiş bir token ile BİR kez daha denenmeli; eskiden doğrudan
    /// kalıcı "tekrar giriş yap" durağıydı.
    #[test]
    fn suresi_dolan_token_bir_kez_tazelenip_denenir() {
        assert_eq!(unauthorized_next(false, true), AuthRetry::Retry);
        // Tazelenemediyse ya da tazelenmiş token da reddedildiyse: dur.
        assert_eq!(unauthorized_next(false, false), AuthRetry::Stop);
        assert_eq!(unauthorized_next(true, true), AuthRetry::Stop);
    }

    #[test]
    fn durum_makinesi_kullaniciya_mesaj_tasiyor() {
        let s = LinkState::Stopped {
            message: "linkUnauthorized".into(),
        };
        match s {
            LinkState::Stopped { message } => assert!(!message.is_empty()),
            _ => panic!("Stopped bekleniyordu"),
        }
    }
}
