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

## 2026-11-10, iPad, the first build (branch `ipad-1`)

The app on NSH's UIKit backend with MPVKit 1.0.0's libmpv (LGPL static
xcframeworks), built on an Apple M4 Mac with Xcode 27.2.

| | Size |
| --- | --- |
| Device `.app`, signed (development) | 69 MB |
| Its executable (libmpv, FFmpeg, libass, libplacebo, MoltenVK, ... linked in) | 72.4 MB; 58.7 MB stripped |
| Unsigned IPA (zip) | ≈ 26 MB |
| Simulator executable | 72.6 MB |

Drawing: mpv's render API into OpenGL ES 3 cost 117 ms a full-screen
frame in the iPad simulator (iPad A16, iOS 27.2 runtime) with mpv's
default scalers, so video crawled; `profile=fast` brought it to 44 ms.
The simulator's GPU is emulated, so these are no device numbers; on the
iPad (10th generation, iPadOS 26.2) the app launched and drew the
wizard, and video playback there is still to be timed. Background and
foreground (simulator, a live HLS channel playing full screen): the
stream stopped when the app left, and on return reopened, its last frame
kept under "Buffering…" until it played in real time again.

## Findings

1. **Guide import peak memory** (≈ 570 MB for 2.45 M programmes on the
   emulator before): the import builds no guide in memory any more
   (desktop: +303 MiB → +84 MiB, and 30 % faster; see above). The window
   read back now builds its slots straight from the store's settled rows
   (7 days: +82 → +58 MiB on the desktop), and the app holds only the
   last day (emulator steady RSS 394 → 319 MB, PSS 277 → 202 MB; guide
   window section). Re-measure on a TV box with `tools/perf-android.sh`.
2. **Guide frame times** exceed the budget on the emulator under load; key
   handling itself is sub-millisecond, so the cost is layout/draw of the grid.
   Needs a real-device run (gfxinfo) before optimising.
3. Cold start and zap are within budget on the emulator.
