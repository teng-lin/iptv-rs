# Combined playlists

The guide opens every enabled playlist together when more than one is saved.
Adding a second playlist also opens this combined browser automatically.
A new single-playlist library keeps its usual All channels view. Disabled playlists
are excluded. **Playlists** in the rail remains the place to add, rename,
refresh, enable or delete sources; it is no longer required to browse between
them. Its All playlists action also works with just one enabled source.
Show this playlist temporarily opens an individual source for organization;
with multiple enabled sources, the next launch returns to the combined browser.

Open the guide's groups column to see **Favourites**, **Recently watched**,
**All playlists**, then a section for each enabled playlist in stored order.
Each playlist header previews all of its channels, with its categories indented
underneath. Tap a header or press OK on it to expand/collapse its categories;
Right enters that playlist's channels. Select a category to narrow the guide.
Sections retain expansion by source ID across refreshes, renames and restart.
Collapsing a section hides only its categories: the playlist's channels remain
available in its header, All playlists, favorites and search. Channel zapping
keeps the owning category's boundaries even when its section is collapsed.

The Favourites entry combines the enabled playlists' favorites; membership is
stored independently in each source. There is not a separate favorites row
inside each playlist section. In the player's channel chooser, playlist parent
rows select that source's complete channel list; expansion is controlled in
the guide's groups column.

Channel search and programme search use the combined catalogue. Channel labels
append the playlist name, so equal provider names can be distinguished. Category
names sit beneath their owning playlist. Underlying identities are qualified by
the stored source ID and provider channel/group ID; display names and list
positions never identify a stream or a customization.

Each source retains its own channel/group ordering, hidden items, custom
memberships, favourites, guide bindings, catch-up policy and playback context.
Favorites toggles and bulk membership changes write to the owning source.
Switching browsing modes waits for queued favorite/history writes before
loading another mode. The combined Favorites list follows source order and
each source's favorite order. Global favorite reordering and cross-playlist
custom-group creation are outside this implementation; use an individual
playlist's organization editors. Those unsupported aggregate menu actions
are disabled. Per-channel guide assignment remains available in either mode.

Adding a playlist keeps its successful import's playback context and guide
work when the browser automatically joins it with the other sources. It does
not require another login or playlist download to enter the combined view.
Pending guide downloads, including manual updates, still reach the combined
view when automatic EPG updates are off. Newer source/guide edits and deletion
supersede older work. Startup resume waits for the owning source's playback
context, and a cached portal whose login failed is loaded again when joining.
Each constituent otherwise loads its cached channels and guide before refreshing.
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
preference can resume it when its source becomes available. When there is no
combined resume key yet, startup uses the active source's previous saved channel
(including the older single-channel file), if that source is enabled. Changing source
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
descriptions, per-source request headers, archive URL times, playlist parents,
collapsed sections, category/channel identity through section changes,
persisted expansion, mode and resume metadata, and source-specific multiview warning cancellation and
acceptance. Headless app scenarios cover two generated sources sharing
channel/feed identities, guide titles, channel/programme search, favorite
write ownership, actual player source commands, mode switching, disable,
empty startup/re-enable, failed-refresh cache retention, deletion and recovery.
Browser scenarios additionally cover automatic combined startup, disabled source
exclusion, touch and remote expansion, parent/category selection, expansion
after relaunch and adding a second playlist without a management-mode switch.

The required core tests, core WebAssembly build and native app build are run
alongside guide/store tests and the full app suite. These validate Linux and
portable behavior; native Android/iOS decoder and lifecycle behavior need
platform smoke testing. No Android or iOS host code changes are required.

Earlier aggregate-mode Android validation on an owned `tab36` emulator showed six channels
from two generated playlists, retained All playlists after process restart,
and restored the same combined view through a whole-app backup round trip.
The browser change passes 177 Linux app unit tests and 49 UI scenarios, plus
core, guide and store tests and the core/guide WebAssembly builds. A fresh
`tab36` smoke run with two generated cached M3U sources showed all six channels
by default, touch collapse of one playlist, remote Down/Right selection of the
other playlist's three channels, and remembered expansion after a process
restart. This UI smoke did not start a network stream.

## Behaviour reference

The local TiviMate research (`~/src/tivimate/docs/03-settings-reference.md`,
Appearance → Groups) documents All playlists visibility, grouping by playlist,
and a playlist-name selection policy for All channels versus Favorites.
[Firsthand reports of the guide interaction](https://www.reddit.com/r/TiviMate/comments/1n5zf3y/)
describe selecting a playlist name in the groups column and OK collapsing its
categories. [Reports about expansion settings](https://www.reddit.com/r/TiviMate/comments/19c7w1p/)
describe remembered expanded/collapsed state. The local runtime observations
used one generated playlist with a Premium gate, so they do not independently
verify the complete multi-playlist interaction. This implementation adopts the
behaviour of browsable playlist sections using the app's existing components;
it does not reproduce reference assets, wording or visual layouts.
