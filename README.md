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
| `crates/iptv-core` | UI-independent core: guide model and focus, player panels, context menus. No NSH, wasm-safe |
| `crates/iptv-nav` | Remote keys, short/long press, layered key → action map. Wasm-safe |
| `crates/iptv-settings` | One settings schema: catalogue, scoped store, versioned backup JSON. Wasm-safe |
| `crates/iptv-epg` | Sans-IO XMLTV parser (gzip), EPG channel matching, compact guide store. Wasm-safe |
| `crates/iptv-providers` | Sans-IO M3U parser, Xtream and Stalker clients, stable ids, catch-up URLs. Wasm-safe |
| `crates/iptv-guide` | The real guide schedule: imported channels + EPG store behind the `Schedule` trait, with gap filling. Wasm-safe |
| `crates/iptv-import` | Runtime-agnostic import pipeline: M3U/Xtream/Stalker sources and XMLTV guides into a `Catalog` and `GuideStore`, typed failures, progress, cancel; EPG refresh policy. Wasm-safe |
| `crates/iptv-net` | Native HTTP adapter (reqwest + rustls) implementing `Fetch`; strips identity headers on cross-host redirects |
| `crates/iptv-store-sqlite` | SQLite store: provider catalog and guide data replaced per refresh, user overlays (favourites, order, hidden, history) kept by stable id, FTS5 search, tested migrations |
| `crates/iptv-search` | Channel and programme search (folded, ranked, favourites first), history, search-screen state. Wasm-safe |
| `crates/iptv-app` | NSH shell: screens, placeholder theme, video layer |
| `hosts/android` | Kotlin host: `NshView`, Media3 `tv.player` component, TV launcher manifest |
| `tools/` | `sync-host.sh`, `make-hls-fixtures.sh`, `make-iptv-fixtures.py`, `mock-server.py`, `mock-xtream.py`, `burst.sh` |

## Prerequisites

- Rust stable with `x86_64-linux-android`, `aarch64-linux-android`, `wasm32-unknown-unknown`.
- JDK 21, Android SDK (platform 35, build-tools 35.0.1, platform-tools, emulator), NDK 30.
- NSH CLI built from the fork: `~/src/nsh` (branch `tv-keys`), `cargo build --release -p nsh-cli`.
- `ffmpeg` and `python3` for synthetic test streams.

## Run

```sh
cargo test -p iptv-core                                   # focus/guide model
cargo build -p iptv-core --target wasm32-unknown-unknown  # portability check
cargo run -p iptv-app                                     # Linux desktop (keyboard: arrows, Enter, Esc, o, f)

tools/make-hls-fixtures.sh --serve &                      # synthetic HLS on :8089
tools/sync-host.sh                                        # build libnsh.so (x86_64, arm64) into the host
(cd hosts/android && ./gradlew assembleDebug)
adb -s emulator-5554 install -r hosts/android/app/build/outputs/apk/debug/app-debug.apk
```

Emulators: `tab36` (Pixel Tablet, API 36) and `tv36` (Android TV, API 36).
Start them with `QT_QPA_PLATFORM=xcb` so the host keyboard reaches them.

## Before any public release

- Replace `crates/iptv-app/src/theme.rs` (placeholder palette) with our own design.
- Choose the product name, icon and banner. The application id
  `com.hantekllc.iptvrs` is final once uploaded to Play.
- See Phase 9 of the plan.
