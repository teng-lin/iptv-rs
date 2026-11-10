# Plan: TiviMate-parity TV/tablet IPTV app on Rust + NSH

Date: 2026-11-09. Inputs: the TiviMate 5.3.3 specification in
`~/src/tivimate/docs/01–05` (evidence codes D/C/I/P/U are theirs), the D-pad
spike in this repository, and IPTVnator's provider/EPG contracts
(`~/src/iptvnator/docs/architecture/`).

## 1. Goal and boundaries

**Goal.** One Android APK (TV and tablet) plus desktop builds that reproduce
TiviMate's user experience: information architecture, screens, remote and
touch interaction, focus/selection/playing states, settings and workflows.

**Parity means behaviour, not artwork.**

| In scope now | Placeholder now, replaced before public release | Never |
| --- | --- | --- |
| Screens, flows, regions, state model, remote vocabulary, settings catalogue, timings once measured | Colour palettes and selector rules (`src/theme.rs`), layout proportions copied from measurements, label wording | Copying APK artwork, icons, fonts, layouts files, code or brand; the name "TiviMate" in the product |

Icons come from Material Symbols (Apache-2.0); fonts are the platform's.
Phase 9 is the release gate that replaces every placeholder.

**Test data.** Synthetic only: generated M3U/XMLTV, local mock Xtream and
Stalker servers. No real provider URLs, names or credentials anywhere.

## 2. What the spike established (2026-11-09)

| Finding | Evidence | Consequence |
| --- | --- | --- |
| NSH Android gives ordinary views no D-pad focus; `focus()` only works on inputs | `Mounter.java:2185`, `NshView` holds focus | App-owned focus model (`guide.rs`) is the architecture, not a workaround |
| Arrow keys reach `on_key`; 200-key bursts drop nothing; handler p50 18–27 µs | `results/stock`, `results/patched-tablet` | Rust-side focus is fast enough; frame cost is the budget to watch |
| Remote OK (`DPAD_CENTER`) is unmapped in stock NSH | stock 0/1, patched 3/3 | NSH patch on branch `tv-dpad-spike` (keymap.rs), upstream it |
| Media, channel, Guide, Info, Menu, colour keys are unmapped | `Key::from_android_keycode` | Needs new `Key` variants upstream (Phase 0) |
| No key-up events reach the app | `keymap.rs` `keyUp` only consumes | Long-press (Long OK/Back/Left…) is impossible without an NSH change (Phase 0) |
| Built-in `Video` uses `MediaPlayer`; no headers/tracks | NSH docs, `VideoBox.java` | Player is a native component (Media3 on Android, libmpv on desktop) |
| Touch and keys coexist: touch hides the focus fill, first key adopts a visible row | spike screenshots `results/rework` | Keep "input mode" as a first-class state |
| Android swallows the first D-pad key when leaving touch mode | tablet run | Expected platform behaviour; document, don't fight |

## 3. Decisions (2026-11-09)

| Decision | Choice |
| --- | --- |
| UI language | Rust only (no TSX screens) |
| First targets | Android TV and Android tablet, one APK |
| Later targets | Desktop (Windows/Linux/macOS), iOS/iPadOS, tvOS, web/smart TVs; the core stays portable from day one |
| NSH changes | Patches on our fork branch (`tv-dpad-spike` and successors); upstream PR filed later |
| tvOS | NSH has no tvOS; when it is scheduled, either a thin native shell over the core's view models or NSH tvOS support. Not decided now |

## 4. Architecture

### Principles

1. **Headless app core.** Everything except drawing lives in pure-Rust crates
   that do not depend on NSH. The core exposes view models (screen snapshots:
   guide window, panels, focus, hints) and one `dispatch(Action)` entry point.
   NSH screens only render view models and turn input into actions.
2. **Unidirectional flow.** Input → `Action` → state machines → effects
   (I/O, timers, player) → state → signals → views. Focused, selected,
   playing and being-edited are explicit fields; the delayed channel switch is
   a cancelable timer effect.
3. **App-owned focus with a host-driven mode.** On NSH our engine moves focus.
   A future shell with its own focus system (tvOS) reports focus changes to the
   core instead; the core always owns action meaning.
4. **One persistent video layer.** A single native player surface sits under
   the UI; preview, overlay and full screen are layout changes, never decoder
   restarts. Multiview is N surfaces plus an active-surface field.
5. **Capability-gated features.** Each platform declares capabilities
   (TS/HLS/DASH, headers, tracks, AFR, PiP, multiview, background recording,
   alarms, SMB, file writes). Features check them in their state machine and
   explain absence; unsupported never looks like empty (AC11).
