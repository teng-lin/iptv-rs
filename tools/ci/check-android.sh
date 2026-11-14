#!/usr/bin/env bash
# Type-check the app library and every crate under it for Android, without NSH
# packaging or Gradle. Android-only code (cfg(target_os = "android")) is not
# compiled by the host checks, and the APK job runs on pull requests only when
# packaging inputs change, so this is what catches a Rust change that breaks
# the Android build. Needs the NDK and the Rust target (rustup target add).
# Usage: tools/ci/check-android.sh [target triple, default aarch64-linux-android]
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
source "$root/tools/ci/android-ndk-env.sh"
target=${1:-aarch64-linux-android}
compiler_variable="CC_${target//-/_}"
[ -x "${!compiler_variable:-}" ] || { echo "no NDK compiler for $target: set ANDROID_NDK_HOME" >&2; exit 1; }
cd "$root"
# The generated Android runner links the library; the desktop binary is not built.
cargo check --locked -p iptv-app --lib --target "$target"
