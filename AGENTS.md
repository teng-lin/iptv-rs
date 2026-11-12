# Agent rules for iptv-rs

- **Synthetic data only.** Fixtures, tests, docs, screenshots and commit text
  use generated playlists/EPG and local mock servers. Never commit real
  provider URLs, names, channel lists or credentials. Real-provider testing
  reads credentials at runtime from outside the repo and leaves no trace in
  logs, screenshots or files.
- **Behaviour, not artwork.** The UX reference is `~/src/tivimate/docs`. Never
  copy files, code, artwork, icons, fonts, layouts or wording from the
  reference APK. `crates/iptv-app/src/theme.rs` holds the original Patina palette;
  preserve its independent design.
- **Core stays portable.** `crates/iptv-core` (and future core crates) must not
  depend on NSH or platform APIs, must not block, and must build for
  `wasm32-unknown-unknown`.
- **State lives in the core.** Views render state and send actions; focus is
  restored by content identity, never by list index.
- **NSH fork.** NSH comes from `~/src/nsh`, branch `tv-keys`. Keep
  patches small with tests; they go upstream later.
- **Video layer.** One persistent player surface under the UI; modes change
  layout only. Never hide, detach or re-parent the player view (see
  `docs/research/android-video-and-input.md`). Multiview adds one
  `tv.player` per further tile while it is open (`video.rs`). The
  desktops' picture-in-picture window shows surface 0's player through
  its `tv.pip` view without a second player or stream (`mpv/pip.rs`).
- **Check before finishing:** `cargo test -p iptv-core`, the wasm build of
  `iptv-core`, `cargo build -p iptv-app`, and for Android changes a run on
  `tab36` and/or `tv36`.
