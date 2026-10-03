# ALEPH EDGE — Product Requirements Document

**Sürüm:** 1.0.0 · **Tarih:** 2026-07-07 · **Durum:** Onay bekliyor
**Ürün:** Sentinel Aleph üyeliğiyle çalışan, masaüstü (Rust/Tauri) otomatik trade botu
**Slogan:** *Dünyanın en dürüst oto-trade botu — kanıtlanmış sinyal, korunan sermaye, kilitli anahtar.*

---

## 1. Vizyon ve Felsefe

Aleph Edge, Sentinel Aleph'in doğasının masaüstündeki uzantısıdır. O doğa üç ilkedir:

1. **Dürüstlük** — Bot yalnızca *qualified* (gerçek TP/SL ile çözülmüş) sonuçları sayar. Sahte winrate, gizlenen kayıp, kozmetik metrik yoktur. PnL ekranında komisyon ve slipaj dahil **net** rakam gösterilir.
2. **Koruma** — Sentinel'in sunucu tarafındaki koruma katmanları (BTC trend guard, sinyal geçersizleştirme, confirmation gate) botta birebir karşılık bulur: **geçersizleştirilen sinyalin pozisyonu botta da kapanır.** Kullanıcı fark etmese bile bot fark eder.
3. **Sadelik** — Kullanıcı işlem yapmak ister; ayar cangılında kaybolmak istemez. Varsayılan akış: *Aç → Vault kilidini çöz → Bot seç → Başlat.* Dört adımdan fazlası ürün hatasıdır.

**Kırmızı çizgiler (pazarlanamaz, tartışılamaz):**
- Kullanıcının borsa API anahtarları, secret'ları ve kripto varlık bilgileri **hiçbir koşulda** ribqa.com'a, veritabanına veya üçüncü tarafa gönderilmez. Anahtarlar yalnızca kullanıcının cihazında, şifreli Vault içinde yaşar.
- R:R hiçbir zaman 1.0'ın altına düşmez (Sentinel sunucu kuralıyla aynı).
- Bot, üyelik doğrulanmadan tek bir emir dahi gönderemez.

---

## 2. Hedef Kullanıcı ve Başarı Metrikleri

**Kullanıcı:** Sentinel Aleph abonesi; sinyalleri elle uygulamak yerine botun disiplinli uygulamasını isteyen bireysel trader. Teknik bilgi varsayılmaz.

