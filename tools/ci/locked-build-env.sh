# Source before invoking NSH: it honors CARGO for generated builds. Cargo itself
# resets CARGO inside `cargo run`, so NSH's xtask also adds --locked explicitly.
# Pin nested commands that change directory into the sibling NSH workspace.
RUSTUP_TOOLCHAIN=$(python3 -c 'import sys,tomllib; print(tomllib.load(open(sys.argv[1], "rb"))["toolchain"]["channel"])' "$(dirname "${BASH_SOURCE[0]}")/../../rust-toolchain.toml")
export RUSTUP_TOOLCHAIN
IPTV_REAL_CARGO=$(rustup which --toolchain "$RUSTUP_TOOLCHAIN" cargo)
export IPTV_REAL_CARGO
export CARGO="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/locked-cargo.py"
