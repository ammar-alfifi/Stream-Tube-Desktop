# C2 — جلب libmpv.so حقيقية لـ arm64-v8a + x86_64 والتحقق منها

> **الحالة: ✅ مكتمل — عبر مسار «الجلب من إصدار mpv-android الرسمي».**
> لا حاجة لحاجز DR-4 ولا لخطة Media3 البديلة (مسار البناء من المصدر موثّق في §7 كخيار CI).
>
> - التاريخ: 2026-11-26 (10:47–11:00 UTC تقريبًا)
> - الأدلة الخام: `~/.hermes/cache/scratch/streamtube-native/` (نسخ من السجلات والسكربتات في هذا الدليل)
> - المصدر: إصدار mpv-android الرسمي **`2026-09-17`** من GitHub Releases (بناء upstream نفسه بـ NDK r30)

---

## 0) الخلاصة التنفيذية

| المطلب (plan2.md C2) | النتيجة | الدليل |
|---|---|---|
| `libmpv.so` حقيقية arm64-v8a | ✅ 8,256,792 بايت | `verify-libmpv-arm64-v8a.log` + SHA256 |
| `libmpv.so` حقيقية x86_64 | ✅ 8,423,392 بايت | `verify-libmpv-x86_64.log` + SHA256 |
| الاسم حرفيًا `libmpv.so` | ✅ SONAME=`libmpv.so` | llvm-readelf -d |
| شحمات client/render API | ✅ 54 رمز `mpv_*` مصدّرة لكل ABI (mpv_create/initialize/command/render_context…) | llvm-readelf --dyn-syms |
| محاذاة 16KB | ✅ **كل الـ 20 مكتبة** (libmpv + 9 تبعيات × ABI) كل LOAD=Align `0x4000` | `verify-16kb.log` |
| تشغيل حقيقي على أندرويد | ✅ x86_64 **native** على المحاكي API35: `mpv_initialize()=0`؛ arm64 عبر ndk_translation (موسوم) | `runtime-test.log` |
| مثبّت SHA-256 | ✅ 29 ملفًا في `SHA256SUMS.txt` | سكربت استرجاع قابل للتكرار `fetch-libmpv.sh` |
| mediacodec للـ C3 | ✅ موجود في البناء (`mediacodec` + NEEDED `libmediandk.so`) | `verify-libmpv-*.log` |

ملاحظة صدق (قاعدة §4-2): **تحقق x86_64 native على المحاكي؛ تحقق arm64 عبر مترجم ndk_translation على نفس المحاكي — الجهاز arm64 الحقيقي معلّق (حاجز خارجي §5-3).**

---

## 1) القرار والمنهج

خياران كانا متاحين:
1. **البناء من المصدر** (`mpv-android/buildscripts/buildall.sh`): FFmpeg + ~15 تبعية (mbedtls, dav1d, libxml2, freetype, fontconfig, fribidi, harfbuzz, unibreak, libass, lua 5.2.4, libplacebo, curl…) لكل ABI — ساعة+ لكل ABI على 8 أنوية.
2. **الجلب من الإصدار الرسمي `2026-09-17`**: نفس بناء upstream الذي يُشحن للمستخدمين، مبني بـ NDK r30 مع **فرض 16KB صراحةً** عبر `LDFLAGS="-Wl,-z,max-page-size=16384"` (buildall.sh:71)، ومثبّت بهاشاتنا.

اختَرنا (2) لأنه يحقق كل معايير القبول في دقائق مع إثبات كامل (ساكن + تشغيل حي)، و(1) موثّق حرفيًا في §7 كمسار CI إن لزم بناء LGPL مخصص (انظر DR-6 في §6).

---

## 2) الأوامر الحقيقية المنفذة

