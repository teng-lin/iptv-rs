#!/usr/bin/env bash
# Builds the Rust crate's Android libnsh.so with nsh and copies it, plus the
# app's NSH assets, into the Gradle host (hosts/android/app/build/nsh).
# Usage: tools/sync-host.sh [--no-build]
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
app="$root/crates/iptv-app"
nsh=${NSH:-/home/blackmyth/src/nsh/target/release/nsh}
out="$root/hosts/android/app/build/nsh"
export JAVA_HOME=${JAVA_HOME:-/usr/lib/jvm/default}
export ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
export ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/30.0.16248370}
export CARGO_PROFILE_RELEASE_LTO=${CARGO_PROFILE_RELEASE_LTO:-thin}
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=${CARGO_PROFILE_RELEASE_CODEGEN_UNITS:-16}

if [ "${1:-}" != "--no-build" ]; then
  # x86_64 for emulators, arm64-v8a for real TV boxes and tablets.
  (cd "$app" && "$nsh" package --target android --abi x86_64,arm64-v8a)
fi

# Newest APK nsh produced; a pattern with no match must not end the script (pipefail).
shopt -s nullglob
apks=("$app"/.nsh/out/*.apk "$app"/.nsh/rust/android/*.apk)
shopt -u nullglob
[ ${#apks[@]} -gt 0 ] || { echo "no APK under $app/.nsh: build the crate first" >&2; exit 1; }
apk=$(ls -t "${apks[@]}" | head -1)
lib="$app/.nsh/rust/android-rust-release/lib"
[ -d "$lib" ] || { echo "no $lib: build the crate first" >&2; exit 1; }

rm -rf "$out" && mkdir -p "$out/jniLibs" "$out/assets"
cp -r "$lib"/. "$out/jniLibs/"
# The app's NSH assets (app.json, images, translations) as nsh packaged them.
(cd "$out/assets" && unzip -q -o "$apk" 'assets/nsh/*' && mv assets/nsh nsh && rmdir assets)
echo "synced from $(basename "$apk"):"
find "$out" -type f | sed "s#$out/##"
