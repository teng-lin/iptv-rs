# Releases and CI

PRs and pushes to master run tools/check.sh on Linux and macOS, including
the headless app scenarios and portable crates' wasm builds.

The Android workflow (`.github/workflows/android.yml`) has two jobs behind one
lightweight change check, `tools/ci/android-changes.py`:

- `android` builds the arm64 native library and the Gradle host. On PRs it runs
  only when Android host files, Cargo manifests/lockfile, app build or package
  metadata/assets, packaging tooling, the NSH or Rust pin, or its workflow/setup
  actions change.
- `android-check` runs `tools/ci/check-android.sh`, a `cargo check` of the app
  library for `aarch64-linux-android`. On PRs it runs when Rust sources or
  manifests change and `android` does not already build them. The host checks
  never compile `cfg(target_os = "android")` code, so this is the check that
  stops a Rust-only change from breaking the Android build.

Both jobs run on every master push (which also keeps their build caches warm
for PRs) and can be started manually. The change check runs on every PR, so
both jobs always report: skipped when not needed, failed when change detection
itself fails. Either can therefore be a required check. Desktop packaging is
path-filtered at the workflow level and must not be required, since skipped
workflows can remain pending.

## Coverage and fuzzing

Neither of these gates a merge. Both workflows are path-filtered on PRs, so do
not make them required checks.

- **Coverage** (`.github/workflows/coverage.yml`) runs `tools/ci/coverage.sh` on
  Linux for PRs that touch `crates/**` and on every master push. It measures the
  library crates' tests with `cargo llvm-cov` 0.9.1 (everything except
  `iptv-app`, whose scenario tests run the whole app), prints the per-file table
  in the job summary, and uploads `lcov.info` and an HTML report as the
  `coverage` artifact. The job is allowed to fail. Run it locally with
  `rustup component add llvm-tools` and `cargo install cargo-llvm-cov --version
  0.9.1 --locked`.
- **Fuzzing** (`.github/workflows/fuzz.yml`) runs `tools/ci/fuzz-imports.sh` once
  per fuzz target, in parallel. The targets are the `[[bin]]` entries of
  `fuzz/Cargo.toml` (what `cargo fuzz list` prints), so a new target is picked up
  without touching the workflow. PRs that change the playlist, guide or provider
  parsers, the import pipeline, the playback manifest rewriters or the harness
  get 60 seconds per target. The nightly Import stress workflow calls the same
  workflow with 300 seconds per target and saves each target's corpus to the
  Actions cache; every later run, including PR passes, starts from the newest
  saved corpus plus the committed seeds. Crashing inputs are uploaded as the
  `fuzz-<target>` artifact. The harness has its own `fuzz/Cargo.lock`, built
  with `--locked`; `tools/bump-version.py` keeps its workspace crate versions in
  step.

## Packaging and releases

Desktop Package checks likewise run only for package/build inputs on PRs and
support manual dispatch. Signed release packaging still runs on tags or manual
requests.

Android packages builds the arm64 native library and the Gradle host's debug APK/AAB,
verifies the APK signature, and uploads both as the android-debug artifact.
It uses a generated debug key and does not receive release secrets.
NSH is checked
out beside the repository at the commit in `nsh.rev` (from the fork's tv-keys
branch). That file is the only place the commit is named: both setup actions
read it through `tools/ci/pins.py`, and `tools/check.sh` refuses a `../nsh`
checkout at any other commit. Update `nsh.rev` when adopting fork changes.

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
every older distribution. `tools/ci/check-linux-packages.sh` inspects all three
native package formats and then installs and launches every package, each with
empty temporary app state under Xvfb:

- the DEB on the Ubuntu 24.04 runner itself;
- the AppImage on the runner, and again in a minimal Ubuntu container that has
  no system GTK, mpv or GStreamer (`Dockerfile.appimage-smoke`);
- the RPM in a Fedora 43 container (`Dockerfile.rpm-smoke`) and the pacman
  package in an Arch Linux container (`Dockerfile.pacman-smoke`). Each installs
  the package with the distribution's package manager, which resolves its
  dependencies, checks that the notices were installed, and launches the app.

These containers have no GPU and no audio device, so they prove installation
and start-up, not playback on real desktops.

macOS produces a self-contained .dmg with Patina.app and an Applications
shortcut. Packaging verifies the disk image, mounts it read-only, verifies the
app's signature and arm64 architecture, and launches it with empty temporary
app state. Bundled dependencies are checked for unresolved external references.

Developer ID signing and notarization are wired but conditional.
`tools/ci/package-macos.sh` checks for `MACOS_CERTIFICATE`: when it is set, the
sign job imports the certificate into a temporary keychain, signs every bundled
binary with the hardened runtime, notarizes and staples the app
(`tools/ci/notarize-macos.sh`), then signs, notarizes and staples the disk
image. When it is not set, the script says so and produces an ad hoc signed
image. As of v0.1.1 no Apple secrets are configured, so every DMG released so far is
ad hoc signed and not notarized, and Gatekeeper warns about it (the README
explains how to open it).

