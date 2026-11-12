# Missing features and implementation scope — 2026-11-11

This backlog follows the [functionality audit](parity-audit-2026-11-11.md).
The current implementation work covers the four priorities below. Further
multiview enhancements and parental controls are deferred;
channel/group blocking and PIN enforcement remain outside this work.

## 1. Hide/show channels and groups

- Individual channel visibility and a playlist-scoped group manager.
- Bulk channel visibility editing with clear selection/status feedback.
- An explicit hidden-items recovery mode; hidden content must never become
  impossible to restore through the UI.
- Apply visibility to ordinary guide lists and search; hiding an item must
  not delete provider data or stop an already playing stream accidentally.
- Keep favorite membership, names and order when hiding/showing.
- Persist by source/content identity across restart and provider refresh.
- Cancel navigation must preserve a valid focus; selection moves to a
  remaining visible neighbor, with an accessible empty state when needed.

Acceptance: generated playlists cover individual/bulk hide, group hide,
restore, last visible item, active playback, restart, and provider refresh.

## 2. Rename and reorder channels/groups

- Edit channel and provider-group display names; empty/reset restores the
  provider's original name rather than modifying imported provider rows.
- A remote and pointer accessible ordering editor; move earlier/later and
  to either end; expose restore-provider-order.
- Apply custom order consistently to the guide, groups, chooser and search
  where applicable, preserving content identity and favorite membership.
- Persist per playlist/group, including after refresh; missing provider
  items must not corrupt the remaining order.
- Custom-group creation/copy/move and cross-playlist aggregation are later
  work, rather than prerequisites for these basic organization editors.

Acceptance: name edit/reset, movement/boundaries/reset, duplicate names,
focus by identity, refresh and restart; remote and touch/mouse paths.

## 3. Playlist enablement and automatic refresh

- Enable/disable a stored playlist without deleting its configuration or
  organization. Disabled playlists remain visible in management.
- Disabled sources must not be automatically chosen at startup, included
  in refresh-all, or refreshed by the scheduler.
- If the displayed source is disabled, choose another enabled source or
  show a usable no-enabled-playlist state; define current playback policy.
- Per-playlist refresh interval and refresh-on-start, persisted separately
  from EPG refresh settings. Manual refresh remains available.
- Foreground scheduler: use last attempt/success state, avoid duplicate
  jobs and overlapping refreshes, retain cached content on failure, and
  stop scheduling deleted/disabled sources. This is not an OS background
  wakeup service.

Acceptance: persistence, startup fallback, all disabled, enable recovery,
interval off/due/not due, refresh-on-start, running-job deduplication,
failure retention, and cancellation/deletion.

## 4. Editable remote bindings

- Edit short/long key actions per supported context (guide/player/list/
  search); show effective bindings and preserve current defaults.
- Explicit conflict handling when a physical key/press already has an
  override; users can replace, clear an override or restore defaults.
- Persist validated overrides, load at startup, and apply changes without
  restart; unsupported action/context combinations are refused.
- Keep a reliable Back path while editing; long-key release must not
  additionally execute the short action or platform Back.
- Remote capture and tappable choice controls must be available without
  requiring a mouse; keyboard aliases remain documented.

Acceptance: assignment, replacement, removal/default reset, per-context
isolation, restart persistence, invalid data, repeats and long-release.

## Follow-up: guide and list personalization

Implemented on `feat/guide-personalization`: cancelable guide playback policy,
independent favorites/group ordering, custom groups with filtered atomic
bulk membership operations, and guide text/logo/number/time-span controls.
See [behavior, UI entry points and validation](guide-personalization.md).

## Remaining backlog, outside this implementation

- Playlist manual ordering, source-specific
  logo/catch-up overrides, advanced Stalker identity/login fields.
- Remaining guide visibility/emphasis controls, history limits, animations,
  advanced logo-source preferences and additional playback settings.
- Global/shared EPG source workflow and interface localization.
- Recording, reminders, watchlist and VOD end-to-end journeys remain future
  product scope. TV hardware/boot capabilities require separate decisions.

## Validation and safety

