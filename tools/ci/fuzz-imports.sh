#!/usr/bin/env bash
# cargo-fuzz 0.13.2 calls `cargo build` through PATH and has no --locked flag.
# Reuse the release wrapper so both harness metadata and builds enforce its lock.
set -euo pipefail
cd "$(dirname "$0")/../.."
export RUSTUP_TOOLCHAIN="${IPTV_FUZZ_TOOLCHAIN:-nightly-2026-10-24}"
export IPTV_REAL_CARGO
IPTV_REAL_CARGO=$(rustup which --toolchain "$RUSTUP_TOOLCHAIN" cargo)
fuzz_bin=$(mktemp -d)
trap 'rm -rf "$fuzz_bin"' EXIT
ln -s "$PWD/tools/ci/locked-cargo.py" "$fuzz_bin/cargo"
export PATH="$fuzz_bin:$PATH"
mkdir -p fuzz/corpus/imports
cp fuzz/seeds/imports/* fuzz/corpus/imports/
cargo-fuzz run imports -- -max_total_time="${1:-300}" -max_len=65537 -rss_limit_mb=512