Create a GitHub environment named release and set these secrets for Android:
- ANDROID_KEYSTORE_BASE64, ANDROID_STORE_PASSWORD, ANDROID_KEY_PASSWORD,
  ANDROID_KEY_ALIAS: a persistent release signing key.
To sign and notarize the macOS disk image, also set:
- MACOS_CERTIFICATE_BASE64, MACOS_CERTIFICATE_PASSWORD, MACOS_SIGN_IDENTITY: a
  Developer ID Application certificate and its identity name.
- APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD: the account and app-specific
  password `notarytool` submits with.
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
`Cargo.lock`, `tools/ci/package.Cargo.lock` and the fuzz harness's `fuzz/Cargo.lock`,
and the NSH packaging version in `crates/iptv-app/nsh.json` without
updating dependencies. It accepts increasing MAJOR.MINOR.PATCH versions.
Review and commit those changes, then tag that commit `v<version>` when ready.
The command does not commit, tag, or publish. Tag/version mismatch fails.
workflow_dispatch builds artifacts
without publishing; use platforms=desktop to validate desktop packages alone.
A manual run on master is also the dry run for the release-only steps: it
downloads the native source archives and writes the SBOM, and stops before the
checksum, attestation and publish steps.
A tag publishes Android and desktop artifacts and SHA256SUMS to GitHub Releases only
after every job succeeds.

## Published releases are immutable

`tools/ci/publish-release.py` does the publishing, and it only ever writes to a
release that does not exist yet or is still a draft:

1. It reads the release's state. If the release for the tag is already public,
   the job fails before changing anything.
2. It writes `SHA256SUMS` for the files to release.
3. It creates a draft (or reuses the draft an interrupted run left), uploads
   every file to it, checks that it is still a draft, and makes it public once.

So a failed or cancelled publish job can simply be rerun: the draft is completed
and published. Nothing can be re-uploaded to a public release, because users may
have downloaded its files and the checksums name those bytes. To fix a
published release, bump the version and tag again; do not delete and recreate
the release or move the tag. An error while reading the release state also
fails the job rather than being read as "no release yet".

## Checksums, provenance and SBOM

Every release carries, next to the packages:

- `SHA256SUMS`: the SHA-256 of every other file in the release, in the format
  `sha256sum --check` reads.
- `Patina-<version>-sbom.cdx.json`: a CycloneDX 1.5 SBOM written by
  `tools/ci/sbom.py` (`cargo cyclonedx` 0.5.10). It lists the app crate's
  resolved `Cargo.lock` graph for all targets (normal and build dependencies,
  with registry checksums) and records the NSH commit. It does not list the
  platform host crates of the generated NSH runners (those are in
  `runner-components.json` inside each package) or the native media libraries
  (see "Corresponding source for bundled media libraries").
- Build provenance attestations, stored by GitHub rather than as release files.
  The publish job attests every released file and `SHA256SUMS` itself with
  `actions/attest-build-provenance`. The signature is keyless: Sigstore issues a
  short-lived certificate for that workflow run, so there is no signing key to
  keep or to leak. `SHA256SUMS` is not signed separately; its attestation is the
  signature.

