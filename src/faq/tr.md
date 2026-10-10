# Sık sorulan sorular

Bu sayfa, kenar çubuğundaki her menüyü sırayla anlatır: ne işe yarar, ekranda ne görürsünüz ve en sık sorulan sorular. Ekran görüntülerindeki bütün rakamlar örnek veridir; her görüntünün altında "Örnek veri" etiketi bulunur. Sizin hesabınız ya da bir işlem kaydı değildir.

> Ayrıntılı DCA ve Grid anlatımı için [Rehber](#/guide) sayfasına bakın. Bu sayfa menülerin kullanımını anlatır.

## Uygulamanın düzeni {#layout}

![Pano ekranı: solda kenar çubuğu, üstte başlık çubuğu, altta durum çubuğu](shot:dashboard)

Her sayfa aynı düzeni kullanır:

- **Sol kenar çubuğu:** menüler. Gruplar: Genel bakış, Botlar, Piyasa, Araştırma, Portföy, Kontrol. En altta **Ayarlar**, **Hesap**, tema düğmesi ve kenar çubuğunu daraltma düğmesi bulunur.
- **Sol panel:** o sayfanın filtreleri ya da ayarları.
- **Orta alan:** sayfanın asıl işi; tablolar, formlar, grafikler.
- **Sağ panel:** özetler ve durum bilgisi. Dar pencerede sağ panel gizlenir; başlıktaki **Özet paneli** düğmesiyle açılır.
- **Başlık çubuğu (üstte):** sayfa adı ve düğmeleri, mod etiketi (**Simüle** ya da **Simüle + Canlı**), testnet kullanılıyorsa **Testnet borsa** rozeti, uyarı zili, Binance ve Sentinel bağlantı etiketleri, hesabınız.
- **Durum çubuğu (altta):** borsa bağlantıları ve gecikme, Sentinel sinyal akışı ve son sinyalin yaşı, BTC fiyatı ve rejimi, kasanın durumu (açık ya da kilitli), üyelik durumu ve sürüm numarası.

**"Simüle" etiketi bir düğme mi?** Hayır, yalnızca bilgi gösterir. Bütün botlar simüle (paper) çalışıyorsa **Simüle**, en az bir bot gerçek parayla işlem yapıyorsa **Simüle + Canlı** yazar. Gerçek para her bot için ayrı ayrı açılır.

**Binance ve Sentinel etiketlerindeki noktalar ne anlatır?** Yeşil nokta bağlantının çalıştığını gösterir. Etikete tıklarsanız bağlantı hemen yeniden denetlenir. Binance etiketinde "anahtar yok" yazıyorsa kasada kayıtlı bir Binance anahtarı yoktur; paper botlar için anahtar gerekmez.

**Uyarı zilindeki sayı ne?** Okunmamış uyarılar: bütçe ya da pozisyon sınırının dolması, zarar durdurma, kapanan işlemler, bugün kapanan DCA/Grid döngüleri, stopla kapanan DCA/Grid döngüleri, sona eren bir DCA/Grid botu, kasanın otomatik kilitlenmesi, sinyal akışının kesilmesi, BTC rejimi değişikliği. Hangi uyarıları göreceğinizi **Ayarlar → Bildirimler** sekmesinden seçersiniz.

**Kenar çubuğundaki sayılar ve noktalar ne?** Bot menülerinin yanındaki sayı çalışan bot sayısıdır. **Pozisyonlar ve emirler** yanındaki sayı açık pozisyon ve açık döngü sayısıdır. **Sinyaller** yanındaki yeşil nokta sinyal akışının bağlı olduğunu gösterir. **Risk ve güvenlik** yanında nokta varsa günlük stop tetiklenmiş ya da BTC rejimi normal değildir; **LIVE** etiketi varsa en az bir bot gerçek parayla çalışıyordur.

**Temayı nasıl değiştiririm, kenar çubuğunu nasıl daraltırım?** Kenar çubuğunun en altındaki tema düğmesi Açık → Koyu → Yüksek kontrast → Sistem sırasıyla değişir. Yanındaki ok kenar çubuğunu daraltır; klavyeden Ctrl+B de aynı işi yapar.

**Neden kilit ekranı geliyor?** Kasa (borsa anahtarlarınızın şifreli kaydı), pencerede belirli bir süre hiçbir işlem yapılmazsa kendini kilitler: fare, klavye ya da kaydırma hareketi olmaması. İçinde anahtar olmayan kasa kilitlenmez. Botlar kilitliyken de çalışmaya devam eder; kilit yalnızca arayüzü ve anahtarları korur. Süreyi **Ayarlar → Borsa anahtarları → Otomatik kilit** alanından seçersiniz: 5 dk ile 1 ay arasında. Gerçek para açık bir pozisyonu tutarken kasa otomatik kilitlenmez.

**Uygulamayı ikinci kez açarsam ne olur?** Aynı anda yalnızca bir kopya çalışır. Uygulamayı yeniden açmak, açık olan pencereyi öne getirir. TESTNET sürümü ile normal sürüm ayrı uygulamalardır ve yan yana çalışabilir.

## Pano {#dashboard}

Pano, neyin çalıştığını, ne kazandırıp ne kaybettirdiğini ve neyin dikkat istediğini tek ekranda gösterir. Uygulamada olmayan bir rakam tahmin edilmez; veri yoksa alan boş kalır.

- **Sol panel:** **Kapsam** (çalışan simüle ve canlı bot sayısı), **Botlar** (her türden kaç bot çalışıyor), **BTC rejimi** ve **Sinyal akışı** kartları.
- **Orta alan:**
  - **Başlarken** listesi: Sentinel'e giriş, bir botun ayarlanması ve çalışması. Her bot türü sayılır. Henüz bot yoksa liste, testini geçen DCA Long Classic şablonunu önerir. Sinyal akışı adımı ancak bir sinyal botu ayarlandığında görünür. Hepsi tamamlanınca **Listeyi gizle** ile kaldırılır.
  - Özet kutuları: **Bugün gerçekleşen**, **Net K/Z, tüm zamanlar**, **Kullanılan günlük zarar**, açık pozisyon, **Kullanılan sermaye**, **Sinyal kazanç oranı**.
  - **Ayarlı botlar** tablosu: her botu buradan başlatıp durdurabilirsiniz.
  - **Son işlemler**: kapanan son 10 işlem.
- **Sağ panel:** **Uyarılar**, **Günlük stop** kartı ve bir borsa hesabı bağlıysa **Borsa hesabı (gerçek)** paneli.

**K/Z kutularına DCA ve Grid botları dahil mi?** Evet. **Bugün gerçekleşen** ve **Net K/Z, tüm zamanlar**, kapanan DCA/Grid döngülerini de ekler; ücretler ve fonlama düşülür, silinen botlar da sayılır. Her kutunun altındaki not toplamı ayırır: "Sinyal x · DCA/Grid y". **Sinyal kazanç oranı** ve **Kullanılan günlük zarar** yalnızca sinyal botlarını sayar.

**Sinyal kazanç oranının altında "değerlendirmek için az" yazıyor, neden?** 30'dan az sinyal işlemi kapandıysa kazanç oranı güvenilir değildir; uygulama bunu açıkça yazar.

**"Kullanılan günlük zarar" neyi ölçer?** Sinyal botlarının bugün gerçekleşen zararının, günlük stop sınırına oranını. Sınır, Risk ve güvenlik sayfasındaki simüle bakiye ve günlük stop yüzdesinden hesaplanır. Açık pozisyonların henüz gerçekleşmemiş zararı sayılmaz. DCA/Grid döngüleri de sayılmaz: bu botlar günlük stopun dışındadır.

## Tüm botlar {#all-bots}

![Tüm botlar: solda filtreler, ortada sinyal botları ile DCA ve Grid botları](shot:bots)

Her türden botu (sinyal, DCA, Grid) tek tabloda gösterir.

- **Sol panel (filtreler):** arama (bot adı ya da sembol), tür, durum, piyasa (Spot/Futures), mod (Paper/LIVE), borsa ve **Filtreleri temizle**. Filtreler adres satırında saklanır.
- **Orta alan:** sinyal botları ve **DCA ve Grid botları** ayrı bölümlerde. Birden çok sinyal botunu seçip birlikte durdurabilirsiniz.
- **Sağ panel:** her bot türü için özet ve **Dikkat gerekenler** listesi: günlük stop yüzünden durmuş sinyal botları, duraklatılmış ya da sona ermiş DCA/Grid botları.

**Yeni bir sinyal botu oluşturabilir miyim?** Hayır. Üç sinyal botu sabittir: **Futures**, **Spot** ve **Pump**. Yalnızca ayarlarını değiştirir, başlatır ya da durdurursunuz. DCA ve Grid botlarından ise en fazla 10 tane oluşturabilirsiniz.

**Bir botu neden silemiyor ya da değiştiremiyorum?** Her paper botun satırında onu silen bir çöp kutusu simgesi vardır. Ayarlar botun kendi sayfasındadır: bot adına tıklayın. Ayrıntılar [DCA botları](#/faq?s=dca-bots) bölümünde.

## Piyasa trendi {#market-trend}

Üst çubuktaki **BTC trendi** etiketi ve bot sayfalarındaki **Piyasa trendi** paneli BTC'nin yavaş trendini gösterir. Yalnızca bilgidir: hiçbir bot bunu okumaz ve hiçbir şey kendiliğinden değişmez.

- **Nasıl okunur:** BTC'nin günlük kapanışı 50 günlük ortalamasıyla karşılaştırılır. Üstündeyse **Yükseliş**, altındaysa **Düşüş**. Yön, ancak öbür tarafta üst üste 2 günlük kapanıştan sonra değişir. Kaynak: Binance günlük BTC/USDT kapanışları, 15 dakikada bir okunur.
- **Başlangıç / gün:** şu anki yönün ne zaman teyit edildiği.
- **Ortalamaya uzaklık:** son kapanışın 50 günlük ortalamanın ne kadar üstünde ya da altında olduğu.
- **Son 12 ayda yön değişimi:** trendin kaç kez döndüğü. Sayı yüksekse trend, hareket kazandırmadan önce sık sık döner.
- **Bot kurma formlarında:** şu anki trendi gösteren bir not çıkar; botun yönü trende ters ise uyarır (düşüş trendinde long bot, yükseliş trendinde short bot).

**Botlar neden trendi kendiliğinden takip etmiyor?** Bir şey kurmadan önce tam olarak bunu iki yıllık veriyle test ettik (8 Ekim 2026, kurallar çalıştırmadan önce yazıldı). DCA'da düşüş trendinde yeni döngü başlatmamak en kötü botun düşüşünü 5 ila 7 puan azalttı, ama test döneminde getiriyi yaklaşık üçte bir düşürdü. Sinyal botlarında trendle açılan işlemler trende ters açılanlardan daha az kaybetti, ama üç dönemin ikisinde yine de zarar etti. İki sonuç da uygulamanın buna göre işlem yapmasına yetecek kadar güçlü değildi; bu yüzden uygulama trendi gösterir, kararı siz verirsiniz.

## Sinyal botları {#signal-bots}

![Sinyal botları: solda risk seviyesi, BTC rejimi ve sinyal akışı; ortada bot tablosu, açık pozisyonlar ve atlanan işlemler](shot:signal-bots)

Sentinel'in yayınladığı sinyallerle işlem yapan üç bot burada.

- **Futures:** futures sinyallerini alır; long ve short.
- **Spot:** spot sinyallerini alır; yalnızca long.
- **Pump:** Sentinel'in pump motorundan gelen ve aynı zamanda yapı kırılımı (MSB ya da CHoCH) ile order block ya da FVG gösteren futures sinyallerini alır. Her risk seviyesinde, o seviyenin sınırları içinde ve yalnızca simüle çalışır. **Test edilmedi** etiketi taşır: bu kural için geçmiş veride ya da ileriye dönük yapılmış bir test yok.

**Short sinyaller:** Sentinel, önceden yazılmış kendi durdurma kuralı tetiklendiği için 8 Ekim 2026'da short sinyal yayınlamayı durdurdu. Short'lar yine önceden yazılmış bir kuralla geri dönene kadar, **Yön** ayarı **Short** olan bot hiç Sentinel sinyali almaz; Futures ve Pump botları yalnızca long sinyal alır.

Ekranda:

- **Sol panel:** risk seviyesi özeti, **BTC rejimi**, **Sinyal akışı** ve akış filtreleri (**Akış: bot**, **Akış: atlama nedeni**).
- **Orta alan:** bot tablosu, **Açık pozisyonlar**, **Atlanan işlemler** ve seçili botun ayar formu.
- **Sağ panel:** bugün açılan ve kapanan işlemler, **Atlama nedenleri** sayıları ve **Bu bot nasıl çalışır** ilkeleri.

![Bir sinyal botunun ayar formu](shot:settings)

Ayar formunun bölümleri:

- **Bot:** borsa, **Pozisyon başına sermaye (USDT)** (altında risk seviyenizin pozisyon başına tavanı yazar), **Maks eşzamanlı pozisyon** ve kaldıraç (Futures ve Pump'ta).
- **Pozisyon boyutlandırma:** **İşlem başına risk** ya da **Sabit boyut**.
- **Kâr al:** TP1, TP2, TP3 ya da özel hedef.
- **Sinyal filtresi:** **En fazla sinyal yaşı (saat)**, **Min Sentinel puanı (%)**, **Yön**, **Pozisyon başına maks. zarar (%)**, **Sembol beyaz listesi**, motor listesi ve botun işlem yapabileceği kurulumlar. Kaydettiğiniz filtre değişiklikleri, eski ayarların atladığı bekleyen sinyallere de uygulanır.

**Bot çalışıyor ama işlem açmıyor, neden?** Bot hiçbir sinyali sessizce atlamaz: her atlanan sinyal nedeniyle birlikte **Atlanan işlemler** listesine yazılır. Sağdaki **Atlama nedenleri** paneli hangi nedenin ne kadar sık görüldüğünü gösterir. En sık görülenler:

| Atlama nedeni | Anlamı | Ne yapmalı |
|---|---|---|
| Pozisyon tavanının üstünde | Botun pozisyon başına sermayesi, simüle bakiyenin risk seviyesine göre izin verilen payını aşıyor (Temkinli seviyede %2). Böyle bir değer kaydedilirken ve başlatılırken reddedilir; bu atlama, bot çalışırken bakiye ya da seviye düşürüldüyse görülür | **Pozisyon başına sermaye** değerini düşürün ya da Risk ve güvenlik sayfasında bakiyeyi veya seviyeyi yükseltin |
| Borsanın minimum emrinin altında | Pozisyon yaklaşık 5 USDT'nin, yani Binance'in çoğu paritede kabul ettiği en küçük emrin altında kalacaktı; işlem borsada var olamayacağı için kağıt defter de reddeder. Küçük bakiyelerde sık görülür: Temkinli seviyede 100 USDT, pozisyon başına 2 USDT'ye izin verir | Risk ve güvenlik sayfasında bakiyeyi veya seviyeyi yükseltin ya da **Pozisyon başına sermaye** değerini en az 5 USDT yapın |
| Pozisyon limiti doldu | Bu botun açık pozisyon sayısı seviyenin üst sınırına ulaştı (seviye limiti her sinyal botuna ayrı uygulanır: Spot, Futures ve Pump) | Pozisyonların kapanmasını bekleyin ya da seviyeyi yükseltin |
| Bot pozisyon limiti doldu | Bu botun **Maks eşzamanlı pozisyon** sınırı doldu | Sınırı yükseltin |
| Başka piyasa için yayınlandı | Spot sinyali Futures botuna (ya da tersi) gelmiş | Bir şey yapmanız gerekmez; her bot kendi piyasasının sinyalini alır |
| Sinyal bu botun yaş sınırından eski | Sinyal, **En fazla sinyal yaşı** değerinden eski | Varsayılan 4 saattir; 0 sınırı kaldırır |
| BTC kırılımı, yeni long'lar bekletiliyor | BTC rejimi "Kırılım"; yeni long açılmaz, short'lar devam eder | Rejimin normale dönmesini bekleyin |
| Kombinasyon beyaz listede değil | Sinyalin kurulumu, botun izin verdiği listede yok | Ayar formundaki kurulum listesini genişletin |
| Sinyal zaten sonuçlandı | Sinyal hedefe ya da stopa çoktan ulaştı; girmek için geç | Bir şey yapmanız gerekmez |
| Sentinel puanı filtresinin altında | Sinyalin puanı **Min Sentinel puanı** değerinin altında | Bu sinyalleri istiyorsanız filtreyi düşürün |

**Atlanan işlemler** listesi ve Geçmiş sayfasındaki **Atlanan sinyalleri dışa aktar** dosyası her bot, sinyal ve neden için tek satır tutar; satırın zamanı ilk görüldüğü andır.

**Pozisyon başına sermaye ile simüle bakiye arasındaki fark ne?** **Pozisyon başına sermaye** bu botun bir pozisyona ayırdığı tutardır. **Simüle bakiye**, Risk ve güvenlik sayfasındaki toplam bakiyedir ve bütün sınırlar ondan hesaplanır. Örnek: Temkinli seviyede bakiye 5000 USDT ise bir pozisyona en fazla %2'si, yani 100 USDT ayrılabilir. Botun sermayesini 5000 yaparsanız uygulama bunu kaydetmez ve botu başlatmaz. Yeni bir botun sermayesi 100 USDT'dir; tavan daha düşükse tavana indirilir: yeni kurulumda (Temkinli, 1000 USDT bakiye) 20 USDT.

**İşlem başına risk nasıl hesaplanır?** Pozisyon büyüklüğü, stop vurulursa sermayenin seçtiğiniz yüzdesi kaybedilecek şekilde hesaplanır. Örnek: sermaye 100 USDT, risk %1, stop %3 uzakta ise yaklaşık 33 USDT'lik pozisyon açılır. Pozisyon hiçbir zaman sermaye × kaldıraçtan büyük olmaz; bu sınıra takılırsa **Sınırlandı** etiketi görünür ve gerçek risk seçtiğinizden azdır.

**Pozisyon başına maks. zarar ne yapar?** Bir pozisyonun zararı, botun sermayesinin bu yüzdesine ulaşınca pozisyonu kapatır; kaldıraç ve komisyon dahildir. Boş bırakılırsa kapalıdır. Alanın altındaki satır, yazdığınız değerin ne yapacağını söyler: **Sabit boyut**'ta pozisyonu kapatacak ters fiyat hareketini; **İşlem başına risk**'te değer risk yüzdesine eşit ya da büyükse etkisi olmadığını, çünkü önce stop kapatır.

**Sentinel puanı neyi ölçer?** Sentinel'in 0–100 arası sinyal puanıdır: kurulum, indikatörler, kesişim ve BTC rejimi ağırlıklı olarak toplanır. Bir olasılık değildir. Geçmiş sinyallerde test edildi ve kazananı kaybedenden ayırmadı (AUC 0.496). **Min Sentinel puanı** filtresi yine ayarlandığı gibi çalışır.

**Pump botunda neden "Test edilmedi" yazıyor?** Bu botun kuralı için geçmiş veride ya da ileriye dönük yapılmış bir test yok. Her risk seviyesinde, o seviyenin sınırları içinde ve yalnızca simüle çalışır. Botun sayfasında kuralın tamamı yazar. Gerçek paraya alınamaz.

**Simüle bir işlem hangi fiyattan kapanır?** Stoplar, hedefler ve kısmi çıkışlar kendi seviyelerinden yazılır. Stop, uygulama izlemiyorken aşıldıysa (yeniden başlatma, bilgisayarın uyku modu), aradan sonra görülen ilk fiyattan yazılır.

**Günlük stop nedir?** Bugün gerçekleşen zarar, simüle bakiyenin seviyeye göre belirlenen yüzdesine ulaşırsa (Temkinli seviyede %2) bütün sinyal botları durur. **Günlük durdurmada pozisyonları kapat** açıksa açık pozisyonlar da kapatılır. Botlar UTC 00:00'dan sonra yeniden başlatılabilir. DCA ve Grid botları günlük stoptan etkilenmez; onların kendi korumaları vardır.

**Sinyal botunu gerçek paraya nasıl alırım?** Yalnızca **Futures** botu, yalnızca Binance'te ve yalnızca gerçek para destekli sürümde. Ayar formunun altındaki **Gerçek para** panelinde **Gerçek paraya geç**'e basın, LIVE yazın ve onaylayın. Bunun için kasada doğrulanmış, yalnızca işlem yetkili bir Binance anahtarı gerekir. İlk 3 gerçek giriş, pilot olarak en fazla 50 USDT ile açılır. Bot gerçek paradayken ya da gerçek pozisyon tutarken borsası değiştirilemez. Uygulama yeniden başlayınca sinyal botu kendiliğinden simülasyona döner.

**Uygulama yeniden başlayınca sinyal botlarına ne olur?** Çalışan paper botlar kendiliğinden yeniden başlar. Sermayesi pozisyon tavanının üstündeyse, kayıtlı ayarları bir kontrolden geçmiyorsa, kayıtlı verisi okunamıyorsa ya da günlük stop tetiklenmişse bot durmuş kalır; **Atlanan işlemler** listesi nedenini yazar ("Yeniden başlatmadan sonra devam etmedi"). Gerçek paradaki bir bot durmuş ve simüle olarak gelir.

## Sinyal botunun sayfası {#signal-bot-detail}

![Sinyal botu sayfası: genel bakış, açık pozisyonlar](shot:bot-detail)

Tablodaki bot adına tıklayınca açılır. Sekmeler: **Genel bakış** (açık pozisyonlar), **İşlemler** (kapanan işlemler ve CSV), **Emirler** (her açık pozisyonun kâr al ve zarar durdur emirleri), **Ayarlar** ve **Kayıt** (atlama notları).

**Açık bir pozisyonu elle kapatabilir miyim?** Evet, pozisyon satırındaki **Kapat** düğmesiyle. Paper pozisyon anlık fiyattan kapanmış sayılır. Gerçek parayla açılmış bir pozisyonda ise borsaya gerçek bir piyasa emri gider; bu yüzden onay için CLOSE yazmanız istenir.

## DCA botları {#dca-bots}

![DCA botları listesi](shot:dca)

DCA botu bir döngü boyunca fiyat düştükçe kademeli alım yapar (güvenlik emirleri), ortalama maliyeti düşürür ve kâr al seviyesinde bütün pozisyonu satar. Ayrıntılı anlatım [Rehber](#/guide?s=dca) sayfasında.

- **Liste:** ad, sembol, piyasa, durum, bütçe, toplam K/Z, gerçekleşen kâr, düşüş, döngü sayısı. Satırdaki düğme bot çalışıyorsa **Duraklat**, duraklatılmış ya da durmuşsa **Sürdür**, hiç çalışmamışsa **Başlat**'tır.
- **Başlıktaki düğmeler:** **DCA hazır ayarları**, **CSV dışa aktar** ve **Yeni DCA botu**.
- **Sağ panel:** botların özeti ve **Strateji bütçesi ve kesici**: beyan edilen bakiye, strateji bütçe sınırı, **Botların ayırdığı**, **Sınır altında boş**, en yüksek kaldıraç ve portföy kesicisi.

![DCA botunun sayfası: genel bakış, açık döngü ve bütçe paneli](shot:dca-detail)

Bot adına tıklayınca botun sayfası açılır:

- **Sekmeler:** **Genel bakış**, **Döngüler**, **Emirler**, **Dolumlar**, **Ayarlar**, **Kayıt**. Bot gerçek paraya alındıysa **Gerçek ve paper** sekmesi de eklenir.
- **Düğmeler:** **Başlat** (hiç çalışmamış bot) ya da **Sürdür**, **Duraklat**, **Döngüyü kapat ve durdur**, **Kopyala**, **Sil**.
- **Sağ panel:** botun durumu, **Gerçek para** paneli ve strateji bütçesi.

**Bot durumları ne anlama gelir?**

| Durum | Anlamı |
|---|---|
| Giriş bekliyor | Bot çalışıyor, açık döngüsü yok; başlangıç koşulunu bekliyor |
| Döngüde | Bot çalışıyor ve açık bir döngüsü var |
| Duraklatıldı: açık döngü yönetiliyor | Yeni döngü açılmaz; açık döngü kâr al ya da stopa kadar yönetilir |
| Duraklatıldı: fiyat akışı 3 dakikadan eski | Bot 3 dakikadır yeni fiyat alamadı; fiyat gelince kendiliğinden devam eder |
| Durdu | Yeni döngü açmıyor ve açık döngüsü yok |
| Sona erdi | Bot likidasyon ya da kayıplar yüzünden kalıcı olarak bitti; **Kopyala** ile yeniden kurulabilir |

**Duraklat, Döngüyü kapat ve durdur, Sil arasındaki fark ne?**

- **Duraklat:** yeni döngü açılmaz. Açık döngü kâr al ya da stop seviyesine kadar yönetilmeye devam eder.
- **Döngüyü kapat ve durdur:** açık döngü hemen piyasa fiyatından kapanır (komisyon ve kayma dahil) ve bot durur. Onay için CLOSE yazmanız istenir.
- **Sil:** botu masadan kaldırır. Döngüleri ve dolumları CSV dışa aktarımında kalır. Onay için DELETE yazmanız istenir.

**Botu neden silemiyor ya da değiştiremiyorum?** Silme, listede botun satırındaki çöp kutusu simgesi ve botun sayfasındaki **Sil** düğmesidir. Açık bir döngü varsa onay penceresi döngünün büyüklüğünü ve açık kâr/zararını gösterir; DELETE yazınca döngü piyasa fiyatından kapanır, bot durur ve silinir. Döngü açıkken yalnızca ad, başlangıç, yeniden başlatma ve koruma ayarları değişebilir; sembol, bütçe, kaldıraç ve strateji ayarları kilitlidir. Bunları hemen değiştirmek için **Döngüyü kapat ve durdur**'u kullanın, düzenleyin, sonra **Başlat**'a basın. Gerçek paradaki bir bot, paper'a dönmeden silinemez.

**"Duraklatıldı: fiyat akışı 3 dakikadan eski" ne demek?** Bot 3 dakikadır yeni fiyat alamadı ve eski fiyatla işlem yapmamak için bekliyor. Genellikle borsanın ya da internet bağlantısının geçici bir kesintisidir. Fiyat gelince bot kendiliğinden devam eder ve kaçırdığı dakikaları sırayla işler; **Sürdür**'e basmak bu durumu çözmez. Bu sırada döngüyü kapatmak da fiyat gelene kadar mümkün olmaz, çünkü uygulama eski fiyatla kapanış yazmaz. Yeni bot kurmak da işe yaramaz: yeni bot aynı fiyat kaynağını kullanır.

**Uygulamayı kapatıp açınca botlarıma ne olur?** Çalışan paper botlar en eskisinden başlayarak kendiliğinden devam eder, ama yalnızca başlatma kontrollerinden yeniden geçerlerse: kaldıraç risk seviyesinin üst sınırını aşmamalı, ayarlar geçerli olmalı, bütçe sınırında ve bakiyede yer olmalı. Bir kontrolden geçemeyen bot durmuş kalır ve sayfası nedenini yazar ("Yeniden başlatmadan sonra devam ettirilmedi"). Açık döngüsü okunamayan bot da durmuş kalır. Gerçek paradaki bir bot durmuş gelir ve sizi bekler. Açık döngüler kaybolmaz; kapalı kalınan süre işlenir ve döngü yönetilmeye devam eder. Yeniden başlatmadan önce fiyat bulamadığı için yapılamayan zorunlu bir kapanış saklanır ve yeniden denenir.

**Bütçe sınırı nedir? "DCA/Grid bütçe sınırı aşıldı" hatası alıyorum.** Bütün DCA ve Grid botlarının bütçelerinin toplamı, simüle bakiyenin risk seviyesine göre belirlenen payını aşamaz:

| Risk seviyesi | Temkinli | Sakin | Dengeli | Hırslı | Aç Gözlü |
|---|---|---|---|---|---|
| Toplam DCA/Grid bütçesi | %20 | %30 | %40 | %60 | %80 |

Örnek: yeni kurulumda (simüle bakiye 1000 USDT, seviye Temkinli) bütün DCA/Grid botları birlikte en fazla 200 USDT ayırabilir. Hata mesajı boş tutarı ve sınırı gösterir. Sınırı yükseltmek için Risk ve güvenlik sayfasında **Simüle bakiye (USDT)** değerini ya da risk seviyesini yükseltin. Durmuş ve döngüsü olmayan botlar bütçe ayırmaz.

**Asgari bütçe nedir?** Botun her emrinin en az 5 USDT olduğu en küçük bütçe. **Özet** panelinde **Asgari bütçe** satırında görünür. DCA Long Classic 176,98 USDT, DCA Long Safe 218,10 USDT ister; bu yüzden yeni kurulumda (200 USDT sınır) Safe sığmaz. Oluşturma sayfası, sınırın altında boş kalan tutar asgari bütçeyi karşılıyorsa açılış bütçesini bu tutara indirir; karşılamıyorsa asgari bütçeyi yazan ve Risk ve güvenlik sayfasına giden bir uyarı gösterir.

**"Başka bir bot bu sembolü, piyasayı ve yönü zaten kullanıyor" hatası?** Aynı sembol, piyasa ve yönde aynı anda yalnızca bir bot çalışabilir; aksi halde botların emirleri birbirine karışır. Ya diğer botu duraklatın ya da başka bir sembol seçin.

**Kaldıracı neden yükseltemiyorum?** Kaldıraç, risk seviyesinin izin verdiği en yüksek değerle sınırlıdır: Temkinli 2x, Sakin 3x, Dengeli 5x, Hırslı 10x, Aç Gözlü 20x. Spot her zaman 1x'tir. Seviyeyi bir botun kaldıracının altına düşürürseniz o bot yeni döngüleri bekletir ("Kaldıraç risk seviyenizin üst sınırının üstünde").

**DCA botunu gerçek paraya nasıl alırım?** Yalnızca gerçek para destekli sürümde ve Binance Futures botlarında. Botun sayfasındaki **Gerçek para** panelinde **Gerçek paradan önce** listesindeki üç koşul sağlanmalıdır: doğrulanmış bir Binance işlem anahtarı, bir zarar durdur (ya da düşüş durdurucusu) ve açık paper döngüsünün olmaması. Sonra **Gerçek paraya geç**'e basıp LIVE yazarsınız. İlk 3 döngü pilot olarak küçük boyutla (yaklaşık 100 USDT'lik pozisyon) çalışır; **Pilotu bitir** ile tam boyuta geçilir. Gerçek para, uygulama yeniden başladıktan sonra da açık kalır. Bot gerçek paradayken ya da gerçek pozisyon tutarken borsası değiştirilemez.

**"Gerçek ve paper" sekmesi neyi gösterir?** Her gerçek dolumu, simülasyonun aynı andaki dolumuyla karşılaştırır: fiyat farkı (kayma), bunun USDT maliyeti ve gecikme. Pozitif kayma maliyettir.

## Grid botları {#grid-bots}

![Grid botları listesi](shot:grid)

Grid botu bir fiyat aralığına eşit aralıklı alış ve satış emirleri yerleştirir ve fiyat bu aralıkta gidip geldikçe küçük kârlar toplar. Ayrıntılı anlatım [Rehber](#/guide?s=grid) sayfasında.

![Grid botunun sayfası](shot:grid-detail)

Liste, bot sayfası, düğmeler, durumlar, bütçe sınırı ve silme kuralları DCA botlarıyla aynıdır; yukarıdaki [DCA botları](#/faq?s=dca-bots) bölümü Grid için de geçerlidir. Grid'e özgü olanlar:

**"Duraklatıldı: fiyat grid aralığının dışında" ne demek?** Fiyat, botun alım-satım aralığının dışına çıktı. Grid emirleri aralığın içinde çalıştığı için bot bekler; fiyat aralığa dönünce devam eder.

**Neden hiç alım-satım olmuyor?** Grid seviyeleri arasındaki mesafe kadar fiyat hareketi olmadan dolum olmaz. Sakin bir piyasada saatlerce dolum olmaması normaldir. Botun sayfasındaki **Emirler** sekmesi bekleyen alış ve satış seviyelerini gösterir.

**Hazır grid şablonlarından hangisi testi geçti?** Hiçbiri. Testten yalnızca iki DCA şablonu geçti: DCA Long Classic ve DCA Long Safe. Grid şablonları öğrenmek ve denemek için duruyor; önce [Geriye dönük test](#/faq?s=backtest) ile denemeniz önerilir.

## Yeni bot {#new-bot}

![Yeni bot: üç bot türü](shot:bot-new)

Bot türünü seçtiğiniz sayfa. Üç kart vardır: **Sinyal botu (Sentinel)**, **DCA botu** ve **Grid botu**. Her kartta **Oluştur** düğmesi vardır; DCA ve Grid kartlarında ayrıca **Bu türün hazır ayarları** bağlantısı bulunur. Klavyede N tuşu da bu sayfayı açar.

![DCA botu oluşturma formu](shot:dca-new)

DCA ya da Grid formunda:

- **Bot:** ad, piyasa, yön (DCA'da) ya da grid modu, sembol, **Bütçe**, **Kaldıraç** ve **Marjin modu** (her zaman izole).
- **Emirler ve çıkışlar** (DCA) ya da **Grid** ayarları.
- Başlangıç ve yeniden başlatma ayarları.
- **Koruma:** **Bot düşüş durdurucusu**, **BTC kırılımında yeni döngüleri beklet**, portföy kesicisi.

Sağdaki **Özet** paneli asgari bütçeyi, bütçenin sınıra sığıp sığmadığını, emir merdivenini, tahmini likidasyonu ve en kötü durumu gösterir. Her ayarın anlamı için formun üstündeki "Her ayar ne işe yarar?" bağlantısına basın.

Boş bir **Yeni DCA botu** formu, DCA Long Classic merdiveniyle açılır; bot düşüş durdurucusu, BTC kırılımında bekletme ve portföy kesicisi açıktır. Formun üstündeki not, kontrolleri geçen şablonların adını yazar; boş grid formu 4 grid şablonundan 0'ının geçtiğini söyler.

**Oluştur ile Oluştur ve başlat arasındaki fark ne?** **Oluştur** botu kaydeder ve durdurulmuş bırakır; bütçesi sınırın içinde kalmalıdır. **Oluştur ve başlat** kaydeder ve hemen başlatır; bütçesi şu anda sınırın altında boş olan tutara sığmalıdır. Başlatma reddedilirse bot yine de oluşturulur ve sayfasında hata nedeni gösterilir.

**Uygulama neden "Likidasyon mümkün, stop yok" onayı istiyor?** Özet panelinde bir likidasyon fiyatı var ve pozisyonu ondan önce kapatacak bir şey yok: zarar durdur (DCA) ya da **Aralık dışında durdur** (grid) kapalı. Buna 1x short DCA da girer: 1x short, fiyat yaklaşık iki katına çıkınca tasfiye olur.

**Zarar durdur neden reddediliyor?** DCA'da zarar durdur, son güvenlik emrinin ötesinde ve merdivendeki her emrin likidasyon fiyatından önce olmalıdır; likidasyonun ötesindeki bir stop hiç tetiklenmez. Zarar durduru açtığınızda form uygun bir değer önerir.

**Sinyal botu kartında Oluştur'a basınca neden yeni bot açılmıyor?** Sinyal botları sabit üç bottur; kart sizi **Sinyal botları** sayfasına götürür.

## Sinyaller {#signals}

![Sinyaller: solda filtreler, ortada sinyal tablosu, sağda seçili sinyalin ayrıntısı](shot:signals)

Sentinel'in yayınladığı sinyalleri tablo olarak gösterir. Her satırda ve seçili sinyalin panelinde bir **Çalıştır** düğmesi vardır.

- **Sol panel (filtreler):** sembol, yön, zaman dilimi, durum (aktif ya da süresi dolmuş), **Min. puan** ve sinyal yaşı.
- **Orta alan:** zaman, sembol, zaman dilimi, yön, giriş, TP1–TP3, zarar durdur, **Puan**, kurulum, durum ve yaş.
- **Sağ panel:** seçili sinyalin ayrıntısı (giriş, hedefler, risk/ödül, konfluans, **Yönetim planı**) ve **Bot kararları**: botlarınızın bu sinyalle ne yaptığı.

**Çalıştır ne yapar?** Sinyal için şu bölümleri sırayla gösteren bir pencere açar. **Bu sinyali şimdi al · simüle** sinyali uygun bir sinyal botunda simüle pozisyon olarak açar: vadeli sinyalde Futures, spot long'da Spot, ayarlıysa ve sinyal vadeliyse Pump. Giriş güncel fiyattan yapılır; sinyalin giriş fiyatı geçilmiş olsa da fiyat stop ile botun hedefi arasındaysa açılır, hedef ya da stop geçildiyse reddedilir. Tıklamadan önce her bot güncel fiyattaki dolumu, sinyalin girişini, dolumdaki ödül:riski yayınlanan değerin yanında (0,5'in altında uyarı: işlem hareket etti), pozisyon büyüklüğünü, stopta zararı ve hedefte kârı (ikisi de komisyon dahil) gösterir. **Pozisyon aç** pozisyonu açar; botun çalışıyor olması gerekmez, kayıtlı ayarı olması yeter. Botun kendi filtreleri atlanır (yaş sınırı, Min. puan, yön, semboller, kurulumlar, motorlar, Pump kuralı): kararı siz verdiniz. Risk limitleri geçerlidir: pozisyon sınırları, pozisyon başı tavan, borsa minimumu, likidasyon kontrolü, günlük stop, funding ve derinlik, BTC koruması. Reddedilirse nedeni yazılır. Pozisyon Pozisyonlar ve emirler, bot sayfası ve Geçmiş'te **Elle** etiketi taşır. Çalıştır gerçek parayla pozisyon açmaz: LIVE'a alınmış bot reddeder. Altında: Sentinel sinyallerini filtreleri içinde kendileri de alan sinyal botlarına bağlantı; sinyalin yönüne uyan DCA ve Grid hazır ayarları (long sinyalde long DCA ile long ya da nötr Grid), her biri kararı ve bot-ay başına test ortalamasıyla; ve boş bir DCA ya da Grid formu. **… ile aç** oluşturma formunu hazır ayarla ve sinyalin paritesiyle doldurur. Kontrolleri geçemeyen hazır ayar önce onay ister. Yalnızca BTC için olan hazır ayar sadece BTCUSDT'de görünür. Hazır ayarlar bu paritede değil kendi paritelerinde test edildi; bot, canlı derleme ve yazılı LIVE onayı olmadan simüle çalışır.

**Puan bir olasılık mı?** Hayır. Sentinel'in 0–100 arası sinyal puanıdır: kurulum, indikatörler, kesişim ve BTC rejimi ağırlıklı olarak toplanır. Sinyalin hedefe ulaşma olasılığı değildir. Geçmiş sinyallerde test edildi ve kazananı kaybedenden ayırmadı (AUC 0.496).

**Neden short sinyal yok?** Sentinel, önceden yazılmış kendi durdurma kuralı tetiklendiği için 8 Ekim 2026'da short sinyal yayınlamayı durdurdu. Short'lar ancak yine önceden yazılmış bir kuralla geri döner.

**USDC ya da PAXG için neden sinyal yok?** Sentinel artık stablecoin, fiat, altın ve wrapped tokenları (USDC, FDUSD, EUR, PAXG, XAUT, WBTC ve benzerleri) taramıyor.

**Bir sinyal neden botuma girmedi?** **Bot kararları** yalnızca açılan ve kapanan pozisyonları gösterir. Atlama nedenleri **Sinyal botları** sayfasındaki **Atlanan işlemler** listesinde ve botun **Kayıt** sekmesindedir.

**Sinyal akışı kesilirse ne olur?** Sinyal botları yeni sinyal alamaz; Panoda ve Sinyal botları sayfasında uyarı çıkar. **Yeniden bağlan** ile bağlantıyı hemen yeniden deneyebilirsiniz. DCA ve Grid botları sinyal akışına bağlı değildir.

## Hazır ayarlar {#presets}

![Hazır ayarlar listesi](shot:presets)

Geçmiş fiyatlar üzerinde test edilmiş DCA ve Grid şablonları. Her şablonun test sonuçları ve bir kararı vardır. Her rakam bot-ay başınadır: her ay parite başına yeni bir 1000 USDT bot açılır; ay sonunda açık kalan döngü kapanana kadar sürer. Aylarca çalışan tek bir bot değildir.

- **Kontrolleri geçti:** dört kontrolün hepsi geçti. Her veri döneminde kâr, test döneminin güven aralığı sıfırın üstünde, test dönemindeki her ay kârlı ve likidasyon yok.
- **İncelemede:** dört kontrolün hepsini özgün test penceresinde geçti, ama yarım bir ayı da içeren sonraki bir çalıştırma her kontrolü doğrulamadı. Şablon, o ay gösterilerek, tarihli yeniden okumaya kadar listede kalır.
- **Geçemedi:** en az bir kontrol başarısız. Hangisinin başarısız olduğu yazar.

![Bir hazır ayarın sonuç sayfası](shot:preset)

Bir şablona tıklayınca ayarları, veri dönemlerine göre sonuçları ve test yöntemi açılır. **Hazır ayarı kullan** formu bu şablonla doldurur; **Test et** şablonu geriye dönük test sayfasında açar.

**"Geçemedi" yazan bir şablonu kullanabilir miyim?** Evet. Uygulama önce hangi kontrollerin başarısız olduğunu gösterir ve **Yine de kullan** onayını ister. Önce geriye dönük testte denemeniz önerilir.

**Şablonun ayarlarını değiştirirsem ne olur?** Ad ve bütçe serbesttir. Başka bir ayarı değiştirirseniz şablonun test sonuçları artık sizin botunuzu anlatmaz; form bunu bir uyarıyla bildirir ve bot şablon bağlantısı olmadan kaydedilir. Parite de önemlidir. Uyarı satırı, şablonun hangi paritelerde test edildiğini yazar. Yalnızca BTC şablonları BTCUSDT ister; başka paritede bağlantı düşer. Sıralı bir listede test edilen şablonlarda (örneğin hacme göre ilk 5 ya da altcoin şablonunun 6-15. sıradaki pariteleri) uyarı, paritenizin bu listede olup olmadığının denetlenmediğini söyler.

**"8 Ekim tarihinden beri gölge" sütunu nedir?** 8 Ekim 2026'dan beri sunucu 12 şablonun hepsini, testlerindeki simülatör ve ayarlarla 6 saatte bir ileriye doğru çalıştırıyor: her ay parite başına yeni bir 1000 USDT bot, 1x, ayın coin listesi; ücret, kayma ve funding dahil; açık döngüler son kapanıştan değerlenir. İçlerinde kimsenin parası yok; her şablonun testi bittikten sonra nasıl gittiğini gösterir. Sütun, başlangıçtan beri toplam getiridir, aylık değildir; test sütunu ise bot-ay başınadır. İlk 30 gün sütunda **Veri birikiyor** yazar ve hiçbir şey sıralanmaz: birkaç hafta çoğunlukla piyasanın ne yaptığını gösterir. Şablonun kendi sayfası rakamları ilk günden, bu notla gösterir.

**Bu sonuçlar gelecekteki kârı gösterir mi?** Hayır. Geçmiş fiyatlar üzerinde, gösterilen masraflarla yapılmış bir simülasyondur; bir işlem kaydı ya da tahmin değildir.

## Geriye dönük test {#backtest}

![Geriye dönük test: solda test ayarları, ortada geçmiş testler](shot:backtest)

Bir DCA ya da Grid ayarını geçmiş fiyatlar üzerinde denersiniz. Gerçek paraya geçmeden önce en güvenli deneme yoludur.

- **Sol panel (Test ayarları):** bot türü, **Aralık** (15 dk, 1 sa, 4 sa, 1 gün), **Dönem** (30 gün ile 2 yıl arası ya da özel tarih; en fazla 3 yıl) ve botun bütün ayarları.
- **Testi çalıştır** testi başlatır; bu sırada mum verisinin indirilme durumu gösterilir. Aynı anda tek test çalışır.
- **Testler:** bu bilgisayarda saklanan son 50 test. Her satırda net sonuç, en büyük düşüş, döngü sayısı ve veri kapsamı yazar.

![Bir test raporu](shot:backtest-report)

Rapor; net K/Z, kapanan döngüler, en büyük düşüş, ücretler, en derin güvenlik emri, likidasyonlar, özkaynak grafiği ve döngü tablosunu gösterir. **Düzenle ve tekrar çalıştır** aynı ayarlarla formu açar.

**Test için neden giriş yapmam gerekiyor?** Geçmiş mum verisi, oturumunuzla Sentinel sunucusundan alınır.

**Raporda "kapsam" düşük görünüyor, sorun mu?** Kapsam, istenen mumların ne kadarının alınabildiğini gösterir. %95'in altındaysa uyarı çıkar; eksik veriyle test sonucu daha az güvenilirdir.

**Test, botun gerçek sonucunu birebir gösterir mi?** Hayır. Komisyon dahildir, ancak futures'ta funding ödemeleri, BTC kırılımı kuralı ve risk kapıları testte yoktur. Rapor bunu etiketlerle belirtir. Düşüş, seçilen mum aralığının kapanışlarında okunur; simüle botlar ise 1 dakikalık mum kullanır. Uzun aralık düşüşün daha azını gösterir.

## Rehber {#guide}

![Rehber sayfası](shot:guide)

DCA ve Grid botlarının uygulama içi el kitabı: nasıl çalıştıkları, sayılarla örnekler, her ayarın anlamı, risk hesabı, geriye dönük test, gerçek para ve terimler sözlüğü. Soldaki **İçindekiler** listesi bölümler arasında gezinmenizi sağlar; formlardaki "Her ayar ne işe yarar?" bağlantıları sizi ilgili bölüme götürür. Rehber Türkçe ve İngilizcedir; diğer dillerde İngilizce açılır.

## Pozisyonlar ve emirler {#positions}

![Pozisyonlar ve emirler: Simüle sekmesi](shot:positions)

Açık olan her şey burada. Simüle (paper) ve gerçek para ayrı sekmelerdedir ve birbirine karışmaz.

- **Simüle sekmesi:** sinyal botlarının açık pozisyonları ve DCA/Grid botlarının açık döngüleri. Solda sembol, bot ve yön filtreleri; sağda toplamlar ve beyan edilen bakiyeye göre maruziyet.
- **Borsa sekmesi:** anahtarı kasada olan her borsadaki gerçek pozisyonlar.

![Borsa sekmesi: Binance hesabındaki gerçek pozisyonlar](shot:positions-exchange)

**Kapat düğmesi gerçek emir gönderir mi?**

- **Simüle sekmesinde:** hayır. Pozisyon anlık fiyattan kapanmış sayılır. **Tümünü kapat** bütün paper pozisyonları ve DCA/Grid döngülerini kapatır ve onay için CLOSE yazmanızı ister. Gerçek parayla çalışan bir DCA/Grid botu varsa onun Binance pozisyonu da kapatılır; uygulama bunu onay penceresinde ayrıca yazar.
- **Borsa sekmesinde:** evet. Her **Kapat** ve **Tümünü kapat**, pozisyonun bulunduğu borsaya gerçek bir azaltma (reduce-only) piyasa emri gönderir ve CLOSE yazmanızı ister. Binance, Bybit ve OKX'te çalışır. Bu, uygulamanın her sürümünde yapabildiği tek elle yazma işlemidir.

**Kapatma ne zaman tamamlanmış sayılır?** Yalnızca hesap o sembolde sıfır pozisyon gösterdiğinde. Bir kalıntı kalırsa uygulama bir azaltma emri daha gönderir; hesap sıfırlanana kadar stop iptal edilmez ve hiçbir şey kayda geçmez. **Tümünü kapat**, biri başarısız olsa bile her pozisyonu dener ve sonunda hâlâ açık kalanları listeler.

**Borsa sekmesinde düğmeler neden gri?** Hesap bilgisi 60 saniyeden eskiyse kapatma düğmeleri yeni bilgi gelene kadar bekler; eski bilgiyle gerçek emir gönderilmez.

**"Borsa stopu yok" etiketi ne demek?** Gerçek bir pozisyon için borsa, koruyucu zarar durdur emrini onaylamadı. Uygulama bu pozisyonu korumaya çalışır; böyle bir etiket gördüğünüzde pozisyonu borsada da kontrol edin.

## Geçmiş ve istatistik {#history}

![Geçmiş ve istatistik](shot:history)

Kapanan işlemler ve performans istatistikleri.

- **Sol panel (filtreler):** sembol, dönem (bugün, 7 gün, 30 gün), bot, yön ve çıkış nedeni.
- **Orta alan:** **Net PnL**, kazanç oranı, **Kâr faktörü**, **Maks düşüş**, işlem sayısı ve bugünkü sonuç; altında işlem tablosu; en altta kapanan döngüleri gösteren **DCA/Grid döngüleri** bölümü (ücretler ve fonlama sonrası, silinen botlar dahil).
- **Sağ panel:** bot ve çıkış nedenine göre sayılar.
- Başlıktaki **CSV dışa aktar** işlemleri dosyaya yazar.

**Filtre uygulayınca istatistikler neden değişmiyor?** Filtreler yalnızca işlem listesini daraltır; istatistikler seçili kapsamın tamamını gösterir. Ekranda bu ayrıca yazar.

**Gerçek ve simüle işlemler birlikte mi sayılıyor?** Hayır. Gerçek para destekli sürümde **İstatistik kapsamı** ile **Simülasyon** ya da **Gerçek para** seçilir; ikisi asla toplanmaz.

## Risk ve güvenlik {#risk}

![Risk ve güvenlik: canlıya hazırlık, risk seviyesi ve seviye limitleri](shot:risk)

Bütün sınırlar ve güvenlik kontrolleri tek yerde. Değişiklikler hemen uygulanır. Soldaki **Bölümler** listesi sayfanın bölümlerine götürür.

**Risk seviyeleri neyi değiştirir?**

| Seviye | Maks. kaldıraç | Maks. pozisyon (sinyal botu başına) | Pozisyon başına sermaye | Günlük stop | DCA/Grid bütçesi |
|---|---|---|---|---|---|
| Temkinli | 2x | 3 | %2 | %2 | %20 |
| Sakin | 3x | 5 | %4 | %4 | %30 |
| Dengeli | 5x | 8 | %6 | %6 | %40 |
| Hırslı | 10x | 12 | %10 | %10 | %60 |
| Aç Gözlü | 20x | 20 | %15 | %15 | %80 |

Yüzdelerin hepsi **Simüle bakiye (USDT)** üzerinden hesaplanır. Aç Gözlü seviyesini seçerken uygulama bir uyarı gösterir ve **Riski anlıyorum** onayı ister. Seviye Pump botunu kilitlemez: Pump her seviyede, o seviyenin sınırları içinde simüle çalışır.

**Kendi günlük zarar sınırımı koyabilir miyim?** Evet, ama yalnızca daha sıkı bir sınır. Seviyenin sınırından gevşek bir değer yazarsanız seviyenin sınırı geçerli kalır.

**Canlıya hazırlık kontrolü ne yapar?** Yalnızca gerçek para destekli sürümde görünür. Gerçek paraya geçmeden önce gereken her şeyi tek seferde okur: canlı sürüm, Binance adresi, yalnızca işlem yetkili anahtar, bilgisayar saatinin Binance'ten farkı, Futures bakiyesi, tek yönlü pozisyon modu, hiçbir botun tutmadığı pozisyonlar, günlük stop, BTC rejimi, üyelik, sinyal akışı ve durmuş canlı botlar. Her satır **Tamam**, **Bak** ya da **Engel** olarak işaretlenir; sorunlu satırda düzeltileceği yere giden **Düzelt** bağlantısı vardır. Kontrol yalnızca okur, hiçbir emir göndermez. **Kontrol et** ile yeniden çalıştırırsınız.

**BTC rejim kapısı nedir?** Sentinel'in BTC değerlendirmesi. **Normal**: engel yok. **Kırılım**: yeni long girişler bekletilir, short'lar devam eder. **Bilinmiyor**: rejim bilgisi gereken girişler reddedilir.

**Acil durumda ne yaparım?** **Acil durum** bölümünde:

- **Tüm botları durdur:** çalışan bütün sinyal botlarını durdurur. Yeni giriş açılmaz; açık pozisyonlar yönetilmeye devam eder.
- **Tüm DCA ve Grid botlarını duraklat**.
- **Tüm DCA ve Grid döngülerini kapat** (CLOSE yazarak).

Bir borsadaki gerçek pozisyonları kapatmak için **Pozisyonlar ve emirler → Borsa** sekmesini kullanın.

## Ayarlar {#settings}

![Ayarlar: Genel sekmesi](shot:settings-page)

Bölümler solda, her birinin güncel durumuyla listelenir (dil ve tema, gösterilen uyarılar, telefon, anahtar sayısı, sürüm). Sağ panel bu kurulumu özetler: sürüm, güncelleme durumu, derleme, Binance emirlerinin gittiği yer, kasa, anahtarlar, telefon ve gösterilen uyarılar; altında yardım bağlantıları.

- **Genel:** **Dil** (sayı, tutar, tarih ve saat dilimi önizlemesiyle), **Tema** (Açık, Koyu, Yüksek kontrast, Sistem; her biri önizlemeli) ve **Klavye kısayolları**. Örnekler: Ctrl+1…9 menülere gider, N yeni bot açar, Ctrl+B kenar çubuğunu daraltır.
- **Bildirimler:** zilin hangi **Uygulama içi uyarılar**ı göstereceği; Güvenlik, Botlar ve işlemler, Piyasa ve bağlantı olarak gruplanır. Her satır uyarının ne zaman çıktığını ve şu an kaç tane olduğunu söyler. Birini gizlemek yalnızca bildirimi gizler; botlar yine buna göre davranır.
- **Cihazlar:** telefondan uzaktan kontrol için eşleştirme, eşlenen telefonun neler yapıp neler yapamayacağı ve eşleştirmenin nasıl korunduğu.
- **Borsa anahtarları:** API anahtarlarınız, hangi anahtarın ekleneceği, bağlantı testleri, kasa ve parolası.
- **Hakkında:** sürüm ve derleme, güncellemeler, kurulum ayrıntıları (sistem, WebView2, uç noktalar, veri klasörü) ve destek için **Tanılama bilgisini kopyala**, verilerinizin nereye gittiği, lisans ve kaynak kod, iletişim. **Tanılama bilgisini kopyala** veri klasörü yolunu ve masaüstü kimliğini içermez.

![Borsa anahtarları sekmesi: kayıtlı anahtarlar ve kasa](shot:accounts)

**Hangi Binance anahtarını eklemeliyim?** Yalnızca işlem (trade) yetkisi olan, **çekim yetkisi kapalı** ve Futures izni açık bir anahtar. Uygulama anahtarı eklerken Binance'e sorarak yetkilerini doğrular; çekim yetkisi açık anahtarları kabul etmez. Anahtar ve gizli anahtar yalnızca bu bilgisayarda, şifreli kasada tutulur; hiçbir sunucuya gönderilmez.

**Hangi borsalarda gerçek emir verilebilir?** Şimdilik yalnızca Binance vadeli işlemleri. Binance emirleri Binance test ağında uçtan uca denendi. Bybit ve OKX bu denemeyi kendi test ağlarında henüz geçmedi: onlar için gerçek parayı açma isteği reddedilir. Anahtarları yine de hesap görünümü ve **Borsa** sekmesinden pozisyon kapatmak için çalışır. Bybit ve OKX'te başabaşa taşıma, borsadaki stopu yerinde değiştirir; diğer tüm emir çağrıları gibi bu da iki borsa gerçek paraya açılmadan önce test ağı denemesinde çalıştırılır. Bitget emir borsası değildir: uygulama izinlerini kontrol edemediği için Bitget anahtarları kabul edilmez. Bot kararları her borsada Binance fiyatlarıyla verilir. Durum çubuğundaki "Borsalar" sayısı fiyat verisi okunan borsaları gösterir.

**Kasa nedir, şifremi unutursam ne olur?** Kasa, anahtarlarınızın bu bilgisayardaki şifreli kaydıdır. Şifre kurtarma yoktur: şifreyi unutursanız **Kasayı sıfırla** ile kasa ve içindeki bütün anahtarlar silinir ve anahtarlarınızı yeniden eklersiniz. Gerçek para açık bir pozisyonu tutarken kasa kilitlenemez, sıfırlanamaz ve anahtarlar değiştirilemez.

**Otomatik kilit süresini nasıl değiştiririm?** **Borsa anahtarları** sekmesindeki **Otomatik kilit** alanından: 5 dk, 15 dk, 30 dk, 1 sa, 4 sa, 8 sa, 12 sa, 24 sa, 1 hafta ya da 1 ay. Süre, penceredeki son işleminizden itibaren sayılır. İçinde anahtar olmayan kasa kilitlenmez. Uzun süreler, bilgisayar açık kaldığı sürece anahtarların da açık kalması demektir.

![Cihazlar sekmesi: telefon eşleştirme](shot:settings-devices)

**Telefonla ne yapabilirim?** Eşleştirilen telefon botların durumunu görebilir, botları durdurabilir ve her şeyi kapatabilir; asıl iş bu bilgisayarda kalır. **QR oluştur** tek kullanımlık, 2 dakika geçerli bir kod üretir. Kodu kimseyle paylaşmayın ve ekran görüntüsünü almayın.

![Hakkında sekmesi: sürüm ve güncellemeler](shot:settings-about)

**Güncellemeler nasıl gelir?** Uygulama açıldıktan kısa süre sonra ve 6 saatte bir güncelleme denetler. Yeni sürüm varsa **Kur ve yeniden başlat** ile kurulur. Her yeniden başlatmadan sonra pozisyonlar ve döngüler geri yüklenir; çalışan paper botlar başlatma kontrollerinden yeniden geçerlerse kendiliğinden devam eder. Ayrıntılar [DCA botları](#/faq?s=dca-bots) ve [Sinyal botları](#/faq?s=signal-bots) bölümlerinde. Gerçek paradaki bir bot durmuş gelir ve sizi bekler. Her güncelleme kurulmadan önce uygulamanın içindeki imza anahtarıyla doğrulanır. Gerçek para açıkken güncelleme kurulmaz.

## Hesap {#account}

![Hesap sayfası: Sentinel üyeliği](shot:account)

Sentinel (ribqa.com) üyeliğiniz. Üstte: adınız ve e-postanız, üyelik durumu ve plan, **Yenile** ve **Çıkış yap**.

- **Üyelik:** plan, abonelik durumu, geçerlilik tarihi (açık betada **Bitiş tarihi yok**) ve **Son denetim**: uygulamanın üyeliği ribqa.com'dan en son ne zaman okuduğu (girişte, açılışta ve **Yenile**'ye basınca).
- **Üyelik neleri kapsar:** sinyal akışı ve sinyal botları, DCA ve Grid botları, backtest ve girişten önceki piyasa kontrolleri. Aktif üyelik olmadan hiçbir bot işlem açmaz.
- **Bu bilgisayardaki oturum:** servis, girişin saklandığı yer (Windows Kimlik Bilgileri Yöneticisi) ve TESTNET derlemesinin ayrı giriş yaptığı.
- **ribqa.com'da yönet:** profil ve parola, ödeme ve API anahtarları web sitesinde açılır.
- **Sağ panel:** üyelik, plan, botların işlem açıp açamayacağı, sinyal akışı ve borsa anahtarı sayısı.

**Çıkış yap** önce onay ister. Sonrasında siz yeniden giriş yapana kadar sinyal botlarına yeni sinyal gelmez; botlar, pozisyonlar, geçmiş ve borsa anahtarları bu bilgisayarda kalır.

**Uygulamayı kullanmak için neler gerekiyor?** Üç şey: Sentinel'e giriş yapmış olmak, üyeliğin aktif olması ve kasanın açık olması. Biri eksikse uygulama ilgili ekranı gösterir: giriş, üyelik ya da kasa kilidi.

**Çıkış yaparsam botlarım ne olur?** Sentinel sinyal akışı kesilir, bu yüzden sinyal botları yeni sinyal alamaz. Botların durumu korunur.

**TESTNET sürümünü de kullanıyorum. Neden orada giriş yapmamı istiyor?** Her sürüm kendi oturumunu ve kendi kasa kaydını Windows Kimlik Bilgileri Yöneticisi'nde tutar; bu yüzden bir sürümde giriş, çıkış ya da kasa sıfırlama diğerini etkilemez. TESTNET sürümünde bir kez giriş yapın. Kasası, orada ilk kez kilidini açtığınızda sürümün kendi kaydına taşınır.
