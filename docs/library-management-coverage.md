# Library management: feature coverage

Scope: `feat/library-management`, including the four features in
[the implementation report](library-management-implementation.md). The
comparison base is `12dc1a3`, before the feature implementation. This is a
feature-scoped Linux measurement, separate from the other agent's work on
overall repository coverage. Other worktrees' source files were untouched;
profiles and reports were isolated and baseline reports were not overwritten.

## Measured production line coverage

| Scope | Covered / executable lines | Coverage | Local target |
| --- | --- | --- | --- |
| Six new production modules | 819/866 | 94.6% | 90% |
| New core, navigation and storage modules | 364/367 | 99.2% | 95% |
| Three new app modules | 455/499 | 91.2% | 90% |
| Added/changed executable source lines versus base | 1345/1485 | 90.6% | 90% |

All four feature-specific targets passed. These do not change the global
coverage policy being worked on by the other agent.

| New module | Covered / executable lines | Coverage |
| --- | --- | --- |
| `crates/iptv-core/src/organization.rs` | 79/79 | 100.0% |
| `crates/iptv-nav/src/remap.rs` | 189/191 | 99.0% |
| `crates/iptv-store-sqlite/src/library_preferences.rs` | 96/97 | 99.0% |
| `crates/iptv-app/src/data/organization.rs` | 38/43 | 88.4% |
| `crates/iptv-app/src/organization_view.rs` | 218/241 | 90.5% |
| `crates/iptv-app/src/remote_editor.rs` | 199/215 | 92.6% |

Method: Rust `instrument-coverage` and LLVM LCOV export, using the same
production-source exclusions as the existing coverage audit. Integration
fixtures, examples, dependencies and inline `#[cfg(test)]` items are excluded.
Changed-line coverage intersects LCOV executable lines with the Git diff;
blank lines, declarations without executable coverage and deleted lines
are not a denominator. This is line coverage, not a branch-coverage claim.

## Behavior covered

- Identity-based editor navigation, boundaries, duplicate names, selection
  toggling, deletion/reorder recovery, and cancellation of drafts/sheets.
- Channel and group rename, empty-name reset, explicit provider-name reset,
  Cancel and Back; movement earlier/later/first/last and provider-order reset.
- Bulk selected/all hide/show, recovery after every channel/group is hidden,
  source isolation, favorite/name retention, missing/returning provider groups,
  empty edits, deletion cascades and removal of empty channel overlays.
- Stable guide/playback keys after customization and provider refresh,
  newly added channels without preferences, hidden-channel zapping, and
  filtering hidden channel/programme search results before result limits.
- Remote token validation, protected editing/Back paths, context/key/press
  conflict identity, short/hold persistence, key cycling/capture, capture-Back,
  replacement acceptance/cancellation, clear/reset confirmation, and a newly
  saved key actually opening Search immediately without restarting the app.
- Cached startup with playlist updates off, background startup update of an
  enabled source, disabled-source exclusion, the real 60-second foreground
  scheduler, failed update cache retention, manual refresh of a disabled
  source, and fallback after disabling the displayed source.
- Independent EPG loading while playlist refresh is off; replacing the EPG
  source with an unavailable feed records failure and retains cached programmes
  without recording a playlist-import failure.

## Regression fixed

A provider refresh could remove the channel being renamed while a name
sheet was open. The editor moved focus to a neighboring identity but kept
its old draft, allowing that draft to rename the neighbor. Reload now cancels
both the actions and the name draft when their focused identity disappears.
The new regression failed before this fix and passes afterward.

## Validation and reproduction

The scoped instrumented run completed **510 tests, 0 failures, 1 ignored tool**,
across 13 test executables. The WebAssembly core build and native app build
also pass. The expanded timer test intentionally takes about one minute.
Tests wait for refreshed UI content as well as the database commit before
interacting with management, avoiding a store/UI synchronization race in
the test harness.

Run from the feature worktree:

```sh
CARGO_BUILD_JOBS=1 cargo llvm-cov -j 1 -p iptv-core -p iptv-guide -p iptv-nav -p iptv-store-sqlite -p iptv-search -p iptv-app --lib --test library_preferences --test migrations --test library_management --test library_refresh --test library_cached_epg --test tv_player --test type_drift --lcov --output-path target/library-management.lcov
```

(That run predates one test binary per crate: those integration tests are
now modules of `iptv-store-sqlite`'s `--test it` and `iptv-app`'s
`--test app`.)

Raw reports, logs, reproduction/filter scripts and threshold results are
under `~/analysis/iptv-rs-parity/2026-11-11/library-management/coverage/`:
`coverage.lcov`, `summary.json`, `results.json`, `gates.json`, `tests.log`,
`run.py` and `analyze.py`. Native links used one worker and omitted DWARF
debug sections in coverage-only executables to reduce contention; LLVM
coverage mappings are retained. Standard native builds retain their normal
build configuration.

Remaining gaps primarily cover defensive closed-editor/error paths and
native input/platform behavior. Android hold/release handling and actual
device focus/IME behavior still need an available device; no emulator or USB
tablet was driven. Synthetic data only; no provider details or reference
assets are added.
