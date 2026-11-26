# مواءمة إصدارات GPUI للموبايل (B1)

Status: **مكتملة — توافق كامل، لا fork عميق، DR-1 غير مفعّل.**
التاريخ: 2026-11-26 · الأدلة: `docs/evidence/B1/` (سجلات + أوامر قابلة للإعادة) · مرجع gpui-mobile المُدقَّق: `rev 635b7e7` (HEAD).

---

## 1) الخلاصة

gpui-mobile الحالي مبني **على نفس اللقطة التي يستخدمها مشروعنا**:
`gpui-pre "=0.3.12"` و`gpui-pre-wgpu "=0.3.12"`، وgpui-kit 0.10 نفسها تثبّت `gpui-pre "=0.3.12"`
— أي أن الرسم البياني كله **يوحَّد على نسخة واحدة** `gpui-pre 0.3.12` بلا أي انقسام.
عند هدف android لا يسحب gpui-kit أي backend منصة أصلاً (`gpui-pre-platform` خارج رسم android)،
ف_gpui-mobile يملأ هذه الفجوة بتنفيذ `gpui::Platform` عبر Vulkan/wgpu — وهذا هو تكامله المصمَّم.
الاختلاف التجميعي الوحيد في كامل الـ workspace عند هدف android هو سطر واحد في `src/main.rs:86`
(`gpui_kit::application()`) وهو مقصود من upstream نفسه (cfg-gate على android/ios).

**تصحيح مهم لصياغة plan2.md:** «wgpu 29» يخص **هدف iOS فقط** (pin مباشر في قسم iOS
لإصدار semver مختلف يتعايش في القفل). بناء **Android** يستخدم **wgpu 30.0.2 عبر
gpui-pre-wgpu 0.3.12** — نفس النسخة في قفل سطح المكتب الحالي. لا أثر إطلاقاً على
بناء linux/android.

## 2) جدول الإصدارات المُتحقَّق

| المكوّن | عندنا (سطح مكتب، lock حالي) | gpui-mobile @635b7e7 | الحالة |
|---|---|---|---|
| gpui-pre | 0.3.12 (gpui-kit يثبّت `=0.3.12`) | `=0.3.12` حرفياً | ✅ موحّد نسخة واحدة |
| gpui-pre-wgpu | 0.3.12 (عبر gpui-pre-linux) | `=0.3.12` | ✅ نفس النسخة |
| wgpu (linux) | 30.0.2 | (عبر gpui-pre-wgpu) | ✅ يبقى 30.0.2 |
| wgpu (android) | — | **30.0.2** عبر gpui-pre-wgpu (`^30.0.1`) | ✅ مطابق لقفلنا |
| wgpu (iOS فقط) | — | 29.0.4 pin مباشر | ⚪ بند زائد في القفل لا يُبنى على أهدافنا |
| gpui-pre-platform | linux فقط (سطح مكتب) | غير مستخدم | ✅ لا تصادم — فجوة android يملؤها gpui-mobile |

- دقة الحزم تنجح حتى مع `--target all` (نسختا wgpu تتعايشان لأنهما major مختلفان).
- لا خطر انجراف مستقبلي: الطرفان مثبّتان بـ `=0.3.12` حرفياً (gpui-pre 0.3.14 المتاح على
  crates.io لا يمكن أن يتسلل).

## 3) الدليل التجميعي (أهم من الجدول)

1. `cargo check --target aarch64-linux-android --locked --workspace` على مستودعنا:
   - streamtube-core (مع ureq→rustls→ring): **نجح** (بعد ضبط `CC_aarch64_linux_android` من NDK r27c — يتكفل به `cargo ndk` تلقائياً في B3).
   - gpui-kit 0.10 + gpui-pre 0.3.12 + gpui-base/component/assets: **نجح**.
   - الخطأ الوحيد: `gpui_kit::application()` غير موجودة على android لأن
     gpui-kit نفسه يغلقها بـ `#[cfg(not(any(ios, android)))]` (lib.rs:199-200) — وهذا **دليل upstream** أن المسار (gpui-kit + gpui-mobile) مقصود.
2. مسبار تكامل scratch (gpui-kit 0.10 default + gpui-mobile بالـ features الستّ):
   `cargo check --target aarch64-linux-android` → **EXIT=0** (439 حزمة، 1m23s).
3. CI المنبع يشغّل `cargo check --target aarch64-linux-android` للحزمة نفسها ولمثال يبني بـ gpui-kit + gpui-pre =0.3.12.

انظر `docs/evidence/B1/b1-recon.md` للأوامر الحرفية والمخرجات.

## 4) الـ features الدقيقة (B2)

