# Performance against the product plan's budgets

Budgets (`.plans/2026-11-10-product-plan.md` §5) are for a real low-end
Android TV box. Measure with `tools/perf-android.sh [serial]` after adding the
large synthetic playlist:

```sh
python3 -I tools/make-iptv-fixtures.py --channels 10000 --groups 40 \
  --days-back 7 --days-ahead 2 --host 10.0.2.2:8089 --out fixtures/hls/big
sed -i 's#/iptv/guide.xml.gz#/big/guide.xml.gz#' fixtures/hls/big/playlist.m3u
tools/mock-server.py 8089 &   # then add http://10.0.2.2:8089/big/playlist.m3u
```

That fixture is 10 000 channels in 40 groups and 2.45 M programmes
(51 MB gzip), heavier than the budget's "10k channels, 7 days".

## 2026-11-10, tablet emulator (Pixel Tablet, API 36, x86_64), master `fb7db90`

Emulator numbers bound nothing on real hardware (host CPU, GPU through
gfxstream, and other builds were running on the host); they show trends.

| Budget | Target | Result |
| --- | --- | --- |
| Cold start to the stored guide on screen | < 2 s | median 1.92 s (1.77–2.17 s; first frame 0.41–0.51 s) |
| Zap to first picture (local HLS, channel down) | < 1.5 s | median 1.27 s (0.83–1.72 s); player ready 1.17–2.35 s |
| Guide key handling | – | median 0.4 ms, max 38 ms |
| Guide frame time during 30 × Down | < 50 ms key to frame | 50th 44–73 ms, 90th 65–150 ms, 99th 81–250 ms (two runs) |
| Memory, guide loaded (7 days back) | – | PSS 220–244 MB |
| Guide import (playlist + 2.45 M programmes) | – | channels on screen 0.5 s after submit; guide stored 17.5 s later; **peak RSS ≈ 570 MB** |
| APK download (arm64 release) | < 15 MB | ≈ 11 MB (APK 20.7 MB) |

## 2026-11-10, desktop, guide import streamed to disk (branch `epg-streaming`)

The import no longer holds the parsed guide: programmes go to a stage
file next to the store as they are parsed (`iptv_store_sqlite::EpgStage`,
a writer thread, batches of at most 4096 programmes or 1 MiB of text),
each channel is settled there with the same rules as the in-memory
builder, and `replace_epg` copies the bound channels into the store in
one transaction. Only the feeds' channel lists stay in memory for
matching. Headless benchmark of what the app does (playlist import and
store, guide download, replace, prune, window read back), same fixture
served by `python3 -m http.server`:

```sh
cargo run --release -p iptv-store-sqlite --example epg_import_bench -- \
  http://127.0.0.1:8123/big/playlist.m3u fixtures/bench
```

Intel Core 7 240H, 16 threads, NVMe; other builds were running (load
11–16). Old and new runs alternated, three each, store on disk:

| Guide import (2.45 M programmes, descriptions on, 7 days kept) | Before (`61a4bd2`) | After |
| --- | --- | --- |
| Download + parse (+ stage + settle) | 4.6–4.8 s | 4.4–4.6 s |
| Stored (replace, feed results, prune) | at 16.4–16.6 s | at 11.0–11.6 s |
| Window read back (total) | 17.3–17.5 s | 12.0–12.5 s |
| Peak RSS above the app before the import | +303 MiB | +84 MiB |
| … until the guide is stored | +303 MiB | +43 MiB |
| Guide window kept in memory afterwards | 38 MiB | 38 MiB |
| Stage file (deleted after) | – | 239 MiB (104 MiB without descriptions) |

With the store on tmpfs: 14.2–14.4 s before, 10.6–10.7 s after; without
descriptions (tmpfs, one run each): 10.5 s before, 8.5 s after, peak +79
MiB before, +84 MiB after (+67 → +41 MiB until stored). `cargo test -p iptv-store-sqlite --test
epg_stream_memory` checks that the import's Rust heap (~2 MiB, every
thread) and SQLite heap (stage page cache 4 MiB; 32 MiB store cache
while copying) do not grow with the guide's length.

The peak is now reading the window back (`Store::load_epg`, the same
code as at app start): `SourceBuilder` holds 32-byte raw rows for every
programme of the window before compacting them into 16-byte slots.

## 2026-11-10, tablet emulator, streamed import (master `c998208`)

Re-import of the same 2.45 M-programme guide over a stored one (the stored
guide marked two days old, startup refresh), RSS sampled every ~1.6 s:

