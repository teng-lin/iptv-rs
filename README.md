# iptv-rs

Codename for an IPTV player for Android TV and Android tablets, written in Rust
with [Native Shell (NSH)](https://github.com/longbridge/nsh). The UX targets
parity with the reference TV player described in `~/src/tivimate/docs` (remote
first, guide over live video, touch as a first-class second input). The
product is Patina IPTV Player ("Patina" on the launchers, the menu bar and
the packages); `iptv-rs` stays the codename, the crate names and the data
directories' name.

Plan and decisions: [`.plans/2026-11-09-tivimate-ux-replication.md`](.plans/2026-11-09-tivimate-ux-replication.md).
Android video/input research: [`docs/research/android-video-and-input.md`](docs/research/android-video-and-input.md).
Feature backlog: [Playlist, settings and channel/group gaps](docs/missing-features-2026-11-11.md).
Guide and list work: [Browsing behavior, custom groups and appearance](docs/guide-personalization.md).

## Layout

| Path | What |
| --- | --- |
| `crates/iptv-core` | UI-independent core: guide model and focus, channel lists and the rail/groups/guide navigation, favourites, player panels, context menus. No NSH, wasm-safe |
| `crates/iptv-nav` | Remote keys, short/long press, layered key → action map. Wasm-safe |
| `crates/iptv-settings` | One settings schema: catalogue, scoped store, versioned backup JSON. Wasm-safe |
| `crates/iptv-epg` | Sans-IO XMLTV parser (gzip), EPG channel matching, compact guide store. Wasm-safe |
| `crates/iptv-providers` | Sans-IO M3U parser, Xtream and Stalker clients, stable ids, catch-up URLs. Wasm-safe |
| `crates/iptv-guide` | The real guide schedule: imported channels + EPG store behind the `Schedule` trait, with gap filling and older days of a stored guide paged in on demand; stream resolution (URL + headers, live and catch-up) per provider. Wasm-safe |
| `crates/iptv-import` | Runtime-agnostic import pipeline: M3U/Xtream/Stalker sources into a `Catalog`; XMLTV guides streamed into a `GuideSink` (the app stages them on disk) and matched; typed failures, progress, cancel; EPG refresh policy. Wasm-safe |
| `crates/iptv-net` | Native HTTP adapter (reqwest + rustls) implementing `Fetch`; strips identity headers on cross-host redirects |
| `crates/iptv-store-sqlite` | SQLite store: provider catalog and guide data replaced per refresh (the guide from a download staged in its own file; with each feed's result and channel list), guide sources per playlist, user overlays (favourites, order, hidden, history, a channel's guide choice) kept by stable id, FTS5 search, tested migrations |
| `crates/iptv-search` | Channel and programme search (folded, ranked, favourites first), history, search-screen state. Wasm-safe |
| `crates/iptv-app` | NSH shell: screens, placeholder theme, bundled Roboto and Roboto Condensed (`fonts/`, SIL OFL 1.1; `tools/fetch-fonts.sh` rebuilds them), video layer; on Linux and macOS the `tv.player` component on libmpv (`src/mpv/`: the player, with `gtk.rs` and `mac.rs` for each desktop's view) |
| `hosts/android` | Kotlin host: `NshView`, Media3 `tv.player` component, TV launcher manifest |
| `tools/` | `sync-host.sh`, `make-hls-fixtures.sh`, `make-track-fixtures.sh`, `make-iptv-fixtures.py`, `mock-server.py`, `mock-xtream.py`, `mock-live-cuts.py`, `burst.sh`, `package-macos.sh` and `bundle-macos.py` (the self-contained macOS app), `fetch-fonts.sh` (the bundled fonts from Google's release, pinned by SHA-256) |

## Prerequisites

- Rust stable with `x86_64-linux-android`, `aarch64-linux-android`, `wasm32-unknown-unknown`.
- JDK 21, Android SDK (platform 35, build-tools 35.0.1, platform-tools, emulator), NDK 30.
- The NSH fork checked out next to this repository (`../nsh`, branch `tv-keys`;
  `git clone -b tv-keys https://github.com/teng-lin/nsh.git`), with its CLI built:
  `cargo build --release -p nsh-cli`. Cargo, Gradle and `tools/sync-host.sh` find it there.
- `ffmpeg` and `python3` for synthetic test streams.
- Linux desktop: GTK 4 and libmpv 2 (client API 2.x, `pkg-config mpv`;
  Arch `mpv`, Debian/Ubuntu `libmpv-dev`). The player links `libmpv.so.2`.
- macOS desktop: Xcode's command line tools and Homebrew's `mpv`
  (`brew install mpv`: libmpv 2, found through `pkg-config` or Homebrew's
  prefix by `crates/iptv-app/build.rs`). The player links `libmpv.2.dylib`.

## Run

```sh
cargo test -p iptv-core                                   # focus/guide model
cargo build -p iptv-core --target wasm32-unknown-unknown  # portability check
cargo run -p iptv-app                                     # Linux or macOS desktop (see below)

tools/make-hls-fixtures.sh --serve &                      # synthetic HLS on :8089
tools/make-track-fixtures.sh --serve &                    # several video/audio/subtitle tracks on :8097
                                                          # (--host 10.0.2.2:8097 for the emulators)
tools/mock-live-cuts.py &                                 # live streams the server cuts, on :8090
tools/sync-host.sh                                        # build libnsh.so (x86_64, arm64) into the host
(cd hosts/android && ./gradlew assembleDebug)
adb -s emulator-5554 install -r hosts/android/app/build/outputs/apk/debug/app-debug.apk
```

### Live streams the provider cuts

Providers cut live connections every minute or so, and a player has to
play on. `tools/mock-live-cuts.py` (ffmpeg and python3; a synthetic test
pattern with a burned-in clock and frame counter, made live) reproduces
both kinds of cut:

- live HLS, `/live/<name>.m3u8`: a 302 to a tokenized address whose
  playlist reloads are refused (403) once the token is older than
  `--hls-token-secs` (default 40). ffmpeg (mpv) reloads the redirected
  address and reaches the end of the stream; Media3 reloads the address
  it was given and gets a new token;
- live MPEG-TS, `/live/<name>.ts`: the live segments in real time, the
  connection closed after `--ts-close-secs` (default 70).

`/playlist.m3u` lists two channels of each kind (`--port`, default
8090). The app bridges both on the desktop and TS on Android (see
`crates/iptv-app/src/mpv/worker.rs` and `PlayerComponent.kt`); measured
gaps are in [`docs/perf.md`](docs/perf.md).

Emulators: `tab36` (Pixel Tablet, API 36) and `tv36` (Android TV, API 36).
Start them with `QT_QPA_PLATFORM=xcb` so the host keyboard reaches them.

### Linux desktop

`cargo run -p iptv-app` opens a 960×540 window (Wayland or X11). Video plays
through libmpv rendering into a GTK GL area under the UI. Without a stored
playlist the wizard opens with an empty playlist link; a build with
`IPTV_RS_DEV_PLAYLIST` set starts it with that address instead (e.g.
`IPTV_RS_DEV_PLAYLIST=http://127.0.0.1:8089/iptv/playlist.m3u cargo run -p iptv-app`
for the mock server; generate desktop fixtures with
`python3 -I tools/make-iptv-fixtures.py --host 127.0.0.1:8089`; for the
emulator `IPTV_RS_DEV_PLAYLIST=http://10.0.2.2:8089/iptv/playlist.m3u tools/sync-host.sh`).
Release builds never carry a development address: the synthetic guide shown
before a playlist plays the mock server's streams in debug builds only.
Data lives in
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
| Search | `/`, Ctrl+F (Cmd+F on macOS) |
| Long Left in the guide (the groups) | Tab, Shift+Tab |
| Volume up / down, mute (desktop only) | Ctrl+Up, Ctrl+Down, Ctrl+M |
| Window full screen | F11, a double click on the video (Escape leaves it first) |
| Overlay / full screen (development) | `o`, `f` |

In the guide Left walks back through the programmes a channel can replay
(catch-up: down to the oldest inside its archive and the stored guide;
without catch-up: to the programme on air) and from there opens the
groups; Left in the groups focuses the rail's Search. Right walks on to
the end of the stored guide. Home goes back to now, Shift+Left/Right an
hour, Ctrl (Cmd)+Left/Right a day, both stopping where Left and Right
do. The timeline pans back as far as the list's catch-up goes (no
further than now's half hour without catch-up); past programmes a row
cannot replay are dimmed.

Over full-screen video: OK shows the info card with TV guide and
History, Down the control row (Up back), Menu the control row at once,
Long OK the menu; Left the guide's list (Left in it: the groups), Long
Left the list beside the video, Right the most recent channel, Up/Down
change channel, Long Back stops. During catch-up Left/Right seek.

With a mouse or a touchscreen (never on a TV) the video has a play bar:
moving the mouse shows it, a tap or click on the video shows it and
another hides it; it hides 3 s after the last input, but not while the
mouse rests on it. It has mute and (desktop) volume, the stream's
quality, the channel, rewind/play-pause/fast-forward (catch-up), the
favourite star, tracks, programme details, picture size, the channel
list, the guide, (desktop) window full screen and the frame rate; under
it the programme's progress, where a click or a drag seeks in catch-up.
Any remote key hides it; OK still shows the info card.

Multiview (the control row's or the play bar's Multiview): the playing
channel becomes the first tile and the channel list opens to add the next
(up to four on the desktop, two on Android); the screen holds only the
tiles. Arrows choose the tile that is heard (a thin light outline), OK
opens its menu, a panel at the right edge that leaves the tiles in view
(Try again after a failure, Add screen, Search and add, Change channel,
Pause or Play for that tile alone, Enlarge screen or Reduce screen, Full
screen, Remove screen), Long OK enlarges or reduces it, Back watches the
active tile full screen. Search and add opens the search screen over the
tiles: a channel or programme result adds its channel as a new tile.
Full screen is a detour: Back (or Multiview again) returns to the same
tiles; changing channel there ends multiview. A tap makes a tile active,
a tap on the active one opens its menu, a long press enlarges it; a
moving mouse or a tap beside the tiles shows a toolbar (Add screen, Exit
multiview) and each tile's enlarge and remove buttons.
Adding more streams than an Xtream account allows asks first.

Environment: `IPTV_RS_AUDIO=null` mutes the player (mpv `ao`; use it for
tests), `IPTV_RS_HWDEC` sets mpv's `hwdec` (default `auto-copy-safe`),
`IPTV_RS_LIVE_BRIDGE=0` turns off playing on across live connections the
server closes (each one is then reconnected by the app),
`RUST_LOG=info` shows the player's state (never stream addresses).
The guide header's key-timing line ("keys N · X µs") shows in debug builds,
or with `IPTV_RS_DEBUG_OVERLAY=1`; the matching log line is always written.
`GDK_DEBUG=no-portals` makes the playlist-file picker GTK's own dialog
instead of the desktop portal's.

A live channel recovers by itself: after a cut or an error it keeps its
last frame (with a small "Reconnecting…" after a second) and retries,
fast at first (1, 2, 4, 4, 4 s), then after 5 and 10 s and every 20 s,
for as long as it stays selected; the "Can't play" card shows only when
a retry cannot help (the stream is gone, its format is not supported,
the account is refused) or after 2.5 minutes down, and even then retries
go on behind it every 30 s. Try again (OK) retries at once. A refusal by
the provider (often the account's streams are all in use elsewhere) says
how many streams the account allows when the provider told us.

On Android nothing plays in the background: when the activity stops
(Home, another app, the screen off) every player stops and closes its
connections, and when it starts again each one reopens its stream (live at
the live edge, catch-up where it was). A minimised desktop window plays on.

Known limit: mpv sends a stream's headers (Cookie, Authorization, ...) with
every request of that stream, including redirects and HLS segments on other
hosts; the Android player keeps identity headers to the stream's own host.

Package (release runner, `.desktop` entry and a `.tar.gz`; libmpv stays a
system dependency):

```sh
(cd crates/iptv-app && ../../../nsh/target/release/nsh package --target linux -o ../../target/package .)
```

### macOS desktop

The app opens at 1280×720 (`nsh.json` `"window"`, which NSH's runners
read: `nsh run --target macos`, `nsh package --target macos`;
`cargo run -p iptv-app` sets the same size) through NSH's AppKit backend.
Video plays through libmpv rendering into a `CAOpenGLLayer` under the UI,
one per player surface (mpv's render API has no Metal backend). Everything
above applies, with macOS's keys and places:

- Menus: the app menu (About, Settings… ⌘,, Hide, Quit), Edit (cut, copy
  and paste in text fields), View (Enter Full Screen) and Window.
- Data lives in `~/Library/Application Support/iptv-rs`; a `HOME`
  override moves it (and NSH's own data) for test runs.
- Window full screen: Ctrl+Cmd+F or fn+F (View menu), the play bar's
  button, a double click on the video; Escape leaves it first.
- Volume: Cmd+Up / Cmd+Down (macOS keeps Ctrl+Up/Down for Mission
  Control); mute Ctrl+M.
- `IPTV_RS_AUDIO=null`, `IPTV_RS_HWDEC` (default `auto-copy-safe`:
  VideoToolbox) and `RUST_LOG=info` as on Linux.
- Fonts: the bundled Roboto, Roboto Medium and Roboto Condensed register
  with CoreText at startup; NSH may log ``font family `Roboto` is not
  available`` once before they do, and lays that text out again after.
  Scripts Roboto lacks (Arabic, Hebrew, CJK, Thai, Devanagari, emoji)
  come from the system's fallback fonts.
- `nsh test <file>.test.tsx --target macos` drives the real app in the
  background (synthetic keys and clicks, window screenshots that include
  the video). Window full screen needs `--foreground`; while the screen
  is locked macOS neither enters full screen nor draws OpenGL layers, so
  pictures of the video come out empty then.

Package: `tools/package-macos.sh` builds the `.app` with
`nsh package --target macos`, then `tools/bundle-macos.py` copies libmpv
and the 50 or so dylibs it pulls in from Homebrew (FFmpeg, libass,
libplacebo, ...) into `Contents/Frameworks`, points the executable and
each other at them (`@rpath`), and signs everything again ad hoc. The
result (about 100 MB) runs on a Mac without Homebrew:

```sh
brew install mpv                     # the libmpv the app links and bundles
tools/package-macos.sh               # target/package/Patina.app
```

Limits of this package, and what a public one needs:

- Apple silicon only (Homebrew's arm64 bottles), and the macOS those
  bottles were built for or newer (the bundler raises
  `LSMinimumSystemVersion` to it: 26.0 on a macOS 26 build machine). A
  release builds libmpv and FFmpeg itself for an older deployment target
  (`MACOSX_DEPLOYMENT_TARGET`) and both architectures.
- Licences: Homebrew's FFmpeg and mpv include GPL parts (x264, x265, ...),
  so a package carrying them is GPL; a release that must not be uses an
  LGPL build of both.
- Notarization (not done): a Developer ID Application certificate; every
  dylib, then the app, signed with it, the hardened runtime and a secure
  timestamp (`codesign --force --options runtime --timestamp --sign
  "Developer ID Application: …"`; mpv's scripts are off, so no JIT
  entitlement should be needed, to be confirmed under the hardened
  runtime); then `ditto -c -k --keepParent Patina.app Patina.zip`,
  `xcrun notarytool submit Patina.zip --keychain-profile <profile>
  --wait` and `xcrun stapler staple Patina.app`.

### iPad

The iPad app runs through NSH's UIKit backend, in landscape, full screen
(`nsh.json` `"ios"`). Video plays through the same libmpv player as the
desktops, from MPVKit's LGPL static build, drawing with mpv's render API
(OpenGL ES 3) into a `CAEAGLLayer` under the UI (`src/mpv/ios.rs`). The
guide keeps its full sizes, as on an Android tablet.

```sh
tools/fetch-mpvkit.sh                # MPVKit 1.0.0's xcframeworks into target/mpvkit (checksums pinned)
cd crates/iptv-app
nsh package . --target ios-simulator -o ../../target/ios-sim   # Patina.app for the simulator
nsh package . --target ios -o ../../target/ios \
  --sign <signing identity SHA-1> --profile <app.mobileprovision>
xcrun devicectl list devices
xcrun devicectl device install app --device <id> ../../target/ios/Patina.app
xcrun devicectl device process launch --device <id> com.hantekllc.iptvrs
```

- Signing: Xcode's automatic signing with a personal team gives a
  development certificate and a profile for the bundle id once a project
  with that id is built for the connected iPad. The iPad needs Developer
  Mode on (Settings → Privacy & Security, then a restart) and, on the
  first launch, the developer trusted (Settings → General → VPN & Device
  Management). If `codesign` fails with `errSecInternalComponent` from a
  terminal, `security set-key-partition-list -S
  apple-tool:,apple:,codesign: -s ~/Library/Keychains/login.keychain-db`
  lets it use the key.
- Touch: a tap previews, a second watches full screen; the timeline pans
  and flings (Now returns). A round ‹ top left, over the video (the
  full-screen picture or the guide's preview), walks back the way a
  remote's Back and Left do: full-screen video → the guide (or
  multiview's grid) → the groups → the menu with Search, Live TV,
  Playlists and Settings named; a swipe in from the left edge is Back
  too. The header's ☰ also opens the groups;
  a tap beside them, or on the video beside the channel list, closes
  them. Settings has a Close button and the wizard a ‹ Back button; the
  remote's key hints show only after a hardware keyboard's key. These
  touch controls are the iPad's only: Android tablets keep their back
  button's model.
- Leaving the app stops the streams, as on Android, and coming back opens
  them again.
- Data lives in the app's `Library/Application Support/iptv-rs`.
- Development: `IPTV_RS_DEV_PLAYLIST=<url>` while building adds that
  playlist on the first start (a mock server on the Mac's LAN address for
  a device); `nsh test <file>.test.tsx --target ios` drives the app in a
  simulator. The simulator plays without sound (`ao=null`: its audio
  output stalls mpv's clock or aborts on a reopen). mpv runs with
  `profile=fast` on iOS: its default scalers took 117 ms a full-screen
  frame on the simulator's OpenGL ES, `fast` 44 ms.
- Size: the signed device app is about 69 MB (the executable 72 MB,
  59 MB stripped), the IPA about 26 MB.

## Tests

`tools/check.sh` runs every host-side check (formatting, the crates'
tests, the wasm builds, the app's build and tests); run it before
committing.

Every test binary is a link of its own, and the app's link NSH, GTK and
libmpv, so each crate's integration tests are one binary: `tests/it/`
(`tests/app/` in `iptv-app`), a module per topic, `main.rs` listing them.
A test that needs process-wide state of its own stays a separate file in
`tests/` (`iptv-store-sqlite`'s `epg_memory` and `epg_stream_memory`
count allocations with a global allocator).

The app's scenarios (`crates/iptv-app/tests/app/`) are headless runs of
the whole app, and the app keeps its data under `$HOME` and holds
process-wide state, so each still runs in a process of its own: its
`#[test]` starts with

```rust
let Some(home) = isolated!() else { return };
```

which, under `cargo test`, runs the test binary again for just that test
with a fresh, empty `$HOME`, forwards its output (shown when it fails, or
with `--nocapture`) and fails if it fails (`tests/app/isolated.rs`). At
most four scenario processes run at once; `IPTV_RS_TEST_JOBS=N` changes
that. Under cargo-nextest, which runs every test in a process of its own,
the scenario runs directly. To add a scenario, write
`tests/app/<name>.rs` with such a test and add `mod <name>;` to
`tests/app/main.rs`. `cargo test -p iptv-app guide_look` runs one
scenario (`-- --nocapture` shows its log).

Debug builds keep full debug info for our crates only; other packages
(crates.io and the NSH checkout) keep line tables (backtraces show
file:line, a debugger no variables there), and debug info stays in the
object files under `target/` (`split-debuginfo = "unpacked"`), which
keeps the app's test binary at ~175 MB instead of ~1 GB (workspace
`Cargo.toml`).

## Before any public release

- Replace `crates/iptv-app/src/theme.rs` (placeholder palette) with our own design.
- The application id `com.hantekllc.iptvrs` is final once uploaded to
  Play. The name (Patina), icon and TV banner are in place (`art/icon`,
  `tools/make-icons.py`).
- See Phase 9 of the plan.
