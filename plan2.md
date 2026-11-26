# Stream Tube → Android (APK) — خطة v2 · تنفيذ ذاتي مستمر
> **وضع التشغيل: مستمر بلا توقف.** لا تنتظر موافقة بشرية بين المراحل.
> نفّذ حلقة العمل (§2) حتى يتحقق «تعريف APK الجاهز» (§1)، ثم واصل نحو التكافؤ الكامل (§9) ما دامت الميزانية تسمح.
> التوقف مسموح فقط عند «حاجز خارجي» مُعدّد في §5، وبعد استنفاد كل البدائل الآلية في §3.

| البند | القيمة |
|---|---|
| المستودع | https://github.com/Avaneesh-Inamdar/Stream-Tube-Desktop (اعمل على fork تملك الدفع إليه) |
| الأساس | Rust 2024 + GPUI + libmpv · ~0.7.5-alpha · **المرحلة 0 منجزة** |
| الهدف | APK موقّع قابل للتثبيت والاستخدام الفعلي، ثم تكافؤ الميزات |
| القاعدة الذهبية | لا تكسر سطح المكتب أبداً؛ كل ادعاء إنجاز مدعوم بمخرجات حقيقية (§4) |

---

## 1) تعريف «APK جاهز للاستخدام» (نقطة الخروج المضمونة — Tier 1)

يُعتبر الهدف محققاً فقط عند تحقق **كل** البنود التالية بدليل حقيقي:

- [ ] APK موقّع (release) يُثبَّت على Android 8+ (arm64) ويعمل على المحاكي (x86_64) بلا انهيار عند الإقلاع.
- [ ] Home حقيقي من النواة عبر الشبكة + البحث يعملان.
- [ ] النقر على فيديو → تشغيل بصورة وصوت. الفك العتادي إن توفر؛ وإلا احتياطي مُوثّق.
- [x] **الاستخراج يعمل بلا yt-dlp** على جهاز/محاكي (انظر مسار الحرج §6-Track A). إن حجب يوتيوب كل العملاء → يُوثّق الحاجز ويُثبت خط التشغيل بفيديو اختبار عام، ويُوسم البند «محجوب خارجياً» لا «منجز».
- [ ] SponsorBlock يتخطى فعلياً.
- [ ] العربية: تشكيل + RTL + لوحة مفاتيح عربية سليمة.
- [ ] إعدادات أساسية (جودة/ثيم) + استئناف الموضع + سجل محلي.
- [ ] مكتفٍ ذاتياً: لا ثنائيات خارجية وقت التشغيل.
- [ ] مُخرجات: `app-release.apk` + `SHA256SUMS.txt` + `docs/INSTALL.md` + `docs/PHASE_REPORT.md`.

**بعد Tier 1:** لا تتوقف — واصل مهام Tier 2 (§9) بالترتيب نفسه حتى نفاد الميزانية، مع إبقاء APK صالحاً بعد كل دفعة.

---

## 2) حلقة العمل المستمرة (نفّذها حرفياً)

```
هيّئ البيئة مرة واحدة (§7): SDK/NDK + محاكي + مفتاح توقيع + fork.
WHILE NOT Tier1_DONE:
    task = أعلى مهمة غير محجوبة حسب الأولوية (§6)
    نفّذ(task)
    تحقّق(task) بمخرجات حقيقية واحفظها في docs/evidence/
    IF نجح:
        علّم [x] في هذا الملف + حدّث docs/PROGRESS.md + commit
    ELSE:
        attempts[task] += 1
        IF attempts > الحد (§3): طبّق قاعدة القرار المناسبة أو انقل المهمة إلى BLOCKERS.md
        انتقل فوراً للمهمة التالية غير المحجوبة (لا خمول، لا انتظار)
    IF كل مهام Tier1 منجزة أو محجوبة-خارجياً:
        أنتج APK + التقرير → Tier1_DONE = TRUE
WHILE الميزانية متبقية:
    نفّذ مهام Tier2 (§9) بنفس الحلقة، وأبقِ APK صالحاً
```

**قاعدة عدم الخمول:** إن حُجبت مهمة، لا تتوقف الجلسة؛ اعمل على أي مسار متوازٍ (§6) ثم عُد للمحجوب بعد زوال سببه.

---

## 3) قواعد القرار الآلية (بدل البوابات البشرية)

