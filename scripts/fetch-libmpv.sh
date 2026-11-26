#!/usr/bin/env bash
# Stage libmpv + its dependencies into the Android shell's jniLibs directory.
#
# Source: the official mpv-android release 2026-09-17 (NDK r30 build, 16KB
# page-aligned, verified in docs/evidence/C2/SHA256SUMS.txt). We reuse its
# prebuilt FFmpeg/libmpv instead of building our own until Track C2 moves the
# build in-tree.
#
# The 9 extracted libraries per ABI: libmpv, libavcodec, libavdevice,
# libavfilter, libavformat, libavutil, libswresample, libswscale,
# libc++_shared. libplayer.so (mpv-android's own JNI wrapper) is dropped.
#
# These are optional at build time: Gradle packages whatever sits in jniLibs,
# and a Home-only debug APK builds fine without them — nothing links libmpv
# until Track C wires playback.
#
# Usage:
#   scripts/fetch-libmpv.sh [OUT_DIR]        (default: android/app/src/main/jniLibs)
#   ST_APK_DIR=/path/with/apks scripts/fetch-libmpv.sh
#       reuse pre-downloaded mpv-android-{arm64-v8a,x86_64}.apk instead of
#       downloading them again (hashes are still verified).
set -euo pipefail

RELEASE_TAG="2026-09-17"
BASE_URL="https://github.com/mpv-android/mpv-android/releases/download/${RELEASE_TAG}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
OUT="${1:-android/app/src/main/jniLibs}"

# SHA-256 pins for the release APKs (docs/evidence/C2/SHA256SUMS.txt).
declare -A APK=(
  [arm64-v8a]="app-default-arm64-v8a-release.apk 0b0e0a534dd6b8d2b117f06952e0600064ad97702e48bff02543e3e32107803d"
  [x86_64]="app-default-x86_64-release.apk 347cef5d0c2fe76ec95e19685597ad5d208e5309cc0642d51ae656b125ce1d07"
)

for abi in arm64-v8a x86_64; do
  read -r apk_name apk_hash <<< "${APK[$abi]}"
  mkdir -p "$OUT/$abi"
  if [[ -n "${ST_APK_DIR:-}" && -f "${ST_APK_DIR}/mpv-android-$abi.apk" ]]; then
    echo ">> [$abi] using ${ST_APK_DIR}/mpv-android-$abi.apk"
    cp "${ST_APK_DIR}/mpv-android-$abi.apk" "$OUT/mpv-android-$abi.apk"
  else
    echo ">> [$abi] downloading $apk_name ..."
    curl -L --retry 4 --retry-delay 2 --retry-all-errors --connect-timeout 20 \
         -o "$OUT/mpv-android-$abi.apk" "$BASE_URL/$apk_name"
  fi
  echo "$apk_hash  $OUT/mpv-android-$abi.apk" | sha256sum -c -
  # All native libs; libplayer.so is mpv-android's JNI wrapper — not ours.
  unzip -o -j "$OUT/mpv-android-$abi.apk" "lib/$abi/*.so" -d "$OUT/$abi/" >/dev/null
  rm -f "$OUT/$abi/libplayer.so"
  echo ">> [$abi] staged $(ls "$OUT/$abi" | wc -l) libs into $OUT/$abi"
  rm -f "$OUT/mpv-android-$abi.apk"
done

echo ">> DONE: $OUT/{arm64-v8a,x86_64} hold the 9 playback libraries (libmpv + FFmpeg + libc++_shared)."
