# Playback and source transport policy

Server and portal addresses without a scheme default to HTTPS. HTTP remains
available when the viewer explicitly supplies an `http://` address; onboarding
and saved-source details show “Not encrypted” and explain the exposure. The app
does not silently retry HTTPS sources over HTTP. Network redirects, HLS resource
URIs and DASH resource URLs that downgrade HTTPS to HTTP are refused.

Android's network-security configuration permits explicit HTTP because source
domains are user supplied and cannot be enumerated in a static manifest. It also
permits the app's loopback player connection. The Rust transport enforces the
per-request downgrade policy on every platform. Its TLS verifier uses public
Mozilla roots; Android's declarative trust configuration is not a replacement
for that native verifier.

Both Media3 and mpv use one bounded loopback transport per player. Only the
transport sends upstream identity headers: Authorization, Cookie,
Proxy-Authorization, SN and X-User-Agent. They stay on the original stream's
scheme, host and effective port, including redirects, HLS variant/segment/key/map
requests and DASH resources. Players receive no global identity headers. mpv also rejects decoder-selected top-level references outside the current relay capability before opening them, and disables automatic sidecar discovery. The
loopback URL contains a random capability and must never be logged, exported or
persisted. A replacement stream cancels its old transport and active requests;
the persistent video surface is unchanged.

Playback accepts HTTP and HTTPS resources; non-HTTP schemes and DRM key schemes are not supported by this transport. URL basic authentication is retained only for the original origin. Response cookies persist only on that exact origin, including its port. Cookie storage is capped at 64 entries and 16 KiB per player; oversized or excess new cookies are ignored while updates to existing cookies remain possible.

Progressive media is streamed with backpressure. Supported adaptive manifests
are HLS and DASH (with DASH HTTP clock/steering extensions explicitly refused); unknown textual playlist formats (such as ASX, PLS and
SmoothStreaming, sidecar SRT/TTML and multipart MJPEG) fail explicitly instead of letting a decoder follow URLs
outside the policy. WebVTT subtitle segments remain readable. This narrows
mpv's formerly unrestricted manifest auto-detection; additional formats require
a dedicated parser and tests before they can be enabled. Manifests are limited to 2 MiB,
rewritten output to 16 MiB, incoming request headers to 16 KiB, and simultaneous
connections to twelve per player. At most two manifest buffers are live across
all players; each manifest has a thirty-second body deadline and local writes
have a fifteen-second deadline. Redirects are limited to ten; connection and
idle-read timeouts are eight and fifteen seconds. Whole-stream duration is not
limited, because a live channel can run indefinitely.

OS backup is separate from user-initiated exports. Android disables backup and
explicitly excludes every storage domain from both cloud backup and device
transfer, including device-protected storage, plus legacy backup rules. iOS
marks the app data directory and temporary restore/backup directories as
excluded from OS backup. User-initiated backup files retain their existing
explicit export behavior.

Desktop startup requires an absolute HOME, makes app directories 0700 and files
0600, and sets a restrictive process mask before opening storage. Existing
files, including SQLite sidecars and copied playlists, are repaired at startup;
symbolic links in private app storage are refused rather than followed.

## Synthetic Android validation

On `tv36`, the app imported a generated one-channel M3U and played HLS in
secondary user 10. The same synthetic two-origin journey passed on `tab36` in
its primary user. The host supplied that user's files directory (0700), and
the synthetic catalog remained separate from user 0. A two-origin local fixture
recorded Authorization, Cookie, SN and X-User-Agent on the original redirect and
manifest requests, and none on the second origin's MPEG-TS segment requests.
The fixture override is available only in debug-assertion builds.

Android's local backup transport refused the package with “Backup is not
allowed”, both normally and with `is_device_transfer=true`. This exercises the
emulator's transfer transport mode, not a physical device migration. User 10 had
no registered backup transport; the backup attempts used user 0. JVM tests also
verify manifest references and exclusion rules for every cloud/transfer domain.