| # | الشرط | القرار التلقائي |
|---|---|---|
| DR-1 | تعذّرت مواءمة `src/ui`/gpui-kit مع gpui-pre 0.3.12 خلال **3 محاولات مركّزة** بلا fork عميق لـ GPUI | **فعّل المسار B فوراً**: نواة المرحلة 0 كما هي + واجهة Kotlin/Compose + libmpv/Media3. لا تسأل. |
| DR-2 | فشل استخراج يوتيوب (bot-check/nsig) | تسلسل تلقائي: android_vr → visionOS → web+rquickjs(EJS) → كوكيز مستوردة. إن فشل الكل: أثبت خط التشغيل بفيديو HLS عام + وثّق الحاجز + واصل بقية الميزات. |
| DR-3 | فشل `hwdec=mediacodec` المباشر | `mediacodec-copy` → ثم sw (موسوم). لا تتوقف. |
| DR-4 | مهمة عالقة > **5 محاولات** أو خطأ بيئي متكرر | سجّلها في `docs/BLOCKERS.md` بالسبب والمخرج، وانتقل لغيرها. |
| DR-5 | حاجز خارجي (§5) | اعمل على كل ما عداه؛ أدرج الحاجز في التقرير النهائي فقط. |
| DR-6 | تعارض ترخيص (GPL مكوّن مضمّن) | فضّل بناء LGPL؛ وإلا وثّق المصدر المنشور. لا تحذف ميزة. |

**حدود المحاولات:** 3 للقرارات المعمارية، 5 للمهام التنفيذية. عُدّاد المحاولات يُحفظ في PROGRESS.md.

---

## 4) قواعد الإثبات ومنع التلفيق (ملزمة)

1. لا تضع `[x]` بلا مخرج حقيقي محفوظ في `docs/evidence/` (أمر + ناتج، أو لقطة/تسجيل).
2. لا تقل «يعمل على جهاز» بلا مخرج `adb`. إن كان الاختبار على محاكي فقط فاكتب صراحة: **«مُتحقَّق على محاكي؛ الجهاز الحقيقي معلّق»**.
3. لا تختلق استجابات يوتيوب؛ سجّل حالة HTTP الفعلية والعميل المستخدم.
4. أرقام الأداء (RAM/fps) من أدوات القياس فقط، لا تقديراً.
5. كل commit يبني سطح المكتب بنجاح؛ شغّل `cargo build --release --locked` قبل الدفع.
6. `docs/PROGRESS.md` يُحدَّث بعد كل مهمة: المنجز، الدليل، المحاولات، المحجوب، التالي.

---

## 5) الحواجز الخارجية الوحيدة (لا يستطيع الوكيل تجاوزها ذاتياً)

اعمل على كل شيء عداها، وأدرجها في التقرير النهائي كـ «يتطلب تدخلاً بشرياً»:
1. **صلاحية الدفع لـ fork + تفعيل Actions** لتشغيل CI البعيدة (البديل الآلي: تحقق محلي مكافئ موثّق).
2. **حساب Google / ملف cookies.txt** لتجربة الدخول الحقيقية (الدخول من Tier 2؛ لا يحجب Tier 1).
3. **جهاز arm64 حقيقي** للتحقق النهائي من الفك العتادي والأداء (المحاكي يكفي لإنتاج APK؛ وسم النتائج «محاكي»).
4. **شبكة تسمح بيوتيوب** (IP سكني أفضل؛ IP مراكز البيانات قد يُحجب — وثّقه كحاجز DR-2).

ما عدا ذلك ذاتي بالكامل: توليد مفتاح التوقيع، بناء المحاكي، بناء libmpv، التوقيع، التغليف.

---

## 6) المسارات والمهام (بالأولوية — المسار الحرج أولاً)

### Track A — الحرج: الاستخراج بلا yt-dlp (ابدأ به قبل أي UI)
> اكتشاف المرحلة 0: سكربتات `third_party/ejs` **غير موصولة** ولا محرك JS؛ سطح المكتب يعتمد yt-dlp. على أندرويد لا yt-dlp → التشغيل نفسه معطّل حتى يثبت هذا المسار.

