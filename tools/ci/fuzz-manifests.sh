#!/usr/bin/env bash
# The playback relay's HLS and DASH manifest rewriters (fuzz target
# `manifests`), run like tools/ci/fuzz-imports.sh: cargo-fuzz 0.13.2 calls
# `cargo build` through PATH and has no --locked flag, so the release wrapper
# enforces fuzz/Cargo.lock for both harness metadata and builds.
set -euo pipefail
cd "$(dirname "$0")/../.."
export RUSTUP_TOOLCHAIN="${IPTV_FUZZ_TOOLCHAIN:-nightly-2026-10-24}"
export IPTV_REAL_CARGO
IPTV_REAL_CARGO=$(rustup which --toolchain "$RUSTUP_TOOLCHAIN" cargo)
fuzz_bin=$(mktemp -d)
trap 'rm -rf "$fuzz_bin"' EXIT
ln -s "$PWD/tools/ci/locked-cargo.py" "$fuzz_bin/cargo"
export PATH="$fuzz_bin:$PATH"
mkdir -p fuzz/corpus/manifests
cp fuzz/seeds/manifests/* fuzz/corpus/manifests/
# The relay buffers at most 2 MiB of manifest; 64 KiB reaches every branch.
cargo-fuzz run manifests -- -max_total_time="${1:-300}" -max_len=65537 -rss_limit_mb=512
