#!/usr/bin/env bash
# The macOS app, self-contained: NSH's release runner as an ad hoc signed
# .app (nsh package --target macos), then libmpv and the dylibs it pulls
# in from Homebrew bundled into its Contents/Frameworks and signed again
# (tools/bundle-macos.py). Runs on a Mac without Homebrew (Apple silicon,
# the macOS the bundled dylibs were built for or newer); not notarized.
# Usage: tools/package-macos.sh [output directory, default target/package]
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
source "$root/tools/ci/locked-build-env.sh"
nsh=${NSH:-$root/../nsh/target/release/nsh}
out=$(mkdir -p "${1:-$root/target/package}" && cd "${1:-$root/target/package}" && pwd)
rm -rf "$out/Patina.app"
(cd "$root/crates/iptv-app" && "$nsh" package --target macos -o "$out" .)
bash "$root/tools/ci/notices.sh"
cp "$root/LICENSE" "$out/Patina.app/Contents/Resources/LICENSE"
cp "$root/crates/iptv-app/fonts/OFL.txt" "$out/Patina.app/Contents/Resources/OFL.txt"
cp -R "$root/target/notices" "$out/Patina.app/Contents/Resources/notices"
python3 -I "$root/tools/bundle-macos.py" "$out/Patina.app"
python3 "$root/tools/ci/check-notices.py" "$out/Patina.app" native/components.json
python3 "$root/tools/ci/check-notices.py" "$out/Patina.app" native/sources.json
codesign --verify --deep --strict "$out/Patina.app"
echo "$out/Patina.app"