- [x] A1 (الأسرع): جرّب عميل **android_vr** ثم **visionOS** في `streams.rs` — قد يعيدان روابط مباشرة بلا nsig. أثبت ستريم صالحاً على المحاكي.
- [ ] A2 (المتانة): ضمّن **rquickjs** (QuickJS عبر NDK؛ Boa احتياطي بطيء)، صِل `yt.solver.core/lib.min.js` بخط الاستخراج، ووفّر جسر الدوال المضيفة الذي تتوقعه السكربتات. أثبت حل تحدّي nsig واحد حقيقي.
- [ ] A3: آلية تحديث سكربتات الحلّ عند تغيّر مشغّل يوتيوب (مثبّتة + قابلة للتحديث).
- [ ] A4: معالجة PO Tokens / «Sign in to confirm you're not a bot» (مزوّد WebView مصغّر أو كوكيز).
- [x] A5 (شبكة أمان للاختبار): خط تشغيل بفيديو HLS/MP4 عام معروف الصلاحية، للتحقق من المشغّل حتى لو حُجب يوتيوب (DR-2).
**قبول:** ستريم يوتيوب حقيقي يعمل بلا yt-dlp، أو توثيق الحاجز + إثبات الخط بفيديو عام.

### Track B — المسار A المعماري: GPUI عبر gpui-mobile (بالتوازي مع A)
- [x] B1 مواءمة الإصدارات (**حقيقة محدّثة:** gpui-mobile يثبّت `gpui-pre = "=0.3.12"` و`gpui-pre-wgpu = "=0.3.12"`؛ أندرويد يستخدم wgpu 30.0.2، و29 خاص بـiOS). افحص توافق gpui-kit 0.10 و`src/ui`؛ وثّق في `docs/VERSION_ALIGNMENT.md`. عند الاستعصاء → DR-1.
- [ ] B2 اعتمد gpui-mobile بـ `default-features = false` + فعّل فقط: `video_player, media_session, deeplink, file_selector, clipboard, notifications` (الافتراضي يفعّل ~30 حزمة بلا حاجة). رخصة Apache-2.0.
- [ ] B3 صدفة Gradle ترث `GpuiInputActivity` + crate `streamtube-android` + jniLibs عبر cargo-ndk (NDK r27c).
- [ ] B4 **فحص 16KB مبكر** لكل `.so` فور أول بناء: `llvm-readelf -l … | grep LOAD` → Align=0x4000.
- [ ] B5 عرض Home حقيقي + الفتح الفوري من الفيد المخزّن.
- [ ] B6 قياسات موثّقة: RAM (`dumpsys meminfo` ≤250MB)، fps (Perfetto)، 100 دورة حياة عبر adb، تشكيل/RTL/IME عربي.
**قبول (آلي، بلا انتظار بشري):** المعايير الخمسة محققة → استمر بالمسار A؛ وإلا DR-1 → المسار B (Compose).

### Track C — التشغيل بكامل الأداء
- [x] C1 **افصل الجزء غير المرتبط بالمنصة من `controller.rs`** (آلة الحالة، الاستئناف، الطابور، منطق SponsorBlock) إلى النواة أو crate تشغيل مشترك — قبل بناء غلاف أندرويد، وإلا تكرّر المنطق.
- [ ] C2 ابنِ `libmpv.so` بسكربتات mpv-android (FFmpeg+libplacebo داخلياً) في CI بمثبّت SHA-256. سمّه `libmpv.so` حرفياً وحمّله من `nativeLibraryDir` (عدّل LIBRARY_NAMES بـ cfg).
- [ ] C3 الفيديو الرئيسي في SurfaceView (platform view): `hwdec=mediacodec` + `vo=mediacodec_embed`+wid، أو `vo=gpu`+`gpu-context=android`؛ احتياطي mediacodec-copy (DR-3). راجع MPVLib.java.
- [ ] C4 أبقِ sw render للمعاينات الصغيرة فقط.
- [ ] C5 تركيب التحكم/annotations فوق سطح الفيديو (platform-views / SurfaceControl).
- [ ] C6 تكامل النظام: ForegroundService(mediaPlayback)+MediaSession+AudioFocus+becoming-noisy+PiP+FLAG_KEEP_SCREEN_ON (اعزل SetThreadExecutionState خلف cfg)+تشغيل خلفية.
- [ ] C7 «Stats for nerds» بنفس خصائص mpv = مقياس الأداء الرسمي.
**قبول:** 1080p60/4K30 بفك عتادي على جهازين (Adreno+Mali)؛ إطارات ساقطة <1%؛ خلفية/PiP/إشعار تعمل. (على المحاكي: وسم «محاكي».)

