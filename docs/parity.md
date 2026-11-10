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
| Multiview | gated | none | missing | P3 |

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
| Several playlists; choose the one shown; refresh, rename, change EPG URL, delete with consequences stated; add (wizard) | done: playlists screen (`playlists_view.rs`) with show (remembered; startup opens it), refresh now / refresh all, rename, provider details and User-Agent (checked by importing, kept only if they work), one guide link per playlist, delete with consequences; per-playlist last channel. Not yet: several guide sources per playlist, enable/disable, logo priority, catch-up type, update interval, manage groups | P0 |
| Settings screen over the `iptv-settings` catalogue (sections, rows, choosers, persisted) | missing (catalogue and store exist, no screen) | P1 |
| Settings that change behaviour now hard-coded: EPG update interval / keep past days, catch-up seek keys and steps, startup channel, stay-on-guide/search, channel numbering | missing | P1 |

## 6. Workstreams (2026-11-10)

1. **Groups and navigation** (§1, §2 layout): list model with virtual groups
   (Favorites, All channels, Recent), live filtering, favourites saved by
   channel key, rail, Back chain, positional numbers, logos, guide header.
2. **Player panel and controls** (§3): info card, quick actions, control
   row, track choosers on both hosts, History panel, desktop controls.
3. **Playlists** (§5 first row): playlists screen from the rail, active
   playlist, refresh/rename/EPG URL/delete.
4. **Settings** (§5): settings screen over the catalogue, wiring the
   hard-coded behaviours.
