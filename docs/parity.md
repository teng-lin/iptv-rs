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
| A group shows only its channels | Moving focus over groups filters the guide live; OK commits and focuses the group's first channel | One long list; choosing a group scrolls to it | missing | P0 |
| "All channels" | First real group, the default selection | none | missing | P0 |
| "Favorites" | Group above All channels, the user's favourite channels | Favourites are an unsaved set of row numbers; no group | missing | P0 |
| Recent channels / History | Player panel button; recent channels list | In-memory history for previous channel only | missing | P1 |
| Tapping or clicking a group | Same as focusing it: filters | Scrolls the list | missing | P0 |
| Navigation rail | Left of the groups: Search, TV, Recordings, My list, Settings (+ Movies, Shows when the playlist has VOD); collapsed to icons, expands with labels when focused | none | missing | P0 |
| Playlists | In Settings | none (newest source only) | missing | P0 (ours: a rail entry, see §5) |
| Back chain | hint → guide → groups → rail → leave | guide → groups → leave | partial | P0 |
| Left from the guide's live programme | Opens groups + rail together (the guide shifts right) | Opens the groups | partial | P1 |
| Hint card after entering a group | Long OK menu / Left groups / Long Left past | Bottom hint line | partial | P3 |
| Channel numbers | Position in the current list (1..n) | Provider number, else position | partial | P1 (setting later) |
| Channel logos | In rows, the info area and the player card | none | missing | P1 |
| Playing marker | ▶ after the playing channel's name | Name tinted | partial | P2 |

## 2. Guide screen

| Behaviour | Reference | Us | Status | P |
| --- | --- | --- | --- | --- |
| Preview + programme info | Preview top-left; to its right the focused programme's title, times, progress, minutes left, season/episode, description, group name, favourite star | Preview and info in a bottom panel | partial | P1 |
| Date and time | At the left of the time bar | Clock in a top bar | partial | P2 |
| Programme grid, now line, past dimming, Long Left past, Back to now | as built | as built | done | – |
| Long OK menu (search, settings, programme and channel sections) | as built | as built (several items disabled) | partial | P2 |
| Channel list mode (no grid) | Setting / "Channels list" | none | missing | P2 |
| EPG still loading | "No information" cells | Gap filler cells | done | – |

## 3. Player

| Behaviour | Reference | Us | Status | P |
| --- | --- | --- | --- | --- |
| OK / tap shows the info card | Logo, title, times, progress, minutes left, number + name, stream badges (resolution, frame rate, audio), next programme, full-width time bar; top bar with group name and date/time | Card with number, name, now/next, progress; no logo, badges, top bar | partial | P1 |
| Quick actions under the card | TV guide, History | Full strip at once | partial | P1 |
| Down: full control row | Search, Channels list, Recordings, Multiview, Picture-in-picture, Video track, Audio track, Audio delay, Subtitles, Picture size, Sleep timer, Favourite, … | Strip present; Video/Audio/Subtitles/Audio delay/Recordings/Multiview disabled | partial | P1 |
| Track selection (video, audio, subtitles) | Choosers | none (hosts expose no tracks) | missing | P1 |
| Back in full screen | Panels → guide with preview still playing | same | done | – |
| Tap preview → full screen; tap video → panels | yes | yes | done | – |
| Desktop: play/pause, volume/mute, window full screen, mouse-visible controls | (TV app: n/a) | none | missing | P0 on desktop |
| Catch-up seeking | settings-driven | done (time bar, FF/RW) | done | – |
| Picture-in-picture on Home | yes | none | missing | P2 |
| Multiview | gated | none | missing | P1 |

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
| Several playlists; choose the one shown; refresh, rename, change EPG URL, delete with consequences stated; add (wizard) | missing (wizard only; startup shows the newest) | P0 |
| Settings screen over the `iptv-settings` catalogue (sections, rows, choosers, persisted) | missing (catalogue and store exist, no screen) | P1 |
| Settings that change behaviour now hard-coded: EPG update interval / keep past days, catch-up seek keys and steps, startup channel, stay-on-guide/search, channel numbering | missing | P1 |

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
| Multiview | add/remove/enlarge/reduce, full screen one, active audio, per-surface failure, connection-limit warning | **P1** |
| Recording, reminders, VOD, My list, parental controls | – | P3 |

Gaps against these tiers that no running workstream covers yet: **EPG
management** (sources, update/clear, status, per-channel assignment) and
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
