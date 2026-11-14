#!/usr/bin/env bash
# Audit exactly the Rust graphs that the package scripts ship: the four native
# runners NSH generates, and the Windows executable.
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
# Windows has no generated runner: tools/build-windows.ps1 ships the app crate's
# own executable, built from the source Cargo.lock. Audit that graph for its target.
cargo deny --locked --exclude-dev --manifest-path crates/iptv-app/Cargo.toml \
  --target x86_64-pc-windows-msvc --config deny.toml check