| | Result |
| --- | --- |
| RSS before the download (stored 7-day window loaded, a channel playing) | ≈ 370 MB |
| Peak RSS during the import | ≈ 436 MB (+66 MB; the old path added ≈ +300 MB) |
| Download → stored | 32.5 s (replacing 2.4 M stored rows; the earlier 17.5 s was a first import into an empty store) |
| PSS after | ≈ 319 MB |

Next: the stored window itself (≈ 370 MB RSS with 7 days × 10 000 channels
loaded) dominates on Android; load a narrower window (e.g. now − 1 day …
end) and page older days in on demand when browsing back. Done: see the
guide window section below.

## 2026-11-10, guide window: the last day held, older days paged in (branch `epg-window`)

The app held every kept day of the stored guide in memory (`epg.keep_past`,
a week by default). Now a shown schedule holds the stored guide from one
day before the hour on (`data::pages::LOADED_PAST_DAYS`, at most what is
kept); older days are read one day at a time when something needs them
(`iptv_guide::pages`: a guide cell browsed back to, a programme's details,
catch-up of an older programme, which waits for its day and pins it), on
their own thread, and show "Loading…" until then. At most three older
days are held; they go when the guide's window is back at now. Programme
search reads the titles of every kept day from the store on its own
thread (`Store::for_each_epg_title`) instead of indexing the guide in
memory. Reading a window back builds the slots straight from the store's
settled rows (`iptv_epg::SettledBuilder`), so it no longer peaks at
~2.5× what it keeps (finding 1). SQLite schema and retention unchanged.

