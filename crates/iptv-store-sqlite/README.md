# iptv-store-sqlite

Native persistent store of iptv-rs: sources, the provider catalog and the
user's customisations, in one SQLite file. Adapter crate (not wasm): uses
`rusqlite` with bundled SQLite, so the same code builds for Android through
the NDK. A web store is a later, separate adapter.

## Rules

- **Provider data is replaceable, user data is not.** `replace_catalog`
  swaps a source's groups, channels, VOD and series in one transaction.
  Overlay tables (favourites, hidden, blocked, names, numbers, EPG override,
  manual order, custom groups, history) are keyed by the stable ids from
  `iptv-providers` (`(source, ContentId)`, see `iptv_providers::id`) and are
  never touched by a refresh. Reads merge them.
- **Orphans are kept.** An overlay whose channel is not in the current
  catalog stays, is listed by `orphaned_overlays()`, and applies again when
  the id returns (its place in favourites, manual order and custom groups
  included). `purge_orphaned_overlays()` deletes them on request.
- **Forward-only migrations.** `migrations/000N_*.sql`, applied at open in
  one `BEGIN IMMEDIATE` transaction, recorded in `schema_migrations`, safe on
  every start. A file from a newer build is refused (`SchemaTooNew`). Never
  edit a shipped migration. Each version's exact DDL is kept as
  `tests/fixtures/schema_vN.sql` with synthetic data `data_vN.sql`;
  `tests/migrations.rs` builds a file at every historical version, opens it
  and checks the data through the API (users skip releases). The steps for
  adding a migration are in that file.
- **Secrets.** Source settings without secrets are JSON in `sources.config`;
  passwords, MACs, serials and playlist URLs are rows in
  `source_credentials`. They are stored as plain text for now:
  **encrypting them with the Android Keystore is a later step**
  (`secrets-android` adapter in the plan). Stream URLs and headers in the
  catalog are provider data and may embed credentials too, so treat the
  whole file as sensitive. `Debug` of `ChannelRow` leaves the stream out;
  `StoredSource` uses the redacting types of `iptv-providers`; errors name
  columns, never values; import failures store only an error kind.
- **Guide data is provider data.** `replace_epg` swaps a playlist source's
  whole stored guide (feeds, bound EPG channels, bindings, programmes,
  descriptions) in one transaction; a failed download (`record_epg_failure`)
  keeps the stored guide. Only EPG channels a playlist channel is bound to
  are stored; channels sharing one EPG channel share its rows. Feed URLs
  can embed credentials (Xtream `xmltv.php`) and are stored like the
  catalog's stream URLs (sensitive file, `SecretUrl` in reads).

## Guide data (EPG)

- `replace_epg(source, &GuideStore, &[EpgFeedMeta], match_fingerprint)`:
  after `iptv_import::import_epg`; `EpgFeedMeta::from_import` builds the
  feed rows (settings the feed was fetched with, fetch time, bytes; ETag /
  Last-Modified columns exist but the network layer does not expose them
  yet).
- `load_epg(source, &EpgWindow)`: programmes overlapping `[from, to)` for
  all or some playlist channel keys, into a `GuideStore` bound by those
  keys, normally without descriptions; `epg_description(source, key,
  start)` reads one on demand (`EpgSchedule::with_description_lookup`).
- `epg_guide(source)`: `StoredGuide` with `refresh_state()` for the refresh
  policy (`iptv_import::decide` / `plan_startup`) and `configs()` to refresh
  without downloading the playlist.
- `prune_epg(before)`, `delete_epg(source)`; deleting a source deletes its
  guide.

The startup and background-refresh wiring for the app is in the module
docs of `src/epg.rs`. Memory (`tests/epg_memory.rs`, release, Rust heap):

| Guide | In memory, as imported | Stored, loaded from now − 2 d, no descriptions |
| --- | --- | --- |
| 1000 ch, 82.5k programmes | 7.5 MiB (11.5 before compaction) | 2.0 MiB |
| 5000 ch × 9 d, 1.23M programmes | 97.6 MiB (159.4 before) | 11.9 MiB (22.3 MiB for all 9 days) |

SQLite's own heap comes on top (page cache, up to `cache_size` = 16 MiB
per connection). Replacing 1.23M programmes takes ~3.5 s (file 160 MiB
with descriptions); loading the window ~0.25 s on a desktop CPU.

## Schema (v4)

| Table | Kind | Key | Notes |
| --- | --- | --- | --- |
| `schema_migrations`, `store_meta` | bookkeeping | version / key | |
| `sources` | source | id | kind, name, config JSON, position |
| `source_credentials` | source | (source, name) | secrets |
| `source_imports` | source | source | last attempt/success, counts, last error kind |
| `groups` | provider | (source, id) | kind, provider order |
| `channels` | provider | rowid, unique (source, id) | stream/catch-up JSON, provider order |
| `channel_groups`, `vod_groups`, `series_groups` | provider | (source, group, item) | membership |
| `vod`, `series` | provider | (source, id) | basics |
| `channel_overlays` | overlay | (source, channel) | custom name/number, hidden, blocked, EPG override |
| `favourites` | overlay | (source, channel) | ordered |
| `manual_order` | overlay | (source, scope, channel) | scope `''` = all channels, else a group id |
| `custom_groups`, `custom_group_members` | overlay | id / (group, channel) | ordered |
| `watch_history` | overlay | id | per play; newest 2000 per source kept |
| `channel_fts` | derived | rowid = `channels.rowid` | FTS5 over displayed + provider name; rebuilt at open when missing |
| `epg_guides` | guide | source | replaced/attempted at, last error kind, match fingerprint, bound count, latest end |
| `epg_feeds` | guide | (source, feed) | URL and import settings, fetched at, ETag/Last-Modified, bytes, counts |
| `epg_channels` | guide | id, unique (source, feed, EPG channel) | bound EPG channels only |
| `epg_bindings` | guide | (source, channel key) | playlist channel → EPG channel, match kind |
| `epg_programmes` | guide | (channel, start) | times, title, sub-title, category, icon, rating, catch-up id, season/episode, flags; index on stop |
| `epg_descriptions` | guide | (channel, start) | apart from the grid rows, read on demand |

## Concurrency

One `Store` is one connection (`Send`, not `Sync`). Use one writer store
(on the background runtime) and reader stores as needed on the same file.
WAL lets readers keep reading the last committed catalog while a refresh is
being written; a second writer waits up to `busy_timeout` and then gets a
busy error.

## Performance

`cargo test --release -p iptv-store-sqlite --test perf -- --ignored --nocapture`
replaces a synthetic 50k-channel catalog in a file-backed store and times
the common reads. On a desktop CPU (2026-11): replace 0.5–0.9 s (fresh and
refresh), first page of 50 under 1 ms, visible count ~14 ms, a group with
manual order ~2 ms, FTS5 search ~10 ms. The asserted budget is 3 s per
replace. A low-end TV box is several times slower; measure there before
setting the Phase 2 budget.

## Android

```sh
NDK=$HOME/Android/Sdk/ndk/30.0.16248370/toolchains/llvm/prebuilt/linux-x86_64/bin
CC_x86_64_linux_android=$NDK/x86_64-linux-android26-clang \
AR_x86_64_linux_android=$NDK/llvm-ar \
cargo check -p iptv-store-sqlite --target x86_64-linux-android
```

Use `aarch64-linux-android26-clang` and the `aarch64` variables for arm64.
