# iptv-rs

Codename for an IPTV player for Android TV and Android tablets, written in Rust
with [Native Shell (NSH)](https://github.com/longbridge/nsh). The UX targets
parity with the reference TV player described in `~/src/tivimate/docs` (remote
first, guide over live video, touch as a first-class second input). The
product name is not decided; `iptv-rs` is the working name.

Plan and decisions: [`.plans/2026-11-09-tivimate-ux-replication.md`](.plans/2026-11-09-tivimate-ux-replication.md).
Android video/input research: [`docs/research/android-video-and-input.md`](docs/research/android-video-and-input.md).

## Layout

| Path | What |
| --- | --- |
| `crates/iptv-core` | UI-independent core: guide model and focus, channel lists and the rail/groups/guide navigation, favourites, player panels, context menus. No NSH, wasm-safe |
| `crates/iptv-nav` | Remote keys, short/long press, layered key → action map. Wasm-safe |
| `crates/iptv-settings` | One settings schema: catalogue, scoped store, versioned backup JSON. Wasm-safe |
| `crates/iptv-epg` | Sans-IO XMLTV parser (gzip), EPG channel matching, compact guide store. Wasm-safe |
| `crates/iptv-providers` | Sans-IO M3U parser, Xtream and Stalker clients, stable ids, catch-up URLs. Wasm-safe |
| `crates/iptv-guide` | The real guide schedule: imported channels + EPG store behind the `Schedule` trait, with gap filling; stream resolution (URL + headers, live and catch-up) per provider. Wasm-safe |
| `crates/iptv-import` | Runtime-agnostic import pipeline: M3U/Xtream/Stalker sources and XMLTV guides into a `Catalog` and `GuideStore`, typed failures, progress, cancel; EPG refresh policy. Wasm-safe |
| `crates/iptv-net` | Native HTTP adapter (reqwest + rustls) implementing `Fetch`; strips identity headers on cross-host redirects |
| `crates/iptv-store-sqlite` | SQLite store: provider catalog and guide data replaced per refresh (with each feed's result and channel list), guide sources per playlist, user overlays (favourites, order, hidden, history, a channel's guide choice) kept by stable id, FTS5 search, tested migrations |
| `crates/iptv-search` | Channel and programme search (folded, ranked, favourites first), history, search-screen state. Wasm-safe |
| `crates/iptv-app` | NSH shell: screens, placeholder theme, video layer; on Linux the `tv.player` component on libmpv (`src/mpv/`) |
| `hosts/android` | Kotlin host: `NshView`, Media3 `tv.player` component, TV launcher manifest |
| `tools/` | `sync-host.sh`, `make-hls-fixtures.sh`, `make-track-fixtures.sh`, `make-iptv-fixtures.py`, `mock-server.py`, `mock-xtream.py`, `burst.sh` |

## Prerequisites

- Rust stable with `x86_64-linux-android`, `aarch64-linux-android`, `wasm32-unknown-unknown`.
- JDK 21, Android SDK (platform 35, build-tools 35.0.1, platform-tools, emulator), NDK 30.
- NSH CLI built from the fork: `~/src/nsh` (branch `tv-keys`), `cargo build --release -p nsh-cli`.
- `ffmpeg` and `python3` for synthetic test streams.
- Linux desktop: GTK 4 and libmpv 2 (client API 2.x, `pkg-config mpv`;
  Arch `mpv`, Debian/Ubuntu `libmpv-dev`). The player links `libmpv.so.2`.

## Run

```sh
cargo test -p iptv-core                                   # focus/guide model
cargo build -p iptv-core --target wasm32-unknown-unknown  # portability check
cargo run -p iptv-app                                     # Linux desktop (see below)

tools/make-hls-fixtures.sh --serve &                      # synthetic HLS on :8089
tools/make-track-fixtures.sh --serve &                    # several video/audio/subtitle tracks on :8097
                                                          # (--host 10.0.2.2:8097 for the emulators)
tools/sync-host.sh                                        # build libnsh.so (x86_64, arm64) into the host
(cd hosts/android && ./gradlew assembleDebug)
adb -s emulator-5554 install -r hosts/android/app/build/outputs/apk/debug/app-debug.apk
```

Emulators: `tab36` (Pixel Tablet, API 36) and `tv36` (Android TV, API 36).
Start them with `QT_QPA_PLATFORM=xcb` so the host keyboard reaches them.

### Linux desktop

`cargo run -p iptv-app` opens a 960×540 window (Wayland or X11). Video plays
through libmpv rendering into a GTK GL area under the UI. Without a stored
playlist the wizard opens; its playlist link defaults to the mock server at
`http://127.0.0.1:8089/iptv/playlist.m3u` (generate desktop fixtures with
`python3 -I tools/make-iptv-fixtures.py --host 127.0.0.1:8089`). Data lives in
`~/.local/share/iptv-rs` (settings in `settings.json` there). Settings open
from the guide's menu (Menu, `m`).

| Remote | Keyboard |
| --- | --- |
| D-pad, OK | arrows, Enter |
| Back | Escape, Backspace (Backspace edits text in a field) |
| Menu | `m`, the context-menu key |
| Channel up / down | Page Up / Page Down or `=` / `-` (over video they zap, in the guide they page) |
| Digits | `0`–`9` |
| Play/pause | Space |
| Info, Guide | `i`, `g` |
| Previous channel, back to live | `p`, `n` |
| Rewind / fast-forward (catch-up) | `[`, `]` |
| Stop (Long Back on a remote) | the Stop media key |
| Long Left over video (channel list beside it) | Shift+Left |
| Volume up / down, mute (desktop only) | Ctrl+Up, Ctrl+Down, Ctrl+M |
| Window full screen | F11, a double click on the video (Escape leaves it first) |
| Overlay / full screen (development) | `o`, `f` |

Over full-screen video: OK shows the info card with TV guide and
History, Down the control row (Up back), Menu the control row at once,
Long OK the menu; Left the guide's list (Left in it: the groups), Long
Left the list beside the video, Right the most recent channel, Up/Down
change channel, Long Back stops. During catch-up Left/Right seek. Moving
the mouse shows the panels with play/pause, mute, volume and full screen
buttons; they hide 3 s after the mouse rests.

Environment: `IPTV_RS_AUDIO=null` mutes the player (mpv `ao`; use it for
tests), `IPTV_RS_HWDEC` sets mpv's `hwdec` (default `auto-copy-safe`),
`RUST_LOG=info` shows the player's state (never stream addresses).
`GDK_DEBUG=no-portals` makes the playlist-file picker GTK's own dialog
instead of the desktop portal's.

Known limit: mpv sends a stream's headers (Cookie, Authorization, ...) with
every request of that stream, including redirects and HLS segments on other
hosts; the Android player keeps identity headers to the stream's own host.

Package (release runner, `.desktop` entry and a `.tar.gz`; libmpv stays a
system dependency):

```sh
(cd crates/iptv-app && ~/src/nsh/target/release/nsh package --target linux -o ../../target/package .)
```

## Before any public release

- Replace `crates/iptv-app/src/theme.rs` (placeholder palette) with our own design.
- Choose the product name, icon and banner. The application id
  `com.hantekllc.iptvrs` is final once uploaded to Play.
- See Phase 9 of the plan.