Same fixture (10 000 channels, 2.45 M programmes, 7 days kept, stored
by the app's import), `c6f5d4a` before.

**Tablet emulator** (Pixel Tablet, API 36, x86_64; x86_64-only debug
host APK, `nsh package` thin LTO; the host had a load of 9–23 from other
work), the stored guide fresh (no download at start), nothing playing.
Memory sampled after a cold start (two runs each, they agree within
1 MB); `tools/perf-android.sh emulator-5554` for the rest (zap measured
nothing: the fixture's channels did not start playing):

| Tablet emulator | Before | After |
| --- | --- | --- |
| RSS, 10 s after a cold start (guide on screen) | 442 MB | 333 MB |
| RSS, steady (70–100 s) | 394 MB | 319 MB |
| PSS, steady | 277 MB | 202 MB |
| Native heap (PSS), steady | 208 MB | 133 MB |
| Cold start to the stored guide on screen (median of 3) | 1825 ms (1761–2522) | 1875 ms (1574–2340) |
| Guide key handling (30 × Down) | median 0.59 ms, max 4.5 ms | median 0.53 ms, max 2.5 ms |
| Guide frame time (30 × Down), 50th / 90th / 99th | 42 / 65 / 101 ms | 40 / 61 / 77 ms |
| An older day paged in (‹ in the time bar, 10 000 channels) | – | 256–331 ms, ≈ 5.1 MiB each |
| RSS after paging in 1 / 2 / 3 / 4 days (3 held) | – | 320 / 339 / 348 / 359 MB |
| RSS back at now (3 days dropped) | – | 342 MB |

The script's own memory line (taken after its Back key and 30 × Down)
read PSS 217 / 216 MB, RSS 335 / 334 MB for both builds, so it is not
used here. Dropped pages free their heap, but the allocator keeps most of
it (RSS stays ≈ 20 MB above the start until it is reused); the cap of
three days bounds that.

**Desktop** (Intel Core 7 240H, load 9), one window read per process
(`epg_window_bench`, store filled by `epg_import_bench`), three
alternating runs each, identical results:

```sh
cargo run --release -p iptv-store-sqlite --example epg_window_bench -- fixtures/bench window 7
```

| Desktop window read (`Store::load_epg`) | Before | After |
| --- | --- | --- |
| 7 days (what the app held before): time | 0.85–1.38 s | 0.79–0.80 s |
| … guide heap / RSS peak over the read / held after | 38 MiB / +82 MiB / +82 MiB | 38 MiB / +58 MiB / +58 MiB |
| 1 day (what the app holds now): time | 0.37–0.47 s | 0.35–0.36 s |
| … guide heap / RSS peak / held after | 13 MiB / +49 MiB / +49 MiB | 13 MiB / +33 MiB / +33 MiB |
| One older day (a page): time, heap, RSS peak | 0.19 s, 5.1 MiB, +28 MiB | 0.18 s, 5.1 MiB, +25 MiB |

RSS includes the connection's SQLite page cache (up to 16 MiB; the app
closes the connection after each read). So the app's guide at start
goes from +82 MiB (7 days, old read) to +33 MiB (1 day, settled read).

Not measured: programme search's index over every kept day now builds
off the UI thread from the store (2.45 M titles); its build time and size
on the emulator, and a real TV box.

## 2026-11-10, desktop, live streams the server cuts (branch `seamless-reconnect`)

Providers cut live connections every 40–90 s (live HLS: the playlist reload
at the redirect's tokenized address answers 403; MPEG-TS: the server
closes the connection). Before, each cut was `ended` → the app's retry →
a new load behind the black shutter. Now mpv plays on with a spare entry
of the same address that it opened while the buffered rest played
(`crates/iptv-app/src/mpv/worker.rs`), and a reconnect the app still has
to make keeps the last frame, with "Reconnecting…" only after 1 s.

Measured with `tools/mock-live-cuts.py` (HLS token 40 s, TS closed after
70 s; `--open-delay-ms 1500` for a slow provider) on the Linux release
build, `IPTV_RS_AUDIO=null`, on a headless Hyprland output captured with
`grim` every ~0.25 s (1/8 scale): a frame is black below mean luma 12,
frozen when it equals the one before; times are bounded by the captures
around them. The picture carries a wall clock, read off the captures
for content jumps. Audio gaps are from mpv 0.41 itself (`--ao=null`
log: the old entry's audio running out to the spare's restart) with the
same prefetch.

| Cut | Before (`0f19b82`) | After |
| --- | --- | --- |
| HLS, local mock (2 cuts) | black ≤ 0.65 s and ≤ 0.45 s | no black, no frozen capture; content jumps back ≈ 2 s |
| HLS, 1.5 s per new connection (1 cut before, 3 after) | black ≤ 3.6 s | no black; ≤ 1 s skipped forward or frozen (≤ 0.8 s) |
| TS, local mock (2 cuts) | black ≤ 1.2 s and ≤ 1.4 s | no black, no frozen capture; content jumps back ≈ 1 s |
| TS, 1.5 s per new connection | black ≤ 2.7 s | no black; frozen ≤ 1.3 s |
| Audio at a bridge (mpv CLI, local mock) | stops until the reopen plays | ≈ 0.2 s (HLS), ≈ 0.25 s (TS) |
| App reconnect without bridging (`IPTV_RS_LIVE_BRIDGE=0`), HLS, 1.5 s | black ≤ 3.6 s | last frame held ≈ 3 s, "Reconnecting… (1 of 5)" from 1 s |

No `ended`, retry or reopen reaches the app at a bridged cut; latency
stays where the stream started (HLS ≈ 7 s behind the clock on the
picture, TS ≈ 4 s). The spare of an HLS stream starts two segments back
(`live_start_index=-2`): three (ffmpeg's default) replayed ≈ 3.5 s at each
cut; one replayed nothing on the local mock but left a single segment
buffered, so the slow provider then froze ≈ 2 s at the next cut and
rebuffered between cuts. A TS bridge has only what the server sent ahead
(≈ 1 s here), so a provider slower than that to open freezes for the
difference. Android (Media3) was compiled, not run: it reloads HLS
playlists from the original address (no 403 to bridge) and plays TS as
two items of the same address (`PlayerComponent.kt`).

## 2026-11-10, macOS, release build (branch `macos-2`)

Apple M4 (10 cores, 24 GB), macOS 26.6; load average about 9 (other
builds were running). `cargo build --release -p iptv-app` (the same code
as `tools/package-macos.sh`'s app; its `cargo run` entry point logs with
`RUST_LOG=info`), a scratch `HOME` holding the large synthetic playlist
above (10 000 channels, 2.47 M programmes, imported once beforehand) with
channel 1 last watched, `IPTV_RS_AUDIO=null`, mock server on :8089 serving
the fixture's three local HLS streams. The screen was locked: macOS draws
no OpenGL layer then, so the video's frames were decoded but not drawn
(the UI's own drawing still ran).

| Budget | Target | Result |
| --- | --- | --- |
| Cold start to the stored guide (process start → `iptv data: guide switched`) | < 2 s | 0.81 s (two warm runs); 1.91 s on the first launch of the new binary (cold caches) |
| … and the resumed channel ready (`iptv player 0 state: ready`) | – | 1.09–1.10 s warm, 2.20 s cold |
| Zap to first picture (Channel Up → the player's first frame, `state: ready`, which also lifts the black shutter), 8 zaps | < 1.5 s | median 744 ms (675–865 ms), no errors |
| Memory, guide loaded and a channel playing | – | RSS ≈ 409 MB; physical footprint ≈ 535 MB (peak ≈ 600 MB); ≈ 440 MB RSS after 8 zaps |

Zap is measured to `state: ready` rather than to the player's `size`
event (`tools/perf-android.sh`): the desktop player reports `size` only
when it changes, and all synthetic streams are 1280×720. Channel Up came
from a temporary in-app timer (every 5 s, not in the repository): nothing
can send keys to a locked session, and `nsh test`'s agent drives debug
builds only. Memory is not broken down yet; it is not comparable with
the emulator's PSS (220–244 MB) as such (RSS and the physical footprint
count shared and GPU memory differently), and it holds the same 7-day
guide window, so narrowing that window (finding 1) applies here too.

## 2026-11-10, macOS, live streams the server cuts (branch `macos-3`)

The live bridging above (`mpv/worker.rs`, shared by both desktops) on
macOS: Apple M4, macOS 26.6, the app run by `nsh test --target macos`
(its development host, a debug build), display kept awake (`caffeinate
-d`: macOS draws no OpenGL layer while the screen is locked or asleep).
`tools/mock-live-cuts.py` as above (HLS token 40 s, TS closed after
70 s, `--open-delay-ms 1500` for a slow provider); on macOS its picture
has testsrc2's own time code instead of the wall clock (Homebrew's
FFmpeg has no `drawtext`). A temporary trace in the app (not in the
repository) recorded every draw of the player's `CAOpenGLLayer` and
whether the black shutter was up, every new frame mpv offered, and each
bridge and reopen: a frame is black when drawn behind the shutter, frozen
when mpv offers no new one. Content jumps are read off the time code in
window pictures taken every ≈ 2.3 s (that run only: taking a picture
holds the main thread, so its frame gaps are not counted; the slow
provider's runs took none and give the frame cadence). Audio gaps
are mpv's own (0.42 CLI, `--ao=null --prefetch-playlist=yes`, the
address listed again as the app appends its spare): from the old
entry's `audio end or underrun` to the spare's `starting AO`.

| Cut | macOS |
| --- | --- |
| HLS, local mock (2 cuts) | both bridged; no black; content back ≈ 0.7 s and ≈ 2.1 s |
| TS, local mock (2 cuts) | both bridged; no black; content back ≤ 0.2 s |
| HLS, 1.5 s per new connection (3 cuts) | all bridged; no black; 25 fps through each (largest gap between new frames within ±3 s: 0.07 s) |
| TS, 1.5 s per new connection (1 cut) | bridged; no black; 25 fps through it (largest gap 0.05 s) |
| Audio at a bridge (mpv 0.42 CLI, local mock, 3 each) | HLS 0.17–0.22 s, TS 0.26–0.43 s |
| Real provider (anonymised), HLS, one channel, 5 min | no cut in 5 min; ready 1.3 s after opening; no black, no freeze |
| Real provider (anonymised), TS, one channel, 5 min | 2 cuts (≈ 2 min apart), both bridged; no app-level reconnect; ready 1.1 s; no black, no freeze over 0.3 s |

No `ended`, retry or reopen reached the app at any cut; the only black
frames were deliberate zaps to another channel. The real provider played
with sound at 20 % (its audio gaps were not measured); its login was read
at run time from outside the repository, and the test profile, traces
and pictures were deleted afterwards.

## 2026-11-10, macOS, multiview and the restyled guide (branch `macos-4`)

Apple M4, macOS 26.6, the app run by `nsh test --target macos` with its
development host built at `opt-level = 3` without debug assertions or
debug info (`CARGO_PROFILE_DEV_*`; close to a release build, which the
test agent cannot drive), `IPTV_RS_AUDIO=null`, mock server on :8089.

Multiview: the synthetic playlist's three local HLS streams (1280×720),
a 960×540 window, display kept awake (`caffeinate -d`) with every tile
drawing; process CPU (100 % is one core) and RSS from `ps` once a
second over 15–30 s:

| Case | CPU | RSS | Threads |
| --- | --- | --- | --- |
| One channel full screen | 18 % | ≈ 310 MB | 43 |
| Four tiles | 54 % (43–59 %) | ≈ 520 MB | ≈ 100 |

A debug build gives about the same CPU with four tiles (≈ 55 %, RSS
≈ 580 MB): the cost is mpv's decoding and drawing, four VideoToolbox
decodes and four `CAOpenGLLayer`s.

Guide scrolling: the large synthetic playlist above (10 000 channels,
2.45 M programmes) added through the wizard, the restyled guide at
1280×720, keys from the test agent (Down and Up 120 times with 50 ms
pauses, about 4 a second; Down 60 times without; Right 40 times), the
NSH performance monitor's readings (`nsh test --fps -v`) once a second;
two runs with the screen locked (it paces the display link at ≈ 48 Hz,
so their frame rate is not a result), three shorter ones locked and one
unlocked for the long frame below. The costs:

| Moves | Main thread per frame | JS per commit | Core per commit | Dropped frames |
| --- | --- | --- | --- | --- |
| Down / Up (the list scrolls, animated) | p95 7–8 ms (max 14 ms) | ≈ 2 ms | p95 ≈ 4 ms (max 5 ms) | 0 % |
| Down 60 times without pauses | p95 ≈ 8 ms (max 9.5 ms) | ≈ 2 ms | p95 ≈ 4 ms | 0 % |
| Right (the time window moves, not animated) | ≤ 9.5 ms | ≈ 2 ms | p95 9–10 ms (max 12–16 ms) | – |

With the screen unlocked the list scrolls at 60 fps (the display's
rate), the main thread at most 11.5 ms per frame, no dropped frames. The
monitor's physical footprint went from 105 MB (guide idle, nothing
playing) to 125 MB after the scrolling. With the screen locked, the
first animated scroll after the time window moved sideways took one
130–240 ms frame (all five locked runs; two also had one core commit of
107 and 114 ms there), and a `sample` profile over it showed neither the
main thread nor the engine thread busy that long; unlocked, the same
steps gave no long frame (at most 11.5 ms): the locked session's
scheduling, not the guide's work.

## 2026-11-10, desktop, live streams that fail for a while (branch `resilient-live`)

A live channel now retries for as long as it stays selected (1, 2, 4, 4,
4 s, then 5, 10 and every 20 s), keeps its last frame, and shows the
"Can't play" card only for failures a retry cannot fix or after 2.5 min
down, retrying every 30 s behind it (`iptv_core::playback::Health`).
Refusals (401/403/429/458/509) are retried like any other transient
failure.

Measured on Linux with libmpv 0.41 headless (`--vo=null --ao=null`) and
`tools/mock-live-cuts.py`, driven by a scratch harness (not in the
repository) that runs the app's glue in miniature: the real
`iptv_core::playback::Health` of each build (before: `8ac1309`), its
retry and stall timers, mpv's per-load options of each build and the
desktop player's rule for the black shutter (before: every error; after:
only a live stream that never showed a frame). No window was opened: the
desktop session had no private output to put one on. Recovery is from
the stream's end or first error to the next first frame; outages are
in the mock's own words (`--refuse-*`, `--drop-*`).

