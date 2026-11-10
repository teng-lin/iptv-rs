# Product plan: one IPTV player for TV, tablet/phone and desktop

Date: 2026-11-10. Supersedes the scope (not the architecture) of
`2026-11-09-tivimate-ux-replication.md`: parity with the reference app is a
means, the product is a bring-your-own-playlist player that is excellent on
every screen a household uses.

## 1. Product

**Who.** People who already have an IPTV subscription (M3U, Xtream, Stalker)
and watch live TV on a living-room TV, a tablet or phone around the house,
and a computer at a desk. The app provides no content.

**Promise.** Add your playlist once; get a fast, familiar TV guide that
works with a remote, a finger or a mouse; zapping feels like cable.

**Next after the core (P1).** Multiview: 2–4 channels at once with one
active audio; limited by the account's connection count and the device's
decoders (cheap TV boxes: 2; desktop: 4), which the app checks and
explains.

**Not now.** Recording, reminders, VOD libraries, My list, parental
controls, casting, cloud sync. They come after the core is
excellent everywhere (VOD first: most Xtream accounts include it).

## 2. Platforms and form factors

| Form factor | Input | Platforms (order) | Notes |
| --- | --- | --- | --- |
| TV (10-foot) | D-pad remote, media keys; no touch | **Android TV / Google TV**, **Fire TV** (Android, often 32-bit), later Apple TV (tvOS) | The primary market for IPTV. Low-end boxes set the performance budget. |
| Tablet / phone | Touch, optional keyboard | **Android tablet**, then Android phone, iPad/iPhone | Tablet shares the TV's landscape layout; phone needs a portrait layout (list-first, bottom navigation) and picture-in-picture. |
| Desktop | Mouse, keyboard, trackpad | **Linux**, **macOS**, later Windows | Resizable windows, hover controls, keyboard shortcuts, window full screen, system media keys. |

One codebase: the headless Rust core (providers, EPG, store, focus and
playback state machines) is shared; each platform contributes a thin host
(window, player component, storage paths). NSH covers Android, Linux,
macOS, Windows, iOS and the web; **tvOS is not covered** (decision in §6).

## 3. Core journeys (the product's definition of done)

Each must be excellent with a remote, with touch and with a mouse:

1. **Add a playlist** (M3U link/file, Xtream, Stalker) and see channels and
   guide in seconds; errors say what to fix.
2. **Browse**: groups (Favorites, All channels, playlist groups), the guide
   grid, search; focus never gets lost.
3. **Watch and zap**: first frame fast, channel up/down, last channel,
   recover from stream errors by itself.
4. **Player controls**: info, tracks (audio/subtitles), picture size,
   pause, volume (desktop), full screen.
5. **Catch-up** where the provider has it: play from the guide, seek.
6. **Favourites**: add/remove anywhere, a Favorites group, survives refresh.
7. **Manage playlists and guide**: several playlists, switch, update, edit
   login, EPG sources, clear/update EPG.
8. **Settings** for the above only.

## 4. Experience rules per form factor

- **TV:** everything reachable with D-pad + OK + Back; long presses for
  secondary actions; key map matches the reference's defaults (remappable
  later); Leanback launcher, banner, no touch requirement.
- **Touch:** every focusable thing is tappable; swipes zap and scroll;
  no hover-only controls; minimum touch target 48 dp.
- **Desktop:** controls appear on mouse movement and hide after a few
  seconds; every remote action has a keyboard shortcut (documented);
  window full screen; scroll wheel scrolls lists and the guide.
- **Phone (portrait):** a list-first layout from the same view models;
  full-screen player in landscape; picture-in-picture.
- **Layout is chosen by form factor, not OS** (a 10" Android tablet and a
  macOS window both get the landscape layout).

## 5. Milestones

| Milestone | Scope | Exit |
| --- | --- | --- |
| **M1 Core on Android + Linux** (now) | §3 journeys on Android TV, Android tablet, Linux; groups/rail/favourites, player controls and tracks, playlists screen, settings, EPG management, TV remote defaults | Real Android TV box + Fire TV stick pass; tablet and Linux pass; budgets met (below) |
| **M1b Multiview (P1)** | 2–4 surfaces (layouts 1+1, 2×2), add/remove/enlarge, active audio, per-surface errors; connection-limit warning from the account's max connections; decoder cap per device | Works on desktop, tablet and a real TV box within its decoder limit |
| **M2 macOS** | macOS host + libmpv player, menus, shortcuts, notarized build | Same journeys on macOS |
| **M3 Release 1** | Our name, icon, theme (originality gate, Phase 9), Play Store + Amazon Appstore listings, crash reporting without personal data, armeabi-v7a build | Store-approved builds |
| **M4 Phone + iPad/iPhone** | Portrait layout, PiP, iOS player (libmpv or VLCKit) | Phone journeys pass |
| **M5 VOD** | Movies/Shows browsing, details, resume | – |
| **M6 Apple TV, Windows** | per §6 decision | – |

**Budgets (set on a real low-end Android TV box):** cold start to guide
< 2 s with a stored playlist; zap to first frame < 1.5 s on a healthy HLS
stream; key-to-frame < 50 ms in the guide; 10k channels and 7 days of EPG
without stalls; APK download < 15 MB.

## 6. Decisions needed

1. **Apple TV approach** (before M6): a native SwiftUI/TVML shell over the
   Rust core via FFI, or adding tvOS to NSH. Recommendation: shell over the
   core — tvOS focus engine is native anyway.
2. **iOS/tvOS player:** libmpv (same as desktop, App Store-compatible
   builds exist) or VLCKit. Recommendation: libmpv for one behaviour set.
3. **Fire TV / 32-bit:** ship armeabi-v7a in M3 (many sticks run 32-bit).
4. **Monetisation** (affects gating, not architecture): free, one-time
   purchase, or subscription. No feature should be built twice for it.
5. **Product name and design system** (M3 gate).

## 7. What changes in the current work

- The four running workstreams (groups/navigation, player controls,
  playlists, settings) are M1 and stay. Each must work with remote, touch
  and mouse; desktop gets hover controls and shortcuts.
- Next M1 workstreams: **EPG management**; **TV remote defaults** checked
  on a real Android TV box and a Fire TV stick; **performance budgets**
  measured on that box.
- Multiview is P1 (M1b), right after the core; the player contract must
  allow several `tv.player` instances (one persistent surface each).
- Deferred: recording, reminders, VOD, My list, parental controls,
  key remapping UI, backup/restore, appearance options beyond
  numbers/logos/timeout.