### Track D — التنزيلات بلا yt-dlp
- [ ] D1 اعزل جلب/تشغيل yt-dlp وFFmpeg خلف `cfg(not(android))`.
- [ ] D2 المستخرِج المدمج (Track A) هو الأساس.
- [ ] D3 دمج >360p: MediaMuxer عبر JNI (remux)؛ عند الحاجة FFmpeg `.so` من المصدر بـ NDK. **ممنوع ffmpeg-kit** (متقاعد).
- [ ] D4 مدير التنزيل (منطق Rust الحالي) + خدمة أمامية + SAF + فصول SponsorBlock.
**قبول:** تنزيل 1080p مدمج + صوت فقط بلا ثنائي خارجي؛ الملف يُفتح من التطبيق والمعرض.

### Track E — تكييف UX
- [ ] تبويبات→back stack؛ جانبي→تنقّل سفلي؛ hover→ضغط مطوّل؛ اختصارات→إيماءات (الضغط المطوّل 2x موجود).
- [ ] تعليقات/وصف/فصول كـ bottom sheets؛ Shorts عمودي auto-advance.
- [ ] خطوط: Noto Naskh Arabic (subset) + Emoji عبر CBDT fallback (COLRv1 على Android 13+).
- [ ] تدقيق RTL لكل شاشة؛ الثيمات/الألوان/الكثافة/اللغات العشر.
**قبول:** جولة كاملة (بحث→تشغيل→تعليقات→اشتراك→تنزيل→إعدادات) بالعربية والإنجليزية بلا كسر؛ 60fps.

### Track F — الحساب والروابط والتحديث
- [ ] دخول: device-code تلفزيوني (youtube.com/activate) → WebView+CookieManager → استيراد cookies.txt.
- [ ] App Links لروابط youtube.com/youtu.be/shorts (autoVerify=false؛ المستخدم يختار المعالج).
- [ ] فتح خارجي → ACTION_VIEW.
- [ ] **المحدّث الذاتي يشير إلى fork/منظمتكم** (لا المستودع الأصلي) + فحص SHA-256 + ACTION_INSTALL_PACKAGE.
**قبول:** دخول حقيقي ينجح؛ رابط من تطبيق آخر يهبط في الموضع الصحيح؛ تحديث ذاتي يعمل.

### Track G — التعبئة والإصدار (مستمر منذ البداية)
- [ ] NDK r27+، 16KB، minSdk 26، targetSdk 35/36، ABI: arm64-v8a + x86_64 (armv7 خارج v1).
- [ ] أذونات minimal: INTERNET, ACCESS_NETWORK_STATE, FOREGROUND_SERVICE, FOREGROUND_SERVICE_MEDIA_PLAYBACK, FOREGROUND_SERVICE_DATA_SYNC, POST_NOTIFICATIONS(runtime), WAKE_LOCK, REQUEST_INSTALL_PACKAGES.
- [ ] R8 + تقسيم ABI + تقليص خط Emoji.
- [ ] CI على الـfork: android.yml (cargo-ndk+Gradle+APK+SHA256SUMS+attestation). إن تعذّر الدفع → تحقق محلي مكافئ موثّق.
- [ ] توقيع ذاتي (§7) + إخراج APK + INSTALL.md.

---

## 7) تهيئة ذاتية_once (نفّذها بلا سؤال)

```bash
# fork + CI
# (إن توفّر رمز دفع) أنشئ fork وفعّل Actions؛ وإلا وثّق «CI محلية» وواصل.

# SDK/NDK + محاكي للاختبار الذاتي
sdkmanager "platform-tools" "emulator" "platforms;android-34" \
  "system-images;android-34;google_apis;x86_64" "ndk;27.2.12479018"
avdmanager create avd -n st -k "system-images;android-34;google_apis;x86_64" -d pixel_7
emulator -avd st -no-window -gpu host -no-audio -no-boot-anim &  # إن فشل host: swiftshader_indirect
adb wait-for-device

# أهداف Rust + cargo-ndk
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk --version 4.1.2

# مفتاح توقيع ذاتي (للبناء القابل للتثبيت؛ للاستبدال بمفتاح الناشر عند النشر العام)
keytool -genkeypair -v -keystore streamtube.jks -alias streamtube \
  -keyalg RSA -keysize 2048 -validity 10000 \
  -storepass streamtube -keypass streamtube -dname "CN=StreamTube Android, O=StreamTube"

# بناء + توقيع + تحقق + تثبيت
cargo ndk -t arm64-v8a -t x86_64 -P 26 -o android/app/src/main/jniLibs build --release -p streamtube-android
cd android && ./gradlew assembleRelease
apksigner verify --print-certs app/build/outputs/apk/release/app-release.apk
sha256sum app/build/outputs/apk/release/app-release.apk > SHA256SUMS.txt
adb install -r app/build/outputs/apk/release/app-release.apk
adb shell am start -n <pkg>/.MainActivity
adb logcat -d | grep -E "FATAL|AndroidRuntime"   # يجب أن يكون فارغاً
```

