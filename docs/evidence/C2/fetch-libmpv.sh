#!/usr/bin/env bash
# C2: استرجاع libmpv + تبعياتها من إصدار mpv-android الرسمي 2026-09-17 مع تحقق SHA-256.
# الاستخدام: ./fetch-libmpv.sh [OUT_DIR]   (الافتراضي: ./libmpv-fetch)
# المصدر: https://github.com/mpv-android/mpv-android/releases/tag/2026-09-17
set -euo pipefail

RELEASE_TAG="2026-09-17"
BASE_URL="https://github.com/mpv-android/mpv-android/releases/download/${RELEASE_TAG}"
OUT="${1:-libmpv-fetch}"

# مثبتات SHA-256 للـ APKs (قِس بنفسك عند تغيير الإصدار)
declare -A APK=(
  [arm64-v8a]="app-default-arm64-v8a-release.apk 0b0e0a534dd6b8d2b117f06952e0600064ad97702e48bff02543e3e32107803d"
  [x86_64]="app-default-x86_64-release.apk 347cef5d0c2fe76ec95e19685597ad5d208e5309cc0642d51ae656b125ce1d07"
)

for abi in arm64-v8a x86_64; do
  read -r apk_name apk_hash <<< "${APK[$abi]}"
  mkdir -p "$OUT/$abi"
  echo ">> [$abi] downloading $apk_name ..."
  curl -L --retry 4 --retry-delay 2 --retry-all-errors --connect-timeout 20 \
       -o "$OUT/mpv-android-$abi.apk" "$BASE_URL/$apk_name"
  echo "$apk_hash  $OUT/mpv-android-$abi.apk" | sha256sum -c -
  # استخراج كل المكتبات الأصلية؛ libplayer.so غلاف mpv-android JNI — غير مطلوب لتطبيقنا
  unzip -o -j "$OUT/mpv-android-$abi.apk" "lib/$abi/*.so" -d "$OUT/$abi/" >/dev/null
  echo ">> [$abi] extracted $(ls "$OUT/$abi" | wc -l) libs"
done

echo ">> DONE: $OUT/{arm64-v8a,x86_64}/libmpv.so + deps (official NDK r30 build, 16KB-aligned)"
echo ">> jniLibs set (drop libplayer.so): libmpv libavcodec libavdevice libavfilter libavformat libavutil libswresample libswscale libc++_shared"
