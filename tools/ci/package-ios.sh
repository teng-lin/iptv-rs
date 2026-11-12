#!/usr/bin/env bash
set -euo pipefail
umask 077
: "${VERSION:?}"
test -f target/ios/Patina.app/Info.plist
python3 tools/ci/check-notices.py target/ios/Patina.app mpvkit/components.json
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
# No Cargo, Gradle, dependency download or package build runs with credentials.
root=$PWD
cp "$RUNNER_TEMP/ios.mobileprovision" target/ios/Patina.app/embedded.mobileprovision
security cms -D -i "$RUNNER_TEMP/ios.mobileprovision" > "$RUNNER_TEMP/ios-profile.plist"
python3 tools/ci/ios-entitlements.py "$RUNNER_TEMP/ios-profile.plist" target/ios/Patina.app/Info.plist crates/iptv-app/nsh.json "$RUNNER_TEMP/ios.entitlements"
codesign --force --sign "$IOS_SIGN_IDENTITY" --entitlements "$RUNNER_TEMP/ios.entitlements" target/ios/Patina.app
codesign --verify --deep --strict target/ios/Patina.app
rm -rf target/ipa
mkdir -p target/ipa/Payload dist
cp -R target/ios/Patina.app target/ipa/Payload/
(cd target/ipa && zip -qry "$root/dist/Patina-$VERSION-ipad.ipa" Payload)
