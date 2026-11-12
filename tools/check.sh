#!/usr/bin/env bash
# Every host-side check: formatting, core tests, wasm builds of the portable
# crates, and the app build. Run before committing.
set -euo pipefail
cd "$(dirname "$0")/.."
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools -p test_version.py
# Only our packages: `--all` would also format path dependencies (the NSH checkout).
cargo fmt -p iptv-core -p iptv-nav -p iptv-settings -p iptv-epg -p iptv-providers -p iptv-guide -p iptv-import -p iptv-net -p iptv-store-sqlite -p iptv-search -p iptv-app --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo deny --locked check
cargo test --locked -p iptv-core -p iptv-nav -p iptv-settings -p iptv-epg -p iptv-providers -p iptv-guide -p iptv-import -p iptv-net -p iptv-store-sqlite -p iptv-search --quiet
for c in iptv-core iptv-nav iptv-settings iptv-epg iptv-providers iptv-guide iptv-import iptv-search; do
  cargo build --locked -p "$c" --target wasm32-unknown-unknown --quiet
done
cargo build --locked -p iptv-app --quiet
# The app's own tests, among them headless runs of the whole app (tests/).
cargo test --locked -p iptv-app --features test-support --quiet
IPTV_REAL_CARGO="$(command -v cargo)" python3 tools/ci/test-locked-cargo.py
python3 tools/ci/test-mpvkit-notices.py
python3 tools/ci/test-macos-source-notices.py
python3 tools/ci/test-linux-native-notices.py
python3 tools/ci/test-crypto-native-notices.py
python3 tools/ci/check-package-lock.py
python3 tools/ci/test-ios-entitlements.py
python3 tools/ci/test-unpack-signing-input.py
echo "all checks passed"
