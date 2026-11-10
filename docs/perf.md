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
end) and page older days in on demand when browsing back.

## Findings

1. **Guide import peak memory** (≈ 570 MB for 2.45 M programmes on the
   emulator before): the import builds no guide in memory any more
   (desktop: +303 MiB → +84 MiB, and 30 % faster; see above). Next: the
   window read back (`load_epg`) is now the peak (+41 MiB transient over
   the 38 MiB it keeps): build the slots directly from the store's
   sorted, settled rows. Re-measure on the emulator / a TV box with
   `tools/perf-android.sh`.
2. **Guide frame times** exceed the budget on the emulator under load; key
   handling itself is sub-millisecond, so the cost is layout/draw of the grid.
   Needs a real-device run (gfxinfo) before optimising.
3. Cold start and zap are within budget on the emulator.
