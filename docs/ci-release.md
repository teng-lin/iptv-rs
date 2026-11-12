# CI and releases

PRs and pushes to master run tools/check.sh on Linux and macOS, including
the headless app scenarios and portable crates' wasm builds. NSH is checked
out beside the repository at a pinned commit from tv-keys; update the SHA
in .github/actions/setup/action.yml when adopting fork changes.

Release runs repeat those checks and build Linux x86_64, macOS Apple silicon,
Android arm64 (one APK and AAB for tablets and TVs), and iPad device IPA.
Linux outputs are deb, rpm, pacman pkg.tar.zst, and AppImage. Native Linux
packages depend on system GTK4 and mpv; AppImage bundles linked libraries.
Linux compatibility starts at the Ubuntu 24.04 build environment, not every
older distribution.

Create a GitHub environment named release and set these secrets:
- ANDROID_KEYSTORE_BASE64, ANDROID_STORE_PASSWORD, ANDROID_KEY_PASSWORD,
  ANDROID_KEY_ALIAS: a persistent release signing key.
- IOS_CERTIFICATE_BASE64, IOS_CERTIFICATE_PASSWORD, IOS_SIGN_IDENTITY,
  IOS_PROFILE_BASE64: a certificate and matching distribution profile for
  com.hantekllc.iptvrs. Ad hoc profiles install only on registered devices.

The Android version code is the release workflow run number. Preserve its
monotonicity when migrating workflows or publishing to Play.
Set the workspace version in Cargo.toml and update Cargo.lock before tagging
v<version>. Tag/version mismatch fails. workflow_dispatch builds artifacts
without publishing. A tag publishes all artifacts and SHA256SUMS to GitHub
Releases only after every job succeeds. Publication stages a draft before
making it public; if interrupted, delete that draft before rerunning publish.

This does not submit to Play or App Store Connect. An IPA's distribution
profile determines where it can be installed; an App Store profile does not
make a GitHub download sideloadable. macOS uses the existing ad hoc signing
and is NOT notarized. Homebrew libraries determine its minimum OS version.
See README's pre-release theme, licensing, and notarization requirements
before tagging a public release. Bundled mpv/FFmpeg licensing obligations
still apply.

Validation still required on hosted runners: first complete packaging run,
installation of each Linux package and AppImage, macOS DMG launch on a clean
Mac, signed iPad installation, and Android tablet/TV smoke tests on tab36 and
tv36 with synthetic playlists. CI headless tests do not replace those device
checks.