| Case | Before | After |
| --- | --- | --- |
| TS, connection closed into a full account (509 for the next 45 s) | 5 retries, then "Can't play" (30 s after the cut); black from the first refusal; never recovers | plays again 50 s after the cut (≈ 8 s after the account had a stream free); last frame kept, no card |
| HLS, token expired into a full account (403 for 45 s) | "Can't play" at once (403 was final); black; never recovers | plays again 49 s after the cut (≈ 13 s after the refusals ended); last frame, no card |
| HLS, network down 20 s | black 30 s; plays again 15 s after the network returned | no black; plays again 4 s after the network returned (19 s after the cut) |
| TS, network down 60 s | "Can't play" after 30 s; black; never recovers | no black, no card; plays again ≈ 13 s after the network returned |
| HLS, network down 200 s | "Can't play" after 30 s; black 264 s to the end; never recovers | last frame; card after 169 s down (the first failure past 150 s) "still trying"; the retry behind it played 4 s after the network returned and the card went |
| First frame after open, 300 ms per new connection, 6 runs each (HLS / TS) | median 650 / 1049 ms | median 656 / 1050 ms (noise: the cache options start playback at once) |

The pill says "Reconnecting…" after the quiet second; nothing else
changed in zap time. Not measured here: the real app's frames (no
private display), and Android (compiled only: a 10 s minimum live
offset, ~15 s of load retries, and the player's own reopen once per 30 s
need a device).

