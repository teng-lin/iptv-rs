# Library management implementation

Branch: `feat/library-management`; base: `12dc1a3`.
Worktree: `/home/blackmyth/src/iptv-rs-library-management`.
The [baseline backlog](missing-features-2026-11-11.md) remains the list of
parity gaps; this document describes the implemented scope on this branch.

## Channels and groups

Open Playlists, choose a source, then Organize channels or Organize groups.
The guide menu also offers channel options, hidden-item management, channel
ordering and group options. The manager always includes hidden items.

OK or a tap opens actions for the focused identity: hide/show, rename,
restore provider name, move earlier/later/first/last, restore provider order,
select/deselect, hide/show selected, and hide/show all. Up/Down scrolls the
list or action menu. Back cancels a name draft, closes actions, then returns
to the screen that opened management. Empty names restore provider names.

Channel order is playlist-wide; provider-group lists follow that relative
order. Group names/order/visibility have their own source-scoped overlays.
Independent channel orders within each group and custom-group membership
editing remain follow-ups. Group visibility and individual channel
visibility are separate: showing a channel does not unhide its group.

Visibility filters guide lists, favorites/recent guide lists, channel
choosers that use those lists, channel search and programme search. Search
filters before its result limit, so hidden results cannot crowd out visible
ones. Imported rows and EPG matching keys remain intact. An already playing
hidden channel keeps playing; channel zapping chooses visible channels.
Organization changes preserve guide/group focus by stable identity and do
not rebuild or replace the player surface. Hidden items can be restored
through playlist management even when the guide has no visible items.

## Playlist policy

Source actions offer enablement, periodic refresh (Off, 1/6/12/24 hours), and
refresh on startup. Defaults retain enabled sources and startup refresh;
periodic refresh is Off. A cached source can be opened without downloading
its playlist when startup/interval policy does not request a refresh. EPG
refresh policy still applies independently.

Disabled sources remain manageable, are skipped at startup and by refresh
all/automatic scheduling. A disabled displayed source gives way to another
enabled source; changing sources stops the old source's playback. With none
enabled, the guide is empty and playlist management offers recovery. Manual
refresh of a disabled source remains available and does not enable it.

The foreground scheduler checks every minute, counts failed attempts toward
the interval and deduplicates work per source. Cancellation tickets prevent
old jobs from replacing newer activity; cached data survives failed imports.
This does not install an operating-system background wakeup service.

## Remote editor

Settings → Remote control → Edit remote bindings. Contexts: guide, groups,
player, player controls, channel list and search. Up/Down chooses a row;
Left/Right or a tap changes a value. Choose a physical key, short/hold press,
and a supported action, or capture the next delivered key. Save applies and
persists immediately. The screen shows the effective current binding.

Saving over an existing override asks to replace it, with Cancel focused.
Clear returns the selected key/press to its default. Restore all defaults
asks for confirmation. Back cancels capture/confirmation, then closes the
editor. Editing/menu input and short Back remain reserved, so an invalid
mapping cannot trap the user. Unrecognized or unsupported saved mappings
are ignored. Explicit user mappings take precedence over the player's seek
and Up/Down convenience settings. Hold detection retains the existing
single-long-action/no-short-release behavior and repeat rules.

Long presses execute on Android, whose NSH backend reports releases;
desktop input retains its keyboard aliases and plain-press behavior. Key
capture is limited to keys the host delivers to the application.

## Persistence and review

SQLite migration 7 adds source policy and provider-group overlays; it does
not modify imported provider records. Channel organization uses the existing
stable-ID overlay/manual-order tables. A bulk visibility edit is atomic and
retains names, numbering, favorites and group membership. Remote overrides
use the existing versioned settings JSON.

All fixtures and UI tests use generated playlists/reserved example hosts
and temporary storage. No reference code/assets or provider data are added.
Validation:

| Check | Result |
| --- | --- |
| `cargo test -p iptv-core -p iptv-guide -p iptv-nav -p iptv-store-sqlite -p iptv-search` | 421 passed, 5 existing tool/fixture/doctest exclusions |
| `cargo test -p iptv-app --lib --test library_management --test type_drift --test tv_player` | 132 passed |
| `cargo build -p iptv-core --target wasm32-unknown-unknown` | Passed |
| `cargo build -p iptv-app` | Passed |
| Android device smoke test | Pending an available target |

The library UI journey checks individual channel/group hide and recovery,
channel movement, playlist disable/re-enable and refresh policy, and
remote override save/conflict cancellation/clear against persisted values.
Storage tests cover refresh/reopen retention, source isolation, bulk
visibility and migrations from schemas 1–7. Guide tests cover renamed
group identity, hidden playback retention and visible-channel zapping.
The existing fresh-install player test also passes.

Logs: `~/analysis/iptv-rs-parity/2026-11-11/library-management/`.
Builds used one Cargo worker and one native linker thread to limit pressure
on the shared machine. No emulator or USB tablet has been driven.