6. **Form factor, not OS, chooses layout.** TV (10-foot, D-pad) and tablet
   (touch + optional keys) share the guide; phone and desktop layouts come
   later from the same view models.

### Workspace

```
core/        no NSH, wasm-safe (CI builds wasm32-unknown-unknown)
  domain       entities, stable content ids, sort/override rules
  providers    M3U/M3U8, Xtream, Stalker, catch-up URL builders
  epg          streaming XMLTV, matching, priority/offsets, retention
  nav          focus owner stack, regions, input mode, key→action layers
               (user overrides → context defaults → global), long press
  player-ctl   player state machine (idle/loading/playing/buffering/error)
  settings     one schema table → storage, backup, settings UI, validation
  viewmodels   screen snapshots + dispatch(Action); spawner trait for async
adapters/    one per backend, behind core traits
  store-sqlite (rusqlite, WAL, forward-only migrations)   [store-web later]
  net-native   (reqwest)                                   [net-web later]
  secrets-android (Keystore)                               [others later]
shells/
  nsh-app      Rust NSH screens: TV + tablet now, desktop/iOS/web later
hosts/
  android      thin Kotlin: NshView, Media3 player component, Leanback
               manifest/banner, PiP, permissions, SAF/SMB, alarms
```

### Data

- Provider tables are replaced on refresh; user customisations (names,
  favourites, hidden, blocked, order, custom groups) live in overlay tables
  keyed by stable channel ids and are merged at read time.
- Stable id: provider + stream id, or a hash of URL + name for M3U, decided
  once in `providers`.
- EPG rows are compact (channel, start, end, title, flags) indexed on
  (channel, start) with FTS5 for search. The guide queries visible rows × time
  window, prefetches neighbours, and keeps an LRU of pages.
- Migrations are tested against real historical schemas; users skip versions.

### Threading

The NSH UI thread only reads signals. Imports, EPG parsing, database writes and
HTTP run on a background runtime behind a spawner trait (tokio on native).
Results return as actions, applied in batches per frame.

### Testing

Unit tests for every state machine on the host; synthetic fixtures and local
mock Xtream/Stalker servers; `nsh-testing` headless layout tests; device tests
with `nsh test --target android` on `tv36` and `tab36`; key-burst/frame-stat
budgets set on one real low-end TV box. Rules carried from the spike: restore
focus by content identity, never by index (AC13); no business logic in views.

## 5. Phases

Each phase ends with running builds on the TV and tablet emulators, unit and
interaction tests green, and screenshots in `results/<phase>/`.

### Phase 0 — Platform foundations (NSH and host)

1. Upstream the `DPAD_CENTER`/gamepad-A mapping (branch `tv-dpad-spike`).
2. NSH: add `Key` variants for MediaPlayPause, Play, Pause, Stop, Rewind,
   FastForward, Record, ChannelUp/Down, Guide, Info, Menu, colour keys
   (ProgRed/Green/Yellow/Blue), Mute, VolumeUp/Down, Back-as-key.
3. NSH: deliver key-up (or long-press) to bindings so the app can implement
   long press with a configurable threshold and suppress the short action.
4. Android host: Leanback launcher intent, banner, `touchscreen required=false`,
   `leanback required=false`, landscape on TV / any on tablet, PiP declaration.
5. **Video-layer spike first (highest risk):** a native Media3 surface under
   NSH views with transparent/tinted UI above it, on the TV emulator and one
   real TV box. If NSH cannot layer this way, revisit principle 4 before any
   screen work.
6. Player native component v0: Media3 ExoPlayer with HLS/TS/DASH, custom
   headers/User-Agent, track lists, error codes.
7. Workspace skeleton per section 4, with CI building `core/` for
   `wasm32-unknown-unknown` on every change.
8. Desktop libmpv component: deferred until after Phase 4 (desktop is a later
   target), but `cargo run` on Linux stays green for fast iteration.

Exit: OK/Long OK/Menu/Guide/Ch± reach the app on TV; a synthetic HLS stream
plays under the UI layer on both emulators; `core/` builds for wasm.

### Phase 1 — Focus engine and input layer (`nav/`)

- Regions with an explicit focus owner: groups, channel list, guide grid,
  time bar, info/control panels, dialogs, settings.
- Input mode (keys/touch), touch-to-focus, first-key adoption of visible rows.
- Key → action table with per-context defaults and user overrides
  (spec "Assignable actions": watch, prev/next/recent channel, info/control
  panels, groups, guide, menu, description, page scroll, previous/next
  programmes, return to current, restart, live, stop, play/pause, external
  player, volume, back, exit, ignore).
