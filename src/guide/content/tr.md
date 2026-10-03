# DCA ve Grid botları rehberi

Bu rehber DCA ve Grid botlarını hiç kullanmamış biri için yazıldı. Baştan sona okumanız gerekmez: ilk iki bölüm başlamak için yeterli, gerisi ihtiyaç duyduğunuzda dönmeniz için.

> Aleph Edge'deki DCA ve Grid botları **yalnızca simüle** çalışır. Borsaya emir gitmez, para kaybetmezsiniz. Buradaki her şeyi gönül rahatlığıyla deneyebilirsiniz.

## 5 dakikada başlayın {#start}

1. Kenar çubuğunda [Hazır ayarlar](#/presets) sayfasını açın.
2. "Karar" sütununda **Kontrolleri geçti** yazan bir şablon seçin ve **Hazır ayarı kullan** düğmesine basın.
3. **Parite** olarak büyük ve likit bir coin seçin (BTCUSDT, ETHUSDT gibi).
4. **Bütçe** alanına denemek istediğiniz tutarı yazın. Simüle olduğu için gerçek para gerekmez.
5. Sağdaki **Özet** panelinde "Bütçe kontrolü: Sığıyor" yazdığını görün.
6. **Oluştur ve başlat** düğmesine basın.

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

Kaldıraç bu riski tasfiyeye çevirir. 1x long tasfiye edilmez. 5x long ise ortalama girişin yaklaşık %20 altında tasfiye olur. Uygulama bu yüzden kaldıraçlı ve stopsuz bir DCA'yı başlatmadan önce sizden onay ister.

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

[Hazır ayarlar](#/presets) sayfasındaki her şablon, 2 yıllık Binance vadeli verisinde aynı simülatörle test edildi. Ücretler, kayma ve mum içi en kötü sıra hesaba katıldı. Sonuçlar üç döneme ayrılır:

- **Eğitim (TRAIN):** Ekim 2024 ile Haziran 2025 arası.
- **Doğrulama (VALID):** Temmuz 2025 ile Ocak 2026 arası.
- **Test (TEST):** Şubat 2026 ile Eylül 2026 arası. Ayarlar bu döneme bakılmadan seçildi.

Bir şablonun **Kontrolleri geçti** etiketi alması için:

1. Üç dönemin üçünde de bot başına aylık ortalama getiri pozitif olmalı.
2. Test döneminde %95 güven aralığının alt ucu sıfırın üstünde olmalı.
3. Test döneminin 8 ayının 8'i de pozitif olmalı.
4. Hiç tasfiye olmamalı.

Birini bile sağlamayan şablon **Geçemedi** etiketi alır ve hangi kontrolü geçemediği yazılır. Bu şablonlar öğrenmek ve denemek için listede durur. Kullanmadan önce uygulama sizden onay ister.

### 3 Ekim 2026 testinin sonucu

12 şablondan **2'si** dört kontrolün dördünü de geçti:

| Şablon | Test dönemi, bot başına aylık | Pozitif test ayı | En kötü bot düşüşü |
|---|---|---|---|
| DCA Long Klasik | +%1,98 | 8/8 | −%20,7 |
| DCA Long Temkinli | +%1,51 | 8/8 | −%15,8 |

Geçemeyenlerden öğrenilecekler:

- **DCA Long Hızlı** test döneminde çok iyiydi (ayda +%5,63), ama doğrulama döneminde ayda −%6,84 kaybetti. Kısa merdiven, sert bir düşüşte bütçeyi erken tüketir.
- **DCA Long BTC** iki dönemde kazandı, doğrulama döneminde hafif zararda kaldı.
- **DCA Long Altcoin** test döneminde zarar etti; en kötü bot bütçesinin %97'sini kaybetti.
- **DCA Long Stoplu** stop yüzünden dipte satıp toparlanmayı kaçırdı; doğrulama döneminde zararda.
- **DCA Short** şablonlarının ikisi de **1x'te bile tasfiye oldu**. Short pozisyonda fiyat iki katına çıkınca teminat biter; kripto bunu birkaç ayda yapabilir.
- **Grid** şablonlarının dördü de dönemlerin çoğunda zarar etti. Fiyat aralıktan çıkınca stop ya da süre dolumuyla zararına kapandılar.

### Tablodaki sütunlar

| Sütun | Anlamı |
|---|---|
| Test ort./ay | Test döneminde bot başına aylık ortalama getiri, bütçenin yüzdesi |
| %95 aralık | Bu ortalamanın güven aralığı. Aralık ne kadar dar ve sıfırdan ne kadar uzaksa sonuç o kadar güvenilir |
| Pozitif aylar | Test döneminde kazandıran ay sayısı / toplam ay |
| Düşüş | Test döneminde en kötü botun en büyük düşüşü |
| Karar | Kontrolleri geçti ya da Geçemedi |

> Geçmiş simülasyon gelecek getirinin garantisi değildir. Test verisinde 2022 tipi uzun bir ayı piyasası yok. Bir şablonun geçmesi, yalnızca bu 2 yılda ve bu kurallarla iyi çalıştığını gösterir.

## DCA ayarları tek tek {#dca-settings}

### Bot bölümü

| Ayar | Ne işe yarar | Öneri |
|---|---|---|
| Ad | Botu listede tanımanız için | Coin ve stratejiyi yazın: "BTC DCA temkinli" |
| Piyasa | Spot ya da Vadeli | Vadeli 1x, spot'a çok yakındır ve short'a izin verir |
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
| Zarar durdur | Ortalamanın bu kadar altında piyasa emriyle çıkar | Kayıp sınırlanır ama dipte satıp toparlanmayı kaçırabilirsiniz |
| En uzun döngü süresi | Bu süreyi geçen döngüyü kapatır | Kısa tutulursa zararına kapanışlar artar |

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
| Bot düşüş durdurucusu | Bot özkaynağı zirvesinden bütçenin bu yüzdesi kadar düşünce kapatır ve durdurur |
| BTC kırılımında yeni döngüleri beklet | Sentinel BTC'de kırılım bildirirken yeni döngü açılmaz |
| Portföy kesicisi | Kesiciye dahil botların toplam zararı %15'e ulaşınca hepsini kapatır |

**Korumalar DCA'da iki yönlüdür.** DCA'nın kazancı düşüşte bekleyip dönüşte satmaktan gelir. Dipte kapatan her koruma bu dönüşü kaçırabilir. `dca_long_classic` testinde portföy kesicisi açıkken 2 yıllık sonuç +%41,5'ten +%29,8'e düştü. Koruma açmak sizin risk tercihinizdir: daha az kazanç, daha sınırlı kayıp.

## Riskinizi hesaplayın {#risk}

Bir bot başlatmadan önce şu üç soruyu cevaplayın:

1. **En kötü durumda ne kaybederim?** Özet panelindeki "En kötü durum" satırı stopla, tasfiyede ya da "Sınırsız: stop yok" olarak yazar. 1x ve stopsuz DCA'da teorik kayıp, coin sıfıra giderse bütçenin tamamıdır.
2. **Bütçe ne kadar süre kilitli kalabilir?** Stopsuz DCA, toparlanma gelene kadar bütçeyi tutar. Aylarca beklemeye hazır olmalısınız.
3. **Tasfiyeye ne kadar mesafe var?** Kaldıraçlı DCA'da Özet panelindeki "Son emirden likidasyona" satırı, son güvenlik emrinden sonra fiyatın ne kadar daha gidebileceğini yazar.

**Bütçe kuralı:** risk seviyenize göre tüm DCA ve Grid botlarının toplam bütçesi, beyan ettiğiniz bakiyenin %20 ile %80'i arasında bir sınırı geçemez. Bu sınır [Risk ve güvenlik](#/risk) sayfasında görünür.

## Backtest: önce geçmişte deneyin {#backtest}

[Geriye dönük test](#/backtest) sayfası, bir ayarı geçmiş fiyatlarda aynı motorla çalıştırır.

1. Strateji türünü, coini ve ayarları seçin.
2. Zaman aralığını (30 günden 2 yıla kadar ya da özel) ve mum aralığını seçin. Kısa mum aralığı (15 dk, 1 sa) mum içi hareketi daha doğru yansıtır.
3. **Testi çalıştır** düğmesine basın.

### Raporu okumak

| Alan | Anlamı |
|---|---|
| Net | Dönem sonundaki toplam kâr ya da zarar, bütçenin yüzdesi |
| En büyük düşüş | Özkaynağın zirveden gördüğü en derin düşüş |
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
| Sürdür | Botun yeni döngü açmasına izin verir |
| Duraklat | Yeni döngüyü durdurur, açık döngüye dokunmaz |
| Döngüyü kapat ve durdur | Açık döngüyü hemen piyasa fiyatından kapatır |
| Kopyala | Aynı ayarlarla yeni bot formu açar |
| Sil | Durmuş ve pozisyonsuz botu listeden kaldırır; geçmişi saklanır |

**Acil durum:** [Pozisyonlar](#/positions) sayfasındaki **Tümünü kapat** düğmesi tüm simüle pozisyonları ve DCA/Grid döngülerini kapatır, botları durdurur.

**Uygulama yeniden açılınca** botlar Durdu olarak gelir. Açık döngüler yönetilmeye devam eder ama yeni döngü için Sürdür'e basmanız gerekir.

## Sık yapılan hatalar {#mistakes}

1. **Kaldıraç artırıp stop kapatmak.** Tek bir sert düşüş tüm bütçeyi tasfiyeyle götürür.
2. **İzleme sapmasını kâr al'a yakın ya da büyük seçmek.** Kâr al seviyesine ulaşan döngü zararla kapanabilir. Form artık büyük ya da eşit değeri reddediyor.
3. **Küçük, yeni listelenmiş coinde DCA.** Bu coinler toparlanmadan sıfıra yaklaşabilir.
4. **Merdiveni kısa tutmak.** 4 güvenlik emriyle %15 kapsayan bir merdiven, kripto için sıradan bir düşüşte biter.
5. **Trend piyasasında grid.** Aralık kırılınca grid ya stopta zararla kapanır ya da değer kaybeden envanter tutar.
6. **Backtest'i tek ayda yapıp sonucu genellemek.**

## Sık sorulan sorular {#faq}

**Gerçek para kaybedebilir miyim?** Hayır. DCA ve Grid botları yalnızca simüle çalışır, borsaya emir gitmez.

**Zarar durdur neden çalışmadı?** Formda kapalı olabilir. Kapalıyken uyarı satırı "Stop yok: kayıp sınırsız" yazar.

**Bot neden yeni döngü açmıyor?** Bot sayfasındaki notlara bakın. Olası nedenler: fiyat aralığı koruması, bitiş zamanı, döngü sınırı, BTC kırılımı beklemesi, portföy kesicisi, bütçe sınırı ya da fiyat akışının 3 dakikadan eski olması.

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