To check a download, with the [GitHub CLI](https://cli.github.com/) installed:

```sh
# One file: built by this repository's release workflow from the release tag.
gh attestation verify Patina-<version>-linux-x86_64.AppImage \
  --repo teng-lin/patina-iptv \
  --signer-workflow teng-lin/patina-iptv/.github/workflows/release.yml \
  --source-ref refs/tags/v<version>

# Or verify the checksum file once, then check every file you downloaded.
gh attestation verify SHA256SUMS --repo teng-lin/patina-iptv \
  --signer-workflow teng-lin/patina-iptv/.github/workflows/release.yml
sha256sum --check --ignore-missing SHA256SUMS
```

Releases up to v0.1.1 predate this and have no attestations; their
`SHA256SUMS` is unsigned. An attestation proves which workflow run built a
file from which commit. It does not replace platform code signing: the macOS
and Windows packages are still not trusted by Gatekeeper or SmartScreen (see
below).

## Corresponding source for bundled media libraries

Three packages bundle libmpv, FFmpeg and the libraries they use, which are
LGPL or GPL software. Each release publishes the source those exact binaries
were built from:

| Package | Bundled binaries come from | Source published as |
| --- | --- | --- |
| Windows installer | Patina's own pinned build (`tools/mpv-runtime`) | the retained `mpv-runtime-sources.tar.gz` of the dependency release; see [Windows installer](#windows-installer-experimental) |
| macOS disk image | Homebrew bottles on the build runner | `Patina-<version>-macos-native-sources.tar` in the same release |
| Linux AppImage | Ubuntu 24.04 packages on the build runner | `Patina-<version>-linux-native-sources.tar` in the same release |

The `.deb`, `.rpm` and pacman packages bundle no media libraries; they depend
on the distribution's.

How the macOS and Linux archives are produced:

1. **Packaging records what was bundled.** `tools/bundle-macos.py` writes, into
   `Contents/Resources/notices/native/`, the inventory (`components.json`), each
   Homebrew keg's installed formula, and `sources.json`: for every formula the
   upstream archive, patches and resources with their SHA-256 checksums, or the
   Git commit. It reads the formula Homebrew keeps inside the installed keg, so
   the list describes the installed version even when the tap has moved on.
   `tools/ci/linux-native-notices.py` writes the same files into
   `usr/bin/notices/native/` of the AppImage, naming the Ubuntu source package
   and exact version behind every bundled library (`dpkg-query`). Both also
   write `SOURCE.txt`, which tells a user where the source archive is.
   On macOS, packaging fails when a bundled library has no checksummed
   source; on Linux the check happens in step 2, when the sources are
   downloaded.
2. **The release job downloads exactly that list** with
   `tools/ci/native-sources.py`, reading the manifest back out of the finished
   package. On macOS it downloads each address (mirrors are tried too) and
   compares the SHA-256; Git sources are archived at the pinned commit. On
   Linux it runs `apt-get source --download-only <source>=<version>` after
   `tools/ci/enable-deb-src.sh` and checks every file against the checksums of
   the signed archive index. A version that has left the index (a newer update
   was published after the runner image was built) is fetched from Launchpad,
   which keeps every published version, and checked against its `.dsc`.
3. **The archive and a `.json` manifest are released** next to the packages and
   listed in `SHA256SUMS`. The manifest names every file with its address and
   SHA-256; the archive contains the same manifest, `SOURCE.txt` and, on macOS,
   the formulae.

A source that cannot be fetched, or whose checksum differs, fails the packaging
job, so no release is published without it. The Package checks workflow runs
the same collection on pull requests that change packaging and uploads only the
manifest.

Limits of this interim step, until the libraries are built from pinned sources
as on Windows:

- It covers the libraries that are copied into the package. Code that Homebrew
  or Ubuntu linked statically into those libraries from build-only dependencies
  is not traced.
- On macOS, formula sources that are computed while installing cannot be read
  from the formula text; packaging fails rather than guess. Sample data that a
  formula downloads only for its own test is left out.
- Never delete these release assets. Every distributed DMG and AppImage points
  at the archive of its own release.
- Whether these builds enable GPL-only FFmpeg components was not determined.
  Publishing complete source satisfies the source requirement of both licences,
  but a legal review of the distribution terms is still worthwhile.

This does not submit to Play or App Store Connect. An IPA's distribution
profile determines where it can be installed; an App Store profile does not
make a GitHub download sideloadable. Without the Apple secrets above the macOS
image is ad hoc signed and NOT notarized. Homebrew libraries determine its
minimum OS version. The Windows installer is unsigned. Before calling a release
stable, configure Developer ID notarization and Windows code signing.

Before a stable release, also validate the RPM, pacman package, AppImage and
DMG on real desktops with a GPU and sound (CI covers installation and start-up
only), and signed iPad installation when enabling iOS. Android releases need tablet/TV smoke
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
Rust runner with its actual target and features, and the app crate's own graph
for `x86_64-pc-windows-msvc`: Windows has no generated runner and ships that
executable, built from the source `Cargo.lock`. The generated virtual workspace
also contains JavaScript, web and Windows development hosts that are not shipped
by these package scripts; auditing that virtual root would enable their unrelated
features. Generated notices and inventories conservatively include that superset.

The build environment also exports the repository toolchain to commands that
change directory into NSH. Its patched xtask adds `--locked` to nested runtime
builds and metadata calls: Cargo resets `CARGO` inside `cargo run`, so relying
only on a wrapper environment variable would not protect those nested builds.

## Windows installer (experimental)

Windows PR checks run `tools/build-windows.ps1 -Check`: the same lints, library
crate tests, app tests and portable wasm builds as `tools/check.sh`
(`tools/ci/test-check-parity.py` fails when the crate lists of the two scripts
differ). The Windows setup action uses the same Rust build cache as the other
hosts; only master saves it. Formatting and the workspace-wide `cargo deny` run
once, on Linux and macOS: neither depends on the host, and `cargo deny` without
a target already covers the Windows-only dependencies. `audit-runners.sh`
additionally audits the graph the Windows executable ships, for its own target.

Installer generation runs only in the
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