- Long-press layer and repeat policy (hold to scroll; edge clamp, no wrap until
  measured).

Exit: AC05 (visible focus on bright/dark), AC12 (long titles/missing EPG keep
navigation), AC13 (cancel restores parent focus).

### Phase 2 — Data: providers, storage, EPG

- M3U/M3U8 (URL, file, clipboard), Xtream, Stalker; port behaviour notes from
  IPTVnator's compatibility docs, rewritten in Rust.
- SQLite store, customisations (names, favourites, hidden, blocked, order,
  custom groups) separate from provider data so refresh preserves them.
- XMLTV: streaming parse, source priority, assignments, offsets, retention
  (past days, store descriptions), update intervals/triggers, full scan.
- Performance target: 50k channels, 7 days EPG on a low-end TV box without UI
  stalls (measure; set a budget after the first run).

Exit: AC07 (global vs per-group/per-playlist sort), AC11 (failures never look
like empty content).

### Phase 3 — First run and playlist wizard

Empty state ("add your own playlist", no content provided) → provider type →
parameters/URL/file/paste → processing → result → name and optional EPG URL.
Retain input on failure, block duplicate submits, skippable EPG.

Exit: AC01, AC02.

### Phase 4 — Live TV core: guide, channel list, player

- TV guide: groups, channel rows, programme grid across the full width,
  time bar, now-line, past dimming, current highlight/progress-only options,
  date headers (Yesterday/Today/Tomorrow), Long Left into the past, Back to
  current programmes, future "can't play".
- Channel list mode, preview mode and overlay mode, each over live video.
- Two-stage commitment: browse → (optional delay, cancelable) → play →
  full screen; stay-on-guide/list/search policies.
- Player: info panel, control panel, both, neither; timeouts and transparency;
  history/recent/previous channel; black frame option; live label; buffering
  vs error states; tracks, captions, display modes, sleep timer, external
  player fallback.
- Programme details: watch / from start / reminder / record per availability.

Exit: AC03, AC04; journey benchmarks for guide open, focus move and channel
switch on the TV emulator and one real box.

### Phase 5 — Catch-up, seeking, search

- Catch-up/time-shift per provider type; watch from start, restart current,
  go to live; seek schemes (Left/Down/RW, directional pairs, RW/FF/Pause);
  time-bar and RW/FF steps; start-time offset.
- Search: channels/programs, history, voice intent, favourites first, include
  past without catch-up, stay on search, keep query and focus after playback.

### Phase 6 — Organisation and editing modes

Rename/restore, favourites, hide/show, block/unblock, reorder, reset
positions, custom groups, copy/move; bulk select (OK toggle, Left options,
Right copy/move), status mode (OK change, Left reset all, Right set all),
reorder mode (Up/Down move, Left/Right top/bottom); mode banner with scope,
selection count and hints; confirmations that state consequences.

Exit: AC06, AC08.

### Phase 7 — VOD, My list, recordings, reminders, multiview, PiP

- Movies/Shows categories, details, trailers (failure separate from Play),
  seasons/episodes, resume/restart, next-episode autoplay, My list (movies,
  shows, TV programmes), truncated-catalogue guidance.
- Recordings: one hour / until end / scheduled (date, time, duration, repeat,
  padding); all/ongoing/scheduled; state-specific actions; storage incl. SMB.
- Reminders: minutes before, default action, popup timeout, wake, permission
  guidance (exact alarms, overlay).
- Multiview: add/remove/enlarge/reduce/full screen, active surface and audio,
  per-surface failure; connection-limit warning. PiP on Home.

Exit: AC09.

### Phase 8 — Settings, parental controls, backup, appearance

- Settings tree from `03-settings-reference.md` (≈250 rows): general/startup,
  playlists, groups/channels, EPG, appearance, guide/list presentation, logos,
  clock/player info, playback compatibility (decoders, passthrough, AFR,
  tunneling, buffer), catch-up, history, search, recordings/reminders,
  parental, remote, VOD, storage/account.
- Guided TV forms for long values (URLs, paths); restart-required markers.
- Parental: PIN create/change, require-PIN choices, unlock durations,
  five-failure cooldown, TV/VOD enablement.
- Backup/restore as a replacement transaction with restart; local, link, SMB.
- Appearance: accent/background/selection-style choice, font size, two-line
  names, numbers/logos toggles, panel transparency.

Exit: AC10, AC14 (if any premium tier exists in our product).

### Phase 9 — Originalisation gate (before any public release)

- Replace `theme.rs` palettes and selector rules with our own design system;
  re-derive spacing/type scale rather than reusing measured values.