## 2026-11-10, macOS, live streams that fail for a while (branch `macos-5`)

The recovery above (`resilient-live`) in the macOS app: Apple M4, macOS
26.6, the app run by `nsh test --target macos` (a debug build; the zap
runs on an optimized one), `IPTV_RS_AUDIO=null`,
`tools/mock-live-cuts.py` with its defaults (HLS token 40 s, TS closed
after 70 s), each outage started by the mock's `/control/drop` or
`/control/refuse` while the channel played full screen. The screen was
locked (no OpenGL layer drawn), so a temporary trace in the app (not in
the repository) recorded the player's black shutter, failures, opens and
every frame mpv offered; the pill and the card were read off the screen
every 0.5 s.

| Case | Black | Pill | Card | Opens | Plays again |
| --- | --- | --- | --- | --- | --- |
| HLS, network down 20 s | never | 8.8 s (the cache played 7 s) | – | 5 | 3.8 s after the network returned |
| HLS, the token's 403 into a refusing account (403 for 45 s) | never | when it ran dry | – | 7 | 5.3 s after the refusals ended |
| HLS, network down 200 s | never | 10.5 s | 172 s, "still trying" (the first failure past 150 s down) | 14 (1, 2, 4, 4 s, then 5, 10 and every 20 s) | 1.8 s after the network returned; the card went with it |
| TS, closed into a full account (509 for 45 s) | never | at the close | – | 8 | 17.9 s after the account had a stream free |
| TS, network down 60 s | never | 3.9 s | – | 9 | 18 s after the network returned |