- كل الـ 30 feature في gpui-mobile **فارغة** (`= []`): بوابات `cfg` لوحدات فقط، لا تضيف أي تبعية Rust.
- الطريقة الصحيحة: `default-features = false` + هذه الستّ فقط:
  `video_player, media_session, deeplink, file_selector, clipboard, notifications` — كلها موجودة ومنفّذة فعلاً عبر JNI + Java helpers.
- ملاحظات تنفيذية:
  - **clipboard**: تنفيذ المنصة (داخل AndroidPlatform) هو stub داخل العملية، لكن حزمة clipboard عبر JNI (GpuiClipboard.java) حقيقية → استخدم `gpui_mobile::packages::clipboard::{set_text,get_text}` (لو أردنا لاحقاً توصيله بـ `cx.write_to_clipboard` فذلك رقعة محلية صغيرة، لا fork).
  - **video_player** = غلاف Android MediaPlayer (GpuiVideoPlayer.java) — مرجع فقط؛ الفيديو الرئيسي سيكون mpv عبر `platform_view` (hybrid composition، متاح دائماً بلا feature) — مسار Track C3/C5.
  - **media_session** = MediaSessionCompat (C6)، **deeplink** منفّذ فعلاً (رغم «؟» في TODO.md المنبع) عبر GpuiDeepLink (Track F).
- الرخصة ثلاثية (GPL-3.0 OR AGPL-3.0 OR Apache-2.0) → نعتمد **Apache-2.0** (يحقق DR-6).

## 5) بناء الصدفة (shell) بلا fork — خطة B3

1. التبعية: `gpui-mobile = { git = "…/gpui-mobile", rev = "635b7e7", default-features = false, features = [الستّ] }` (القفل يجمّدها) — أو نسخة path في `third_party/` إن لزم لاحقاً (السابقة موجودة: `vendor/gpui-pre-windows` + `[patch.crates-io]`).
2. قالب الصدفة: انسخ `example/android/gradle/` من gpui-mobile إلى `android/` عندنا وعدّل: namespace/applicationId، `nativeLibraryName`، وقائمة الـ helper activities حسب الستّ المفعّلة. الحقائق المثبتة: AGP 9.1.0، Gradle wrapper 9.9.2، **minSdk 26 / targetSdk 34**، `supportsRtl=true`، `adjustResize`، Vulkan optional + GLES3، jniLibs عبر `cargo ndk` **بلا CMake**، Splash عبر AndroidX حتى `NATIVE_INITIALIZED`.
3. سلسلة الوراثة: `GpuiActivity ← GpuiInputActivity ← NativeActivity` — **GpuiInputActivity إلزامية للـ IME متعدد المراحل** (لوحة مفاتيح عربية، B6) عبر InputConnection أصلي.
4. crate `streamtube-android`: `android_main` → `jni::init_platform` → `shared_platform()` → `Application::with_platform(rc)` (موجودة في gpui-pre 0.3.12 app.rs:242) → `.with_http_client(ui::UreqHttp::new())` (نفس عميل HTTP الحالي، app.rs:282) → `.run(...)`.
5. بناء: `cargo ndk -t arm64-v8a -P 26 -o android/app/src/main/jniLibs build --release -p streamtube-android` ثم `./gradlew assembleRelease`.
6. الخطوط على android: platform.rs يحمّل خطوط النظام صراحةً بما فيها **NotoSansArabic** + نظام emoji (CBDT للأجهزة <API33 من أصول APK)؛ التشكيل عبر cosmic-text/rustybuzz — متوقع أن يعمل، والتحقق التشغيلي (تشكيل/RTL/IME) في B6.

## 6) المخاطر المفتوحة (صغيرة ومحدّدة)

| البند | الحجم | المعالجة |
|---|---|---|
| clipboard على مستوى المنصة = stub | صغير | استخدم حزمة clipboard JNI (جاهزة)، أو رقعة موضعية لاحقاً |
| ring يحتاج CC من NDK | إعداد بيئة فقط | `cargo ndk` يضبطه تلقائياً (أثبتناه يدوياً بـ CC_aarch64_linux_android) |
| بند wgpu 29.0.4 الزائد في القفل | شكلي | يظهر في Cargo.lock لكنه لا يُبنى على linux/android؛ تجاهله |
| جودة IME/تشكيل عربي | غير معروف بعد التشغيل | بند قياس B6 (ليس تعارض إصدارات) |

**القرار:** المسار A (GPUI عبر gpui-mobile) سليم من ناحية الإصدارات والتجميع؛ انتقل إلى B2/B3 مباشرة. DR-1 لا يُفعَّل.