Use generated playlists/XMLTV and local mock responses only. No provider
secrets, APK code/assets or reference wording. Pure core builds for wasm;
views send actions to state models; persisted customization uses stable
source/content/group identity. Preserve the persistent player surface.
Run core tests, core wasm build, app build and affected storage/navigation/
headless UI tests. Native Android changes additionally require a device
run on an explicitly available test target; never take a busy device.

## Detailed gaps outside the first four priorities

These entries describe the audited baseline (`12dc1a3`), not claims of exact
TiviMate implementation. A declared setting or storage API is not counted
as a usable feature. See the audit for evidence and reference uncertainty.

| Area | Missing or incomplete workflow | What completing it requires | Priority |
| --- | --- | --- | --- |
| Playlist order | User-defined order of stored playlists | Remote/pointer move editor, persistence, restore order, valid focus after deletion | Next |
| All playlists | A combined live catalogue across enabled sources | Implemented: source-qualified browsing, search, favorites, guide and playback; see [scope and validation](combined-playlists.md) | Implemented |
| Source overrides | Per-playlist logo and catch-up policy | Distinguish provider defaults from overrides; validate URL templates without showing secrets; refresh/reset behavior | Later |
| Stalker setup | Advanced device/login identity fields | Optional serial/device identity/login UI, validation, retained credentials and tested session renewal; supported fields must be confirmed against provider behavior | Later |
| Shared EPG | Reuse one feed across multiple playlists | Shared-source identity/lifecycle, per-playlist matching/shift, default feed selection, refresh ownership and deletion consequences | Next |
| Custom groups | Create/rename/delete a personal group | Expose existing storage support, choose source scope, manage empty groups and duplicate names, restore focus on deletion | Implemented on guide-personalization |
| Channel membership | Bulk copy/move between personal groups | Multi-select, source/destination chooser, explain copy versus membership removal, transactional writes and reset/recovery | Implemented on guide-personalization |
| Favorite order | Rearrange favorites through UI | Expose stored favorites ordering, identity-based movement, consistent guide/player/chooser order | Implemented on guide-personalization |
| Bulk editing | Filtered edits beyond custom membership | Custom copy/move/remove now has search and selection counts; filtered visibility/renumbering remain | Partial |
| Guide mode | User-selectable guide presentation | Wire catalogue choices into actual layout, persist selection, define preview/overlay/fullscreen transitions and focus return | Implemented on guide-personalization |
| Guide visibility | Remaining programme metadata/emphasis choices | Logos and channel numbers are implemented with reclaimed layout space; other visibility controls remain | Partial |
| Guide navigation | Stay-on-guide/list, autoplay and channel-switch delay choices | Separate focus, preview and committed playback; cancellable timers; no stale channel changes after leaving the screen | Implemented on guide-personalization |
| Guide timing | Visible time span and grid time scale choices | Responsive ruler/cell geometry, horizontal movement/page steps, past floor and now positioning, independent of archive permission | Implemented on guide-personalization |
| History | Configurable recent/history limits | Trim persisted history predictably; guide and player lists use the same limit; clear confirmation and empty state | Next |
| Appearance | Animations, advanced logo-source behavior and remaining guide emphasis controls | Text size, guide logos/numbers and visible span are implemented; animation/source policies remain | Partial |
| Playback options | Additional unwired rows in settings catalogue | Implement the behavior behind each row before enabling it; define portable versus Android-only capabilities and persistence scope | Later |
| Localization | Interface language workflow | Translatable strings, language selection/system fallback, date/time formats, text expansion and bidirectional layout checks | Later |
| Hardware/boot | Device wake/start, frame rate, passthrough and decoder controls | Platform capability detection and native validation; retain unavailable status on unsupported hosts | Separate platform work |
| Recording/reminders | Scheduling, storage and conflict journeys | End-to-end scheduler and persistence, permission/storage UI, recording state, failure/retry/cancellation; enums alone are insufficient | Later product scope |
| VOD | Movies/series, watched state, resume/next episode | Usable catalogue/detail/season/episode screens, player integration, persisted progress and navigation/focus restoration | Later product scope |

Whole-app backup/restore is implemented; see [coverage and platform support](whole-app-backup.md).
Multiview enhancements, parental controls and blocking/PIN work remain explicitly deferred. This backlog does not require them to
ship the four library/input priorities.
