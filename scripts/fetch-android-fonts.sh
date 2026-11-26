#!/usr/bin/env bash
# Fetch the gpui-mobile CBDT emoji fallback used on Android < 33.
# Pinned to the same gpui-mobile revision as Cargo.toml; binary stays out of git.
set -euo pipefail

REV="635b7e7743b756a1cbe006ee239cc8f3762dc3d9"
SHA256="c2f19f6a404baa7da7a710b018c2892d7b51386983ddca146811f76aea0b6861"
URL="https://raw.githubusercontent.com/itsbalamurali/gpui-mobile/${REV}/example/android/gradle/app/src/main/assets/fonts/NotoColorEmoji.ttf"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${1:-$ROOT/android/app/src/main/assets/fonts/NotoColorEmoji.ttf}"
mkdir -p "$(dirname "$OUT")"
TMP="$(mktemp "$(dirname "$OUT")/.NotoColorEmoji.XXXXXX")"
trap 'rm -f "$TMP"' EXIT
curl -4 --http1.1 --fail --location --retry 3 --connect-timeout 20 "$URL" -o "$TMP"
echo "$SHA256  $TMP" | sha256sum -c -
mv "$TMP" "$OUT"
echo ">> staged $OUT"
