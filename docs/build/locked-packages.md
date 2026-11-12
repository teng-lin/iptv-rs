# Locked package builds

Rust is pinned in `rust-toolchain.toml`. Source builds use `Cargo.lock`.
NSH generates a workspace containing additional platform runners, so its
resolved superset lives in `tools/ci/package.Cargo.lock`. Package scripts
source `locked-build-env.sh`: NSH honors `CARGO`, and that entry point copies
the committed runner lock and supplies `--locked`. Stale dependencies fail;
package jobs never update either lock.

To update after changing dependencies or the NSH revision:

1. Update the source lock intentionally with Cargo.
2. Run NSH once locally to generate `crates/iptv-app/.nsh/rust/Cargo.toml`.
   A stale-lock failure after generation is expected.
3. Copy the source `Cargo.lock` into that generated directory. Run plain
   `cargo metadata --manifest-path crates/iptv-app/.nsh/rust/Cargo.toml
   --format-version 1` to resolve its platform runners.
4. Review the dependency delta, then copy the resulting lock to
   `tools/ci/package.Cargo.lock` and commit both locks together.
5. Run package CI for all platforms. Run the stale-lock regression with
   `IPTV_REAL_CARGO="$(command -v cargo)" python3 tools/ci/test-locked-cargo.py`.

`cargo fmt` has no `--locked` option and does not resolve dependencies;
all build and test calls in `tools/check.sh` require the lock.

`check-package-lock.py` requires shared source-lock dependency identities and
checksums to remain present in the runner lock, so lock-only updates cannot
silently leave packaged versions behind. `audit-runners.sh` audits each shipped
Rust runner with its actual target and features. The generated virtual workspace
also contains JavaScript, web and Windows development hosts that are not shipped
by these package scripts; auditing that virtual root would enable their unrelated
features. Generated notices and inventories conservatively include that superset.

The build environment also exports the repository toolchain to commands that
change directory into NSH. Its patched xtask adds `--locked` to nested runtime
builds and metadata calls: Cargo resets `CARGO` inside `cargo run`, so relying
only on a wrapper environment variable would not protect those nested builds.