```bash
# (أ) الجلب — الإصدار الرسمي 2026-09-17 (per-ABI APKs)
cd ~/.hermes/cache/scratch/streamtube-native
curl -L --retry 4 --retry-all-errors -o mpv-android-arm64.apk \
  "https://github.com/mpv-android/mpv-android/releases/download/2026-09-17/app-default-arm64-v8a-release.apk"
curl -L --retry 4 --retry-all-errors -o mpv-android-x86_64.apk \
  "https://github.com/mpv-android/mpv-android/releases/download/2026-09-17/app-default-x86_64-release.apk"
# SHA256 للـ APKs (مطابقة لأحجام GitHub API: 34290346 / 40376972 بايت):
#   0b0e0a534dd6b8d2b117f06952e0600064ad97702e48bff02543e3e32107803d  mpv-android-arm64.apk
#   347cef5d0c2fe76ec95e19685597ad5d208e5309cc0642d51ae656b125ce1d07  mpv-android-x86_64.apk

# (ب) الاستخراج — libmpv + كل المكتبات الأصلية
unzip -o -j mpv-android-arm64.apk "lib/arm64-v8a/*.so" -d arm64-v8a/
unzip -o -j mpv-android-x86_64.apk "lib/x86_64/*.so"  -d x86_64/

# (ج) التحقق الساكن — NDK r27c المحلي (27.2.12479018)
RE=~/Android/Sdk/ndk/27.2.12479018/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-readelf
$RE -d arm64-v8a/libmpv.so | grep -E "SONAME|NEEDED"          # SONAME=libmpv.so + 15 NEEDED
$RE -l arm64-v8a/libmpv.so | grep LOAD                        # Align=0x4000 لكل LOAD
$RE -n arm64-v8a/libmpv.so | grep -A2 NT_ANDROID_TYPE_IDENT  # API level 23 + NDK r30
$RE --dyn-syms arm64-v8a/libmpv.so | grep -cE "GLOBAL DEFAULT +[0-9]+ mpv_"   # 54
strings -a arm64-v8a/libmpv.so | grep -m1 "mpv v0"            # mpv v0.41.0-1049-g0b7ed670f
strings -a arm64-v8a/libmpv.so | grep -m1 "^mediacodec$"     # mediacodec موجود

# (د) التحقق الحي — dlopen على المحاكي (API 35، x86_64)
#     ملف الاختبار: dlopen-test.c (نسخة في هذا الدليل) — مترجم بـ NDK r27c بأهداف android26
NDKBIN=~/Android/Sdk/ndk/27.2.12479018/toolchains/llvm/prebuilt/linux-x86_64/bin
$NDKBIN/x86_64-linux-android26-clang  dlopen-test.c -o test-mpv-x86_64 -ldl
$NDKBIN/aarch64-linux-android26-clang dlopen-test.c -o test-mpv-arm64   -ldl
adb push x86_64/*.so test-mpv-x86_64 /data/local/tmp/c2libs/ 2>/dev/null; adb shell mkdir -p /data/local/tmp/c2libs
adb shell "cd /data/local/tmp/c2libs && LD_LIBRARY_PATH=. ./test-mpv-x86_64 libmpv.so /data/local/tmp/c2libs"
adb shell "cd /data/local/tmp/c2libs-arm64 && LD_LIBRARY_PATH=. ./test-mpv-arm64 /data/local/tmp/c2libs-arm64/libmpv.so /data/local/tmp/c2libs-arm64"
```

مخرجات (د) الحرفية (كاملة في `runtime-test.log`):

```
===== x86_64 (NATIVE على المحاكي) =====
preloaded 8 deps OK
dlopen("libmpv.so", RTLD_NOW) = 0xbedf097c2071a1b9 OK
mpv_client_api_version = 0x20005 (client API 2.0)
mpv_create() = 0x71829a66ac80
mpv_initialize() = 0 (success)
mpv_terminate_destroy() OK
RUNTIME VERIFICATION PASSED

===== arm64-v8a (عبر ndk_translation — موسوم) =====
preloaded 8 deps OK
dlopen("/data/local/tmp/c2libs-arm64/libmpv.so", RTLD_NOW) = 0x31edf2aed92a58a5 OK
mpv_client_api_version = 0x20005 (client API 2.0)
mpv_create() = 0x7414a74d2e40
mpv_initialize() = 0 (success)
mpv_terminate_destroy() OK
RUNTIME VERIFICATION PASSED
```

`mpv_initialize()=0` حتى بلا واجهة وبلا فيديو — أي أن تحميل FFmpeg+libplacebo+OpenSLES كامل داخل Android نجح فعليًا، وليس مجرد حل رموز.

---

## 3) نتائج llvm-readelf (مختصرة — التفصيل في السجلات)

| الفحص | arm64-v8a | x86_64 |
|---|---|---|
| Class/Machine | ELF64 / AArch64 | ELF64 / Advanced Micro Devices X86-64 |
| SONAME | `libmpv.so` | `libmpv.so` |
| LOAD Aligns | `0x4000` ×3 | `0x4000` ×3 |
| ELF Note API/NDK | 23 / `r30` (16248370) | 23 / `r30` (16248370) |
| `mpv_*` مصدّرة | 54 | 54 |
| نسخة mpv المدمجة | `v0.41.0-1049-g0b7ed670f` | نفسها |
| NEEDED (غير نظامية) | libavcodec/avfilter/avformat/avutil/avdevice/swresample/swscale/libc++_shared | نفسها |
| mediacodec | ✅ (`mediacodec` + NEEDED `libmediandk.so` في libavcodec) | ✅ |
| libplacebo | ✅ مدمج ساكنًا داخل libmpv (بدليل مسار البناء المسرب `.../prefix/arm64/usr/local/include/libplacebo/...`) | ✅ |

