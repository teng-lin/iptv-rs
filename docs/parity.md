# Functional parity with the reference app

For the source-and-test audit at commit `12dc1a3`, including backend-only features, the four-versus-nine multiview limit and reference uncertainty, see [the 2026-11-11 parity audit](parity-audit-2026-11-11.md). The tables below retain the earlier product/workstream assessment.

Date: 2026-11-10. Sources: the static specification in `~/src/tivimate/docs`
and a side-by-side runtime pass on the tablet emulator with the same provider
playlist in both apps (captures outside all repositories). Behaviour only:
wording, artwork and layout values here are ours.

Status: **done**, **partial** (exists, behaves differently), **missing**.
Priority: **P0** breaks the basic journey, **P1** expected by every user,
**P2** important, **P3** later.

## 1. Navigation and information architecture

Observed structure of the reference, outermost first:

```
navigation rail          groups list            guide (or channel list)     player
(Search, TV, Recordings, (Favorites, All        rows of the selected group  full screen,
 My list, Settings;       channels, playlist    only, numbered 1..n         panels on OK
 Movies/Shows with VOD)   groups)               within the list
```

| Behaviour | Reference | Us | Status | P |
| --- | --- | --- | --- | --- |
| A group shows only its channels | Moving focus over groups filters the guide live; OK commits and focuses the group's first channel | Same (`iptv_core::browse`, `lists`); an empty list cannot be entered | done | P0 |
| "All channels" | First real group, the default selection | Same, in the playlist's order | done | P0 |
| "Favorites" | Group above All channels, the user's favourite channels | Favourites group (order added), kept by channel key per source in the store; menu, player control and editor update it live; empty state with a hint | done | P0 |
| Recent channels / History | Player panel button; recent channels list | "Recently watched" group after Favourites (store watch history, newest first); the player's History lists the same channels, the playing one marked | done | – |
| Tapping or clicking a group | Same as focusing it: filters | Same; a tap on the list name opens the groups | done | P0 |
| Navigation rail | Left of the groups: Search, TV, Recordings, My list, Settings (+ Movies, Shows when the playlist has VOD); collapsed to icons, expands with labels when focused | Same, plus Playlists; items without a screen are dimmed and skipped; Playlists and Settings open nothing yet | done | P0 |
| Playlists | In Settings | none (newest source only) | missing | P0 (ours: a rail entry, see §5) |
| Back chain | hint → guide → groups → rail → leave | Same; a past or future programme returns to now first (`guide.back_returns_to_now`; off: straight to the groups) | done | P0 |
| Left from the guide's live programme | Opens groups + rail together (the guide shifts right) | Same | done | P1 |
| Hint card after entering a group | Long OK menu / Left groups / Long Left past | Our model in our words (Left replays then groups, Left in the groups search, Hold Left groups, Search; on desktops Tab, `/`, Home, Shift/Ctrl + arrows); any key or Back dismisses it, else 6 s | done | P3 |
| Channel numbers | Position in the current list (1..n) | Same in the guide, its header, the player card, the channel list and History (a channel outside the list on screen shows its playlist number) | done | P1 (setting later) |
| Channel logos | In rows, the info area and the player card | Rows and the guide header; not yet the player card | partial | P1 |
| Playing marker | ▶ after the playing channel's name | Same (name also tinted) | done | P2 |

## 2. Guide screen

