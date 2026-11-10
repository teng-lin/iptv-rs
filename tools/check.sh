#!/usr/bin/env bash
# Every host-side check: formatting, core tests, wasm builds of the portable
# crates, and the app build. Run before committing.
set -euo pipefail
cd "$(dirname "$0")/.."
# Only our packages: `--all` would also format path dependencies (the NSH checkout).
cargo fmt -p iptv-core -p iptv-nav -p iptv-settings -p iptv-epg -p iptv-app --check
cargo test -p iptv-core -p iptv-nav -p iptv-settings -p iptv-epg --quiet
for c in iptv-core iptv-nav iptv-settings iptv-epg; do
  cargo build -p "$c" --target wasm32-unknown-unknown --quiet
done
cargo build -p iptv-app --quiet
echo "all checks passed"