- `mpv_client_api_version()` أعادت `0x20005` كما رُصدت في التنفيذ (client API 2.x).
- **فحص 16KB شمل 20 مكتبة** (10 لكل ABI): libmpv + libavcodec + libavdevice + libavfilter + libavformat + libavutil + libswresample + libswscale + libc++_shared + libplayer — **كلها OK_16KB، ANY_FAIL=0**.

---

## 4) ما يجب على مهام لاحقة (B3/C3/G) معرفته

1. **مجموعة jniLibs المطلوبة = 8 مكتبات لكل ABI** (بدون `libplayer.so` — تلك غلاف JNI الخاص بتطبيق mpv-android ولا نحتاجه؛ تحميلنا عبر Rust/libloading):
   `libmpv.so, libavcodec.so, libavdevice.so, libavfilter.so, libavformat.so, libavutil.so, libswresample.so, libswscale.so, libc++_shared.so` — التسع معًا (libc++_shared مشتركة).
2. **FFmpeg مشحونة `.so` منفصلة وليست مدمجة داخل libmpv** (خلاف توقع plan2 «FFmpeg+libplacebo داخلياً»)؛ لكنها كلها داخل الـ APK → شرط «مكتفٍ ذاتياً: لا ثنائيات خارجية وقت التشغيل» متحقق كما هو. libplacebo **مدمج** ساكنًا.
3. الاسم حرفيًا `libmpv.so` → `LIBRARY_NAMES` في `player/mpv.rs` **لا يحتاج تعديل cfg** لمجرد الاسم (يبقى التعديل المطلوب فقط لتحميلها من `nativeLibraryDir` على أندرويد).
4. min-API للثنائيات = **23** ⊇ هدفنا 26. محاذاة 16KB جاهزة لمتطلب Google Play.
5. إغلاق التبعيات مكتمل: كل NEEDED إما داخل المجموعة أو مكتبات نظام (`libm, libz, libdl, libandroid, libmediandk, libOpenSLES, libEGL, liblog, libc`) — لا تبعية خارجية.
6. `hwdec=mediacodec` و`vo=gpu`/`gpu-context=android` مدعومان في هذا البناء (لازم C3؛ حقل `mediacodec_embed` موجود في mpv v0.41).
7. **قيد ndk_translation مكتشف**: مترجم المحاكي يتجاهل `LD_LIBRARY_PATH` للتبعيات الانتقالية؛ اختبارنا يحمّل التبعيات الثمان بمسارات مطلقة بترتيب طوبولوجي قبل `libmpv.so`. ذو صلة لاحقًا عند تشغيل اختبارات C3 على المحاكي x86_64 بترجمة arm64 — على arm64 native لن يظهر.

---

## 5) الثبات (Provenance) وSHA-256

- المصدر: `https://github.com/mpv-android/mpv-android/releases/tag/2026-09-17` (نُشر 2026-09-17).
- APK المصدر وهاشاه مثبتان أعلاه (§2-أ)، والـ 20 مكتبة + ملفات الاختبار كلها في `SHA256SUMS.txt` (29 مدخلًا).
- أبرز الهاتشات (البقية في `SHA256SUMS.txt`):

```
03d2d02db0451cebf4da2648188df7b2502005c6409fad1841b1d2b91ca8a814  arm64-v8a/libmpv.so
97f0621d8f6e236ed5491a89565c2ed526f5450074ec58df560ada5f5f1bc5f8  x86_64/libmpv.so
```

- ملفات الاختبار/السجلات/السكربتات منسوخة في هذا الدليل؛ الثنائيات الكاملة في
  `~/.hermes/cache/scratch/streamtube-native/{arm64-v8a,x86_64}/` (⚠️ scratch يُقلَّم بعد 24h خمول — الاسترجاع أمران عبر `fetch-libmpv.sh` المنسوخ هنا، مع تحقق هاش تلقائي).

---

## 6) DR-6 (رخصة) — نقطة قرار موثّقة، ليست حاجزًا لـ C2

