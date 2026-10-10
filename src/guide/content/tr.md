# DCA ve Grid botları rehberi

Bu rehber DCA ve Grid botlarını hiç kullanmamış biri için yazıldı. Baştan sona okumanız gerekmez: ilk iki bölüm başlamak için yeterli, gerisi ihtiyaç duyduğunuzda dönmeniz için.

> DCA ve Grid botları varsayılan olarak **simüle (paper)** çalışır: borsaya emir gitmez, para kaybetmezsiniz. Gerçek para yalnızca canlı sürümde, Binance Futures botunda ve o bot için **LIVE** yazıp açtığınızda devreye girer. Ayrıntılar: [Gerçek para](#/guide?s=real-money).

## 5 dakikada başlayın {#start}

1. Kenar çubuğunda [Hazır ayarlar](#/presets) sayfasını açın.
2. "Karar" sütununda **Kontrolleri geçti** yazan bir şablon seçin ve **Hazır ayarı kullan** düğmesine basın.
3. **Parite** olarak büyük ve likit bir coin seçin (BTCUSDT, ETHUSDT gibi).
4. **Bütçe** alanına denemek istediğiniz tutarı yazın. Simüle olduğu için gerçek para gerekmez.
5. Sağdaki **Özet** panelinde "Bütçe kontrolü: Sığıyor" yazdığını ve bütçenin **Asgari bütçe** satırındaki tutardan az olmadığını görün.
6. **Oluştur ve başlat** düğmesine basın.

**Yeni kurulumda bütçe:** simüle bakiye 1000 USDT, seviye Temkinli olduğu için bütün DCA ve Grid botları birlikte en fazla 200 USDT kullanabilir. DCA Long Classic en az 176,98 USDT ister ve sığar. DCA Long Safe en az 218,10 USDT ister; [Risk ve güvenlik](#/risk) sayfasında bakiyeyi ya da seviyeyi yükseltmeden sığmaz. Oluşturma sayfası, sınırın altında boş kalan tutar şablonun asgari bütçesini karşılıyorsa açılış bütçesini bu tutara indirir; karşılamıyorsa asgari bütçeyi yazan ve Risk ve güvenlik sayfasına giden bir uyarı gösterir.

Bot artık 1 dakikalık gerçek fiyatlarla, ücretler dahil simüle işlem yapıyor. Sonuçları bot sayfasındaki **Döngüler** sekmesinden izleyin.

Ayarlara dokunmadan önce bir şablonu birkaç gün çalıştırıp neler olduğunu izlemek, en hızlı öğrenme yoludur.

## Temel kavramlar {#concepts}

Botları anlamak için önce şu kavramları bilmeniz gerekir. Her biri formda bir ayara karşılık gelir.

| Kavram | Anlamı | Neden önemli |
|---|---|---|
| **Spot** | Coini gerçekten satın alırsınız | Kaldıraç yok, tasfiye yok; yalnızca yükselişten kazanılır |
| **Vadeli (futures)** | Coinin fiyatı üzerine sözleşme alıp satarsınız | Hem yükselişten (long) hem düşüşten (short) kazanılabilir; kaldıraç kullanılabilir |
| **Long** | Fiyat yükselirse kazanırsınız | DCA long düşüşlerde kademeli alır |
| **Short** | Fiyat düşerse kazanırsınız | DCA short yükselişlerde kademeli satar |
| **Kaldıraç** | Bütçenizin katı büyüklüğünde pozisyon | 5x'te %1 fiyat hareketi bütçenizde %5 etki yapar; kazanç da kayıp da büyür |
| **Marjin** | Pozisyon için ayrılan teminat | İzole marjinde yalnızca o botun bütçesi risk altındadır |
| **Likidasyon (tasfiye)** | Zarar teminatı bitirince borsanın pozisyonu zorla kapatması | Kaldıraçlı ve stopsuz botun en büyük riski: döngünün tüm bütçesi gider |
| **Maker ücreti** | Bekleyen limit emir dolunca ödenen ücret (%0,02) | Güvenlik emirleri ve kâr al limit emridir, ucuzdur |
| **Taker ücreti** | Piyasa emriyle anında işlem ücreti (%0,05) | Ana emir, zarar durdur ve izleyen kâr al piyasa emridir |
| **Kayma** | Piyasa emrinin beklenen fiyattan biraz kötü dolması (%0,02) | Simülasyon her piyasa emrine ekler |
| **Fonlama (funding)** | Vadeli işlemde 8 saatte bir long ile short arasında ödenen ücret | Uzun süre açık kalan pozisyonun maliyetini artırır |

**1x long vadeli** pozisyon, spot alıma çok yakındır: tasfiye olmaz, yalnızca fonlama ücreti farkı vardır.

## DCA nasıl çalışır {#dca}

DCA (Dollar Cost Averaging, kademeli alım) şu fikre dayanır: fiyat düştükçe biraz daha alırsanız ortalama alış fiyatınız düşer. Fiyatın küçük bir toparlanması bile sizi kâra geçirir.

Bir DCA botu **döngüler** halinde çalışır:

1. **Ana emir** piyasa fiyatından hemen alınır.
2. Fiyat düştükçe önceden belirlenen seviyelerde **güvenlik emirleri** dolar ve ortalama giriş fiyatı düşer.
3. Fiyat ortalama girişin **kâr al** yüzdesi kadar üstüne çıkınca pozisyonun tamamı satılır.
4. Döngü kapanır, bot hemen yeni bir döngü başlatır.

### Sayılarla bir örnek

`dca_long_classic` şablonu, 1.000 USDT bütçe, 1x, coin fiyatı 100 ile başlıyor. 8 güvenlik emri var; ilk sapma %2,5, adım katsayısı 1,3, hacim katsayısı 1,4, kâr al %2.

| Emir | Başlangıçtan düşüş | Fiyat | Emir tutarı | Toplam yatırılan | Ortalama giriş | Satış fiyatı (kâr al) |
|---|---|---|---|---|---|---|
| Ana emir | %0 | 100,00 | 28,3 | 28,3 | 100,00 | 102,00 |
| 1. güvenlik | %2,5 | 97,50 | 28,3 | 56,5 | 98,73 | 100,71 |
| 2. güvenlik | %5,75 | 94,25 | 39,6 | 96,1 | 96,84 | 98,77 |
| 3. güvenlik | %9,98 | 90,03 | 55,4 | 151,4 | 94,23 | 96,11 |
| 4. güvenlik | %15,47 | 84,53 | 77,5 | 229,0 | 90,71 | 92,52 |
| 5. güvenlik | %22,61 | 77,39 | 108,5 | 337,5 | 85,95 | 87,67 |
| 6. güvenlik | %31,89 | 68,11 | 152,0 | 489,4 | 79,49 | 81,08 |
| 7. güvenlik | %43,96 | 56,04 | 212,7 | 702,2 | 70,55 | 71,96 |
| 8. güvenlik | %59,64 | 40,36 | 297,8 | 1.000,0 | 57,69 | 58,85 |

Bu tablodan çıkan dört önemli sonuç:

- **Küçük düşüşler hızlı kapanır.** Fiyat 94,25'e düşüp 98,77'ye dönerse döngü kârla kapanır. Bu dönüş başlangıç fiyatının altında kalıyor.
- **Bütçenin çoğu derinde bekler.** İlk üç emir bütçenin yalnızca %15'ini kullanır. Son iki emir ise %51'ini.
- **Merdiven %59,6 düşüşü karşılar.** Fiyat bundan da fazla düşerse bot yeni alım yapamaz, pozisyonu taşıyıp toparlanmayı bekler.
- **Kâr küçüktür ama sıktır.** Her döngü yatırılan tutarın yaklaşık %2'si kadar kazandırır. Kazanç, çok sayıda kısa döngüden gelir.

### DCA'nın riski

DCA'nın tek büyük riski, fiyatın **dönmeden çok uzun süre düşmesidir.** Stop yoksa bot pozisyonu taşımaya devam eder. Bu sırada bütçe kilitli kalır ve gerçekleşmemiş zarar görünür. Testlerimizde en uzun döngü 329 gün açık kaldı.

Kaldıraç bu riski tasfiyeye çevirir. 1x long tasfiye edilmez. 5x long ise ortalama girişin yaklaşık %20 altında tasfiye olur. Short ise 1x'te bile, fiyat yaklaşık iki katına çıkınca tasfiye olur. Uygulama bu yüzden Özet panelinde bir likidasyon fiyatı görünüp önünde hiçbir stop yoksa sizden onay ister ("Likidasyon mümkün, stop yok"): zarar durduru kapalı bir DCA ya da **Aralık dışında durdur** ayarı kapalı bir grid.

## Grid nasıl çalışır {#grid}

Grid botu belirlediğiniz fiyat aralığını eşit basamaklara böler. Başlangıç fiyatının altındaki her basamağa alış, üstündeki her basamağa satış emri koyar.

- Bir alış dolunca, bir üst basamağa satış emri konur.
- Bir satış dolunca, bir alt basamağa alış emri konur.
- Fiyat her basamağı bir aşağı bir yukarı geçtiğinde bir basamak kârı alınır.

### Sayılarla bir örnek

Fiyat 100, aralık %10 aşağı ve %10 yukarı (90 ile 110 arası), 20 geometrik aralık.

- Her basamak yaklaşık **%1,008** genişliğindedir.
- Alış ve satış maker emridir (her biri %0,02). Ücretler düşülünce basamak başına net kâr yaklaşık **%0,97** olur.
- Fiyat aralıkta gidip geldikçe bu küçük kârlar birikir.

### Grid'in riski

Grid, fiyat **aralıktan çıkınca** zarar eder:

- **Fiyat aralığın altına düşerse** bütün alışlar dolmuş, satışlar bekliyordur. Elinizde düşük değerli coin kalır.
- **Fiyat aralığın üstüne çıkarsa** her şey satılmıştır. Yükselişin geri kalanını kaçırırsınız.

**Aralık dışında durdur** ayarı, fiyat aralığın belirli bir yüzde dışına çıkınca grid'i kapatır ve zararı sınırlar. Testlerimizde grid tasarımları genel olarak zayıf sonuç verdi. Gerçek piyasada uzun süre yatay kalan dönemler az, trendler sık.

## Hangi durumda hangi bot {#which-bot}

Piyasanın yönünü kimse kesin bilemez. Bu tablo bir başlangıç noktasıdır, tavsiye değildir. Her şablonun gerçek test sonucu [Hazır ayarlar](#/presets) sayfasındaki "Karar" sütunundadır.

| Piyasa durumu | Uygun bot | Neden | Dikkat |
|---|---|---|---|
| Uzun vadede yükselen, sık düzeltme yapan | DCA long klasik | Düzeltmelerde ucuza alır, toparlanmada satar | Testi geçti; uzun ayı piyasasında bütçe kilitlenir |
| Belirsiz, büyük coinler | DCA long, temkinli şablon | Derin merdiven uzun düşüşlere dayanır | Testi geçti; kâr daha yavaş gelir |
| Düşüş trendi | DCA short | Yükselişlerde kademeli satar, düşüşte kapatır | Testte iki short şablonu da 1x'te tasfiye oldu |
| Yatay, dar bant | Grid nötr | Aralıkta gidip gelmeden kazanır | Testte grid şablonlarının hepsi zararda |
| Yavaş yükselen | Grid long, yukarı izlemeli | Aralık fiyatla birlikte yukarı kayar | Testte geçemedi |
| Çok oynak altcoin | DCA long, altcoin şablonu, düşük bütçe | Geniş merdiven sert hareketlere dayanır | Testte geçemedi; coin toparlanmayabilir |

**Genel kurallar:**

- Büyük, likit coinlerle başlayın. İlk 5 coinde test edilen şablonlar küçük coinlerde aynı sonucu vermez.
- 1x ile başlayın. Kaldıraç kazancı değil, riski büyütür.
- Bir coine birden fazla bot açmayın. Uygulama aynı coin, piyasa ve yönde tek bota izin verir.

## Şablonlar ve test sonuçları {#templates}

[Hazır ayarlar](#/presets) sayfasındaki her şablon, 2 yıllık Binance vadeli verisinde aynı simülatörle test edildi. Ücretler, kayma ve mum içi en kötü sıra hesaba katıldı. Her rakam bot-ay başınadır: her ay parite başına yeni bir 1000 USDT bot açılır; ay sonunda açık kalan döngü kapanana kadar sürer. Aylarca çalışan tek bir bot değildir. Sonuçlar üç döneme ayrılır:

- **Eğitim (TRAIN):** Ekim 2024 ile Haziran 2025 arası.
- **Doğrulama (VALID):** Temmuz 2025 ile Ocak 2026 arası.
- **Test (TEST):** Şubat 2026 ile Eylül 2026 arası. Ayarlar bu döneme bakılmadan seçildi.

Bir şablonun **Kontrolleri geçti** etiketi alması için:

1. Üç dönemin üçünde de bot-ay başına ortalama getiri pozitif olmalı.
2. Test döneminde %95 güven aralığının alt ucu sıfırın üstünde olmalı.
3. Test döneminin 8 ayının 8'i de pozitif olmalı.
4. Hiç tasfiye olmamalı.

Birini bile sağlamayan şablon **Geçemedi** etiketi alır ve hangi kontrolü geçemediği yazılır. Bu şablonlar öğrenmek ve denemek için listede durur. Kullanmadan önce uygulama sizden onay ister.

**Sinyalden:** [Sinyaller](#/signals) sayfasındaki **Çalıştır**, sinyali uygun bir sinyal botunda hemen simüle pozisyon olarak açar (sinyalin girişi geçildiyse, fiyat stop ile hedef arasındayken güncel fiyattan; botun filtreleri atlanır, risk limitleri geçerlidir, gerçek para açılmaz) ya da sinyalin yönüne uyan şablonları listeler ve birini sinyalin paritesinde açar. O paritede şablon test edilmemiştir.

### 3 Ekim 2026 testinin sonucu

12 şablondan **2'si** dört kontrolün dördünü de geçti:

| Şablon | Test dönemi, bot-ay başına | Pozitif test ayı | En kötü bot düşüşü |
|---|---|---|---|
| DCA Long Klasik | +%1,90 | 8/8 | −%20,7 |
| DCA Long Temkinli | +%1,45 | 8/8 | −%15,8 |

**9 Ekim 2026 yeniden çalıştırması.** Aynı test penceresinde (27 Eylül'e kadar) ikisi de hâlâ geçiyor; yukarıdaki rakamlar bu çalıştırmadan. Ekim eklenince ikisi de tek bir kontrole takılıyor: "her test ayı pozitif". Ekim'in ilk 8,5 gününde majörler %1 ile %15 arası düşmüşken ve tüm işlemler hâlâ açıkken Klasik bot-ay başına −%0,44, Temkinli −%0,14'teydi. İkisi **İncelemede** olarak işaretli; Ekim tamamlanınca, 1 Kasım'da kontroller yeniden okunur. Başka hiçbir şablon geçmedi; o gün denenen altı yeni adaydan hiçbiri şablon olmadı.

Geçemeyenlerden öğrenilecekler:

- **DCA Long Hızlı** test döneminde çok iyiydi (bot-ay başına +%5,63), ama doğrulama döneminde bot-ay başına %6,84 kaybetti. Kısa merdiven, sert bir düşüşte bütçeyi erken tüketir.
- **DCA Long BTC** iki dönemde kazandı, doğrulama döneminde hafif zararda kaldı.
- **DCA Long Altcoin** test döneminde zarar etti; en kötü bot bütçesinin %97'sini kaybetti.
- **DCA Long Stoplu** stop yüzünden dipte satıp toparlanmayı kaçırdı; doğrulama döneminde zararda.
- **DCA Short** şablonlarının ikisi de **1x'te bile tasfiye oldu**. 1x short, fiyat yaklaşık iki katına çıkınca tasfiye olur; kripto bunu birkaç ayda yapabilir.
- **Grid** şablonlarının dördü de dönemlerin çoğunda zarar etti. Fiyat aralıktan çıkınca stop ya da süre dolumuyla zararına kapandılar.

### Tablodaki sütunlar

| Sütun | Anlamı |
|---|---|
| Test, bot-ay başına | Test döneminde bot-ay başına ortalama getiri, bütçenin yüzdesi |
| %95 aralık | Bu ortalamanın güven aralığı. Aralık ne kadar dar ve sıfırdan ne kadar uzaksa sonuç o kadar güvenilir |
| Pozitif aylar | Test döneminde kazandıran ay sayısı / toplam ay |
| Düşüş | Test döneminde en kötü botun en büyük düşüşü |
| Karar | Kontrolleri geçti ya da Geçemedi |
| 8 Ekim tarihinden beri gölge | Şablonun 8 Ekim 2026'dan beri sunucuda ileriye doğru simüle çalışması; içinde para yok. Başlangıçtan beri toplam getiri, aylık değil. 30 gün dolmadan sıralanmaz |

**Şablon bağlantısı, botunuz şablonla aynı kaldıkça korunur.** Ad ve bütçe serbesttir; başka bir değişiklik bağlantıyı düşürür ve form bunu söyler. Uyarı satırı, şablonun hangi paritelerde test edildiğini yazar. Yalnızca BTC şablonları BTCUSDT ister. Sıralı bir listede test edilen şablonlarda (hacme göre ilk 5 ya da altcoin şablonunun 6-15. sıradaki pariteleri) uyarı, paritenizin bu listede olup olmadığının denetlenmediğini söyler.

> Geçmiş simülasyon gelecek getirinin garantisi değildir. Test verisinde 2022 tipi uzun bir ayı piyasası yok. Bir şablonun geçmesi, yalnızca bu 2 yılda ve bu kurallarla iyi çalıştığını gösterir.

## DCA ayarları tek tek {#dca-settings}

### Bot bölümü

| Ayar | Ne işe yarar | Öneri |
|---|---|---|
| Ad | Botu listede tanımanız için | Coin ve stratejiyi yazın: "BTC DCA temkinli" |
| Piyasa | Spot ya da Vadeli | Vadeli 1x, spot'a çok yakındır ve short'a izin verir; 1x short, fiyat yaklaşık iki katına çıkınca tasfiye olur |
| Yön | Long ya da Short | Emin değilseniz Long |
| Parite | Hangi coin | Büyük ve likit coinler |
| Bütçe | Bu botun kullanabileceği en fazla tutar | Toplam bakiyenizin küçük bir parçası |
| Kaldıraç | Pozisyon çarpanı | 1x |

### Emirler ve çıkışlar

| Ayar | Ne işe yarar | Etkisi |
|---|---|---|
| Emir boyutu | "Bütçeye ölçekli": merdiven bütçenin tamamına göre boyutlanır. "Sabit USDT": tutarları siz yazarsınız | Bütçeye ölçekli, merdivenin bütçeye sığmasını garanti eder |
| En fazla güvenlik emri | Kaç kademe ek alım yapılacağı | Çok kademe, derin düşüşe dayanır ama her emir küçülür |
| İlk güvenlik emrine sapma | İlk ek alımın başlangıçtan yüzde kaç aşağıda olacağı | Küçükse sık alır, büyükse daha derinden başlar |
| Adım katsayısı | Her sonraki boşluğun bir öncekinin kaç katı olacağı | 1'den büyükse kademeler aşağıda açılır, merdiven daha derine iner |
| Hacim katsayısı | Her sonraki emrin bir öncekinin kaç katı olacağı | Büyükse ortalama hızlı düşer ama bütçe derinde yoğunlaşır |
| Kâr al | Ortalamanın yüzde kaç üstünde satılacağı | Küçükse sık ve küçük kâr, büyükse seyrek ve büyük kâr |
| İzleyen kâr al | Kâr al seviyesine gelince hemen satmaz, tepeyi izler | **İzleme sapması kâr aldan küçük olmalı.** Testlerde çoğu zaman sonucu kötüleştirdi |
| Zarar durdur | Ortalamanın bu kadar altında piyasa emriyle çıkar. Son güvenlik emrinin ötesinde ve her emrin likidasyon fiyatından önce olmalı; form likidasyonun ötesindeki stopu reddeder. Açtığınızda form uygun bir değer önerir | Kayıp sınırlanır ama dipte satıp toparlanmayı kaçırabilirsiniz |
| En uzun döngü süresi | Süre dolunca döngüyü piyasa fiyatından satar; çalışan bot, bekleme süresinden sonra yeni döngüyü açar | Kısa tutulursa zararına kapanışlar artar |

**Boş form**, DCA Long Classic merdiveniyle açılır; bot düşüş durdurucusu, BTC kırılımında bekletme ve portföy kesicisi açıktır, yani o şablonun aynısı değildir. Formun üstündeki not, kontrolleri geçen şablonların adını yazar; boş grid formu 4 grid şablonundan 0'ının geçtiğini söyler.

**Merdiven kapsamı nasıl hesaplanır:** sağdaki Özet panelinde "Fiyat kapsamı" yazar. Bu, son güvenlik emrinin başlangıçtan ne kadar aşağıda olduğudur. Kapsam, coinin geçmişteki sert düşüşlerinden büyük olmalı.

## Grid ayarları tek tek {#grid-settings}

| Ayar | Ne işe yarar | Öneri |
|---|---|---|
| Fiyat aralığı | "Göreli %": her döngüde başlangıç fiyatının etrafına kurulur. "Sabit fiyatlar": alt ve üst fiyatı siz yazarsınız | Yeni başlayanlar için göreli |
| Alt / Üst | Aralığın genişliği | Coinin son haftalardaki salınımı kadar |
| Grid aralığı sayısı | Kaç basamak olacağı | Çok basamak sık ama küçük kâr demek; basamak kârı ücretlerin altına düşmemeli |
| Aralık tipi | Geometrik: eşit yüzde. Aritmetik: eşit fiyat farkı | Geometrik |
| Grid modu | Nötr: başlangıçta pozisyon yok. Long: başlangıçta coin alır. Short: başlangıçta satar | Yatay piyasada nötr |
| Aralık dışında durdur | Fiyat aralığın bu kadar dışına çıkınca grid'i kapatır | Açık tutun |
| Yukarı izleme | Long grid'de fiyat yükselince aralık da yukarı kayar | Yavaş yükselen piyasada |
| Döngü kârında kâr al | Döngünün kârı bütçenin bu yüzdesine ulaşınca kapatır | İsteğe bağlı |

Özet panelinde **"Ücretler sonrası grid başına kâr"** satırına bakın. %0,3'ün altındaysa basamaklar çok sık demektir; ücretler kârı yer.

## Başlangıç, yeniden başlatma ve koruma {#protection}

| Ayar | Ne işe yarar |
|---|---|
| Başlangıç koşulu | "Hemen" ya da "Fiyat tetiği": fiyat belirlediğiniz seviyeye gelince başlar |
| Döngüler arası bekleme | Bir döngü kapandıktan sonra yenisi için beklenecek dakika |
| Döngü sayısını sınırla | Bu kadar döngüden sonra bot durur |
| Zarar durdurdan sonra yeniden başlat | Kapalıysa stop olan bot durur |
| Fiyat aralığı koruması | Fiyat bu aralığın dışındayken yeni döngü açılmaz |
| Bitiş zamanı | Bu tarihten sonra yeni döngü açılmaz |
| Bot düşüş durdurucusu | Bot özkaynağı zirvesinden bütçenin bu yüzdesi kadar düşünce kapatır ve durdurur. Aynı mum hem bu stopa hem kâr ala dokunursa stop kazanır: önce aleyhe hareket olduğu varsayılır |
| BTC kırılımında yeni döngüleri beklet | Sentinel BTC'de kırılım bildirirken yeni döngü açılmaz |
| Portföy kesicisi | Kesiciye dahil botların toplam zararı %15'e ulaşınca hepsini kapatır |

**Korumalar DCA'da iki yönlüdür.** DCA'nın kazancı düşüşte bekleyip dönüşte satmaktan gelir. Dipte kapatan her koruma bu dönüşü kaçırabilir. `dca_long_classic` testinde portföy kesicisi açıkken 2 yıllık sonuç +%41,5'ten +%29,8'e düştü. Koruma açmak sizin risk tercihinizdir: daha az kazanç, daha sınırlı kayıp.

## Riskinizi hesaplayın {#risk}

Bir bot başlatmadan önce şu üç soruyu cevaplayın:

1. **En kötü durumda ne kaybederim?** Özet panelindeki "En kötü durum" satırı önce gelen çıkışı yazar: zarar durdurda, düşüş durdurucusunda ya da likidasyonda. Bunların hiçbiri yoksa "Sınırsız: stop yok" yazar. 1x ve stopsuz long DCA'da teorik kayıp, coin sıfıra giderse bütçenin tamamıdır.
2. **Bütçe ne kadar süre kilitli kalabilir?** Stopsuz DCA, toparlanma gelene kadar bütçeyi tutar. Aylarca beklemeye hazır olmalısınız.
3. **Tasfiyeye ne kadar mesafe var?** Kaldıraçlı DCA'da Özet panelindeki "Son emirden likidasyona" satırı, son güvenlik emrinden sonra fiyatın ne kadar daha gidebileceğini yazar.

**Bütçe kuralı:** risk seviyenize göre tüm DCA ve Grid botlarının toplam bütçesi, beyan ettiğiniz bakiyenin %20 ile %80'i arasında bir sınırı geçemez. Bu sınır [Risk ve güvenlik](#/risk) sayfasında görünür. Her botun ayrıca Özet panelinde yazan bir **Asgari bütçe**si vardır: her emrin en az 5 USDT olduğu en küçük bütçe.

## Backtest: önce geçmişte deneyin {#backtest}

[Geriye dönük test](#/backtest) sayfası, bir ayarı geçmiş fiyatlarda aynı motorla çalıştırır.

1. Strateji türünü, coini ve ayarları seçin.
2. Zaman aralığını (30 günden 2 yıla kadar ya da özel) ve mum aralığını seçin. Kısa mum aralığı (15 dk, 1 sa) mum içi hareketi daha doğru yansıtır.
3. **Testi çalıştır** düğmesine basın.

### Raporu okumak

| Alan | Anlamı |
|---|---|
| Net | Dönem sonundaki toplam kâr ya da zarar, bütçenin yüzdesi |
| En büyük düşüş | Özkaynağın zirveden gördüğü en derin düşüş; seçilen mum aralığının kapanışlarında okunur. Simüle botlar 1 dakikalık mum kullandığı için uzun aralık düşüşün daha azını gösterir |
| Döngüler | Kapanan döngü sayısı; "+1" dönem sonunda açık döngü olduğunu gösterir |
| Çıkış nedeni | Kâr al, izleyen kâr al, zarar durdur, tasfiye, düşüş durdurucusu |
| Kapsam | İstenen mumların ne kadarının veride bulunduğu |

### Backtest tuzakları

- **Tek dönem yanıltır.** Yükselen bir ayda her long DCA iyi görünür. En az bir düşüş içeren dönemi de test edin.
- **Ayarı sonuca göre oynamak.** Aynı dönemde ayarları değiştire değiştire en iyi sonucu bulmak, geleceği değil geçmişi ezberlemektir. Bulduğunuz ayarı başka bir dönemde tekrar deneyin.
- **Kazanma oranına aldanmak.** 24 döngü kazanıp 1 döngüde tasfiye olan bot, toplamda zarardadır.

## Bot çalışırken {#running}

| Durum | Anlamı |
|---|---|
| Çalışıyor | Yeni döngü açabilir |
| Duraklatıldı | Yeni döngü açmaz; açık döngü kâr al ve stopuyla sürer |
| Durdu | Yeni döngü açmaz, açık döngü yok |
| Sona erdi | Tasfiye ya da bütçe tükenmesiyle bitti; kopyalayarak yeniden kurabilirsiniz |

| Düğme | Ne yapar |
|---|---|
| Başlat | Hiç çalışmamış botta Sürdür yerine görünür |
| Sürdür | Duraklatılmış ya da durmuş botun yeniden yeni döngü açmasına izin verir |
| Duraklat | Yeni döngüyü durdurur, açık döngüye dokunmaz |
| Döngüyü kapat ve durdur | Açık döngüyü hemen piyasa fiyatından kapatır |
| Kopyala | Aynı ayarlarla yeni bot formu açar |
| Sil | Durmuş ve pozisyonsuz botu listeden kaldırır; geçmişi saklanır |

**Acil durum:** [Pozisyonlar](#/positions) sayfasındaki **Tümünü kapat** düğmesi tüm pozisyonları ve DCA/Grid döngülerini kapatır, botları durdurur. Gerçek parada çalışan botların Binance pozisyonu da bir sonraki turda (birkaç saniye) piyasa fiyatından kapatılır. Binance hesabınızdaki her pozisyonu doğrudan kapatmak için [Pozisyonlar → Borsa](#/positions?tab=exchange).

**Uygulama yeniden açılınca** çalışan paper botlar en eskisinden başlayarak kendiliğinden devam eder, ama yalnızca başlatma kontrollerinden yeniden geçerlerse: kaldıraç risk seviyesinin üst sınırını aşmamalı, ayarlar geçerli olmalı, bütçe sınırında ve bakiyede yer olmalı. Kontrolden geçemeyen bot Durdu olarak kalır ve sayfasında bir not çıkar ("Yeniden başlatmadan sonra devam ettirilmedi"). Açık döngüsü okunamayan bot da Durdu kalır; gerçek paradaki bot Durdu olarak gelir. Açık döngüler her durumda yönetilmeye devam eder. Fiyat bulamadığı için yapılamayan zorunlu bir kapanış saklanır ve yeniden başlatmadan sonra tekrar denenir.

## Gerçek para {#real-money}

Gerçek para isteğe bağlıdır ve bot başına açılır. Açmadan önce botu paper'da çalıştırıp davranışını görün.

**Ne gerekir**

1. Canlı sürüm (varsayılan sürüm yalnızca simüle çalışır).
2. [Ayarlar → Borsa anahtarları](#/settings?tab=keys) bölümünde doğrulanmış bir **Binance işlem anahtarı**: Futures açık, para çekme kapalı. Para çekebilen anahtarlar reddedilir. Gerçek para şimdilik yalnızca Binance'te: Bybit ve OKX kendi test ağlarında uçtan uca denemeyi henüz geçmedi, bu yüzden onlarda gerçek parayı açma isteği reddedilir. Anahtarları yine de hesap görünümü ve pozisyon kapatmak için çalışır.
3. Botun piyasası **Futures** olmalı.
4. Bir koruma seviyesi: DCA'da zarar durdur, Grid'de stop-out ya da her ikisinde drawdown stopu. Borsadaki stop bu seviyede durur.
5. Açık bir paper döngüsü olmamalı ve sembolde Binance hesabınızda pozisyon bulunmamalı.

**Nasıl çalışır**

- Bot kararlarını paper'daki gibi 1 dakikalık mumlarla verir. Her alım, satım ve kapanış Binance'e **piyasa emri** olarak gider (izole marjin, botun kaldıracı). Taker ücreti ödenir, mum kapanışını beklediği için 1–2 dakika gecikme olabilir.
- Pozisyon için Binance'te her zaman bir **stop** durur. Uygulama kapalıyken de pozisyonu korur.
- Bot sayfasındaki **Gerçek para** paneli gerçek pozisyonu, girişi, borsadaki stopu, gerçekleşen sonucu ve son eşitleme saatini gösterir. Üst çubukta **LIVE** rozeti görünür.

**Pilot: ilk adımlar küçük**

Gerçek parayı açtığınızda bot önce pilotla başlar. DCA ve Grid'de ilk 3 gerçek döngü, simüle pozisyonun küçültülmüş bir oranıyla (yaklaşık 100 USDT'lik bir pozisyon) Binance'e gider. Kararlar ve stop seviyesi aynıdır, yalnızca miktar küçüktür. Sinyal botunda ilk 3 gerçek giriş en fazla 50 USDT olur. Binance'in o sembol için minimumu daha yüksekse bu sınır minimumun biraz üstüne çıkar. Paneldeki **Pilotu bitir** düğmesi tam boyuta geçirir; DCA ve Grid'de bu bir sonraki döngüden itibaren geçerli olur.

**Gerçek ve paper farkı**

Gerçek paradaki bir DCA ya da Grid botunun sayfasında **Gerçek ve paper** sekmesi açılır. Her gerçek dolumu, simülasyonun aynı anda yaptığı dolumla karşılaştırır: fiyat farkı (kayma, baz puan), bunun USDT maliyeti ve gecikme. Pozitif kayma maliyettir. Testin asıl çıktısı bu farktır.

**Hangi sinyaller:** Futures botu yalnızca futures için yayınlanan sinyalleri, Spot botu yalnızca spot sinyallerini alır. Diğer piyasanın sinyali atlanan işlemlerde nedeniyle görünür.

**Bot kendini ne zaman durdurur**

| Durum | Ne olur | Ne yapmalı |
|---|---|---|
| Borsadaki stop doldu | Normal zarar durdur; bot çalışmaya devam eder | Bir şey gerekmez |
| Pozisyon Binance'te başka bir yolla kapandı (likidasyon, elle kapatma) | Döngü kapanır, bot durur | Nedenini kontrol edin, botu yeniden başlatın |
| Binance'teki pozisyon botun gönderdiğinden farklı | Bot durur, pozisyon stopla korunur | Pozisyonu Binance'te kontrol edin, gerekirse [Borsa](#/positions?tab=exchange) sekmesinden kapatın |
| Binance 2 dakikadan uzun süre okunamadı ve bot bu sırada işlem yaptı | Bot bugünkü fiyattan telafi etmez, durur | Pozisyonu kontrol edip botu paper'a döndürün |
| Emir 3 kez reddedildi | Bot durur | Bakiyeyi ve minimum emir tutarını kontrol edin |
| Gerçek para, test ağı denemesini geçmemiş bir borsa için açılmıştı | Yeni pozisyon açılmaz; gerçek pozisyon stopuyla korunur ve bot başlatılamaz | Pozisyonu [Borsa](#/positions?tab=exchange) sekmesinden kapatın ve botu paper'a döndürün |

**Çalışırken yapmayın:** kasayı kilitlemek, anahtar değiştirmek, güncelleme kurmak. Uygulama bunları zaten engeller. Bot gerçek paradayken ayarları düzenlenemez ve silinemez; gerçek pozisyon tutarken borsası da değiştirilemez: önce pozisyonu kapatıp botu paper'a döndürün.

## Sık yapılan hatalar {#mistakes}

1. **Kaldıraç artırıp stop kapatmak.** Tek bir sert düşüş tüm bütçeyi tasfiyeyle götürür.
2. **İzleme sapmasını kâr al'a yakın ya da büyük seçmek.** Kâr al seviyesine ulaşan döngü zararla kapanabilir. Form artık büyük ya da eşit değeri reddediyor.
3. **Küçük, yeni listelenmiş coinde DCA.** Bu coinler toparlanmadan sıfıra yaklaşabilir.
4. **Merdiveni kısa tutmak.** 4 güvenlik emriyle %15 kapsayan bir merdiven, kripto için sıradan bir düşüşte biter.
5. **Trend piyasasında grid.** Aralık kırılınca grid ya stopta zararla kapanır ya da değer kaybeden envanter tutar.
6. **Backtest'i tek ayda yapıp sonucu genellemek.**

## Sık sorulan sorular {#faq}

**Gerçek para kaybedebilir miyim?** Paper'da hayır: borsaya emir gitmez. Bir botu [gerçek paraya](#/guide?s=real-money) aldıysanız evet; o botun her işlemi Binance hesabınızda gerçekleşir.

**Zarar durdur neden çalışmadı?** Formda kapalı olabilir. Kapalıyken uyarı satırı "Stop yok: kayıp sınırsız" yazar; bot düşüş durdurucusu açıksa "Kaybı yalnızca bot düşüş durdurucusu sınırlar." yazar.

**Bot neden yeni döngü açmıyor?** Bot sayfasındaki notlara bakın. Olası nedenler: fiyat aralığı koruması, bitiş zamanı, döngü sınırı, BTC kırılımı beklemesi, portföy kesicisi, bütçe sınırı, kaldıracın risk seviyenizin üst sınırını aşması, yeniden başlatmadan sonra devam ettirilmemesi ya da fiyat akışının 3 dakikadan eski olması.

**Botu neden silemiyor ya da değiştiremiyorum?** Silme ve ayarlar bot listesinde değil, botun kendi sayfasında: bot adına tıklayın, ayarlar **Settings** sekmesinde, **Delete** üstteki düğmelerde. Açık bir döngü varken, yani bot bir pozisyon tutarken, silme kapalıdır; sembol ve bütçe gibi döngüyü bozacak ayarlar kilitlidir. Önce **Close cycle** ile döngüyü kapatın, sonra **Delete** açılır. Silmeden de yeni bir bot kurabilirsiniz; bütçe sınırında yer varsa ve aynı sembolde çalışan başka bot yoksa.

**"Paused: Price feed older than 3 minutes" ne demek?** Bot 3 dakikadır yeni fiyat alamadı ve eski fiyatla işlem yapmamak için bekliyor. Genellikle borsanın ya da internet bağlantısının geçici bir kesintisidir. Fiyat gelince bot kendiliğinden devam eder; **Resume**'a basmak bu durumu çözmez. Bu sırada **Close cycle**'a basarsanız kapanış da fiyat gelene kadar bekler, çünkü uygulama eski fiyatla kapanış yazmaz. Bu beklemede yeni bot kurmak da işe yaramaz: yeni bot aynı fiyat kaynağını kullanır.

**Kâr neden küçük görünüyor?** DCA ve Grid küçük ama sık kâr eder. Aylık getiriye bakın, tek döngüye değil.

**Şablonda "Geçemedi" yazıyor, kullanabilir miyim?** Evet, ama testte belirlenen kontrollerden en az birini geçemedi. Uygulama kullanmadan önce onay ister. Önce backtest'te deneyin.

**Hangi coini seçmeliyim?** Son 90 günde işlem hacmi en yüksek coinlerden başlayın. Şablonların çoğu ilk 5 coinde test edildi.

## Terimler sözlüğü {#glossary}

| Terim | Anlamı |
|---|---|
| Ana emir | Döngünün ilk alımı (long) ya da satışı (short) |
| Güvenlik emri | Fiyat aleyhe gittikçe yapılan ek alım ya da satış |
| Ortalama giriş | Döngüdeki tüm alımların ağırlıklı ortalama fiyatı |
| Döngü | Ana emirden kapanışa kadar bir işlem turu |
| Merdiven | Ana emir ve tüm güvenlik emirlerinin fiyat ve tutar listesi |
| Kapsam | Merdivenin karşılayabildiği en büyük fiyat hareketi |
| Basamak (grid) | İki komşu grid fiyatı arasındaki aralık |
| Envanter | Grid'in elinde tuttuğu coin miktarı |
| Gerçekleşmemiş K/Z | Açık pozisyonun şu anki fiyata göre kâr ya da zararı |
| Düşüş (drawdown) | Özkaynağın zirveden en düşük noktaya inişi |
| Tasfiye fiyatı | Kaldıraçlı pozisyonun zorla kapatılacağı fiyat |
| Güven aralığı | Gerçek ortalamanın büyük olasılıkla içinde bulunduğu aralık |
