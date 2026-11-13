# Releases and CI

PRs and pushes to master run tools/check.sh on Linux and macOS, including
the headless app scenarios and portable crates' wasm builds. The separate
Android packaging job runs on PRs only when Android host files, Cargo
manifests/lockfile, app build or package metadata/assets, packaging tooling, or
its workflow/setup actions change (see `.github/workflows/android.yml`). It also
runs on every master push and can be started manually. A lightweight change
check runs on every PR so the required `android` check can report skipped for
ordinary Rust source and documentation changes. Change-detection failures fail
the required check. Desktop packaging is path-filtered at the workflow level
and must not be required, since skipped workflows can remain pending.

Desktop Package checks likewise run only for package/build inputs on PRs and
support manual dispatch. Signed release packaging still runs on tags or manual
requests.

Android packages builds the arm64 native library and the Gradle host's debug APK/AAB,
verifies the APK signature, and uploads both as the android-debug artifact.
It uses a generated debug key and does not receive release secrets.
NSH is checked
out beside the repository at a pinned commit from tv-keys; update the SHA
in .github/actions/setup/action.yml when adopting fork changes.

Release runs repeat the host checks while packaging runs in parallel; the
signed Android build takes the place of the PR's debug build. Publication
waits for both the checks and packaging to pass.
Tags and manual runs default to platforms=android-desktop: Android arm64,
Linux x86_64 and macOS Apple silicon. Android produces one distribution-signed
APK and AAB for tablets and TVs. Both signing certificates are checked against
the configured keystore before upload. Manual runs also offer platforms=android
or platforms=desktop independently. platforms=all adds an iPad device IPA;
only that option requires Apple signing secrets and still needs iPad validation.

Linux outputs are .deb, .rpm, .pkg.tar.zst (a pacman package), and .AppImage.
Native packages depend on system GTK4, mpv and GStreamer. The AppImage bundles
linked libraries and GTK resources, and uses X11 (XWayland on Wayland desktops).
It uses the host's OpenGL/EGL/GLES drivers and audio servers.
Linux compatibility starts at glibc 2.39 and GTK 4.14 from Ubuntu 24.04, not
every older distribution. Packaging checks inspect all three native package
formats, install and launch the DEB, and launch the AppImage under Xvfb with
empty temporary app state. A second AppImage launch uses a minimal Ubuntu
container without system GTK, mpv or GStreamer. RPM and pacman metadata checks do not replace
installation tests on their respective distributions.

macOS produces a self-contained .dmg with Patina.app and an Applications
shortcut. Packaging verifies the disk image, mounts it read-only, verifies the
app's signature and arm64 architecture, and launches it with empty temporary
app state. Bundled dependencies are checked for unresolved external references.

Create a GitHub environment named release and set these secrets for Android:
- ANDROID_KEYSTORE_BASE64, ANDROID_STORE_PASSWORD, ANDROID_KEY_PASSWORD,
  ANDROID_KEY_ALIAS: a persistent release signing key.
For the optional all-platform run, also set:
- IOS_CERTIFICATE_BASE64, IOS_CERTIFICATE_PASSWORD, IOS_SIGN_IDENTITY,
  IOS_PROFILE_BASE64: a certificate and matching distribution profile for
  com.hantekllc.iptvrs. Ad hoc profiles install only on registered devices.

The Android version code is the release workflow run number. Preserve its
monotonicity when migrating workflows or publishing to Play.
Run `python3 tools/bump-version.py patch` (or `minor`, `major`, or an explicit
version such as `0.2.1`) from the repository root using Python 3.11 or newer.
Add `--dry-run` to preview. The script also accepts an absolute path when
invoked from another directory.
The command updates `Cargo.toml`, every inherited workspace package version in
`Cargo.lock` and `tools/ci/package.Cargo.lock`, and the NSH packaging version in `crates/iptv-app/nsh.json` without
updating dependencies. It accepts increasing MAJOR.MINOR.PATCH versions.
Review and commit those changes, then tag that commit `v<version>` when ready.
The command does not commit, tag, or publish. Tag/version mismatch fails.
workflow_dispatch builds artifacts
without publishing; use platforms=desktop to validate desktop packages alone.
A tag publishes Android and desktop artifacts and SHA256SUMS to GitHub Releases only
after every job succeeds. Publication stages a draft before
making it public; if interrupted, delete that draft before rerunning publish.

This does not submit to Play or App Store Connect. An IPA's distribution
profile determines where it can be installed; an App Store profile does not
make a GitHub download sideloadable. macOS uses the existing ad hoc signing
and is NOT notarized. Homebrew libraries determine its minimum OS version.
Before tagging a public release, notarize the macOS app (Developer ID,
hardened runtime, `tools/ci/notarize-macos.sh`). Bundled mpv/FFmpeg licensing obligations
still apply.