- رخصة مستودع Stream-Tube-Desktop: **MIT**.
- البناء الرسمي المستجلَب: FFmpeg بـ `--enable-{gpl,version3}` (ffmpeg.sh:32) وmpv بدون `-Dgpl=false` (افتراضي meson: GPL مفعّل) → **الثنائيات GPL.**
- خيارات المنسق قبل أي إصدار عام:
  - **(أ) LGPL (المفضّل بقاعدة DR-6):** بناء مخصص في CI — تعديلان فقط موثقان في §7. يفقد فقط مكوّنات GPL داخل FFmpeg (لا تُستخدم في خط التشغيل لدينا؛ dav1d/mbedtls/mediacodec كلها غير-GPL).
  - **(ب) توزيع GPL مع نشر المصدر** (المستودع مفتوح أصلًا) — لكنه يغيّر رخصة التوزيع الفعلي للتطبيق المجمّع؛ يتطلب موافقة صريحة من مالك المستودع.
- لا يحجب بناء APK الداخلي/الاختباري؛ يلزم حسمه قبل النشر العام (Track G).

---

## 7) مسار البناء من المصدر (جاهز للـ CI — للحصول على LGPL أو تحكم كامل)

```bash
git clone https://github.com/mpv-android/mpv-android && cd mpv-android/buildscripts
./buildall.sh --arch arm64  mpv        # يبني libmpv + كل التبعيات في prefix/arm64
./buildall.sh --arch x86_64 mpv        # الناتج: prefix/<arch>/usr/local/lib/*.so
```

- السكربتات تفرض `apilvl=23` و`LDFLAGS="-Wl,-z,max-page-size=16384"` (16KB) وNDK مثبّت `v_ndk=r30` (30.0.16248370) في `include/depinfo.sh`؛ يمكن توجيه `ANDROID_NDK_ROOT` نحو r27c المحلي إذا لزم.
- إصدارات التبعيات المثبتة في depinfo.sh (master بتاريخ 2026-11-26): lua 5.2.4، mbedtls 3.6.8، libxml2 2.15.4، freetype 2.15.0، fontconfig 2.18.4، fribidi 1.0.18، harfbuzz 15.2.0، unibreak 8.0.1، curl 8.23.0، FFmpeg `n9.1` — والبناء المستجلَب فعليًا يحمل `mpv v0.41.0-1049-g0b7ed670f` وFFmpeg بـ`libavcodec 63.13.100`.
- **لجعله LGPL:** في `scripts/ffmpeg.sh` استبدل `--enable-{gpl,version3}` بـ `--disable-{gpl,version3}`؛ وفي `scripts/mpv.sh` أضف `-Dgpl=false` لأمر meson. (نسخ السكربتات الأصلية محفوظة للمرجع في scratch: `upstream-buildscripts/`.)

---

## 8) خطة بديلة Media3 (احتياط DR-4 — **لم تُفعّل، لم تلزم**)

لو انسدّ الجلب والبناء معًا: ExoPlayer/Media3 (androidx، Apache-2.0) عبر MediaCodec + تشغيل HLS/الداش بلا ثنائيات أصلية، مع خسارة: فك يدوي متقدم خلف المستخرِج (Track A)، مرشحات libplacebo/مقاييس mpv («Stats for nerds» C7 لن تكون بخصائص mpv نفسها)، وواجهة render API التي يبنى عليها غلاف التحكم. المسار غير مطلوب الآن.

---

## 9) ملفات هذا الدليل

| ملف | الوصف |
|---|---|
| `REPORT.md` | هذا التقرير |
| `SHA256SUMS.txt` | 29 هاشًا (APKs + 20 مكتبة + اختبارات وسجلات) — مسارات نسبية إلى جذر scratch |
| `verify-16kb.log` | فحص محاذاة كل المكتبات العشرين |
| `verify-libmpv-arm64-v8a.log` / `verify-libmpv-x86_64.log` | فحص عميق لكل libmpv (SONAME/NEEDED/LOAD/رموز/نسخ/mediacodec) |
| `runtime-test.log` | تشغيل dlopen الحي على المحاكي (x86_64 native + arm64 عبر المترجم، موسوم) |
| `dlopen-test.c` | مصدر اختبار التشغيل (مترجم بـ NDK r27c بأهداف android26) |
| `fetch-libmpv.sh` | استرجاع قابل للتكرار مع تحقق هاش (الاسترداد بعد تقليم scratch) |

> لا commit ضمن نطاق هذه المهمة؛ التحديث في plan2.md/PROGRESS.md متروك للمنسّق.
