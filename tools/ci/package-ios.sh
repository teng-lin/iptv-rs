#!/usr/bin/env bash
set -euo pipefail
: "${IOS_CERTIFICATE:?}" "${IOS_PROFILE:?}" "${IOS_SIGN_IDENTITY:?}"
keychain="$RUNNER_TEMP/release.keychain-db"
password=$(openssl rand -hex 24)
echo "::add-mask::$password"
printf '%s' "$IOS_CERTIFICATE" | base64 --decode > "$RUNNER_TEMP/ios.p12"
printf '%s' "$IOS_PROFILE" | base64 --decode > "$RUNNER_TEMP/ios.mobileprovision"
security create-keychain -p "$password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
security import "$RUNNER_TEMP/ios.p12" -P "$IOS_CERTIFICATE_PASSWORD" -A -t cert -f pkcs12 -k "$keychain"
security list-keychains -d user -s "$keychain"
security set-key-partition-list -S apple-tool:,apple:,codesign: -k "$password" "$keychain"
rustup target add aarch64-apple-ios
bash tools/fetch-mpvkit.sh
root=$PWD
(cd crates/iptv-app && "$NSH" package . --target ios -o "$root/target/ios" --sign "$IOS_SIGN_IDENTITY" --profile "$RUNNER_TEMP/ios.mobileprovision")
mkdir -p target/ipa/Payload dist
cp -R target/ios/Patina.app target/ipa/Payload/
(cd target/ipa && zip -qry "$root/dist/Patina-$VERSION-ipad.ipa" Payload)