Before a stable release, also validate installation of the RPM and pacman
packages on their native distributions, the AppImage and DMG on clean desktops,
and signed iPad installation when enabling iOS. Android releases need tablet/TV smoke
tests on tab36 and tv36 with synthetic playlists. CI build and headless tests
do not replace those device checks.

The About row displays the compiled workspace version and the first eight hex
characters of the source commit, for example `v0.2.1(a3d4123f)`. Cargo tracks
Git HEAD and branch refs, including linked worktrees and detached CI builds.
Uncommitted edits retain the HEAD hash; builds from source archives without Git
show `v0.2.1(unknown)`. Package metadata keeps the plain numeric version required
by platform installers.

## Pinned NSH tool artifacts

Nonsigning CI caches the finished NSH CLI and Android runtime separately from
Cargo dependency caches. Exact identities include the NSH commit, Rust compiler,
host OS/architecture, build recipe, compiler flags and inherited Cargo configuration. The Android recipe pins
its ABI, NDK and SDK versions; its identity also includes the JDK and installed
build-tools inventory/revisions, since NSH selects the newest installed tools.
IPTV source and lockfile changes do not invalidate these tool artifacts.

The shared CLI action uses the same bootstrap profile for desktop and Android
(no LTO, optimization level 1). Caches restore after the general Rust cache and
save immediately after successful tool compilation, before app/package checks.
There are no fallback keys: a changed tool input builds a new artifact. Release
packaging still disables caches, and signing jobs remain isolated. Cache sharing
is subject to GitHub branch scope; PRs can read a default-branch cache but cannot
warm caches for unrelated PRs. The default-branch Android workflow warms the
Linux CLI and Android runtime caches used by subsequent PRs. macOS CLI caches
are reused within a PR; dispatch Package checks on master to seed a macOS cache
that other PRs can restore, since desktop packaging has no master push trigger.

Only master writes the bulk Rust dependency caches. PRs restore those caches
and may save the much smaller finished-tool artifacts. This avoids duplicating
gigabytes of Rust dependencies per PR and evicting the shared tool caches.
Dispatching Package checks on master also seeds desktop packaging dependencies.

After changing this pipeline, compare a cold run with a second run at the same
NSH pin: the latter must skip both NSH build steps while still building the app,
generating notices, linting, packaging and verifying signatures. The expected
saving is the skipped compilation time minus the artifact restore overhead;
measure this in CI before claiming an improvement. Upstream Java-only runtime
preparation is a separate follow-up; this pipeline retains the complete runtime.

## Locked package builds

Windows installer distribution prerequisites are listed in the Windows section below.

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

## Windows installer (experimental)

Windows PR checks build and test the app. Installer generation runs only in the
release workflow for tags and manual desktop selections, using
`tools/package-windows.ps1`. It produces one x64 `.exe` installer, with no portable
ZIP. The per-user install bundles libmpv and the VC runtime, adds a Start menu
shortcut, and preserves application data on uninstall. Signing is not configured.
The package smoke check uses synthetic data and verifies the executable and window
icons, launch, clean exit, and uninstall.

The native runtime comes from the separate, manually dispatched
`.github/workflows/mpv-runtime.yml` dependency workflow. It prepares checksum-pinned
source and toolchain archives, compiles in a pinned Linux Docker environment with
network access disabled, and tests the packaged DLL on Windows with generated
MPEG-TS and local HLS media. Publication depends on that smoke test. Normal app PRs
and releases download the selected runtime; they do not rebuild the native graph.

The dependency release tag includes a digest of the source lock and build scripts.
It contains the runtime ZIP, exact corresponding-source archive, `runtime.json`
and checksums. This ZIP is a build dependency, not a portable Patina distribution.
After publishing a new dependency release, review and commit its `runtime.json`
as `tools/mpv-runtime/runtime.json`. See the [native build instructions](../tools/mpv-runtime/README.md)
for local commands and the update procedure.

Installer generation verifies the bundled DLL against that committed pin and the
native component inventory. It copies all native licenses, the source lock,
inventory, build configuration and `SOURCE.txt` into `notices/libmpv`.
`SOURCE.txt` identifies the exact retained source asset for the installed DLL.

Never prune, replace or delete these dependency releases or their source assets.
Every distributed installer must retain access to its matching sources, including
after a newer runtime is selected. Keep notices and the same source availability
when redistributing installers. If a dependency release is incomplete or invalid,
correct the inputs and publish a new digest; do not overwrite published bytes.
