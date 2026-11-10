#!/usr/bin/env bash
# The macOS app, self-contained: NSH's release runner as an ad hoc signed
# .app (nsh package --target macos), then libmpv and the dylibs it pulls
# in from Homebrew bundled into its Contents/Frameworks and signed again
# (tools/bundle-macos.py). Runs on a Mac without Homebrew (Apple silicon,
# the macOS the bundled dylibs were built for or newer); not notarized.
# Usage: tools/package-macos.sh [output directory, default target/package]
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
nsh=${NSH:-$root/../nsh/target/release/nsh}
out=$(mkdir -p "${1:-$root/target/package}" && cd "${1:-$root/target/package}" && pwd)
rm -rf "$out/iptv-rs.app"
(cd "$root/crates/iptv-app" && "$nsh" package --target macos -o "$out" .)
python3 -I "$root/tools/bundle-macos.py" "$out/iptv-rs.app"
codesign --verify --deep --strict "$out/iptv-rs.app"
echo "$out/iptv-rs.app"