The TS cases came back later than on Linux (8 and 13 s) only because
their outages ended early in a 20 s wait between tries. Zap to first
picture (`state: ready`, Channel Up in the full-screen player, the
fixture's local HLS, ten zaps each, the same optimized test host): median
776 ms (679–845) before the cache options (`8ac1309`), 770 ms (695–878)
after: unchanged.

## 2026-11-10, Android TV box, cold start to the stored guide (branch `tv-cold-start`)

Chromecast with Google TV 4K (Amlogic S905D3, 4 cores, 2 GB, Android 14,
32-bit `armeabi-v7a`), with the user's own provider (counts only: 7258
channels, 6081 with guide data, about 257k programmes stored; the guide
window held is ≈ 5 MiB). Resume of the last channel on. `tools/perf-android.sh
<serial> --cold` (three cold starts: force-stop, `am start -W`); every run
prints the phase marks (`iptv startup: <phase> +ms`, ms since the process
started; `crates/iptv-app/src/startup.rs`). "Guide on screen" is the
script's measure (`guide switched` in the log, from the `am start`).

**Debuggable APKs.** A profile (simpleperf) of the debug APK showed the
Rust side nearly idle: the cost was the host's, NSH's and Media3's Java
running in ART's slow interpreter, which a debuggable app gets (drawing,
`Mounter.apply`, player creation on the main thread; the player's thread).
`assembleRelease` now builds a non-debuggable, `profileable` APK signed
with the debug key, so it installs over a debug build and keeps the data:

```sh
ABIS=armeabi-v7a tools/sync-host.sh --release
cd hosts/android && ./gradlew assembleRelease
adb -s <serial> install -r app/build/outputs/apk/release/app-release.apk
```

Compiling the app with its profile afterwards (`cmd package compile -m
speed-profile -f`) changed nothing measurable (the Java side is no
longer on the critical path).

Phases, three runs each (ranges; ms since the process started):

| Phase | Debug APK, master code | Release APK, master code | Release APK, this branch |
| --- | --- | --- | --- |
| First frame (`am start` TotalTime) | 1209–1237 | 343–399 | 359–587 |
| Rust root component runs | 1064–1109 | 237–261 | 234–456 |
| Store open, startup source read (time on the UI thread) | 1241–1268 (118–183) | 369–446 (110–167; 1739 on the first start after the install) | 255–489 (8–10) |
| Last channel resolved, player opened | 5382–5487 / 5589–5666 | 2328–3388 / 2357–3418 | 260–494 (from its stored row) |
| Fonts registered | 4087–4252 (≈ 3 s after asking) | 1020–2280 (0.78–2.0 s) | 499–766 (unpacked off the UI thread in 0.23–0.29 s) |
| Channels read (import thread) | 1470–1528 | 558–1950 | 578–804 |
| First screen read (61 channels × 7 h) | – | – | 607–909 (29–103 ms) |
| Guide window read (6081 channels) | 2715–3044 (1.19–1.54 s) | 1980–3049 (1.1–1.55 s) | 2481–2706 (1.8–2.1 s, after the guide is shown, beside the video) |
| Guide shown (schedule swapped in) | 5381–5486 | 2327–3387 | 778–1114 |
| Next display frame after it | 10034–10424 | 3007–4021 | 1434–1888 |
| Resumed channel's first video frame | 11638–11926 | 4207–5284 | 2245–2730 |

| Budget (from `am start`) | Before (debug APK, master) | After (release APK, this branch) |
| --- | --- | --- |
| Guide on screen, budget 2 s | 4414–4557 ms, median 4482 | 879–1196 ms, median 976 |
| Resumed channel ready (`state: ready`) | 12130–12244 ms | 2420–2880 ms |

What changed, each verified on the box:

1. Release APK (above): Rust root 1.07 s → 0.25 s, first frame 1.2 → 0.36 s,
   the player's first frame 1.5 s sooner after it opens.
2. The last channel starts from its one stored row as soon as the store
   is open (`player/resume.rs`), beside the catalog and guide reads, and
   is found again by key when the guide arrives (not opened again;
   `tests/cold_start.rs`): player opened 2.3–3.4 s → 0.26–0.49 s.
3. The guide's empty shell replaces the synthetic guide at once (it was
   on screen, made-up channels and all, until the stored guide came).
