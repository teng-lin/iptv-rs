# Combined playlists

Open Playlists and choose **All playlists** to browse every enabled source
in one live catalogue. Choosing an individual playlist returns to its usual
view. The choice survives restart. With no enabled sources the empty guide
remains usable; Playlists still lets the user enable or add sources.

All channels, Favorites, Recently watched, provider/custom groups, channel
search and programme search use the combined catalogue. Channel labels append
the playlist name, and group labels include it, so equal provider names can
be distinguished. Underlying identities are qualified by the stored source
ID and provider channel/group ID; display names and list positions never
identify a stream or a customization.

Each source retains its own channel/group ordering, hidden items, custom
memberships, favourites, guide bindings, catch-up policy and playback context.
Favorites toggles and bulk membership changes write to the owning source.
Switching browsing modes waits for queued favorite/history writes before
loading another mode. The combined Favorites list follows source order and
each source's favorite order. Global favorite reordering and cross-playlist
custom-group creation are outside this implementation; use an individual
playlist's organization editors. Those unsupported aggregate menu actions
are disabled. Per-channel guide assignment remains available in either mode.

Each constituent loads its cached channels and guide before refreshing.
Refresh failures retain its cache and leave other sources available. Source
refresh/status/guide-source management stays accessible in Playlists.
Disabling or deleting a source removes only its rows from the aggregate;
favorites are retained when disabled and deleted with their source when
removed. Enabling a source loads it into the existing guide. A rename updates
its source labels. The combined view uses the existing stored playlist order;
a playlist-order editor is outside this implementation.

Guide and player operations dispatch to the owning source, including stream
headers, Xtream credentials, portal sessions, archive URLs, programme details
and lazily loaded older guide days. Individual guide epochs are translated to
one stable combined clock. Guide settings show aggregate source counts and
status; Update now and Delete stored guide data act across enabled sources.
Manage/Add guide sources opens Playlists so the target is explicit.

The last combined channel is stored separately, and the startup playback
preference can resume it when its source becomes available. Changing source
membership or refreshing keeps guide focus and playing/multiview channels by
qualified identity. Removed playing channels stop rather than selecting a
similarly named channel from another source. The persistent player views
are retained. In combined multiview, connection warnings count tiles for the
chosen destination's source, excluding a tile being replaced; counts are
checked after a channel has been chosen, and no extra stream opens before
confirmation. Limits known from refreshed account data remain session-local.

## Validation

Synthetic portable tests cover duplicate channel/group identities, source
removal, programme-time conversion across different source epochs,
descriptions, per-source request headers, archive URL times, persisted mode
and resume metadata, and source-specific multiview warning cancellation and
acceptance. Headless app scenarios cover two generated sources sharing
channel/feed identities, guide titles, channel/programme search, favorite
write ownership, actual player source commands, mode switching, disable,
empty startup/re-enable, failed-refresh cache retention, deletion and recovery.

The required core tests, core WebAssembly build and native app build are run
alongside guide/store tests and the full app suite. These validate Linux and
portable behavior; native Android/iOS decoder and lifecycle behavior need
platform smoke testing. No Android or iOS host code changes are required.
