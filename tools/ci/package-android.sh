#!/usr/bin/env bash
# Assemble distribution-signed APK/AAB files from tools/sync-host.sh's output.
# Signing material lives outside the checkout (RUNNER_TEMP in CI).
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${VERSION:?}" "${ANDROID_HOME:?}" "${KEYSTORE_PATH:?}"
: "${STORE_PASSWORD:?}" "${KEY_PASSWORD:?}" "${KEY_ALIAS:?}"
test -r "$KEYSTORE_PATH"

(cd hosts/android && ./gradlew --no-daemon assembleRelease bundleRelease)
apk=hosts/android/app/build/outputs/apk/release/app-release.apk
aab=hosts/android/app/build/outputs/bundle/release/app-release.aab
apksigner="$ANDROID_HOME/build-tools/35.0.1/apksigner"
"$apksigner" verify "$apk"
jarsigner -verify "$aab"

# Refuse a debug-signed artifact if Gradle ever falls back to its debug key.
expected=$(keytool -exportcert -keystore "$KEYSTORE_PATH" -alias "$KEY_ALIAS" \
  -storepass:env STORE_PASSWORD | sha256sum | cut -d' ' -f1)
apk_cert=$("$apksigner" verify --print-certs "$apk" | sed -n 's/^Signer #1 certificate SHA-256 digest: //p')
aab_cert=$(keytool -printcert -rfc -jarfile "$aab" | openssl x509 -outform DER | sha256sum | cut -d' ' -f1)
test "$apk_cert" = "$expected"
test "$aab_cert" = "$expected"

mkdir -p dist
cp "$apk" "dist/Patina-$VERSION-android-tablet-tv.apk"
cp "$aab" "dist/Patina-$VERSION-android-tablet-tv.aab"