4. The stored guide comes in two steps (`data/opening.rs`): the channels
   with the first screen's programmes (30 channels either side of the
   one the guide opens on, the hour before now to six hours on: 12 KiB,
   29–103 ms), other channels and times showing "Loading…"
   (`EpgSchedule::held_only`, `held_channels`); then the whole window,
   which replaces it.
5. Fonts are unpacked on their own thread (NSH unpacked each WOFF2 twice
   on the UI thread).
6. The window read seeks to the last programme starting at or before the
   window instead of walking the older days kept (`epg_window.rs`), and
   `from_catalog` finds group positions by hash.

The script's `played`/zap lines matched no log line since the player's
lines carry their surface (`iptv player 0 state: ready`); fixed.

**Desktop** (synthetic store: 7258 channels in 100 groups, two days back
and ahead, 804k programmes; `cargo run --release -p iptv-store-sqlite
--example startup_bench -- <store dir>`, three runs): the guide window
(a day back to the end, 597k programmes) 256–266 ms → 200–212 ms; the
first screen over every channel 53–56 ms. The headless app tests
(`cargo test -p iptv-app`) pass; the desktop app was not timed (no
private display).

Memory after the third run, the channel playing: PSS 199 MB (native heap
99 MB); the 171 MB before was taken without video.

Open: the guide window read takes 1.8–2.1 s beside the starting video
(it is no longer on the guide's path; reading it with fewer, wider
queries was no faster on the desktop). The startup refresh still reads
the window again and rebuilds the schedule once the playlist downloaded
(a few seconds in). The next display frame comes 0.6–0.9 s after the
guide is swapped in; the engine thread is nearly idle then, so it is the
host's apply and draw (or NSH's frame timing), not measured further.
The tablet (Snapdragon 695) was not re-measured.

## 2026-11-10, Android TV box, guide scrolling with the remote (branch `tv-guide-scroll`)

The same box as above (Amlogic S905D3, Android 14, 32-bit; the user's
provider, counts only: 7258 channels in "All channels"), release APKs.
Each run starts the app afresh (force-stop, start, 12 s, Back to the
guide, which opens on the resumed channel: a block of dense rows, half-hour
programmes, logos), then one burst: 30 × Down every 0.1 s and 60 × Down
every 0.15 s with `adb shell input` (which itself takes ~0.25 s a key), and
a **held key**: 60 presses ~58 ms apart from one `monkey` script, as a
remote repeats (now in `tools/perf-android.sh`). `dumpsys gfxinfo`
percentiles; CPU from `/proc` over the burst, all of the app's threads
(of which the preview's video, its decoder and player threads, take
35–40 % of a core), and per thread: RenderThread, the NSH engine thread
(Rust, text layout over JNI), the UI thread.

**The display runs at 29.97 Hz.** Media3 asks the preview's surface for the
channel's frame rate (`VIDEO_CHANGE_FRAME_RATE_STRATEGY_ONLY_IF_SEAMLESS`),
and SurfaceFlinger switches the whole display to it: the guide draws at
30 fps (a vsync every 33 ms) while a 30 fps channel plays, at 25 fps for a
25 fps one. The ~40 ms floor of every percentile below (and TiviMate's 44 ms
50th) is that refresh rate. Runs taken with different channels playing, or
in another part of the list, are not comparable; the user's earlier
numbers (90th 57–150 ms, 99th 77–350 ms, 189 % CPU) were not reproduced
with the master build in these conditions (73–113 %), so before and after
alternate below in the same conditions.

Profile (simpleperf on the profileable release APK, held key, master):

- RenderThread: `RenderNode::prepareTreeImpl` a third of its time (1573
  attached views: 24–32 rows rendered, a viewport of overscan either side,
  17 cell slots each, empty ones drawn while transparent), the rest
  issuing draw commands; framestats: sync 8–9 ms and issue 13–17 ms a frame.
- Past programmes a row cannot replay were dimmed by the view's opacity:
  Android draws each through an offscreen layer (`hasOverlappingRendering`):
  64–85 such views in the headless count.
- Engine thread: 41 % text over JNI (`PrecomputedText`, `StaticLayout`):
  a quarter of it fitting the channel name of each row scrolled in (a
  binary search of ~6 measures with two dots), the rest the titles of the
  two rows whose colour changes and the header (four lines of
  description per move).
- An animated logo in a row of the overscan, off screen, kept the guide
  redrawing at ~13 fps (60 × Down bursts: 270 frames in 21 s, 145 elsewhere).

What changed (look and behaviour the same):

1. A cell reads its row's focus (a memo per row): a move re-runs the cells
   of two rows, not of every row (`grid/cell.rs`).
