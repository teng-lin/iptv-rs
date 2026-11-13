# CI and releases

PRs and pushes to master run tools/check.sh on Linux and macOS, including
the headless app scenarios and portable crates' wasm builds. An Android job
also builds the arm64 native library and the Gradle host's debug APK/AAB,
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
`Cargo.lock`, and the NSH packaging version in `crates/iptv-app/nsh.json` without
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
See README's pre-release theme, licensing, and notarization requirements
before tagging a public release. Bundled mpv/FFmpeg licensing obligations
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
