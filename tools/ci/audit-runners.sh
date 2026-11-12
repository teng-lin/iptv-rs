#!/usr/bin/env bash
# Audit exactly the four native Rust runners that these package scripts ship.
# The generated workspace also includes JS/web/Windows development hosts;
# auditing its virtual manifest would incorrectly enable those hosts' features.
set -euo pipefail
cd "$(dirname "$0")/../.."
python3 tools/ci/check-package-lock.py
for pair in linux:x86_64-unknown-linux-gnu macos:aarch64-apple-darwin android:aarch64-linux-android ios:aarch64-apple-ios; do
  platform=${pair%%:*}
  triple=${pair#*:}
  cargo deny --locked --exclude-dev --manifest-path "crates/iptv-app/.nsh/rust/rust-$platform/Cargo.toml" \
    --target "$triple" --config deny.toml check
 done
