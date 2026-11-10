# Functional parity with the reference app

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
| Hint card after entering a group | Long OK menu / Left groups / Long Left past | Same keys in our words; any key or Back dismisses it, else 6 s | done | P3 |
| Channel numbers | Position in the current list (1..n) | Same in the guide, its header, the player card, the channel list and History (a channel outside the list on screen shows its playlist number) | done | P1 (setting later) |
| Channel logos | In rows, the info area and the player card | Rows and the guide header; not yet the player card | partial | P1 |
| Playing marker | ▶ after the playing channel's name | Same (name also tinted) | done | P2 |

## 2. Guide screen

| Behaviour | Reference | Us | Status | P |
| --- | --- | --- | --- | --- |
| Preview + programme info | Preview top-left; to its right the focused programme's title, times, progress, minutes left, season/episode, description, group name, favourite star | Same without season/episode (not imported) | partial | P1 |
| Date and time | At the left of the time bar | Same | done | P2 |
| Programme grid, now line, past dimming, Long Left past, Back to now | as built | as built | done | – |
| Long OK menu (search, settings, programme and channel sections) | as built | as built (several items disabled) | partial | P2 |
| Channel list mode (no grid) | Setting / "Channels list" | none | missing | P2 |
| EPG still loading | "No information" cells | Gap filler cells | done | – |

## 3. Player

| Behaviour | Reference | Us | Status | P |
| --- | --- | --- | --- | --- |
| OK / tap shows the info card | Logo, title, times, progress, minutes left, number + name, stream badges (resolution, frame rate, audio), next programme, full-width time bar; top bar with group name and date/time | same (logo from the playlist; the group is the playlist's) | done | – |
| Quick actions under the card | TV guide, History | same; History lists the recently watched channels (the store's history, newest first; survives restarts) | done | – |
| Down: full control row | Search, Channels list, Recordings, Multiview, Picture-in-picture, Video track, Audio track, Audio delay, Subtitles, Picture size, Sleep timer, Favourite, … | same order, current values shown; Recordings, Picture-in-picture, Channel options, Settings disabled | partial | P1 |
| Track selection (video, audio, subtitles) | Choosers | Choosers on both hosts (`tv.player` `tracks` event, `videoTrack`/`audioTrack`/`textTrack`); verified on Linux, Android compiled only | done | – |
| Audio delay | Chooser (ms) | ±1 s in 50 ms steps; mpv `audio-delay`, Media3 through a delayed audio sink (Android unverified) | done | – |
| Player remote defaults | OK panels, Long OK menu, Left channels list, Long Left list beside a preview, Right most recent channel, Long Back stop, Up/Down zap | same (keymap); Left's list is the guide's list (same numbers, Left in it: the guide's groups column, moving previews, OK enters). Long Back from NSH's Back key (Android; its release takes no step back), not yet verified on a device; desktop: Stop key | done | P0 |
| Back in full screen | Panels → guide with preview still playing | same | done | – |
| Tap preview → full screen; tap video → panels | yes | yes | done | – |
| Desktop: play/pause, volume/mute, window full screen, mouse-visible controls | (TV app: n/a) | mouse shows the panels (hidden 3 s after it rests); play/pause, mute, volume bar and steps, full screen buttons; F11 (macOS: Ctrl+Cmd+F), double click, Escape; Ctrl+M, Ctrl+Up/Down (macOS: Cmd+Up/Down). Keys and mouse movement verified (Linux, macOS); clicks on play/pause and mute verified on macOS (`nsh test --target macos`): hidden overlays had taken every click until 2026-11-10. Window full screen not verified on macOS (the test host's app is never active) | done | – |
| Catch-up seeking | settings-driven | done (time bar, FF/RW) | done | – |
| Picture-in-picture on Home | yes | none | missing | P2 |
| Multiview | gated | the control row's Multiview: the playing channel is the first tile, 1+1 for two cells, 2×2 for three or four; an add cell while the device's decoders allow (Android 2, desktop 4); D-pad (or a tap) picks the active tile, which alone is heard; OK (or a second tap) opens its options: change channel, watch full screen, remove, try again after a failure; Back watches the active tile full screen; each tile has its own waiting/error state; adding past an Xtream account's `max_connections` asks first. Verified on Linux (keys); Android compiled only, touch untested on screen | done | P1 |

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
| Settings that change behaviour: guide data (update interval, on start, on playlist change, past days kept, descriptions, update now, delete, last update status), catch-up seek keys and steps, panel timeouts, channel-change info, last channel on start, Back twice to leave, User-Agent, search (favourites first, history, past without replay, stay on search), accent and background colours, overlay transparency| done (including Back returns to now) | – |
| Black picture while tuning | missing (needs a `tv.player` prop on every host; the desktop players, Linux and macOS, always blank) | P2 |
| Channel numbering, stay on guide/list, clock, remote key remapping | missing (keys are listed read-only) | P2 |

## 6. Priorities (user, 2026-11-10)

Core: **playlists, EPG, favourites, search, playback**. **Remote control** is
critical on TV. Recording, VOD, parental controls and the rest come later.
Features that only make sense on an Android TV box are not copied (or wait):
auto start on boot/wake, picture-in-picture on Home, AFR, audio
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
| Remote control | per-context key bindings, guide: OK watch, Long OK menu, Back back, Long Back return to player, Left groups, Right later programmes, Ch± page; player: OK panels, Long OK menu, Back back, Long Back stop playback, Left channels list over video, Long Left channels list in preview, Right most recent channel; catch-up seek key schemes | **P0** defaults on TV; P1 user remapping |
| Appearance | guide: sorting, preview on/off, numbers, names, two-line names/titles, catch-up icon, highlight current; player: channels list style, info panel, clock, panels timeout; groups sorting; logos; font size; theme; transparency | P1 (numbers, logos, preview, timeout); P2 rest |
| General | last channel on start; confirm exit with second Back; User-Agent; backup/restore | P1 (start, exit, UA); P2 backup |
| Multiview | add/remove/enlarge/reduce, full screen one, active audio, per-surface failure, connection-limit warning | **P1** (done but enlarge/reduce: tiles are equal) |
| Recording, reminders, VOD, My list, parental controls | – | P3 |

Gaps against these tiers that no running workstream covers yet:
**remote parity in the player** (Left = channels list over the video,
Right = most recent channel, Long Back = stop) — the next workstreams.

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