| Behaviour | Reference | Us | Status | P |
| --- | --- | --- | --- | --- |
| Preview + programme info | Preview top-left; to its right the focused programme's title, times, progress, minutes left, season/episode, description, group name, favourite star | Same without season/episode (not imported) | partial | P1 |
| Date and time | At the left of the time bar | Same | done | P2 |
| Programme grid, now line, past dimming, Long Left past, Back to now | as built | as built | done | – |
| Moving along the timeline (touch, mouse, keyboard) | TV app: Left programme → groups → search (rail); the guide does not walk into history | one model on the remote and the keyboard: Left walks back through a catch-up channel's replays (oldest inside its archive and the stored guide), else from now's programme opens the groups; Left in the groups focuses the rail's Search; Hold Left (remote) / Tab (keyboard) the groups; Search key, `/`, Ctrl (Cmd) + F search; Right to the end of the stored guide; Up/Down keep the time but snap to now on a row without catch-up. Touch and mouse drags pan sideways (direction lock, 1:1 in dp) and fling on (friction 2.0, gain 1.2: about 0.6 × the release speed in dp), the landing day paged in at release; the view reaches back as far as the list's catch-up (now's half hour without), past cells a row cannot replay dimmed; a Now button in the ruler once away. Desktops: horizontal wheel, touchpad swipe and Shift + wheel pan; Home now, Shift + arrows an hour, Ctrl (Cmd) + arrows a day (clamped like Left/Right), Page Up/Down rows. Verified headless (drags, wheel, keys); the remote's Search key (host maps it to F12) and real wheels not yet on a device Also checked on macOS (`nsh test --target macos`, 2026-11-10, before the final Left model): mouse drags and their fling, a vertical drag left to the channels, a trackpad gesture sideways 1:1, Now, ‹ ›, Shift + arrows, Cmd + arrows, the ruler's last time whole at the edge; Shift + wheel and the trackpad's momentum need a hand check (AppKit drops posted scroll events). | done | P1 |
| Long OK menu (search, settings, programme and channel sections) | as built | as built (several items disabled) | partial | P2 |
| Channel list mode (no grid) | Setting / "Channels list" | none | missing | P2 |
| EPG still loading | "No information" cells | Gap filler cells | done | – |

## 3. Player

| Behaviour | Reference | Us | Status | P |
| --- | --- | --- | --- | --- |
| OK / tap shows the info card | Logo, number + name, title, times with one progress line and minutes left, next programme, stream badges (resolution, frame rate, audio); top bar with group name and date/time | same (logo from the playlist; the group is the playlist's). One time line only: the card's progress line live, its seek bar (position, length, a dot; ⇢ while a seek is chosen) in a replay; the separate full-width time bar is gone (`iptv_core::panels::Line`). OK again (no card focused) or Back hides it | done | – |
| Action cards under the card | TV guide, History, … | TV guide, History, Channels, Favourite (★/☆), Multiview, More controls (the strip); none focused until Left/Right, OK presses the focused one; Up/Down keep zapping, the card following the channel; a hint line names the keys | done | – |
| Long OK / Menu: full control strip | Search, Channels list, Recordings, Multiview, Picture-in-picture, Video track, Audio track, Audio delay, Subtitles, Picture size, Sleep timer, Favourite, … (tivimate docs/08 E: Long OK and Menu both) | same order, current values shown, in place of the card (no time line); Menu again hides it; Picture-in-picture where the host can (Android with the system feature; the desktops' own window); Recordings, Channel options, Settings disabled | partial | P1 |
| Channel-change info | Info panel when switching (setting) | a compact banner (number + name, programme on now, its progress and minutes left; no top bar, no input), hidden after 3 s; over the open card or strip those stay and show the new channel (`player.info_on_switch`) | done | – |
| Track selection (video, audio, subtitles) | Choosers | Choosers on both hosts (`tv.player` `tracks` event, `videoTrack`/`audioTrack`/`textTrack`); verified on Linux, Android compiled only | done | – |
| Audio delay | Chooser (ms) | ±1 s in 50 ms steps; mpv `audio-delay`, Media3 through a delayed audio sink (Android unverified) | done | – |
| Live stream recovery | Retries on its own; plays a little behind live | A live channel never dead-ends on a failure a retry can fix (`iptv_core::playback::Health`): retries 1, 2, 4, 4, 4 s, then 5, 10 and every 20 s for as long as it stays selected (Android: while in the foreground), keeping its last frame with a small "Reconnecting…" after 1 s; the "Can't play" card only for a gone stream (404/410), an unsupported format, a refused account, a stream that never played after the fast tries, or one down 2.5 min; retries go on behind it every 30 s, Try again starts over at once. Refusals (401/403/429/458/509) read as "the provider refused for now" and name the account's streams at once when known. Android plays live ≥ 10 s behind the edge with ~15 s of load retries and reopens a refused/cut live stream itself once per 30 s; mpv keeps its cache on with a 2 s refill wait for live. Catch-up retries 5 times once it played, where it was. Desktop measured with libmpv headless and in the macOS app (`nsh test --target macos`, `tools/mock-live-cuts.py`: last frame kept, pill, card after 2.5 min, recovery; docs/perf.md); Android compiled only | done | P0 |
| Player remote defaults | OK panels, Long OK control strip, Menu control strip, Left channels list, Long Left list beside a preview, Right most recent channel, Long Back stop, Up/Down zap | same (keymap, `player_keys.rs`): OK info card + action cards, Long OK / Menu the strip, Up/Down and Ch± zap (`remote.player_up_down`: or open the channel list), Left the list over the video, Long Left beside it, Right the most recent channel, Long Right History (ours), Back hides the panels then the guide, Long Back stops. Left's list is the guide's list (same numbers, Left in it: the guide's groups column, moving previews, OK enters). Keyboard: Enter, M (strip), Shift+Left / Shift+Right for the long ones. Long presses (OK, Left, Right, Back) act on release when tapped; verified headless with keys (`tests/tv_player.rs`), long presses by the keymap's tests; not yet on a device | done | P0 |
| Back in full screen | Panels → guide with preview still playing | same | done | – |
| Tap preview → full screen; tap video → panels | yes | yes | done | – |
| Desktop: play/pause, volume/mute, window full screen, mouse-visible controls | (TV app: n/a) | mouse shows the panels (hidden 3 s after it rests); play/pause, mute, volume bar and steps, full screen buttons; F11 (macOS: Ctrl+Cmd+F), double click, Escape; Ctrl+M, Ctrl+Up/Down (macOS: Cmd+Up/Down). Keys and mouse movement verified (Linux, macOS); clicks on play/pause and mute verified on macOS (`nsh test --target macos`): hidden overlays had taken every click until 2026-11-10. Window full screen verified on macOS with `nsh test --target macos --foreground` (Ctrl+Cmd+F in and out, the guide filling 1470×923 below the notch); the test host's app is active only with `--foreground` | done | – |
| iPad: touch controls and leaving the app | (TV app: n/a) | a round ‹ top left over the video (the full-screen picture, the guide's preview) walks back as Back and Left do: full-screen video → the guide (multiview's grid for a tile watched full screen) → the groups → the rail with its names (two taps from the guide to Search or Settings: ‹ ‹, or ‹ and the rail's icon beside the groups); a swipe in from the left edge is Back (NSH's root view, while no navigation stack can pop); the half-hour ‹ › steps give way to drag, fling and Now; a tap beside the open groups and rail, or on the video beside the channel list, closes them; Settings has Close, the wizard ‹ Back; the remote's hints show only after a hardware keyboard's key; streams stop in the background and reopen in the foreground. iOS only (Android tablets keep the back button's model). Verified in the iPad simulator (`nsh test --target ios`, in portrait: NSH's test host; again on master 34707ee) and on the device, an iPad (10th generation, landscape), by hand on 2026-11-11: the ‹ chain, the swipe in from the left edge, a tap beside the open groups and menu closing them, Settings → Close, the wizard's ‹ Back. Leaving the app and coming back verified in the simulator | done | P1 |
| Catch-up seeking | settings-driven (Left/Right, Down/Up or RW/FF/Pause) | the `catchup.seek_keys` arrows step along the card's seek bar (no long press then: held, a step every 0.2 s, doubling every 1.5 s up to 8×, `iptv_core::seek_hold`); OK opens a chosen seek at once, else plays/pauses (arrow schemes; with transport keys OK shows the card); FF/RW always seek; verified headless (`tests/tv_replay.rs`) | done | – |
| Picture-in-picture | the control strip's item; Home while a channel plays full screen (setting); video only; a tap returns, closing stops | Android: the strip's Picture in picture, and leaving the app (Home) while a channel plays full screen per `player.pip_on_leave` (default on phones and tablets, off on a TV; Android 12+ auto-enter, before and on TVs `onUserLeaveHint`); the window shows surface 0 only (the UI layers hide, multiview leaves to its active tile), at the stream's aspect ratio, with previous / play-pause / next buttons; the launcher icon or a tap expands it back to the full-screen player (`singleTask`); closing it stops the activity and so playback (connection closed). Desktops: the strip's item makes the window small, borderless and floating (bottom right); Back/Escape or a double click restores it. Verified on the tablet emulator (Home and the strip, expand, close → server sees the connection go) and headless (desktop window); the window's buttons and Google TV not yet | done | P2 |
| Multiview | gated | the control row's (or play bar's) Multiview: the playing channel is the first tile and the channel list opens for the next; only tiles on screen, reflowing at once (1 centred at half width, 2 side by side, 3 as 2+1, 4 as 2×2, all 16:9), up to the device's decoders (Android 2, desktop 4); D-pad (or a tap) picks the active tile, which alone is heard; OK (or a tap on the active tile) opens its menu as the reference's side panel (right edge, vertically centred, 208 dp wide, 40 dp rows, the light focus pill, no backdrop over the tiles): Try again (after a failure), Add screen, Search and add (the search screen picks a channel, or a programme's channel, for a new tile; Back adds none), Change channel, Pause/Play (that tile's player alone, ⏸ on it), Enlarge screen/Reduce screen, Full screen, Remove screen (at once); the active tile has a thin light grey outline; Long OK or a long press enlarges/reduces (big tile on top, the rest in a strip); watch full screen is a detour that Back (or Multiview again) undoes, the other tiles playing on hidden; Back from the grid watches the active tile full screen; a pointer gets a toolbar (Add screen, Exit multiview) and per-tile enlarge/remove buttons while it moves; keys get a short hint; each tile has its own waiting/error state; adding past an Xtream account's `max_connections` asks first. Verified headless (pointer), Linux (keys) and macOS (`nsh test --target macos`: layouts, menu, enlarge by menu, long press and button, pointer chrome, the detour and Back / Back to multiview restoring the same grid, a zap during it ending multiview, Back from the grid opening the main player once; each tile's `CAOpenGLLayer` follows its tile as it reflows, only the active tile heard); Android compiled only | done | P1 |

## 4. Library and other surfaces

| Surface | Status | P |
| --- | --- | --- |
| Search (channels, programmes, history) | done | – |
| Movies / Shows (VOD categories, details, seasons, resume) | missing (imported, no screens) | P2 |
| My list | missing | P2 |
| Recordings, reminders | missing | P3 |
| Parental PIN, edit modes (rename, hide, reorder, custom groups) | missing (store has overlays) | P2 |

## 5. Playlists and settings

| Behaviour | Status | P |
| --- | --- | --- |
| Several playlists; choose the one shown; refresh, rename, change EPG URL, delete with consequences stated; add (wizard) | done: playlists screen (`playlists_view.rs`) with show (remembered; startup opens it), refresh now / refresh all, rename, provider details and User-Agent (checked by importing, kept only if they work), guide sources (see the next row), delete with consequences; per-playlist last channel. Not yet: enable/disable a playlist, logo priority, catch-up type, update interval, manage groups | P0 |
| Guide sources per playlist (EPG) | done: guide sources screen (`epg_sources_view.rs`; from a playlist's actions and Settings → TV guide data "Guide sources" / "Add a guide source"): the provider's own guides plus any number the user adds (link + optional name; duplicates refused), each switched on/off, ordered (first with a channel wins), time-shifted in 30 min steps (±12 h), renamed, re-linked or removed (the provider's are only switched off); each row shows its last result (channels used, programmes, or the failure kind). Changes are saved at once (store v6 `guide_sources`) and the guide on screen is checked a moment later (changed feeds download). Update now / delete / status act on every source switched on. No global default source shared between playlists (sources are per playlist) | – |
| Choose a channel's guide | done: Long OK menu → "Choose guide data…" (`guide_assign_view.rs`): Automatic, No guide data, or any channel the stored guides list (search by typing, every / only unused filter); saved by channel key as an overlay; shows at once when the stored guide has its programmes (or for "No guide data"), else the guide downloads right away to apply it (back to Automatic always downloads) | – |
| Settings screen over the `iptv-settings` catalogue (pages in the reference's order: General, Playlists, TV guide data, Appearance, Playback, Remote control, Parental control, Other, About; switches, choice lists, steppers, text; persisted in `settings.json`) | done: rows the app does not act on are dimmed ("not available yet", "in a later version"); device-level TV-box features are not offered | – |
| Settings that change behaviour: guide data (update interval, on start, on playlist change, past days kept, descriptions, update now, delete, last update status), catch-up seek keys and steps, Up/Down in the player, panel timeouts, channel-change info, last channel on start, Back twice to leave, User-Agent, search (favourites first, history, past without replay, stay on search), accent and background colours, overlay transparency| done (including Back returns to now) | – |
| Black picture while tuning | missing (needs a `tv.player` prop on every host; the desktop players, Linux and macOS, always blank) | P2 |
| Channel numbering, stay on guide/list, clock, remote key remapping | missing (keys are listed read-only) | P2 |

## 6. Priorities (user, 2026-11-10)

Core: **playlists, EPG, favourites, search, playback**. **Remote control** is
critical on TV. Recording, VOD, parental controls and the rest come later.
Features that only make sense on an Android TV box are not copied (or wait):
auto start on boot/wake, AFR, audio
passthrough, decoder choice, UDP proxy, voice search, app self-update,
statistics, beta channel.

The reference's settings tree (runtime, with Premium) is the checklist for
what each core area must let the user do. Tier per row:

| Area | Reference controls (behaviour) | Tier |
| --- | --- | --- |
| Playlists | list of playlists; add; update all; per playlist: enable, name, EPG sources, catch-up type, User-Agent, provider parameters (edit login), manage groups (hide/rename/order), update options | **P0** (screen + switch + edit login/EPG); manage groups P1 |
| EPG | EPG sources (add/remove, default, per playlist); past days to keep; store descriptions; update interval / on app start / on playlist change; update now; clear; last update status; assign EPG to a channel | **P0** sources + update/clear + status; P1 per-channel assignment |
| Favourites | Favorites group; add/remove from menu and player; favourites editor; order | **P0** |
| Search | channels + programmes; history; favourites first; past programmes without catch-up; stay on search after switching | done; settings **P1** |
| Playback | info panel + controls; tracks (video/audio/subtitles); audio delay; picture size; sleep timer; buffer size; black screen when switching; panels timeout; clock; history / recent channels; external player; catch-up and skip steps | **P0** panels, tracks, history, desktop controls; P1 buffer, timeout, clock, black screen; P2 external player |
| Remote control | per-context key bindings, guide: OK watch, Long OK menu, Back back, Long Back return to player, Left groups, Right later programmes, Ch± page; player: OK info panel + action cards, Long OK / Menu control strip, Up/Down switch channels (or the channel list), Back back, Long Back stop playback, Left channels list over video, Long Left channels list in preview, Right most recent channel (Long Right History, ours); catch-up seek key schemes | **P0** defaults on TV (done); P1 user remapping |
| Appearance | guide: sorting, preview on/off, numbers, names, two-line names/titles, catch-up icon, highlight current; player: channels list style, info panel, clock, panels timeout; groups sorting; logos; font size; theme; transparency | P1 (numbers, logos, preview, timeout); P2 rest |
| General | last channel on start; confirm exit with second Back; User-Agent; backup/restore | P1 (start, exit, UA); P2 backup |
| Multiview | add/remove/enlarge/reduce, full screen one, active audio, per-surface failure, connection-limit warning | **P1** (done) |
| Recording, reminders, VOD, My list, parental controls | – | P3 |

Gaps against these tiers that no running workstream covers yet: remote
key remapping (P1); the player's remote defaults are in (§3).

## 7. Workstreams (2026-11-10)

1. **Groups and navigation** (§1, §2 layout): list model with virtual groups
   (Favorites, All channels, Recent), live filtering, favourites saved by
   channel key, rail, Back chain, positional numbers, logos, guide header.
2. **Player panel and controls** (§3): info card, quick actions, control
   row, track choosers on both hosts, History panel, desktop controls.
3. **Playlists** (§5 first row): playlists screen from the rail, active
   playlist, refresh/rename/EPG URL/delete.
4. **Settings** (§5): settings screen over the catalogue, wiring the
   hard-coded behaviours.

Detailed follow-up: [Playlist, settings and organization backlog](missing-features-2026-11-11.md).

On `feat/library-management`, see [implementation scope and validation](library-management-implementation.md).