**Başarı metrikleri (ilk 90 gün):**
| Metrik | Hedef |
|---|---|
| Kurulum → ilk canlı bot süresi | < 5 dakika |
| Bot uptime (kullanıcı oturumu içinde) | ≥ %99 |
| Sinyal geliş → emir gönderim gecikmesi | < 800 ms |
| Geçersizleştirilen sinyalde pozisyon kapatma | < 5 sn |
| Vault dışına anahtar sızıntısı | 0 (mutlak) |
| Bot PnL sapması (sinyal teorik PnL'e karşı) | ±%15 içinde |

---

## 3. Teknoloji ve Mimari

### 3.1 Stack
- **Kabuk:** Tauri 2.x (Rust core + system webview) — küçük binary, düşük RAM, OS keychain erişimi
- **Çekirdek (Rust):** trade engine, borsa adaptörleri, Vault, risk yöneticisi, WS istemcileri (`tokio`, `reqwest`, `tungstenite`, `serde`, `ring`/`argon2`, `keyring`)
- **UI:** React + TypeScript + Vite; stil Tailwind; animasyon **Motion** (Framer Motion halefi) — Emil Kowalski ekolü
- **i18n:** `react-i18next` + ICU mesaj formatı
- **Yerel durum:** SQLite (yalnızca işlem geçmişi/metrik önbelleği — anahtar içermez)

### 3.2 Katman diyagramı
```
┌────────────────────────── Tauri (tek masaüstü uygulama) ─────────────────────────┐
│  UI (React/TS)                     Rust Core                                     │
│  ┌──────────────┐  invoke/events   ┌──────────────────────────────────────────┐  │
│  │ Dashboard    │◄────────────────►│ BotOrchestrator (futures/spot/pump)      │  │
│  │ Bot Kartları │                  │ RiskManager (risk seviyesi → limitler)   │  │
│  │ PnL/Metrik   │                  │ ExchangeAdapters (REST+WS, borsa başına) │  │
│  │ Ayarlar      │                  │ Vault (Argon2id + AES-256-GCM + keyring) │  │
│  │ Vault ekranı │                  │ SignalClient (Sentinel SSE/WS + REST)    │  │
│  └──────────────┘                  │ HealthMonitor (borsa/sinyal/BTC pulse)   │  │
│                                    └──────────────────────────────────────────┘  │
└───────────────┬──────────────────────────────┬───────────────────────────────────┘
                │ TLS                          │ TLS (imzalı edge token)
        Borsalar (emir/veri)          Sentinel Aleph (ribqa.com)
        anahtarlar CİHAZDA kalır      sinyal + üyelik + FR/LD + BTC pulse
```

### 3.3 Sentinel Aleph entegrasyonu
- **Üyelik/kimlik:** ribqa.com login → JWT + refresh. Bot her açılışta üyelik durumunu doğrular; abonelik yoksa botlar *devre dışı* (UI görünür, işlem kilitli).
- **Sinyal akışı:** Birincil kanal mevcut **SSE stream** (`/api/v1/stream`, edge-imzalı ticket). Sinyal payload'ı: entry/SL/TP (eksiksiz — eksik geometriyle sinyal yayınlanmaz, sunucu garantisi), yön, mod, güven, confluence, **invalidation olayları** (`btc_trend_flip` vb.).
- **Veri servisleri:** FR + liquidity depth Sentinel API'den, DataHub üzerinden ve tüm USDT-M perp'leri kapsayarak: `/api/v1/market/funding-rates` (oran yüzde, 60 sn önbellek) ve `/api/v1/market/depth/{sym}` (±%1 derinlik, 15 sn önbellek). 2026-10-01 öncesi kaynaklar (arbitraj beslemesi, akıştaki emir defterleri) yalnız ~18 sembolü kapsıyordu ve futures sinyal botları altcoin sinyallerini atlıyordu. BTC pulse `/api/v1/market/btc-macro`.
- **MCP:** Sentinel'in mevcut MCP sunucusu (backend'de hazır) *ikincil* kanal olarak değerlendirilir — ajan-destekli operasyon/tanılama senaryoları için. Emir yolu üzerinde MCP KULLANILMAZ (gecikme ve determinizm nedeniyle); emir yolu doğrudan borsa API'sidir.

### 3.4 Borsa adaptörleri
Ortak Rust trait: `ExchangeAdapter { spot_order, futures_order, positions, balance, funding_rate, orderbook_depth, ws_subscribe, health }`.

| Katman | Borsalar | Not |
|---|---|---|
| **Tier 1 (MVP)** | Binance, OKX, Bybit, MEXC Global | Spot + USDT-M futures, tam WS |
| **Tier 2 (TR)** | BTCTurk, Paribu | Yalnız spot (futures yok); TRY pariteleri |
| **Tier 3 (bölgesel)** | CoinDCX (Hindistan), Bitso (Meksika/LatAm), Mercado Bitcoin (Brezilya), Ripio (Arjantin) | Spot ağırlıklı; API yeterliliğine göre sıralı |
| **Pakistan notu** | Lisanslı yerel borsa + kamu API'si yok; kullanıcılar fiilen Binance kullanıyor → Binance adaptörü bu pazarı kapsar. PRD dürüstlük ilkesi gereği "Pakistan borsası" vaat edilmez. | |

Her adaptör: rate-limit yönetimi, idempotent emir (clientOrderId), otomatik yeniden bağlanan WS, `health()` (gecikme + son başarılı çağrı).

---

## 4. Tasarım Sistemi

### 4.1 Emil Kowalski skill'leri (kurulu: `.claude/skills/`)
- **`emil-design-eng`** — tüm UI bileşen kararlarında zorunlu referans
- **`animation-vocabulary`** — animasyon süre/easing/choreography sözlüğü
- **`review-animations`** — her UI PR'ında animasyon incelemesi bu skill ile yapılır

