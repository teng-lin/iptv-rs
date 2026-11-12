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
Tags and manual runs default to Android
arm64: one distribution-signed APK and AAB for tablets and TVs. Both signing
certificates are checked against the configured keystore before upload.
The manual workflow's platforms=all option additionally builds Linux x86_64,
macOS Apple silicon, and an iPad device IPA; it requires Apple signing
secrets and those packaging paths still need end-to-end validation.
Linux outputs are deb, rpm, pacman pkg.tar.zst, and AppImage. Native Linux
packages depend on system GTK4 and mpv; AppImage bundles linked libraries.
Linux compatibility starts at the Ubuntu 24.04 build environment, not every
older distribution.

Create a GitHub environment named release and set these secrets for Android:
- ANDROID_KEYSTORE_BASE64, ANDROID_STORE_PASSWORD, ANDROID_KEY_PASSWORD,
  ANDROID_KEY_ALIAS: a persistent release signing key.
For the optional all-platform run, also set:
- IOS_CERTIFICATE_BASE64, IOS_CERTIFICATE_PASSWORD, IOS_SIGN_IDENTITY,
  IOS_PROFILE_BASE64: a certificate and matching distribution profile for
  com.hantekllc.iptvrs. Ad hoc profiles install only on registered devices.

The Android version code is the release workflow run number. Preserve its
monotonicity when migrating workflows or publishing to Play.
Set the workspace version in Cargo.toml and update Cargo.lock before tagging
v<version>. Tag/version mismatch fails. workflow_dispatch builds artifacts
without publishing; use platforms=android for the first packaging validation.
A tag publishes the Android artifacts and SHA256SUMS to GitHub Releases only
after every job succeeds. Publication stages a draft before
making it public; if interrupted, delete that draft before rerunning publish.

This does not submit to Play or App Store Connect. An IPA's distribution
profile determines where it can be installed; an App Store profile does not
make a GitHub download sideloadable. macOS uses the existing ad hoc signing
and is NOT notarized. Homebrew libraries determine its minimum OS version.
See README's pre-release theme, licensing, and notarization requirements
before tagging a public release. Bundled mpv/FFmpeg licensing obligations
still apply.

Before enabling all-platform releases, validate complete packaging runs,
installation of each Linux package and AppImage, macOS DMG launch on a clean
Mac, and signed iPad installation. Android releases also need tablet/TV smoke
tests on tab36 and tv36 with synthetic playlists. CI build and headless tests
do not replace those device checks.