- Review every label against the APK string table; rewrite copied wording.
- Confirm no APK-derived file is in the repository (CI check: hash list of
  extracted resources must not match any tracked file).
- Product name, icon, banner and store listing are ours.
- Legal review of the remaining similarity (trade dress) before launch.

## 6. Open questions that need reverse-engineering or runtime capture

Delegated to the `codex_rea` pane (REA with IDA). Static first; runtime only
with synthetic playlists on a separate emulator.

| # | Question | Needed by |
| --- | --- | --- |
| R1 | Default values of the settings in `03-settings-reference.md` (switch delay, panel timeout, autoplay, stay-on-*, history counts, guide time span) from preference defaults in code | Phase 4/8 |
| R2 | Long-press threshold and whether short actions fire before long ones; key repeat handling | Phase 1 |
| R3 | Default key → action map per context (player, guide, list, groups, search, VOD) | Phase 1 |
| R4 | Guide geometry: visible hours, row height, channel column width, programme minimum width, scroll/page steps; decode the protected layouts if possible | Phase 4 |
| R5 | Preview vs overlay: which surfaces overlay live video and with what alpha | Phase 4 |
| R6 | Catch-up URL construction per provider type (behaviour only) | Phase 5 |
| R7 | Multiview maximum screens and layout patterns | Phase 7 |
| R8 | Runtime capture with synthetic M3U/XMLTV on an Android TV emulator: screenshots of guide, list, player panels, settings tree, and timing of focus/switch/panel transitions | Phases 4–8 |

### Status (2026-11-09)

- **R1–R7 static pass** (`~/src/tivimate/docs/06-defaults-and-input.md`):
  numeric defaults, long-press policy, full keymaps, guide geometry,
  catch-up URL formulas and the multiview cap were **not recoverable**
  statically (protected code; 92 layouts still undecodable).
- **R8 partial runtime pass** (`docs/07-runtime-tv-observations.md`, one
  unactivated install on `tv36`, one configuration — not constants):
  - Initial displayed settings: font Medium, theme Dark/Blue, buffer Small,
    hardware audio/video, AFR off.
  - Preview **resizes the real SurfaceView** (≈640×360 px at the top left);
    it is not full-screen video behind the guide. Matches our single-surface
    layout-only design.
  - OK sequence: guide OK starts preview → second OK full screen → OK in
    full screen opens panels. The playing marker stays on the playing
    channel while focus moves.
  - Observed guide: ≈39 dp row pitch, half-hour columns, seven full rows.
  - Multiview beyond one screen is gated; cap and layouts remain unknown.
- Still unknown: timing/count defaults, context keymaps, long-press
  threshold, overlay alpha, catch-up URLs. Pick our own values, make them
  settings, and revisit if a later runtime pass measures them.
- Raw captures and provider data live outside all repos
  (`~/analysis/tivimate-runtime/`); never copy them into this project.

## 7. Validation

- Unit tests for every state machine (focus, commitment policy, edit modes,
  PIN grants, recording states, wizard).
- Device interaction tests with `nsh test --target android` (KeyEvents and
  MotionEvents), run on `tv36` (Android TV 36) and `tab36` (Pixel Tablet 36).
- Key-burst and frame-stat harness (`tools/burst.sh`) per journey; budgets set
  from a real low-end TV box, emulator numbers are upper bounds only.
- Acceptance criteria AC01–AC14 tracked as a checklist with evidence links.
- Visual review in both input modes, bright/dark video, smallest/largest fonts,
  long titles, missing logos/EPG.

## 8. Risks

| Risk | Mitigation |
| --- | --- |
| NSH is young (v0.4); upstream may not accept TV keys/long press quickly | Keep patches small on a fork branch; isolate input in `nav/` |
| Native video surface composition with NSH views on TV hardware | Phase 0 spike on a real box before building Phase 4 |
| 50k-channel guide performance on weak TV SoCs | Virtualised rows only, memoised per-row data, budgets from real hardware |
| Too-close imitation | Phase 9 gate, no APK files in repo, legal review |
| Provider diversity (Xtream/Stalker quirks) | Reuse IPTVnator's compatibility knowledge; mock servers in tests |

## 9. Next three steps

1. Video-layer spike (Phase 0 item 5): Media3 surface under transparent NSH
   views on `tv36`, then on a real TV box.
2. NSH TV keys and key-up delivery (Phase 0 items 2–3) on the fork branch.
3. Workspace skeleton (section 4) with wasm CI for `core/`; move the spike's
   focus model into `core/nav` with region ownership and the key → action
   table. Fold in `codex_rea` R1–R7 answers (sent 2026-11-09) when they land.