### 4.2 Temalar (en az 2)
| Tema | Karakter |
|---|---|
| **Edge Light (varsayılan)** | Beyaz zemin (#FFFFFF), açık gri yüzeyler (#F7F7F8 / #EFEFF1), grafit metin, tek vurgu rengi (Sentinel aqua). Sakin, klinik, güven veren. |
| **Edge Dark** | Koyu grafit zemin, aynı vurgu dili. |
| (Gelecek) Yüksek kontrast | Erişilebilirlik. |

Tema anahtarı: sistem temasını izle / manuel. Tüm renkler design-token (CSS custom properties) üzerinden — hardcoded renk yasak.

### 4.3 Hareket dili
- Referans kalite çıtası: *Claude Opus "düşünme" toggle'ı* tarzı — küçük, amaçlı, yaylı (spring) mikro-animasyonlar.
- Kurallar (animation-vocabulary'den): süreler 150–300 ms; `ease-out` girişler, spring toggle'lar; layout kaymalarında FLIP; asla dekoratif-amaçsız animasyon; `prefers-reduced-motion` desteği zorunlu.
- Animasyonlu ana bileşenler: **Bot Başlat/Durdur butonu** (durum morph'u: idle → arming → live, yayla), risk seviyesi seçici (renk + genişleme geçişi), Vault kilit/kilit-açma (fiziksel his), sinyal kartı giriş/çıkışı, PnL sayaç tıkırtısı (tabular-nums + spring count-up).

### 4.4 Sadelik sözleşmesi
- Ana ekran = **tek ekran**: üstte sağlık şeridi, ortada bot kartları, altta canlı işlem akışı. Ayarlar ayrı pencerede.
- Bir botu başlatmak: kart üzerinde **tek buton**. Gelişmiş ayarlar "detay" içinde katlanır.
- Boş durumlar yönlendirir: "Vault kilitli → Kilidi aç", "Üyelik yok → Giriş yap".

---

## 5. Özellikler

### 5.1 Bot türleri (3)
| Tür | Pazar | Sinyal kaynağı | Not |
|---|---|---|---|
| **Futures** | Seçili borsanın USDT-M futures pazarı | Sentinel canlı sinyalleri | Kaldıraç ayarlı |
| **Spot** | Seçili borsanın spot pazarı | Sentinel canlı sinyalleri | Kaldıraç yok; yalnız long mantığı (spot short yok — dürüst kısıt, UI'da açıkça yazar) |
| **Pump** | Spot veya futures | Sentinel pump radar sinyalleri | En riskli tür; risk seviyesi "Hırslı"nın altındaysa devre dışı |

Kullanıcı **birden fazla botu aynı anda** çalıştırabilir (ör. 1 futures + 1 spot + 1 pump), her biri bağımsız yapılandırılır.

Bu üç tür **sinyal botudur**. Bunlara ek olarak iki **strateji botu** türü vardır: **DCA** ve **Grid** (bkz. §5.9). Strateji botları sinyal beklemez, kendi kural setiyle çalışır ve şimdilik **yalnızca paper modda** çalışır.

### 5.2 Bot ayarları (bot başına)
1. **Maks eşzamanlı pozisyon** ("kaç adet bot"): bu botun aynı anda taşıyabileceği pozisyon sayısı (1–20). Ör: Spot botu 10 → borsanın spot pazarında aynı anda en fazla 10 pozisyon.
2. **Pozisyon sermayesi:** her bir pozisyonun açılış tutarı (quote para biriminde, ör. 100 USDT). Toplam maruz kalınabilecek risk = maks pozisyon × sermaye — UI bunu **canlı hesaplayıp gösterir**.
3. **Kaldıraç** (yalnız futures): 1x–maks; üst sınırı risk seviyesi belirler (aşağıda).
4. **Borsa + pazar seçimi:** Vault'ta anahtarı bulunan borsalar listelenir.
5. **Sinyal filtresi (opsiyonel, katlanır):** min güven, yön (hepsi/long/short), sembol beyaz listesi.

### 5.3 Risk seviyeleri (global ayar; botların sınırlarını belirler)
Her kelime kendini tanımlayan renkte, seçici animasyonlu:

| Seviye | Renk | Maks kaldıraç | Maks eşzamanlı poz. (tüm botlar) | Poz. başına sermaye tavanı | Günlük zarar kill-switch | FR/LD ön kontrol eşiği |
|---|---|---|---|---|---|---|
| **Temkinli** | Buz mavisi | 2x | 3 | bakiyenin %2'si | -%2 | katı |
| **Sakin** | Yeşil | 3x | 5 | %4 | -%4 | katı |
| **Dengeli** | Amber | 5x | 8 | %6 | -%6 | normal |
| **Hırslı** | Turuncu | 10x | 12 | %10 | -%10 | normal |
| **Aç Gözlü** | Kırmızı | 20x | 20 | %15 | -%15 | gevşek + ekstra onay diyaloğu |

- Günlük zarar limitine ulaşan **tüm botlar durur**, pozisyonlar kullanıcı tercihine göre kapanır/korunur (varsayılan: kapat). Ertesi gün (00:00 UTC) manuel yeniden başlatma ister — otomatik devam yok (Sentinel felsefesi: kayıp gizlenmez, üzerine düşünülür).
- Aç Gözlü seçimi, riskleri sayısal örnekle anlatan onay ekranı ister.

### 5.4 İşlem-öncesi kontroller (her emirden önce, Rust core'da)
1. Üyelik geçerli mi
2. Sinyal hâlâ geçerli mi (invalidation kontrolü — `btc_trend_flip` gelen sinyale girilmez)
3. **Funding rate:** futures'ta pozisyon yönünün aleyhine aşırı FR varsa (eşik risk seviyesine bağlı, ör. Temkinli: |FR| > %0.05/interval) → işlem atlanır, nedeni işlem akışına yazılır
4. **Liquidity depth:** hedef pozisyon büyüklüğü, defterin ±%1 derinliğinin belirli oranını aşıyorsa (slipaj riski) → işlem küçültülür veya atlanır
5. Risk bütçesi: günlük zarar + eşzamanlı pozisyon + sermaye tavanı kontrolleri
6. Geometri tutarlılığı: long'da SL < giriş < TP1, short'ta TP1 < giriş < SL. Dolum anında fiyat TP1'i ya da SL'yi geçmişse veya girişten %0,5'ten fazla aleyhe kaydıysa işlem açılmaz.
   *Değişiklik 2026-09-16:* eski "R:R ≥ 1.0" kuralı kaldırıldı. Sentinel'in canlı geometrisinde (stop 2,5×ATR, TP1 medyanı ≈0,6R, kazananların %98,5'inde TP1 < 1R) bu kural neredeyse her sinyali reddediyordu.

Atlanan her işlem, gerekçesiyle görünür ("FR aleyhte: -%0.12" gibi) — sessiz atlama yok. Geçici sebepler (fiyat alınamadı, pozisyon limiti dolu vb.) sinyal süresi dolana kadar yeniden denenir; kalıcı sebepler bir kez yazılır.

**Karar 2026-09-16 — invalidation açık pozisyonu kapatmaz.** Sentinel'de BTC korumasının dolmuş pozisyonları kapatması 360 kez ölçüldü ve defterin en büyük kayıp kalemi oldu (−163 puan; açık bırakılsalar +20). Sunucuda kapatıldı (`BTC_GUARD_CLOSE_OPEN_POSITIONS=false`). Aleph Edge aynı kararı uygular: invalidation dolmamış sinyale girişi engeller, açık pozisyonu yalnızca not olarak bildirir. BTC çöküş ilanı yalnızca yeni **long** girişleri durdurur. Kullanıcının pozisyon başına zarar limiti BTC durumundan bağımsız çalışır.

### 5.5 Vault (yerel anahtar kasası)
- **Oluşturma:** Kullanıcı vault parolası belirler → Argon2id (yüksek maliyet parametreleri) → AES-256-GCM master key. Dosya: `vault.edge` (cihazda). OS keychain'e yalnızca *salt referansı* yazılır, parola/anahtar asla.
- **Kullanım:** Uygulama açılışında Vault ekranı → parola → borsa anahtarları belleğe açılır → botlar "hazır". Oturum kapanınca/uygulama kilitlenince, **bizim ayırdığımız tamponlar** zeroize ile silinir. Bu iddia kendi tamponlarımızla sınırlıdır: HTTP yığınının başlık tamponları, `hmac` kütüphanesinin iç anahtar durumu ve JSON çözümleyicisinin ara tamponları bizim erişimimizde değil ve silinmez (2026-09-20 denetimi).
- **İçerik:** borsa başına API key/secret/passphrase + izin notu (yalnız-trade anahtarı önerilir; withdraw izinli anahtar tespit edilirse uyarı).
- **Mutlak kural:** Vault içeriği hiçbir telemetriye, loga, crash raporuna, ribqa.com'a gitmez. Ağ katmanında anahtar yalnızca ilgili borsanın resmî API host'una gider (allowlist).
- Parola kurtarma YOKTUR (dürüst kısıt, UI'da yazar): parola kaybı = vault sıfırlama = anahtarları yeniden girme.

### 5.6 PnL ve performans ekranı
- **Kazanç/kayıp:** günlük/haftaflık/aylık net PnL (komisyon+funding dahil), equity eğrisi, bot türü ve borsa kırılımı.
- **Performans metrikleri:** qualified winrate (Sentinel tanımıyla aynı: gerçek TP/SL), profit factor, maks drawdown, ortalama R, işlem sayısı, fill oranı.
- **İşlem akışı:** her pozisyon satırında → sembol, yön, giriş/çıkış, PnL **+ o anki FR ve LD değerleri** (hem açılışta kaydedilen hem canlı).
- Veriler yerel SQLite'ta; kullanıcı isterse tek tıkla CSV dışa aktarım.

### 5.7 Sağlık şeridi (ana ekran üstü, her zaman görünür)
- **Borsa bağlantısı:** borsa başına yeşil/amber/kırmızı + gecikme ms
- **Sinyal sağlığı:** SSE bağlı mı, son sinyal ne zaman, gecikme
- **BTC Pulse:** Sentinel BTC makro (trend/güç/faz) — mini sparkline + renk
- **Vault durumu:** kilitli/açık
- **Üyelik:** aktif/dolmak üzere

### 5.8 Çok dillilik (i18n)
En çok kripto işlemi yapan ilk 7 ülkenin dilleri + Türkçe (Chainalysis benimseme endeksi tabanlı):

| Dil | Kapsadığı pazar |
|---|---|
| İngilizce (EN) | ABD, Nijerya, Filipinler, küresel varsayılan |
| Türkçe (TR) | Türkiye |
| Hintçe (HI) | Hindistan |
| Vietnamca (VI) | Vietnam |
| Endonezce (ID) | Endonezya |
| Rusça (RU) | Rusya, Ukrayna, BDT |
| Portekizce (PT-BR) | Brezilya |
| İspanyolca (ES) | Arjantin, Meksika, LatAm |

Sayı/para/tarih formatları locale'e göre; RTL gerekmiyor (bu set LTR).

### 5.9 Strateji botları: DCA ve Grid (yalnızca paper)
**Karar 2026-10-01 (sahip onayı: "paper DCA/Grid'i onaylıyorum"):** Aleph Edge bir bot platformuna dönüşüyor. DCA ve Grid botları ürüne girer, ama **yalnızca paper modda**. Gerçek emir yolu yapısal olarak kapalıdır:
- Veritabanı paper olmayan strateji kaydını reddeder (`CHECK(paper=1)`).
- Motorda canlı emir yolu yoktur; `STRATEGY_LIVE_ALLOWED=false` testle sabittir.
- Canlıya açılması ayrı bir sahip kararı, ayrı bir güvenlik incelemesi ve testnet denemesi ister.

**DCA botu:** başlangıç emri + fiyat düştükçe sınırlı sayıda ek alım (ilk adım %, adım çarpanı, hacim çarpanı, en fazla ek alım sayısı), ortalama maliyete göre kâr al (opsiyonel trailing), opsiyonel stop ve süre sınırı, yeniden başlama bekleme süresi. **Bütçe sabittir**; kademe planı ve en kötü durumda gereken sermaye formda önceden gösterilir.
**Grid botu:** alt/üst fiyat, ızgara sayısı, aritmetik/geometrik, yatırım tutarı, long/short/nötr, kaldıraç (risk seviyesinin tavanıyla), aralık dışına çıkınca stop.

**Doldurma modeli:** paper dolumlar, araştırma simülatörüyle aynı temkinli mum-içi kuralı kullanır (aleyhe uç önce). Fiyat verisi Binance'in herkese açık 1 dakikalık mumları; fonlama Binance'in herkese açık fonlama geçmişinden.

**Risk:** strateji botlarının toplam bütçesi risk seviyesine göre tavanlıdır. Ayrıca bir portföy düşüş sigortası vardır (zirveden −%15). Sigorta bot başınadır: `dca_long_classic` şablonundan kurulan botlarda varsayılan olarak kapalı, Grid ve elle kurulan DCA botlarında açıktır; kullanıcı her botta değiştirebilir (karar 2026-10-01: şablonun 2024-10..2026-09 motor koşusunda sigortasız +%41,5, sigortalı +%29,8; tek tetiklenme stopsuz DCA döngülerini dipte kapattı). Sinyal botlarının günlük zarar limiti strateji botlarını saymaz ve durdurmaz (karar 2026-10-01): stopsuz DCA merdiveni günlük limitten derine iner, o limitle kapatılırsa şablonun kazancı kaybolur. "Hepsini kapat" ve telefondan "hepsini durdur" strateji botlarını da kapsar. Döngü kapatma "CLOSE", silme "DELETE" yazılarak onaylanır.

**Şablonlar (Presets):** Uygulamada yalnızca 2 yıllık veride, hiç görülmemiş test döneminde de kanıtlanmış ayarlar şablon olarak sunulur ve **"geçmiş simülasyon"** etiketiyle gösterilir; asla gerçek işlem geçmişi gibi sunulmaz. 2026-10-01 itibarıyla tek şablon:
- `dca_long_classic`: işlem hacmine göre ilk 5 büyük coin, 1x izole, başlangıç + 8 ek alım (adım %2,5 ×1,3; hacim ×1,4), ortalamadan %2 kâr al, stop yok. Test (Şub–Eyl 2026): bot başına aylık +%1,98 (GA %1,52–2,46), 40 botun 40'ı artıda, en kötü botun düşüşü −%20,7.
- Uyarılar şablonun yanında açıkça yazar: stop yoktur; işlem aylarca zararda bekleyebilir (doğrulama döneminde −%46,8, 329 gün); %60'tan derin bir düşüş son ek alımı da geçer; 2x–3x kaldıraçla veya küçük coinlerle aynı ayar batmıştır.
- Test edilip **reddedilenler:** tüm Grid türleri (nötr grid testte bot başına −%1,42), short DCA, filtreli DCA, stop'lu DCA. Tetikleyiciyle başlayan DCA "riskli" sınıfında, şablon değildir.

---

## 6. Kapsam Dışı (v1 için bilinçli NO)
- Withdraw/transfer işlemleri (asla — yalnız trade)
- Kendi sinyal üretimi (sinyal tek kaynak: Sentinel)
- Mobil uygulama, web versiyonu
- Sınırsız martingale (bütçesi ve ek alım sayısı sınırsız, kaybı büyüterek gizleyen strateji). *Değişiklik 2026-10-01:* sabit bütçeli, sınırlı kademeli DCA ve Grid botları kapsamdan çıkarıldı ve §5.9'da paper-only olarak ürüne alındı.
- DCA/Grid botlarıyla gerçek emir (ayrı sahip kararına kadar)
- Sosyal/copy-trade
- Pakistan yerel borsası (mevcut değil — Binance kapsar)

---

## 7. Güvenlik Gereksinimleri (özet)
1. Anahtarlar: cihazda, Argon2id+AES-256-GCM, kendi tamponlarımızda zeroize, keychain'de yalnız tuz
2. Ağ: TLS pinning (borsa + ribqa.com), anahtar yalnız borsa host allowlist'ine
3. Emirler: idempotent clientOrderId, replay koruması, imza saat sapması yönetimi
4. Loglar: anahtar/secret/parola asla loglanmaz. Bunu sağlayan bir "scrubber" bileşeni YOK; Rust tarafında üretim kodunda tek bir log çağrısı var (`link/session.rs`) ve o da hata metni basıyor; ikinci çağrı yalnızca testin içinde. Sırlar zaten IPC sınırını geçmiyor (2026-09-20 denetimi).
5. Güncelleme: Tauri updater imza doğrulamalı
6. Kill-switch hiyerarşisi: kullanıcı > günlük zarar limiti > bağlantı kaybı davranışı. Sinyal invalidation yalnızca henüz dolmamış sinyale girişi engeller; açık pozisyonu kapatmaz (bkz. §5.4 notu) (bağlantı koparsa yeni emir yok; mevcut pozisyon SL/TP'si borsada zaten duruyor — bot her pozisyonu borsa-tarafı SL/TP emriyle açar, "yazılım-SL" tek başına yasak)

---

## 8. Yol Haritası

| Faz | İçerik | Süre hedefi |
|---|---|---|
| **F0 — İskelet** | Tauri kabuk, tema sistemi, i18n altyapısı, tasarım tokenları, sağlık şeridi mock | 1 hafta |
| **F1 — Vault + Üyelik** | Vault tam akış, ribqa.com auth, üyelik kapısı | 1 hafta |
| **F2 — Sinyal + Tek borsa** | SSE sinyal istemcisi, Binance adaptörü (spot+futures), paper-mode bot döngüsü | 2 hafta |
| **F3 — Canlı Futures/Spot bot** | Risk yöneticisi, işlem-öncesi kontroller (FR/LD), PnL ekranı, Sentinel yönetim planı (başabaş 0,5R, kısmi %50 @1R yalnızca hedeften yakınsa, ufuk) | 2 hafta |
| **F4 — Çoklu borsa** | OKX, Bybit, MEXC; BTCTurk+Paribu (spot) | 2 hafta |
| **F5 — Pump botu + bölgesel** | Pump radar entegrasyonu, CoinDCX/Bitso/Mercado/Ripio, kalan diller | 2 hafta |
| **F6 — Cila** | Animasyon review (skill ile), tema koyucu, edge-case, beta | 1 hafta |
| **F7 — Bot platformu** (2026-10-01) | Çok sayfalı uygulama (sol menü, üst çubuk, durum çubuğu), paper DCA/Grid motorları ve formları, kanıtlı şablonlar sayfası. Sıradaki: uygulama içi backtest (DataHub mumlarıyla, aynı strateji koduyla), şablonların sunucudan sunulması, çevirilerin anadil kontrolü | yapıldı (paper) |

Her faz sonunda: `review-animations` skill'i ile UI incelemesi + gerçek-para öncesi paper-mode zorunlu doğrulama. **Canlı emir gönderme yeteneği F3 sonuna kadar feature-flag arkasında kapalı.**

---

## 9. Açık Sorular (onay/karar gerekli)
1. Pump botunun futures'ta da çalışması isteniyor mu, yoksa yalnız spot mu? (PRD şu an ikisine de izin veriyor, risk seviyesi kapısıyla)
2. Bölgesel borsaların (CoinDCX/Bitso/Mercado/Ripio) API anlaşması/KYC kısıtları — hangileri gerçekten otomatik trade'e izin veriyor, F5 öncesi doğrulanacak
3. Sentinel tarafında bot-özel sinyal aboneliği (mevcut SSE yeterli mi, bot-cancel push kanalı eklenecek mi — sunucuda `FeedSignalToRoute`'un cancel yolu YOK, bilinen boşluk; F3'te sunucuya küçük ek gerekir)
4. Fiyatlandırma: bot mevcut üyeliğe dahil mi, ayrı katman mı?
5. ~~Strateji botu bütçe tavanları ve portföy sigortası~~ **Karar 2026-10-01:** risk seviyesine göre bakiyenin %20/30/40/60/80'i; portföy düşüş sigortası −%15 (zirveden).
6. ~~DCA/Grid zararları ve günlük limit~~ **Karar 2026-10-01:** ayrı. Günlük limit yalnız sinyal botlarını sayar, durdurur ve kapatır.
7. ~~Backtest mum veri kaynağı~~ **Karar 2026-10-01:** DataHub. Uygulama mumları Sentinel API üzerinden DataHub'dan alır, borsaya doğrudan gitmez. Fonlama verisi yok; futures backtest raporu "fonlama dahil değil" yazar.
8. ~~DCA/Grid canlı emir şartı~~ **Karar 2026-10-01:** en az 30 gün paper + testnet denemesi + ayrı güvenlik incelemesi; ardından ayrı sahip onayı.

---

*Bu PRD, Sentinel Aleph üretim sisteminin 2026-07-05→07 döneminde kanıtlanmış derslerini (ters-seçilim, geometri dürüstlüğü, koruma katmanları, qualified-metrik disiplini) doğrudan ürün gereksinimlerine çevirir.*
