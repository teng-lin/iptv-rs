# Changelog

All notable changes to Patina IPTV are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). While the version
is below 1.0, a minor release may change behaviour.

## [Unreleased]

### Added

- Releases publish the source of the media libraries bundled in the macOS disk
  image and the Linux AppImage (`Patina-<version>-<platform>-native-sources.tar`
  with a manifest), and the packages say where to find it.
- Releases carry build provenance attestations for every file and for
  `SHA256SUMS`, and a CycloneDX SBOM of the Rust dependencies.
- `docs/architecture.md`, `CONTRIBUTING.md`, `SECURITY.md` and this changelog.

### Changed

- The NSH fork commit is recorded in `nsh.rev`. Clone that exact commit;
  `tools/check.sh` refuses another one unless `IPTV_ALLOW_NSH_DRIFT=1` is set.
- The minimum supported Rust version is the pinned toolchain, 1.99.1.
- Package scripts work without `rustup`, using the Cargo on `PATH`.

### Fixed

- A release that is already public can no longer have its files replaced by
  rerunning the release workflow.
- Pull requests that change only Rust sources are compiled for Android.
- The Windows check runs the tests of every library crate.

## [0.1.1] - 2026-11-13

### Added

- Interface language selection: 11 language options, detection of the system
  language, and a picker that switches the interface without a restart.
- Windows x64 support (experimental), with a per-user installer that bundles
  libmpv built from pinned sources.
- Group browsing and scoped search in the multiview channel picker, which
  keeps its scroll position by the selected channel.
- On-screen Back controls on desktops and tablets.

### Fixed

- A failure to write local storage during an import is reported as a storage
  error.

## [0.1.0] - 2026-11-12

First public release, for Android TV and tablets, Linux and macOS.

### Added

- Sources: M3U playlists by address or file, Xtream Codes accounts and Stalker
  portals; several playlists browsable together.
- Guide: XMLTV guides, plain or gzip, with automatic channel matching, a
  per-channel guide choice and catch-up playback where the provider offers it.
- Watching: the guide over live video, a channel list beside the video,
  multiview, picture-in-picture on the desktops, and track and picture-size
  choice. Live streams that the provider cuts reconnect.
- Library: favourites, custom groups, reordering, hiding, history, and channel
  and programme search.
- Privacy: the local catalog, including credentials, is encrypted with
  SQLCipher; whole-app backup and restore to a `.patinabackup` file.
- Packages: a signed Android APK, Linux `.deb`, `.rpm`, pacman package and
  AppImage, and an ad hoc signed macOS disk image.

[Unreleased]: https://github.com/teng-lin/patina-iptv/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/teng-lin/patina-iptv/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/teng-lin/patina-iptv/releases/tag/v0.1.0
