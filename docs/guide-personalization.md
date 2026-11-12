# Guide behavior and personalized lists

Branch: `feat/guide-personalization`, based on `2d60f6e` (the merged library
management work and settings-editor cleanup). This implements the next four
priorities without adding multiview, backup, or parental-control features.

## Browsing and playback

Appearance settings now apply the guide and channel-list browsing layouts,
autoplay, switch delay, fullscreen inactivity timeout, and stay-open policies.
The persistent player surface keeps its identity; layout changes only its
rectangle. Existing search-return behavior remains available.

| Setting | Default | Behavior |
| --- | --- | --- |
| Guide layout | Preview | Video beside the guide, or full-screen video under the translucent guide |
| Channel-list layout | Overlay | Normal channel-list entry uses this choice; explicit preview entry still opens a preview |
| Autoplay | Off | A focused live programme/channel tunes after the switch delay; past/future cells do not trigger live autoplay |
| Switch delay | 500 ms | 0–5000 ms in 250 ms steps; applies to browsing choices and channel zapping |
| Fullscreen timeout | Never | 0–60 seconds; runs for a playing, focused preview; input restarts it |
| Stay on guide, preview | Off | An explicit live choice goes full screen after tuning unless enabled |
| Stay on guide, overlay | On | Independent policy for the guide over video |
| Stay on channel list | Off | An explicit choice closes the list unless enabled |

These are the existing iptv-rs catalogue defaults, not newly claimed
reference-application defaults. An explicit OK on the already playing live
channel goes full screen immediately. Automatic preview keeps browsing open;
the inactivity timeout can subsequently open full screen. Choosing catch-up
retains the existing replay behavior.

Only the latest pending channel is tuned. Moving away, opening a blocking
screen/menu, Back, changing playlist or choosing another playback action
invalidates earlier pending work. Timer callbacks revalidate the current
identity and context. Repeated channel zaps advance from the pending target,
so a burst can skip intermediate streams. Settings changes take effect without
restart. Returning from playback respects the configured guide layout.

## Favorites and group-specific ordering

In a guide list, open the programme menu and choose Reorder channels. The
manager uses the current list's ordering scope:

- Favorites: changes favorite order, retaining favorite membership.
- Provider group: changes only that group's channel order.
- Custom group: changes only its member order.
- All channels (and the recent-list fallback): changes playlist-wide order.

Move earlier/later/first/last applies immediately. Restore provider order is
available. Orders use stable source/channel/group identity and survive
provider refresh and restart. Missing channels keep their stored positions
for a future return. Renames and visibility remain independent overlays.
Favorite membership changes and reorder writes use the same ordered writer
queue. Reopening the editor uses its current in-memory favorite order.

## Custom groups and bulk membership

Open Playlists → a source → Custom groups, or the guide menu's New group /
Copy channels actions. Empty custom groups are visible in the guide.

Create or rename a group with a nonempty trimmed name. Open its details to
edit members, reorder members, rename, or delete. Delete asks for confirmation
with Cancel focused. Deletion removes the custom group and its memberships;
all channels, guide records and imported provider groups are retained.

Browse all channels to populate groups. Search matches all typed words against channel-name word prefixes,
case-insensitively. Rows can be toggled individually; Select filtered adds all
matches to the selection; Clear selection removes the selection. The screen
shows total selected, matching and source-list counts. Filtering preserves
selections outside the current result set.

| Operation | Membership semantics |
| --- | --- |
| Copy selected | Append to a custom destination; existing members retain their position; duplicates are ignored |
| Move selected | Available from a custom group; copy to another custom group and remove only the original custom membership |
| Remove selected | Remove membership from the currently edited custom group; retain channels |
| Copy/move cancel | Return to the same filtered list and selection without writing |

Transfers are atomic, source-scoped, and preserve source-list selection order.
Moving to the same group is a no-op. The destination UI excludes the source
group. Missing/foreign selections or destinations reject the whole transfer.
Provider membership is never rewritten by a move. Custom groups reference the
same catalog channels and streams as other lists; they do not duplicate players
or guide data. Focus restoration uses identities, including duplicate group names.

## Guide appearance

Appearance now exposes working text size, guide channel logos, channel numbers,
and visible guide minutes. Medium is the existing size; Small/Large/Extra large
use 0.9/1.15/1.3 times the role's normal text size, independent of responsive
window geometry. Channel-name fitting and number-column measurement use the
chosen text size. Font families and weights are unchanged.

Logos/numbers default on. Turning either off reclaims its column space.
The guide spans 60–360 minutes in 30-minute steps (default 150). Programme
cells, ruler spacing, touch panning and keyboard bounds use the same span.
Changing the span retains the focused programme; scroll steps remain 30 minutes
and hour/day jumps retain their existing meaning. Settings persist in the existing
versioned settings JSON; group/order data uses the existing SQLite tables.

## Validation

The test matrix covers pure timing/cancellation, every supported guide span,
stable identity, filtered selection, atomic/invalid transfers, independent
orders, persistence/reimport, and headless remote/touch workflows. UI checks
inspect actual player source commands, persistent surface geometry, text size,
number/logo widths, and programme-cell width changes. Fixtures are generated;
no provider data, APK code or reference assets are used.

| Check | Result |
| --- | --- |
| Instrumented feature/regression run | 560 passed, 0 failed, 1 existing ignored schema tool; 25 test executables |
| Full core/guide/settings/SQLite suites | 488 passed, 0 failed, 5 existing ignored/tool/doctest cases (overlaps the instrumented run) |
| New production modules | 703/755 executable lines, **93.11%** |
| New pure core modules | 154/154, **100%** |
| New app modules | 549/601, **91.35%** |
| Added executable source lines versus `2d60f6e` | 1187/1289, **92.09%** |
| WASM core and native app builds | Passed |
| Guide scrolling-cost guard | Passed with the existing budgets |
| Clippy review | Completes with existing workspace warnings; `-D warnings` is blocked by the unchanged `organization.rs` nested-if warning |

The feature-local line gates are 90% for new modules, 95% for new pure
modules, 90% for new app modules and 90% for added lines; all pass. These
are separate from the other agent's repository-wide coverage policy. This
is executable-line coverage, not branch coverage: it excludes dependencies,
integration-test sources, and inline `#[cfg(test)]` code. Four new modules
are measured: core browsing policy and selection, app playback adapter and
custom-group UI. Existing production changes are also counted by the added-line
measurement. The suite adds 20 tests, including seven complete UI journeys.

The final playback style cleanup was followed by another passing instrumented
run of playback, selection and zapping. Other scenarios include source-scoped
transfer rejection/rollback, duplicate destination membership, reopen/reimport,
empty groups, duplicate names, menu/search/Back cancellation, active surface
identity, default versus preview/overlay policies, and actual typography/cell geometry.

Builds and tests used one Cargo worker and one linker thread; validation
executables omit DWARF debug output to reduce load on the shared workstation.
No global configuration or NSH source was changed. Measurements are for Linux
and default features. Android/iOS device, native IME and hardware-decoder smoke
tests remain outstanding; no emulator or USB tablet was driven.

Logs and reproducible `coverage/run.py`, `coverage/analyze.py`, LCOV, isolated
profiles, per-target results and gate JSON remain outside the repository:
`~/analysis/iptv-rs-parity/2026-11-11/guide-personalization/`.
