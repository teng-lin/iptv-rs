# Android video layering, live playback, and remote input: research notes

Research date: 2026-11-09. Scope: inform the NSH (Longbridge Native Shell) IPTV app for
Android TV and tablets. The planned design: a Media3/ExoPlayer video View registered as an
NSH native component, kept as ONE persistent video surface at the bottom of the NshView
FrameLayout. All UI is drawn above it, so preview, guide-over-video, and full screen are layout
changes and never restart the decoder.

## How to read the evidence labels

| Label | Meaning |
| --- | --- |
| **[V]** | Verified: stated in an official doc, AOSP source, or Media3 source that I read. A URL or file is given. |
| **[C]** | Verified from code: I read this in an open-source app's code at the commit listed below. It shows what that app does, not that the approach is right. |
| **[R]** | Reported: a claim in a third-party code comment or issue that I could not check independently. |
| **[I]** | Inference: my own reasoning. It is not confirmed by a source and must be tested. |

Open-source code was read at these commits (shallow clones, November 2026):

| Project | Commit |
| --- | --- |
| [jellyfin/jellyfin-androidtv](https://github.com/jellyfin/jellyfin-androidtv) | `aa24978` |
| [yuliskov/SmartTube](https://github.com/yuliskov/SmartTube) | `2bcad2c` |
| [android/tv-samples](https://github.com/android/tv-samples) (JetStream, Leanback, ReferenceApp) | `15ac8df` |
| [anilbeesetti/nextplayer](https://github.com/anilbeesetti/nextplayer) | `9120e5e` |
| [oxyroid/M3UAndroid](https://github.com/oxyroid/M3UAndroid) (M3U/Xtream, TV + phone) | `32f3658` |
| [yaoxieyoulei/mytv-android](https://github.com/yaoxieyoulei/mytv-android) (live IPTV, Compose for TV) | `575aa36` |
| [FongMi/TV](https://github.com/FongMi/TV) (VOD + live, leanback and mobile flavors) | `4512b4c` |
| [oerd/OwnTV](https://github.com/oerd/OwnTV) (M3U/Xtream/Stalker IPTV for Android TV, Media3 + libmpv) | `0f8724b` |
| [androidx/media](https://github.com/androidx/media) (Media3 `PlayerView`, `MediaCodecVideoRenderer`) | `8c6678b` |
| [flutter/packages](https://github.com/flutter/packages) (`video_player_android`) | `08faf64` |
| [xbmc/xbmc](https://github.com/xbmc/xbmc) `tools/android/packaging/xbmc/src` | `master`, fetched 2026-11-09 |
| AOSP `frameworks/base` (`ViewRootImpl`, `ViewConfiguration`, `TvView`), `frameworks/native` (`InputDispatcher.cpp`), `packages/apps/TV` (`TvViewUiManager`) | `main`, fetched 2026-11-09 |

---

## 1. Video layering: SurfaceView vs TextureView

### 1.1 What the platform and Media3 say

- **[V] Media3 prefers SurfaceView.** Its listed advantages are: much lower power use on many
  devices, more accurate frame timing, better HDR output, secure output for DRM content, and,
  on Android TV, *rendering at the display's full resolution on devices that upscale the UI
  layer*. Media3 suggests TextureView only below API 24, and only when you need smooth
  animation or scrolling of the video.
  <https://developer.android.com/media/media3/ui/surface>
- **[V] TextureView can add up to 30% to total power draw** during video playback on some
  devices. <https://developer.android.com/media/media3/exoplayer/battery-consumption>
- **[V] AOSP graphics docs:** "In API 24 and higher, it's recommended to implement SurfaceView
  instead of TextureView." Also: "SurfaceView has performance advantages when compositing UI
  elements layered over videos." With TextureView the GPU composites the content into the view
  hierarchy, so "every visible pixel is composited twice". "Video players that support protected
  content must be implemented with SurfaceView."
  <https://source.android.com/docs/core/graphics/arch-tv>
- **[V] The TV UI-layer resolution trap.** Some Android TV devices render the UI layer below panel
  resolution (for example 1080p UI on a 4K panel) and upscale it. Only a SurfaceView reaches
  full panel resolution, because a TextureView is part of the UI layer. To compare, use
  `Util.getCurrentDisplayModeSize` (panel) against `Display.getSize` (UI layer).
  <https://developer.android.com/media/media3/ui/surface>
- **[V] TextureView** needs a hardware-accelerated window. Its contents are copied internally
  from the underlying surface into the view, and it "may have slower performance than
  SurfaceView". <https://developer.android.com/reference/android/view/TextureView>

### 1.2 How UI above a SurfaceView behaves (the core of our design)

- **[V] Hole punching.** "The surface is Z ordered so that it is behind the window holding its
  SurfaceView; the SurfaceView punches a hole in its window to allow its surface to be
  displayed. The view hierarchy will take care of correctly compositing with the Surface any
  siblings of the SurfaceView that would normally appear on top of it." The cost: "a full
  alpha-blended composite will be performed each time the Surface changes".
  <https://developer.android.com/reference/android/view/SurfaceView>
- **[V] Caveat on transforms.** "The transparent region that makes the surface visible is based
  on the layout positions in the view hierarchy. If the post-layout transform properties are
  used to draw a sibling view on top of the SurfaceView, the view may not be properly composited
  with the surface." (Same page.) For NSH, overlays animated with
  `translationX/Y`/`scale` over the video need testing. Layout-driven movement is the safe path.
- **[V] Z-order controls.**
  - Default: the surface is behind the window, so the window's views draw over it. This is our case.
  - `setZOrderMediaOverlay(true)`: the surface sits above another normal SurfaceView, still behind the window.
  - `setZOrderOnTop(true)`: the surface sits above the whole window, so the UI could not draw over the video.

  We want the default. (SurfaceView reference above. The Compose equivalents
  `AndroidExternalSurfaceZOrder.Behind / MediaOverlay / OnTop` are described at
  <https://developer.android.com/reference/kotlin/androidx/compose/foundation/AndroidExternalSurface.composable>.)
- **[V] Alpha.** Before Android 14, the platform ignored alpha values between 0 and 1 on a
  SurfaceView. From Android 14 (API 34) it supports arbitrary alpha blending (SurfaceView
  reference). So to fade or dim the video on older boxes, put a semi-transparent black View
  above it. Do not animate the SurfaceView's alpha.
- **[C] Every mainstream player uses translucent UI over a SurfaceView.**
  - The Media3 `PlayerView` puts its controller and shutter views above a SurfaceView in the same
    `FrameLayout` (`androidx/media` `PlayerView.java`).
  - Leanback's `PlaybackSupportFragment` dims video with `BG_DARK`/`BG_LIGHT` over a SurfaceView
    (`VideoSurfaceView`, `PlaybackSupportFragment.java` in SmartTube's leanback fork).
  - NextPlayer draws a 30% black scrim over a `SURFACE_TYPE_SURFACE_VIEW` `PlayerSurface`
    (`PlayerContentFrame.kt`).
  - FongMi's live screen overlays a channel/group list with a translucent background drawable
    over a full-screen PlayerView (`app/src/leanback/res/layout/activity_live.xml`).

  This is the pattern we plan for NSH.
- **[R] Overlays and the hardware overlay path.** OwnTV's code comments claim that putting
  *any* view over a SurfaceView, "even an empty one", knocks it off the hardware-overlay /
  direct scan-out path on some TVs, which "stutters 4K to a ~2 fps slideshow under GPU
  composition". OwnTV therefore mounts its subtitle view only while subtitles are on
  (`player/MpvVideoSurface.kt`, `player/LivePreviewEngine.kt`). I could not verify this and it
  conflicts with the AOSP statement above. **[I]** The likely mechanism is the Hardware Composer
  running out of planes or scaling capability and falling back to GPU (client) composition,
  which at 4K is costly on Mali-450/G31-class GPUs. This is the single most important thing to
  measure on our target boxes (see Risks).
- **[V] `gatherTransparentRegion`** is "used by the ViewRoot to perform an optimization when the
  view hierarchy contains one or several SurfaceView" (View reference). **[I]** A UI that covers
  the video with fully transparent, layout-positioned views is the friendliest case for this
  optimization. Large translucent full-screen layers are the least friendly.

### 1.3 What requires SurfaceView

- **[V]** DRM secure output and protected content: SurfaceView only (AOSP arch-tv; Media3 surface page).
- **[V]** Tunneled playback: output goes only to a SurfaceView. FongMi's code switches the
  render type to SurfaceView when tunneling is turned on, and turns tunneling off when the user
  picks TextureView (`setting/PlayerSetting.java`, `DecodeSetting.java`) **[C]**.
- **[V]** Higher-quality HDR output and full-panel-resolution output on TVs whose UI layer is
  upscaled (Media3 surface page).
- **[I]** Frame-rate hints (`Surface.setFrameRate`) apply per surface. They work most naturally
  on a SurfaceView, where the video is its own layer.

### 1.4 TextureView cost on low-end TV SoCs (Amlogic / MediaTek / Realtek)

- **[V]** General costs: up to +30% power, double composition of every visible pixel, less
  accurate frame timing, and capped at UI-layer resolution (sources above).
- **[I]** On boxes with weak GPUs (for example Mali-450 or G31 with shared LPDDR3/4), a 4K or
  1080p60 stream through TextureView means a GPU texture sample plus a full-frame composition
  per video frame. That competes with the UI for memory bandwidth. Expect more dropped frames
  and a guide that scrolls less smoothly than with SurfaceView. I found no authoritative
  benchmark specific to Amlogic, MediaTek or Realtek.
- **[C]** Apps keep TextureView only as an escape hatch:
  - SmartTube defaults to SurfaceView and uses TextureView only when the user enables it or
    rotation is needed. The default is TextureView only on API ≤ 19
    (`common/.../prefs/PlayerTweaksData.java`, `SurfacePlaybackFragment.java`).
  - FongMi defaults to `RENDER_SURFACE` with a user toggle.
  - Counter-examples: Google's JetStream sample (`VideoPlayerScreen.kt`) and M3UAndroid's TV app
    (`TvPlayerScreen.kt`) use `SURFACE_TYPE_TEXTURE_VIEW` in Compose. JetStream is a demo, not a
    performance reference. **[I]** Compose apps likely choose TextureView to avoid SurfaceView
    and Compose interop issues (see 1.5).

### 1.5 What changed across Android versions

| Version | Change | Status |
| --- | --- | --- |
| Android 7.0 (API 24) | SurfaceView position updates are synchronized with other View rendering. "Translating and scaling a SurfaceView on screen will not cause rendering artifacts." Before this, a SurfaceView lagged behind or showed black during animation. | **[V]** SurfaceView reference; Media3 surface page |
| Android 10 (API 29) | Public `SurfaceControl` / `SurfaceControl.Transaction`; `SurfaceView.getSurfaceControl()` for re-parenting child surfaces. | **[V]** API presence checked in the SDK `android.txt` API files 28→29. Do not apply transactions to the SurfaceView's own control except re-parenting children (reference doc). |
| Android 11 (API 30) | `Surface.setFrameRate`; `SurfaceView.setChildSurfacePackage` / `SurfaceControlViewHost`. | **[V]** frame-rate guide; API files |
| Android 12 (API 31) | `setFrameRate` with a `changeFrameRateStrategy` argument (seamless vs always); the user setting "Match content frame rate"; `MediaCodec` asynchronous queueing on by default in Media3. | **[V]** frame-rate guide; Media3 customization page |
| Android 14 (API 34) | `SurfaceView.setSurfaceLifecycle(SURFACE_LIFECYCLE_FOLLOWS_ATTACHMENT)`, which keeps the surface alive while the view is attached even if it is not visible. Media3 `PlayerView` turns this on automatically on 34+. Arbitrary SurfaceView alpha. `applyTransactionToFrame`. | **[V]** Media3 surface page; SurfaceView reference; API files |
| Android 14 (API 34) only | Bug: when a SurfaceView inside a resizing parent (reported with Compose `AndroidView`) resizes, its surface can be "stretched/cropped/leaked". Media3 has `PlayerView.setEnableComposeSurfaceSyncWorkaround`, which uses a `SurfaceSyncGroup` on API 34 only. The source says "only present on API 34, fixed on API 35". | **[V]** `PlayerView.java` (`SurfaceSyncGroupCompatV34`), androidx/media#1237 |
| Android 15 (API 35) | `SurfaceView.setDesiredHdrHeadroom`. | **[V]** API files |

**[I]** The question mentions "synchronization since Android 10/14". The real animation-sync
milestone was API 24. Android 12's BLAST buffer queue and Android 14's sync groups refined
*resize* synchronization. Bug #1237 shows that resize sync on API 34 can still glitch. Chromecast
with Google TV and Google TV Streamer class devices run Android 12–14, so API 34 is a real target.

---

## 2. Moving and resizing video without restarting playback

### 2.1 Primitives

- **[V] `PlayerView.switchTargetView(player, old, new)`** attaches the new view *before*
  detaching the old one, "because this ordering allows the player to swap directly from one
  surface to another, without transitioning through a state where no surface is attached"
  (`PlayerView.java`). With raw views, follow the same order: `setVideoSurfaceView(new)`, then
  release the old one.
- **[V] Swapping surfaces keeps the codec when possible.** `MediaCodecVideoRenderer.setOutput`
  calls `MediaCodec.setOutputSurface` when the codec supports it. When it does not (a
  hard-coded device list, for example several Xiaomi MiBox/MiTV devices up to API 28, issues
  google/ExoPlayer#5169, #8014 and others), it releases and re-creates the codec. With no
  surface at all, ExoPlayer can render into a `PlaceholderSurface` instead of tearing down
  (`MediaCodecVideoRenderer.java`, `evaluateDeviceNeedsSetOutputSurfaceWorkaround`). So even a
  surface swap is cheap on most devices, but **one persistent surface avoids the question
  entirely.**
- **[V] SurfaceView lifecycle.** By default the surface is created while the view is visible and
  destroyed when it is hidden. On API 34+, `SURFACE_LIFECYCLE_FOLLOWS_ATTACHMENT` ties it to
  attachment. Before 34, Media3 suggests translating views off-screen rather than hiding them
  (Media3 surface page). **[C]** Leanback's `VideoSurfaceView` overrides
  `setTransitionVisibility()` as a no-op, because activity transitions toggled visibility and
  caused flicker (`androidx/leanback/widget/VideoSurfaceView.java`).
- **[V] Resizing by re-layout is the established TV pattern.**
  - **[C]** AOSP Live TV (`packages/apps/TV`, `TvViewUiManager.java`) animates the main
    `TvView` (which wraps a SurfaceView, see `TvView.java`) "by repeatedly re-layouting
    TvView". Its comment says scale and translation "do not work" on the SurfaceView inside
    TvView. It calls `setFixedSurfaceSize(w, h)` with the target size, so the surface buffer
    does not change on every animation frame.
  - **[C]** Jellyfin's in-player Live TV guide calls `VideoManager.contractVideo(300dp)`. That
    changes `FrameLayout.LayoutParams` (width, height, margins) of the `PlayerView` to shrink
    the video into a corner while the guide grid is shown, and `setVideoFullSize(true)` restores
    it on hide (`CustomPlaybackOverlayFragment.showGuide/hideGuide`, `VideoManager.java`).
    The player is not touched.
  - **[C]** Kodi positions its video `SurfaceView` by setting `RelativeLayout` margins
    (`XBMCVideoView.setSurfaceRect`). Its comment: "Android does not allow to adjust AR and/or
    size of a frame on a Surface. Instead, we render fullscreen on the surface and adjust the
    size of the surface for AR". It also notes that negative margins for zoom "doesn't work on
    Philips/Sony TV's".

### 2.2 Known glitches and how apps avoid them

| Glitch | Cause | Mitigation seen |
| --- | --- | --- |
| Black flash on channel change | The player is reset or the shutter closes before the first frame of the new stream | **[V]** `PlayerView` `keep_content_on_player_reset` keeps the last frame. The shutter opens on `onRenderedFirstFrame`. **[C]** FongMi's `Player.Live` style sets `keep_content_on_player_reset=true`. **[C]** mytv-android uses a bare `SurfaceView` instead of `PlayerView`; comment: PlayerView flickers black when switching video. |
| Aspect "jump" | The new stream's `VideoSize` (including pixel aspect ratio) is known only after decode starts; MediaCodec scales to fit the surface | **[V]** `PlayerView` sizes an `AspectRatioFrameLayout` from `onVideoSizeChanged` (`width * pixelWidthHeightRatio / height`). **[I]** In NSH, keep the video view's aspect equal to the stream aspect, letterboxed by NSH layout. Cover the area with an opaque/blurred placeholder until the first frame of a new channel. |
| Stretched or cropped frame during resize on API 34 | Platform resize-sync bug | **[V]** `setEnableComposeSurfaceSyncWorkaround` (API 34 only). **[I]** NSH is not Compose, but the bug is in SurfaceView resize sync, so test animated resizes on API 34. |
| Surface destroyed when the view is hidden or detached | Default lifecycle | Keep the video view attached and VISIBLE. On API 34+ use `FOLLOWS_ATTACHMENT`. Cover it with an opaque view instead of hiding it. |
| Freeze or failure after back-to-back 4K sessions on one surface | **[R]** OwnTV: some Realtek decoders throw `0x80001000` when reusing a "dirty" surface. Some Hisense panels leak the 4K decoder unless the player is fully released. | **[C]** OwnTV keys its `SurfaceView` on a "surface generation" counter and rebuilds it after releasing a UHD decoder. **[I]** Keep a "recreate surface" escape hatch. |
| Decoder restart on surface switch | Device on the `setOutputSurface` workaround list | **[V]** Unavoidable on those devices. Avoid surface switches. |

### 2.3 Other preview-to-full-screen strategies seen

- **[C] OwnTV:** a separate, muted Media3 player feeds a SurfaceView in the channel-list preview
  pane. Going full screen either "promotes" the preview (same ExoPlayer engine, re-hosted on a
  full-screen SurfaceView) or hands off to mpv. Preview and full screen use separate SurfaceViews
  on separate screens ("so the two decoders never share a surface"). Media3's frame-rate hints
  are disabled in the preview "so the preview must never reconfigure the display while the user
  is just scrolling" (`ExoPreviewSurface`, `LivePreviewEngine.kt`).
- **[R] OwnTV muting:** a volume of 0 does not mute AC-3/E-AC-3/DTS *passthrough*, because
  `AudioTrack.setVolume` has no effect on an IEC61937 bitstream. A muted preview therefore
  deselects the audio track type. **[I]** This is plausible and cheap to guard against.
- **[C] Jellyfin's guide over live video** is a View overlay on top of a PlayerView in the same
  `FrameLayout`. The video is contracted, not paused or re-attached.

---

## 3. How open-source apps do it

| App | Surface type | How guide or overlay UI is composed | Preview → full screen |
| --- | --- | --- | --- |
| **Jellyfin for Android TV** | `PlayerView` (default `surface_view`) in `vlc_player_interface.xml`. The newer playback module uses a raw `SurfaceView` wrapped in a `FrameLayout` (`PlayerSurfaceView.kt`), hosted in Compose via `AndroidView` (`PlayerSurface.kt`) **[C]** | Live TV guide = View tree (`overlay_tv_guide.xml`: channel `ScrollView` + program grid with semi-opaque backgrounds) shown over the player in the same `FrameLayout` **[C]** | Shrinks the `PlayerView` with `LayoutParams` (`contractVideo` / `setVideoFullSize`), no player restart **[C]**. Refresh-rate switching uses `Display.getSupportedModes()` + a mode-matching heuristic, with user-selectable behavior (`PlaybackController.findBestDisplayMode`) **[C]** |
| **SmartTube** | SurfaceView by default; TextureView via a "tweak" or when rotation is needed; switching type restarts the engine (`SurfacePlaybackFragment.setRotation`) **[C]** | Forked Leanback `PlaybackSupportFragment` with controls overlay over `VideoSurfaceView` **[C]** | Full-screen player only, plus system PiP (`PlaybackActivity.enterPipMode`) **[C]** |
| **Kodi (Android)** | Two SurfaceViews: GUI is a GLES `SurfaceView` with `setZOrderOnTop(true)` + `PixelFormat.TRANSPARENT` (`XBMCMainView`); video `SurfaceView` with `setZOrderMediaOverlay(true)` + transparent format (`XBMCVideoView`) **[C]** | GUI drawn into its own transparent top surface | Video rectangle set by layout margins (`setSurfaceRect`) **[C]** |
| **Google AOSP Live TV** (`packages/apps/TV`) | `TvView` → internal `SurfaceView` (`TvView.java`) **[V]** | Side panels and menus as Views above `TvView` | Animates `TvView` layout params per frame; fixed surface size during the animation (`TvViewUiManager`) **[C]** |
| **TIF `TvView` / `TvInputService`** | `TvView` exposes `setZOrderMediaOverlay/OnTop` passthroughs to its `SurfaceView` **[V]** | App Views over `TvView`; the input service renders into the provided `Surface` | Re-layout of the `TvView` |
| **Leanback `VideoSupportFragment`** (tv-samples `ReferenceAppKotlin`) | `SurfaceView` (`lb_video_surface.xml`) via `SurfaceHolderGlueHost` **[C]** | Playback controls row over video with dimmed background (`BG_DARK`) **[C]** | Full screen only |
| **JetStream (Compose for TV sample)** | Media3 `PlayerSurface(surfaceType = SURFACE_TYPE_TEXTURE_VIEW)`; replaced `AndroidView` in Feb 2025 **[C]** | Compose overlay in the same `Box` | Full screen only |
| **NextPlayer** | `PlayerSurface(SURFACE_TYPE_SURFACE_VIEW)` with `graphicsLayer` zoom and pan applied to the surface **[C]** | Compose scrim + subtitle view + shutter over the surface **[C]** | Full screen + PiP |
| **M3UAndroid** (M3U/Xtream) | TV: `PlayerSurface(SURFACE_TYPE_TEXTURE_VIEW)`; phone: `PlayerView` in `AndroidView` **[C]** | Compose controls with 78%-opaque rounded panels over video **[C]** | Separate player screen |
| **mytv-android** (live IPTV) | Raw `SurfaceView` in `AndroidView`, sized with `Modifier.aspectRatio(...)`; avoids `PlayerView` because of black flicker on channel switch **[C]** | Compose channel list, EPG and info panels over video | Video always full screen; panels overlay it |
| **FongMi TV** (live) | `PlayerView` with `surface_type="none"` in XML, then `setRender(SURFACE or TEXTURE)` at runtime (fork API); default SurfaceView **[C]** | Channel/group `RecyclerView`s with translucent background over the full-screen video `FrameLayout` **[C]** | Video always full screen; `keep_content_on_player_reset` for live; uses `DefaultPreloadManager` **[C]** |
| **OwnTV** (M3U/Xtream/Stalker) | Raw `SurfaceView`s in Compose `AndroidView`, keyed for re-creation **[C]** | Compose; avoids mounting views over the surface unless needed **[C]/[R]** | Separate preview SurfaceView in the list pane; full screen uses another SurfaceView ("promote" or mpv handoff) **[C]** |

**Takeaways.** **[C]** Every serious TV player uses SurfaceView by default. The two apps closest to
our design, Jellyfin's live guide and AOSP Live TV, already do "one surface, re-layout it under
a View overlay". **[I]** No app I read does an *animated* morph from guide preview to full screen
on a single surface while UI is composited above it. Jellyfin snaps instantly, and AOSP Live TV
animates margins. Our design is a straightforward extension of proven patterns, but the animated
morph is the least-proven part.

---

## 4. Live IPTV playback tuning in Media3

### 4.1 Buffering and LoadControl

- **[V] Current `DefaultLoadControl` defaults (Media3 reference, 2026):**
  - `DEFAULT_BUFFER_FOR_PLAYBACK_MS = 1000` (start or resume after seek)
  - `DEFAULT_BUFFER_FOR_PLAYBACK_AFTER_REBUFFER_MS = 2000`
  - `DEFAULT_MIN_BUFFER_MS = DEFAULT_MAX_BUFFER_MS = 50000`
  - `DEFAULT_VIDEO_BUFFER_SIZE = 131072000` bytes (about 125 MiB), with the target computed from
    the selected tracks
  - `DEFAULT_PRIORITIZE_TIME_OVER_SIZE_THRESHOLDS = false`
  - Back buffer 0

  Configure through `DefaultLoadControl.Builder().setBufferDurationsMs(min, max, start, rebuffer)`
  and `setTargetBufferBytes`. A separate local-playback variant also exists.
  <https://developer.android.com/reference/androidx/media3/exoplayer/DefaultLoadControl>
  **[I]** Older ExoPlayer guides quote 2500/5000 ms start thresholds. The modern defaults are
  already "fast zap" values.
- **[C] What IPTV apps do:**
  - Jellyfin passes user-configurable durations, falling back to the defaults.
  - FongMi multiplies min/max by a user "buffer" factor and keeps the default start thresholds.
  - OwnTV, tuned for raw MPEG-TS live: start 1 s, rebuffer 2 s, min = latency preset (8 s
    "balanced"), max = min + 2 s, target bytes 24 MiB (16 MiB on low-RAM devices)
    (`LiveLatency.kt`, `LivePreviewEngine.kt`).
- **[R] OwnTV's reasoning** for a narrow max − min window on raw TS: on a single long-lived
  HTTP response, ExoPlayer stops reading the socket when the buffer is full. "Provider
  restreamers/proxies cull a connection that sits idle that long", and EOF on a source with no
  duration surfaces as STATE_ENDED or an IO error. HLS does not have this problem because each
  segment is a separate request.
- **[I] Byte cap.** On 1–2 GB RAM boxes, cap `targetBufferBytes` (16–32 MiB). The default
  per-track video allocation is sized for VOD on phones.

### 4.2 LiveConfiguration

- **[V]** Live offset and speed parameters: `targetOffsetMs`, `minOffsetMs`, `maxOffsetMs`,
  `minPlaybackSpeed`, `maxPlaybackSpeed`. Set them per `MediaItem.LiveConfiguration`, or
  globally with `DefaultMediaSourceFactory.setLiveTargetOffsetMs`. Speed adjustment is used for
  low-latency streams and can be disabled with min = max = 1.0.
  `DefaultLivePlaybackSpeedControl` adds `targetLiveOffsetIncrementOnRebufferMs`.
  <https://developer.android.com/media/media3/exoplayer/live-streaming>
- **[V]** "Unlike adaptive livestreams, progressive livestreams don't have a live window and can
  only be played at one position." LiveConfiguration therefore does nothing for raw `.ts` over
  HTTP, which is the common Xtream live format (same page). **[C]** OwnTV reached the same
  conclusion and drives raw-TS latency through LoadControl instead.
- **[R]** OwnTV's notes on the Media3 source: for plain (non-LL) HLS with no hold-back tags,
  Media3 disables speed adjustment unless the app sets the speeds. Setting min/max speed
  *enables* a live-edge chase on those playlists.

### 4.3 HLS vs MPEG-TS over HTTP

- **[V]** ExoPlayer supports HLS (TS, fMP4, CMAF, AES-128, LL-HLS) and progressive MPEG-TS.
  HLS chunkless preparation is on by default and needs `CODECS` in `EXT-X-STREAM-INF`. Disabling
  it slows startup. For live, providers should use `EXT-X-PROGRAM-DATE-TIME` and a long window.
  <https://developer.android.com/media/media3/exoplayer/hls>,
  <https://developer.android.com/media/media3/exoplayer/progressive>
- **[V]** Content type is inferred from the URI. For HLS without `.m3u8`, set
  `MimeTypes.APPLICATION_M3U8` (HLS page).
- **[C] Container fallback is standard in IPTV apps.**
  - mytv-android: on `ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED`, retry as HLS, then as progressive.
  - M3UAndroid: on the parsing/manifest error family, remembers a per-channel MIME type.
  - OwnTV: retries a `.ts` URL that redirects to a manifest through `HlsMediaSource`, and
    remembers "known HLS hosts".
- **[C]** OwnTV sets the TS extractor flag `FLAG_OVERRIDE_CAPTION_DESCRIPTORS` to expose CEA-608.
  **[I]** Also consider `FLAG_ALLOW_NON_IDR_KEYFRAMES` / `FLAG_DETECT_ACCESS_UNITS` for broadcast
  H.264 TS that starts without an IDR frame, and test zap time with and without them.

### 4.4 Headers, User-Agent, redirects

- **[V]** `DefaultHttpDataSource.Factory` has `setUserAgent` (default `null` = platform UA),
  `setDefaultRequestProperties`, `setConnectTimeoutMs`/`setReadTimeoutMs`, and
  **`setAllowCrossProtocolRedirects` (default `false`)**. IPTV redirects between http and https
  are common, so set it to true.
  <https://developer.android.com/reference/androidx/media3/datasource/DefaultHttpDataSource.Factory>
- **[V]** For per-request or per-channel headers, wrap the upstream in
  `ResolvingDataSource.Factory` and use `dataSpec.withRequestHeaders(...)`. Inject the result via
  `DefaultMediaSourceFactory(context).setDataSourceFactory(...)`. Customize retries with a
  `LoadErrorHandlingPolicy` (`DefaultLoadErrorHandlingPolicy.getRetryDelayMsFor`).
  <https://developer.android.com/media/media3/exoplayer/customization>
- **[C]** M3UAndroid and OwnTV use `OkHttpDataSource.Factory(...).setUserAgent(ua)` with
  per-channel headers. mytv-android sets the UA and cross-protocol redirects on the default HTTP
  source. **[R]** OwnTV retries once with a fallback UA when a provider rejects the default one.

### 4.5 Error recovery

- **[V] `ERROR_CODE_BEHIND_LIVE_WINDOW`:** call `player.seekToDefaultPosition(); player.prepare()`
  (live-streaming page). **[C]** mytv-android and M3UAndroid do exactly this.
- **[I] Recovery policy for live:**
  - Network or IO errors: reconnect with backoff, tied to connectivity callbacks.
  - HTTP 401/403: surface as "provider refused"; no tight retry loop.
  - Raw-TS `STATE_ENDED`: treat as a dropped connection and re-prepare.
  - Watchdogs for "position advancing but no frames" and "frames frozen": OwnTV implements both
    using a frame-metadata hook **[C]**.

### 4.6 Fast zapping

- **[V]** `DefaultPreloadManager` (a shared LoadControl arrived in Media3 1.8, per the Android
  Developers blog; I did not check which version introduced the class) pre-buffers items
  ranked by distance from the current one. It is `@UnstableApi`. Players must come from the
  manager's builder (`buildExoPlayer()`).
  <https://developer.android.com/media/media3/exoplayer/preloading-media/preloadmanager>
  **[C]** FongMi uses it.
- **[I]** For *live* channels, preloading neighbors holds open connections. Many providers
  enforce a limit of 1–2 concurrent connections per account, so a preloaded neighbor can kill
  the playing stream. A preloaded raw-TS buffer also goes stale. Make it opt-in, HLS-only, and
  limited to channel +1 and −1. Most zap time is the HTTP connect plus waiting for the first
  keyframe, and preloading does not help with the keyframe wait.
- **[V]** Asynchronous `MediaCodec` queueing is on by default on API 31+ and can be forced on
  from API 23 with `DefaultRenderersFactory.forceEnableMediaCodecAsynchronousQueueing()`. The
  docs suggest this for devices that drop frames (customization page).
- **[C]** Jellyfin uses `setEnableDecoderFallback(true)` and the FFmpeg extension renderer mode.
  **[I]** Broadcast TS often carries MPEG-1 Layer II, AC-3 or E-AC-3 audio. Bundle the FFmpeg
  audio decoder extension for boxes without those decoders.

### 4.7 Tunneling and AFR (frame-rate / display-mode switching)

- **[V] Tunneling:** `DefaultTrackSelector.Parameters...setTunnelingEnabled(true)`. ExoPlayer
  turns it on only when the renderers and tracks support it. It "moves all video decoding and
  release timing to a dedicated hardware processor", but "performance and reliability can vary
  greatly between devices". Test it on each target device.
  <https://developer.android.com/media/media3/exoplayer/track-selection>,
  <https://developer.android.com/media/media3/exoplayer/battery-consumption>
  **[C]** M3UAndroid, FongMi and SmartTube offer it as an opt-in setting, default off.
- **[V] Frame rate:**
  - `Surface.setFrameRate(fps, FRAME_RATE_COMPATIBILITY_FIXED_SOURCE, strategy)` (API 30, strategy argument API 31).
  - By default the framework will not perform non-seamless switches (a black screen for 1–2 s on
    set-top boxes). Non-seamless switching requires the user setting "Match content frame rate"
    *and* the app passing `CHANGE_FRAME_RATE_ALWAYS`.
  - Google recommends `ALWAYS` for long videos only, and "strongly recommend[s] against"
    `ALWAYS` for playback of a few minutes or less.
  - Clear the hint with `setFrameRate(0)` when the surface stays visible without new frames.
  - `preferredDisplayModeId` is the tool for heavy switches (resolution change) on TV.

  <https://developer.android.com/media/optimize/performance/frame-rate>,
  <https://developer.android.com/training/tv/playback/framerate>
- **[V]** Media3's `VideoFrameReleaseHelper` calls `Surface.setFrameRate` itself, with default
  strategy `VIDEO_CHANGE_FRAME_RATE_STRATEGY_ONLY_IF_SEAMLESS`. Control it with
  `ExoPlayer.setVideoChangeFrameRateStrategy(...)` (`VideoFrameReleaseHelper.java`).
  **[C]/[R]** OwnTV sets `VIDEO_CHANGE_FRAME_RATE_STRATEGY_OFF` when AFR is off and in the preview
  pane, because "some vendor stacks advertise a seamless switch but visibly re-handshake HDMI".
- **[I]** For live TV (frequent zapping), default to ONLY_IF_SEAMLESS in full screen, OFF in
  preview, and offer an opt-in "AFR on full screen" setting that uses `preferredDisplayModeId`,
  like Jellyfin's mode matcher.

---

## 5. Remote input on Android TV

- **[V] Required keys.** The app must be usable with up, down, left, right, select, Back and
  Home. Selection = `KEYCODE_DPAD_CENTER`, `ENTER`, `NUMPAD_ENTER`, `BUTTON_A`, `BUTTON_SELECT`.
  Back = `KEYCODE_BACK`, `BUTTON_B`. During playback, the selection keys act as play/pause.
  "The Back button must never act as a toggle." Declare
  `configChanges="keyboard|keyboardHidden|navigation"` so Bluetooth remote reconnects do not
  restart the activity. Pointer remotes: `android.software.leanback.supports_touch` property.
  <https://developer.android.com/training/tv/get-started/controllers>
  **[C]** Jellyfin treats `DPAD_CENTER` and `ENTER` identically everywhere, and treats
  `BUTTON_B`/`ESCAPE` as Back.
- **[V] How long-press works:**
  - `InputDispatcher` sets `FLAG_LONG_PRESS` on the first synthesized auto-repeat
    (`repeatCount == 1`) (`InputDispatcher.cpp`).
  - The first repeat fires after `ViewConfiguration.getKeyRepeatTimeout()`, which defaults to
    `DEFAULT_LONG_PRESS_TIMEOUT = 400` ms, then repeats every 50 ms
    (`ViewConfiguration.java`; user-adjustable through secure settings).
  - Framework pattern: on `ACTION_DOWN`, call `event.startTracking()` and return true. The system
    then calls `onKeyLongPress`, or you check `event.isLongPress()`. On `ACTION_UP`, check
    `FLAG_CANCELED_LONG_PRESS` to tell a short press from the release after a long press.

  **[C]** Jellyfin does exactly this for OK long-press (program options / favorite)
  (`CustomPlaybackOverlayFragment.onKey`).
  **[I]** Some IR remotes and receivers send repeated down/up pairs instead of holding the key
  down. Then no auto-repeat and no long-press happen. Keep a timer-based fallback in the NSH key
  pipeline, and never make long-press the only way to reach a function.
- **[V] Media keys and MediaSession:**
  - Media keys go first to the foreground activity's `onKeyDown`. If unhandled, Android routes
    them to the media session (on API 26+, the last session that played audio).
  - "When you use a MediaSession, don't override the handling of media-specific buttons"
    (controllers page). The media-buttons guide says the same.

  <https://developer.android.com/media/legacy/media-buttons>
  **[I]** Use a Media3 `MediaSession` for play/pause/stop. Let NSH UI handle
  `KEYCODE_CHANNEL_UP/DOWN`, `KEYCODE_GUIDE`, `KEYCODE_INFO`, `KEYCODE_MENU`,
  `KEYCODE_LAST_CHANNEL`, digits 0–9 (numeric zapping with a ~1.5 s commit timer), and
  `KEYCODE_PROG_RED/GREEN/YELLOW/BLUE`, `KEYCODE_CAPTIONS`
  (<https://developer.android.com/reference/android/view/KeyEvent>). Also map
  `MEDIA_NEXT/PREVIOUS` and `PAGE_UP/DOWN` to channel ±, because many box remotes have no
  channel keys. Some TVs give `GUIDE`/`CHANNEL_*` to the system TV app when the app does not
  handle them. Test on each device.
- **[V] The first key after touch is swallowed.** In `ViewRootImpl`,
  `EarlyPostImeInputStage.processKeyEvent` calls `checkForLeavingTouchModeAndConsume`. This runs
  *before* `ViewPostImeInputStage` calls `mView.dispatchKeyEvent`. The rules:
  - It applies only to `ACTION_DOWN`/`ACTION_MULTIPLE` while in touch mode.
  - It does not apply if the event has `FLAG_KEEP_TOUCH_MODE`.
  - It applies to "navigation keys": D-pad (including CENTER), PAGE_UP/DOWN, MOVE_HOME/END, TAB,
    SPACE and ENTER, with no modifiers.
  - The key is consumed only if leaving touch mode *assigns new focus*. `leaveTouchMode()`
    returns false, so the key is not consumed, when a non-ViewGroup view already has focus.
  - Typing keys leave touch mode but are delivered.

  So on a tablet, the first D-pad press after a tap only "wakes" focus, and the app never sees
  it in `dispatchKeyEvent`. **[V]** `dispatchKeyEventPreIme` runs in an earlier stage
  (`ViewPreImeInputStage`), so NshView can see the key there. **[I]** Mitigations:
  1. Keep a real focusable leaf view focused, so `leaveTouchMode` does not reassign focus.
  2. Or route keys to Rust from `NshView.dispatchKeyEventPreIme`, but only when no text field
     has focus, because this steals keys from the IME.
  3. On TVs without touch, the window normally starts outside touch mode. Verify with
     `View.isInTouchMode()`.

---

## 6. How cross-platform frameworks embed native video

- **Flutter `video_player`:**
  - **[V]** The default is a texture: an ExoPlayer frame into a Flutter `SurfaceProducer`,
    composited by Flutter's GPU, the same trade-off as TextureView.
  - **[C]** The optional `VideoViewType.platformView` mode creates a real `SurfaceView` platform
    view (`platformview/PlatformVideoView.java`). It calls `setZOrderMediaOverlay(true)` on
    API ≤ 25 to "avoid blank space instead of a video on Android versions below 8". It re-binds
    the surface in `onVisibilityChanged` because the surface comes and goes with visibility.
  - **[V]** Flutter's platform-view docs:
    - Texture Layer Hybrid Composition is janky on fast scroll and loses SurfaceView accessibility.
    - Hybrid Composition gives "correct accessibility and SurfaceView support" but merges the
      raster and platform threads, which costs FPS.
    - HCPP (experimental, API 34+, Vulkan) uses native transaction sync.
    - `SurfaceView`/`SurfaceTexture` may need manual `invalidate`.

    <https://docs.flutter.dev/platform-integration/android/platform-views>,
    <https://pub.dev/documentation/video_player/latest/>
- **react-native-video:** **[V]** `viewType`: `surfaceView` (default, "taking fewer resources"),
  `textureView` (transforms and 3D; "DRM playback is not supported"), `secureView` (secure
  surface). The DRM prop forces SurfaceView. The deprecated `useTextureView` docs list two
  SurfaceView limits: it cannot be animated, transformed or scaled, and multiple SurfaceViews
  cannot be overlaid.
  <https://docs.thewidlarzgroup.com/react-native-video/docs/v6/component/props>
  **[I]** "Can't be animated" is outdated for API 24+ (see 1.5), but the overlay limit still applies.
- **Jetpack Compose:**
  - **[V]** `AndroidExternalSurface` (SurfaceView-backed) is recommended over
    `AndroidEmbeddedExternalSurface` (TextureView-backed) "whenever possible", because the
    system compositor "can bypass the GPU".
  - **[V]** Media3 provides `PlayerSurface`/`ContentFrame` with
    `SURFACE_TYPE_SURFACE_VIEW`/`TEXTURE_VIEW`. Media3 also warns that `PlayerView` inside
    `AndroidView` is not guaranteed (the API 34 stretch bug).

  (Compose reference above; Media3 surface page.)

**[I] What this means for NSH.** NSH draws with real Android Views inside a FrameLayout. That is
already the "hybrid composition" model, without the extra thread-merging cost: the video View is
a true sibling in the view hierarchy, and the platform's SurfaceView hole punching does the
compositing. NSH does not need a texture path like Flutter's or a z-order trick like Kodi's.
It needs three things:
1. The video View is the first, bottom child.
2. It stays attached and visible.
3. Overlays are positioned by layout rather than post-layout transforms where they overlap video.

The frameworks that built texture paths (Flutter's default, RN's legacy default, JetStream's
choice) did so for transform freedom, not because texture paths are better for TV.

---

## 7. Recommendations for our design

### 7.1 Surface type to start with

- **Start with `SurfaceView`** as a raw view handed to ExoPlayer with `setVideoSurfaceView`, or
  a `PlayerView` with `use_controller=false` if we want its aspect and shutter logic.
- On API 34+, call `setSurfaceLifecycle(SURFACE_LIFECYCLE_FOLLOWS_ATTACHMENT)`.
- Keep the default z-order. Never call `setZOrderOnTop`. Use `setZOrderMediaOverlay(true)` only
  if NSH ever places another SurfaceView in the same window, and on API ≤ 25 (the Flutter
  workaround).
- Keep a hidden **TextureView fallback setting**, like SmartTube and FongMi, for devices where
  SurfaceView misbehaves. Tie it to "tunneling off". Switching the type requires a re-attach; it
  is not a hot path.

### 7.2 Structuring preview, overlay and full screen

1. **One `NshVideoView` (FrameLayout → SurfaceView)** registered as the NSH native component.
   It is always the bottom child of NshView. It is never removed, never re-parented, and never
   GONE or INVISIBLE. Re-parenting would detach it and destroy the surface. To "hide" video,
   cover it with an opaque NSH view, or move it off-screen by layout. Optionally deselect the
   video track or pause decoding while covered.
2. **States are layout rectangles** computed in Rust: `Preview(rect)`, `GuideOverVideo(full)`,
   `Fullscreen(full)`. Change only `LayoutParams` (position and size), like Jellyfin and AOSP
   Live TV. Drive animations as per-frame layout updates, or snap. Avoid
   `translation`/`scale` on the overlays that sit above the video, and on the video view itself
   while UI is composited over it.
3. **Aspect handling in NSH layout.** Keep the SurfaceView's aspect equal to the video's display
   aspect (`VideoSize.width * pixelWidthHeightRatio / height`) inside the target rect,
   letterboxed with NSH black bars. MediaCodec scales to fit the surface. For animated
   transitions, consider `holder.setFixedSize(videoW, videoH)` so the buffer does not reallocate
   every frame (the AOSP Live TV approach), then measure.
4. **Channel change:** keep the last frame on the surface (no shutter) and overlay a small
   "tuning" indicator. Re-fit the aspect on `onVideoSizeChanged`, and remove placeholders on
   `onRenderedFirstFrame`.
5. **Overlays:** transparent NSH containers wherever video should show. Use translucent scrims
   (black at 40–80% alpha) for the guide-over-video state. Dim with a View, not SurfaceView alpha
   (pre-34 ignores it). Rounded corners on the preview: draw corner masks in UI color above the
   video, rather than relying on SurfaceView clipping **[I]**.
6. **Preview audio:** mute by deselecting the audio track type (the passthrough issue), not only
   with volume 0.
7. **Frame-rate hints:** set `VIDEO_CHANGE_FRAME_RATE_STRATEGY_OFF` in the preview and guide
   states, and `ONLY_IF_SEAMLESS` (or the user's AFR choice) in full screen.

### 7.3 Starter Media3 configuration for live IPTV (illustrative)

```kotlin
// Illustrative only. Tune on real boxes and keep values in one config object.
val http = DefaultHttpDataSource.Factory()
    .setUserAgent(sourceUserAgent)            // per playlist or source; null = platform default
    .setAllowCrossProtocolRedirects(true)     // default is false
    .setConnectTimeoutMs(8_000).setReadTimeoutMs(15_000)
val dataSource = ResolvingDataSource.Factory(http) { spec ->
    spec.withRequestHeaders(headersFor(spec.uri)) // per-channel headers, redacted in logs
}
val loadControl = DefaultLoadControl.Builder()
    .setBufferDurationsMs(/*min*/ 8_000, /*max*/ 12_000, /*start*/ 1_000, /*rebuffer*/ 2_000)
    .setTargetBufferBytes(if (lowRam) 16 shl 20 else 32 shl 20)
    .setPrioritizeTimeOverSizeThresholds(false)
    .build()
val renderers = DefaultRenderersFactory(context)
    .setEnableDecoderFallback(true)
    .setExtensionRendererMode(EXTENSION_RENDERER_MODE_ON) // if the FFmpeg audio extension is bundled
val player = ExoPlayer.Builder(context, renderers)
    .setMediaSourceFactory(DefaultMediaSourceFactory(context).setDataSourceFactory(dataSource)
        .setLiveTargetOffsetMs(8_000))        // HLS/DASH only; ignored for raw TS
    .setLoadControl(loadControl)
    .setHandleAudioBecomingNoisy(true)
    .build()
player.setVideoChangeFrameRateStrategy(C.VIDEO_CHANGE_FRAME_RATE_STRATEGY_OFF) // until full screen
player.trackSelectionParameters = player.trackSelectionParameters.buildUpon()
    .setMaxVideoSize(displayW, displayH).build()
// Tunneling: opt-in setting only, via DefaultTrackSelector parameters setTunnelingEnabled(true).
// Item: MediaItem.Builder().setUri(url).setMimeType(guessOrRemembered) // M3U8 vs TS
// Errors: BEHIND_LIVE_WINDOW -> seekToDefaultPosition()+prepare();
//         PARSING_CONTAINER_UNSUPPORTED -> retry with the other MIME type once, remember per channel;
//         IO/HTTP -> backoff reconnect; raw-TS STATE_ENDED -> re-prepare.
```

Sample URLs in tests and fixtures must use `example.com` / `example.test` or a local mock server.

### 7.4 Risks to test on a real TV box

Run each on at least one Amlogic box (S905X2/X3/X4 class), one MediaTek-based Google TV device,
one Realtek-based TV or box if available, and an API 34 device. Record dropped frames
(`AnalyticsListener.onDroppedVideoFrames`), `dumpsys SurfaceFlinger` composition type
(DEVICE/overlay vs CLIENT/GPU), and power or thermals where possible.

1. **Overlay composition cost:** 4K60 and 1080p50 live TS with (a) no UI over the video,
   (b) a small transparent overlay, (c) a full-screen 60% scrim plus an animating guide grid.
   Does SurfaceFlinger fall back to CLIENT composition and stutter, as OwnTV reports?
2. **Hole punch inside NshView:** test the video under NSH containers with backgrounds, a
   parent with `alpha < 1` or a hardware layer, nested clipping, and focus highlights that draw
   outside their bounds. Confirm the video still shows and the UI blends correctly.
3. **Animated preview → full screen:** run per-frame layout animation, both with and without
   `setFixedSize`. Look for lag, black frames, stretch frames, and aspect jumps on API 28–33 and
   on API 34 (the #1237 class).
4. **Surface survival:** check that the surface survives NSH navigation, re-layouts, activity
   pause/resume, screensaver/Daydream, HDMI-CEC standby, and system PiP. Use
   `surfaceCreated/Destroyed` logs to confirm there is no unexpected destroy, which would mean a
   codec release.
5. **Channel zapping:** measure time to first frame for HLS and raw TS. Look for black flashes
   between channels, the last frame persisting, and new-aspect re-fit. Try 30 zaps in a row to
   expose decoder leaks (the "UHD decoder slot" issue).
6. **Device quirks:** check whether the device is on Media3's `setOutputSurface` workaround
   list (surface swap = codec restart). Check whether back-to-back 4K sessions on one surface fail.
7. **AFR / mode switches:** the default seamless strategy must not trigger HDMI re-handshakes
   while previewing. Test opt-in AFR in full screen (24/25/50/59.94 content). Check the black
   gap and the UI layer resolution after a switch.
8. **Tunneling:** turn it on per device and check A/V sync, first-frame rendering, resize while
   tunneled, and channel changes.
9. **Audio:** check passthrough AC-3/E-AC-3 (muting in preview, switching to stereo), MP2-audio
   TS streams, and the decoder fallback path.
10. **Remote input:** test long-press OK on IR vs Bluetooth remotes (auto-repeat present?), and
    whether CHANNEL/GUIDE/INFO keys arrive at all or are stolen by the system. Check the
    numeric-entry timing, and the first-key-swallowed behavior on a tablet after touch
    (dispatchKeyEventPreIme path).
11. **Memory:** on 1–2 GB RAM boxes, check heap and native memory with the capped
    `targetBufferBytes` during long sessions (more than 2 h live). Watch for OOM-killer events
    while the guide images load.
12. **Connection limits:** check that preloading or next-channel prefetch never opens a second
    concurrent provider connection unless it is enabled. Use a local mock server that enforces
    a limit of one connection.

---

## Source index (official docs)

- Media3 surface choice: <https://developer.android.com/media/media3/ui/surface>
- Media3 battery: <https://developer.android.com/media/media3/exoplayer/battery-consumption>
- Media3 live streaming: <https://developer.android.com/media/media3/exoplayer/live-streaming>
- Media3 customization (headers, LoadErrorHandlingPolicy, async queueing): <https://developer.android.com/media/media3/exoplayer/customization>
- Media3 track selection (tunneling): <https://developer.android.com/media/media3/exoplayer/track-selection>
- Media3 HLS / progressive: <https://developer.android.com/media/media3/exoplayer/hls>, <https://developer.android.com/media/media3/exoplayer/progressive>
- DefaultLoadControl: <https://developer.android.com/reference/androidx/media3/exoplayer/DefaultLoadControl>
- DefaultHttpDataSource.Factory: <https://developer.android.com/reference/androidx/media3/datasource/DefaultHttpDataSource.Factory>
- PreloadManager: <https://developer.android.com/media/media3/exoplayer/preloading-media/preloadmanager>
- SurfaceView: <https://developer.android.com/reference/android/view/SurfaceView>
- TextureView: <https://developer.android.com/reference/android/view/TextureView>
- AOSP graphics, SurfaceView/TextureView: <https://source.android.com/docs/core/graphics/arch-sv-glsv>, <https://source.android.com/docs/core/graphics/arch-tv>
- Frame rate: <https://developer.android.com/media/optimize/performance/frame-rate>, <https://developer.android.com/training/tv/playback/framerate>
- TV controllers: <https://developer.android.com/training/tv/get-started/controllers>
- Media buttons: <https://developer.android.com/media/legacy/media-buttons>
- KeyEvent: <https://developer.android.com/reference/android/view/KeyEvent>
- Compose external surfaces: <https://developer.android.com/reference/kotlin/androidx/compose/foundation/AndroidExternalSurface.composable>
- Flutter platform views: <https://docs.flutter.dev/platform-integration/android/platform-views>
- react-native-video props: <https://docs.thewidlarzgroup.com/react-native-video/docs/v6/component/props>
- Media3 issue: <https://github.com/androidx/media/issues/1237>