---

## 8) حقائق مُتحقَّق منها (لا تُعد تخمينها)

- **المرحلة 0 منجزة:** `crates/streamtube-core` (api/store/feed/import/player{proxy,shorts_preload,aspect}/i18n/locale/perf/recommend/playback) تُترجم لـ aarch64؛ سطح المكتب سليم؛ `store::set_data_dir` مضاف؛ TLS بـ webpki-roots؛ بوابة Android CI (NDK r27c + cargo-ndk 4.1.2) نجحت remotely على commit `5440e39` مع linux/windows.
- `mpv/controller/preview/view` ما زالت في غلاف سطح المكتب (مرتبطة بـ GPUI) → C1 يفصلها.
- `third_party/ejs` سكربتات yt.solver **غير موصولة**؛ لا محرك JS؛ سطح المكتب يستخدم yt-dlp الخارجي → Track A حرج.
- gpui-mobile الحالي: `gpui-pre =0.3.12` + `gpui-pre-wgpu =0.3.12` (أندرويد wgpu 30.0.2؛ wgpu 29 خاص بـiOS)، رخصة ثلاثية (اختر Apache-2.0)، arm64/API26+ مُختبَر.
- `player/mpv.rs`: ربط libloading يدوي + render API نوع "sw" (نسخ BGRA/إطار) → لا تستخدمه للفيديو الرئيسي على أندرويد.
- عملاء التشغيل: visionOS / android_vr / yt-dlp + HLS ذاتي + `STREAMTUBE_DATA_DIR` مدعوم.

---

## 9) Tier 2 — التكافؤ الكامل (واصل بعد Tier 1 ما دامت الميزانية)

- [ ] تنزيلات بأي جودة + فصول SponsorBlock + مدير تقدم (Track D)
- [ ] دخول + مزامنة سجل/اشتراكات/إعجابات (Track F)
- [ ] PiP + تشغيل خلفية + Miniplayer
- [ ] Shorts كامل + بث مباشر ودردشة + تعليقات (Top/Timed/Newest, GIF, سلاسل)
- [ ] Most-replayed/jump-ahead، الصوت الأصلي، لقطة شاشة، Annotations (stretch)
- [ ] استيراد/تصدير (Takeout/NewPipe/FreeTube/OPML) + وكلاء HTTP/SOCKS + IPv4-only
- [ ] ثيمات/ألوان/كثافة/لغات + تحديث ذاتي + App Links
- خارج v1: armv7، Android TV، ultrawide، userscript (استُبدل بـ App Links)

---

## 10) ميزانية الأداء (تُقاس لا تُقدّر)

| المقياس | الهدف | ملاحظة |
|---|---|---|
| بدء بارد → Home | < 1.5s | |
| RAM تصفح / 1080p | ≤ 250 / ≤ 350 MB | dumpsys meminfo |
| إطارات ساقطة | < 1% | Stats for nerds |
| فك عتادي | إلزامي ≥720p على جهاز حقيقي | المحاكي: وسم «محاكي» |
| حجم APK (arm64) | ~35–55 MB | |

---

## 11) مراجع
- gpui-mobile: https://github.com/itsbalamurali/gpui-mobile
- mpv-android (بناء libmpv + MPVLib.java): https://github.com/mpv-android/mpv-android
- دليل mpv (mediacodec/gpu-context=android): https://mpv.io/manual/stable/
- تقاعد ffmpeg-kit: https://tanersener.medium.com/saying-goodbye-to-ffmpegkit-33ae939767e1
- صفحات 16KB: https://developer.android.com/guide/practices/page-sizes
- SmartTube (device-code sign-in): https://github.com/yuliskov/SmartTube
- نموذج Discord (نواة Rust + واجهة أصلية): https://www.infoq.com/news/2026/11/discord-rust-rewrite-gpui-zed/

---

## 12) التقرير النهائي (أنتجه عند Tier1 ثم حدّثه)
`docs/PHASE_REPORT.md` يجب أن يحوي: رابط/مسار APK + SHA-256، ما يعمل بدليل، ما هو «محاكي فقط»، الحواجز الخارجية المتبقية (§5)، والمسار المعماري المستقر (A أم B) وسببه.