2. An empty slot is invisible, so Android does not draw it.
3. A dimmed past programme gets colours mixed with the window colour (what
   the dimmed group showed on the opaque guide, to the rounding); over
   full-screen video, where the guide is translucent, the opacity stays.
4. The description waits for a held key to pause (blank while keys repeat
   under 160 ms apart, then the programme the keys stopped on); a single
   move shows it at once.
5. Channel names are fitted with a guess from the measured widths (3–4
   measures, the same cut) and kept by name, width and size.
6. The timeline's slide is set only when it changes.
7. NSH (`tv-keys` `755c38cb`, `ScrollBox.Content`): a scroll view's content draws only
   the children within its viewport and a quarter viewport around it, and
   draws again when the scroll moves half that or a child moves. The rows
   of a virtualized list's overscan stay laid out and attached, but the
   render thread no longer walks or records them, and what is off screen
   no longer animates.

| Burst | Master `8aba237` (3 runs) | This branch (6 runs, two rounds) |
| --- | --- | --- |
| 30 × Down, frame 50th / 90th / 99th | 44 / 53–57 / 65–77 ms | 38–40 / 42–48 / 48–65 ms (one 89) |
| 30 × Down, app CPU (RenderThread) | 74–77 % (14–15 %) | 69–72 % (10–11 %) |
| 60 × Down, frame 50th / 90th / 99th | 42 / 53–57 / 61–81 ms | 38–40 / 46–48 / 53–69 ms |
| 60 × Down, app CPU (RenderThread) | 93 % (28–30 %) | 75 % (13–14 %) |
| Held key, frame 50th / 90th / 99th | 48–53 / 65–77 / 85–101 ms | 42–44 / 44–65 / 53–77 ms |
| Held key, app CPU (RenderThread, engine, UI) | 112–113 % (30–31, 24–25, 12 %) | 99–100 % (21–22, 20–21, 12–13 %) |
| Key handler, median | 1.2–1.7 ms | 2.4–3.0 ms |

The NSH change alone (the same app build with and without it, 60 ×
Down): RenderThread 23–24 → 13–14 %, app 87 → 75 %, frames drawn 270 →
122–157 in 21 s. TiviMate on the same box (earlier, its own conditions):
44 / 53 / 77 ms, 116 %.

The key handler's own time rose (its log line times the signal update,
in which the per-row focus memos and the two rows' cell memos now run);
the engine thread's total per burst fell. Janky frames (gfxinfo's count against its
deadline) stay 40–50 % for the `adb` bursts in both: at 29.97 Hz each key
lands between vsyncs.

Before the merge of master (master `09d08a6` against items 1–4 and 6,
another channel playing): held key 50th / 90th / 99th 42 / 57–61 / 69–85 →
42–57 / 44–69 / 53–73 ms, RenderThread 26–27 → 18–20 %, engine 24–25 →
18–22 %.

`tests/guide_scroll_cost.rs` (headless, 300 synthetic channels): a Down
press lays out 31 texts and mounts 35 styles and 16–21 frames (unchanged:
NSH mounts only what differs), the guide has 1121 views drawn instead of
1719 and none through a layer instead of 64–85; it fails on layers, on
empty slots drawn, or on a press mounting or laying out much more.

Open: the 29.97 Hz display while a 30 fps channel plays in the preview
(the frame-rate strategy belongs in `PlayerComponent.kt`: off in guide
mode, matching only in full screen; not changed here, the file was being
changed for picture in picture). Text whose colour alone changes is
typeset again on Android (NSH's Android engine does not draw colours from
the mounted text, as the desktop and Apple engines do): the two rows'
titles on every move, half of the 31 texts a press lays out, roughly a
tenth of the engine thread. The NSH list keeps a viewport of rows bound
either side (`OVERSCAN`), now only bound, not drawn. The box goes to its
screensaver after 10 minutes in the guide with the preview playing (no
keep-awake there).

## Findings

1. **Guide import peak memory** (≈ 570 MB for 2.45 M programmes on the
   emulator before): the import builds no guide in memory any more
   (desktop: +303 MiB → +84 MiB, and 30 % faster; see above). The window
   read back now builds its slots straight from the store's settled rows
   (7 days: +82 → +58 MiB on the desktop), and the app holds only the
   last day (emulator steady RSS 394 → 319 MB, PSS 277 → 202 MB; guide
   window section). Re-measure on a TV box with `tools/perf-android.sh`.
2. **Guide frame times** on the TV box: 50th 38–44 ms, 90th 42–65 ms, 99th
   48–73 ms while a 30 fps channel plays in the preview (display at
   29.97 Hz; guide scrolling section). The render thread and text layout
   over JNI were the cost, not key handling.
3. Cold start and zap are within budget on the emulator.
