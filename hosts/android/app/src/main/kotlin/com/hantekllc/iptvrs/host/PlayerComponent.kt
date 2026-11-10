package com.hantekllc.iptvrs.host

import android.content.Context
import android.graphics.Color
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.Gravity
import android.view.SurfaceView
import android.view.View
import android.widget.FrameLayout
import kotlin.math.max
import kotlin.math.roundToInt
import androidx.annotation.OptIn
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.Tracks
import androidx.media3.common.VideoSize
import androidx.media3.common.text.CueGroup
import androidx.media3.common.util.UnstableApi
import androidx.media3.common.util.Util
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.datasource.HttpDataSource
import androidx.media3.datasource.ResolvingDataSource
import androidx.media3.exoplayer.DecoderReuseEvaluation
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.analytics.AnalyticsListener
import androidx.media3.exoplayer.audio.AudioSink
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.ui.SubtitleView
import com.longbridge.nsh.NshComponent
import com.longbridge.nsh.NshComponentContext

/**
 * `tv.player`: one persistent Media3 player on a SurfaceView, aspect-fit in a
 * black frame. The Rust app lays it out like any view and draws its UI above
 * it; changing layout never recreates the decoder.
 *
 * Props:
 * - `source`: `{url, headers: {name: value}, token, live, reconnect}` or
 *   null (stops). A new url or token reopens the stream (the token forces a
 *   retry of the same url). Headers go with every request; identity headers
 *   (Cookie, Authorization, SN, X-User-Agent) only to the stream's own host.
 *   `live` (a live channel, not catch-up): a progressive live stream plays
 *   on across a connection the server closes ([start]). `reconnect`: the
 *   app reopens the stream that played (a retry), so the last frame stays
 *   instead of the black shutter; so does a new token for the same url.
 * - `paused` (bool), `userAgent` (fallback when the source sends none),
 * - `pictureSize` ("normal", "full", "zoom", "cinema", "reduced", "4:3",
 *   "16:9", "2.35:1"; sizes the surface by layout, never by transform).
 * - `videoTrack` ("auto" or a video track id), `audioTrack` ("auto" or an
 *   audio track id), `textTrack` ("off" or a text track id), for the open
 *   stream ([PlayerTracks]; a new source starts again from auto/auto/off).
 * - `muted` (bool; the volume is the system's: `volume` is ignored here),
 *   `audioDelay` (ms, positive: the sound later; [DelayedAudioSink]).
 *
 * Events: `state` {state: idle|buffering|ready|ended}, `error` {code, http?}
 * (code is Media3's error code name; no URLs or messages), `size` {width,
 * height}, `position` {ms} (from where the stream opened; once a second
 * while playing), `tracks` (whenever they change; [PlayerTracks.event])
 * and `format` {fps?, channels?} for what plays. Subtitles of the chosen
 * text track show in a [SubtitleView] over the picture. Each component has
 * its own player and state (several may play at once: multiview mounts one
 * per tile, two on a TV box by default, and mutes all but the active one;
 * a component's [destroy] releases its decoders). Recovers by itself from
 * BEHIND_LIVE_WINDOW, and retries once with
 * the other kind of stream (a container it could not parse as HLS, a
 * playlist it could not parse as progressive); everything else goes to the
 * app, which owns the retry policy. The picture is black from a new source
 * or an error until the next first frame; a reconnect of the same stream
 * keeps the last frame.
 *
 * Live HLS needs nothing here: Media3 reloads a media playlist from the
 * address it was given (a provider's redirect to a tokenized address is
 * followed again on every reload, so an expired token never shows), not
 * from where the redirect led.
 */
@OptIn(UnstableApi::class)
class PlayerComponent(
    context: Context,
    private val nsh: NshComponentContext,
) : NshComponent {
    companion object {
        const val NAME = "tv.player"
        private const val DEFAULT_UA = "iptv-rs/0.1 (Media3)"
        private val IDENTITY = setOf("cookie", "authorization", "sn", "x-user-agent")
        /**
         * A live item that played shorter than this before the server closed
         * it gets no further spare: its end goes to the app, which backs off.
         */
        private const val MIN_BRIDGED_MS = 5_000L
    }

    /** Headers for the current stream; read on loader threads. */
    private class Headers(val host: String?, val all: Map<String, String>)

    private val surface = SurfaceView(context)
    /**
     * Black over the picture from a new source (or an error) until the
     * stream's first frame: the surface would keep the last channel's frame.
     */
    private val shutter = View(context).apply {
        setBackgroundColor(Color.BLACK)
        visibility = View.GONE
    }
    /** The chosen text track's cues, over the picture. */
    private val subtitles = SubtitleView(context).apply {
        setUserDefaultStyle()
        setUserDefaultTextSize()
    }
    private val root = PictureFrame(context).apply {
        setBackgroundColor(Color.BLACK)
        addView(surface, FrameLayout.LayoutParams(-1, -1, Gravity.CENTER))
        addView(subtitles, FrameLayout.LayoutParams(-1, -1, Gravity.CENTER))
        addView(shutter, FrameLayout.LayoutParams(-1, -1, Gravity.CENTER))
    }

    private var userAgent = DEFAULT_UA
    private var url: String? = null
    private var token: Long = 0
    /** The source is a live channel ([start]'s spare items). */
    private var live = false
    /** When the playing item started (a spare that closes too soon gets none). */
    private var itemStartedAt = 0L
    private var sourceHeaders: Map<String, String> = emptyMap()
    @Volatile private var headers = Headers(null, emptyMap())
    /** Set after the one format fallback (HLS or progressive) for the current stream. */
    private var triedOtherFormat = false
    /** The viewer's track choices for the open stream. */
    private var choice = PlayerTracks.Choice()
    /** This player's audio sink (made when the player is built). */
    private var sink: DelayedAudioSink? = null
    private var audioDelayUs = 0L
    private val player: ExoPlayer

    private fun headersFor(uri: Uri): Map<String, String> {
        val h = headers
        val own = uri.host != null && uri.host.equals(h.host, ignoreCase = true)
        return if (own) h.all else h.all.filterKeys { it.lowercase() !in IDENTITY }
    }

    private fun build(context: Context): ExoPlayer {
        // No factory User-Agent: it would override the per-stream one.
        val http = DefaultHttpDataSource.Factory()
            .setUserAgent(null)
            .setAllowCrossProtocolRedirects(true)
            .setConnectTimeoutMs(8_000)
            .setReadTimeoutMs(15_000)
        val withHeaders = ResolvingDataSource.Factory(http) { spec ->
            spec.withAdditionalHeaders(headersFor(spec.uri))
        }
        // The audio sink carries the audio delay (one per player).
        val renderers = object : DefaultRenderersFactory(context) {
            override fun buildAudioSink(
                context: Context,
                enableFloatOutput: Boolean,
                enableAudioTrackPlaybackParams: Boolean,
            ): AudioSink? {
                val base = super.buildAudioSink(context, enableFloatOutput, enableAudioTrackPlaybackParams)
                    ?: return null
                return DelayedAudioSink(base).also {
                    it.delayUs = audioDelayUs
                    sink = it
                }
            }
        }
        return ExoPlayer.Builder(context, renderers)
            .setMediaSourceFactory(
                DefaultMediaSourceFactory(context).setDataSourceFactory(withHeaders),
            )
            .build()
            .also { p ->
                p.setVideoSurfaceView(surface)
                // Live streams that end are reconnected by the app.
                p.repeatMode = Player.REPEAT_MODE_OFF
                p.playWhenReady = true
                // Subtitles only when the viewer picks a text track.
                p.trackSelectionParameters = p.trackSelectionParameters.buildUpon()
                    .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
                    .build()
                p.addListener(listener)
                p.addAnalyticsListener(formats)
            }
    }

    /** The viewer's choices on the stream's current tracks. */
    private fun applyChoice() {
        val next = PlayerTracks.parameters(player.trackSelectionParameters, player.currentTracks, choice)
        if (next != player.trackSelectionParameters) player.trackSelectionParameters = next
    }

    private fun emitFormat(video: Format?, audio: Format?) {
        val format = PlayerTracks.format(video, audio)
        if (format.isNotEmpty()) nsh.emit("format", format)
    }

    /** Frame rate and audio channels as the decoders get them. */
    private val formats = object : AnalyticsListener {
        override fun onVideoInputFormatChanged(
            eventTime: AnalyticsListener.EventTime,
            format: Format,
            decoderReuseEvaluation: DecoderReuseEvaluation?,
        ) = emitFormat(format, null)

        override fun onAudioInputFormatChanged(
            eventTime: AnalyticsListener.EventTime,
            format: Format,
            decoderReuseEvaluation: DecoderReuseEvaluation?,
        ) = emitFormat(null, format)
    }

    private val listener = object : Player.Listener {
        override fun onPlaybackStateChanged(state: Int) {
            val name = when (state) {
                Player.STATE_IDLE -> "idle"
                Player.STATE_BUFFERING -> "buffering"
                Player.STATE_READY -> "ready"
                Player.STATE_ENDED -> "ended"
                else -> "unknown"
            }
            nsh.emit("state", mapOf("state" to name))
        }

        override fun onRenderedFirstFrame() {
            shutter.visibility = View.GONE
            // Again for each stream: the app forgets the last one's.
            val size = player.videoSize
            if (size.width > 0 && size.height > 0) {
                nsh.emit("size", mapOf("width" to size.width, "height" to size.height))
            }
            emitFormat(player.videoFormat, player.audioFormat)
        }

        override fun onMediaItemTransition(mediaItem: MediaItem?, reason: Int) {
            if (reason != Player.MEDIA_ITEM_TRANSITION_REASON_AUTO || mediaItem == null) return
            // The server closed a live stream and the spare plays on: drop
            // what played, and queue the next spare.
            val now = SystemClock.elapsedRealtime()
            val played = now - itemStartedAt
            itemStartedAt = now
            val index = player.currentMediaItemIndex
            if (index > 0) player.removeMediaItems(0, index)
            if (played >= MIN_BRIDGED_MS) player.addMediaItem(mediaItem)
        }

        override fun onTracksChanged(tracks: Tracks) {
            applyChoice()
            nsh.emit("tracks", PlayerTracks.event(tracks, choice))
        }

        override fun onCues(cueGroup: CueGroup) {
            subtitles.setCues(cueGroup.cues)
        }

        override fun onVideoSizeChanged(size: VideoSize) {
            if (size.width > 0 && size.height > 0) {
                root.videoAspect = size.width * size.pixelWidthHeightRatio / size.height
                nsh.emit("size", mapOf("width" to size.width, "height" to size.height))
            }
        }

        override fun onPlayerError(error: PlaybackException) {
            when {
                error.errorCode == PlaybackException.ERROR_CODE_BEHIND_LIVE_WINDOW -> {
                    player.seekToDefaultPosition()
                    player.prepare()
                    return
                }
                // The address's extension can lie (`.ts` serving HLS, `.m3u8`
                // serving MP4): one retry per source with the other kind.
                !triedOtherFormat && isContainerError(error.errorCode) -> {
                    triedOtherFormat = true
                    url?.let { start(it, MimeTypes.APPLICATION_M3U8) }
                    return
                }
                !triedOtherFormat &&
                    error.errorCode == PlaybackException.ERROR_CODE_PARSING_MANIFEST_MALFORMED -> {
                    triedOtherFormat = true
                    // Progressive: the extractors sniff the container.
                    url?.let { start(it, MimeTypes.VIDEO_UNKNOWN) }
                    return
                }
            }
            shutter.visibility = View.VISIBLE
            val event = mutableMapOf<String, Any>("code" to error.errorCodeName)
            httpStatus(error)?.let { event["http"] = it }
            nsh.emit("error", event)
        }
    }

    /** Reports where the stream is once a second while it plays (catch-up seeking). */
    private val ticker = Handler(Looper.getMainLooper())
    private val tick = object : Runnable {
        override fun run() {
            if (player.isPlaying) {
                nsh.emit("position", mapOf("ms" to player.currentPosition.coerceAtLeast(0)))
            }
            ticker.postDelayed(this, 1_000)
        }
    }

    // After `listener` is initialised: build() registers it.
    init {
        player = build(context)
        ticker.postDelayed(tick, 1_000)
    }

    override fun view(): View = root

    override fun setProp(name: String, value: Any?) {
        when (name) {
            "source" -> setSource(value as? Map<*, *>)
            "paused" -> player.playWhenReady = value != true
            "pictureSize" -> root.mode = (value as? String) ?: "normal"
            "videoTrack" -> choose(choice.copy(video = (value as? String) ?: PlayerTracks.AUTO))
            "audioTrack" -> choose(choice.copy(audio = (value as? String) ?: PlayerTracks.AUTO))
            "textTrack" -> choose(choice.copy(text = (value as? String) ?: PlayerTracks.OFF))
            "muted" -> player.volume = if (value == true) 0f else 1f
            "audioDelay" -> {
                audioDelayUs = ((value as? Number)?.toLong() ?: 0L).coerceIn(-10_000L, 10_000L) * 1000
                sink?.delayUs = audioDelayUs
            }
            "userAgent" -> {
                userAgent = (value as? String)?.takeIf { it.isNotBlank() } ?: DEFAULT_UA
                applyHeaders()
            }
        }
    }

    private fun choose(next: PlayerTracks.Choice) {
        if (next == choice) return
        choice = next
        applyChoice()
        nsh.emit("tracks", PlayerTracks.event(player.currentTracks, choice))
    }

    private fun setSource(source: Map<*, *>?) {
        val next = source?.get("url") as? String
        val nextToken = (source?.get("token") as? Number)?.toLong() ?: 0
        live = source?.get("live") == true
        // The same stream again (a retry): keep its last frame.
        val keepFrame = next != null && (next == url || source?.get("reconnect") == true)
        sourceHeaders = (source?.get("headers") as? Map<*, *>)
            ?.entries
            ?.mapNotNull { (k, v) -> (k as? String)?.let { key -> (v as? String)?.let { key to it } } }
            ?.toMap()
            ?: emptyMap()
        val changed = next != url || nextToken != token
        url = next
        token = nextToken
        applyHeaders()
        if (!changed) return
        triedOtherFormat = false
        // A new stream: default tracks, no old cues.
        choice = PlayerTracks.Choice()
        applyChoice()
        subtitles.setCues(emptyList())
        if (!keepFrame) shutter.visibility = View.VISIBLE
        if (next == null) {
            player.stop()
            player.clearMediaItems()
        } else {
            start(next, null)
        }
    }

    private fun applyHeaders() {
        val all = LinkedHashMap(sourceHeaders)
        if (all.keys.none { it.equals("User-Agent", ignoreCase = true) }) {
            all["User-Agent"] = userAgent
        }
        headers = Headers(url?.let { Uri.parse(it).host }, all)
    }

    /**
     * Plays `target`. A live progressive stream (MPEG-TS over HTTP) ends when
     * the server closes the connection, which providers do every minute or
     * so: it plays as two items of the same address, and once the first is
     * fully read the player opens the second while the buffered rest plays,
     * then switches without a gap ([listener]'s `onMediaItemTransition`
     * queues the next spare).
     */
    private fun start(target: String, mime: String?) {
        val builder = MediaItem.Builder().setUri(target)
        if (mime != null) builder.setMimeType(mime)
        val item = builder.build()
        val progressive =
            Util.inferContentTypeForUriAndMimeType(Uri.parse(target), mime) == C.CONTENT_TYPE_OTHER
        if (live && progressive) {
            player.setMediaItems(listOf(item, item))
        } else {
            player.setMediaItem(item)
        }
        itemStartedAt = SystemClock.elapsedRealtime()
        player.prepare()
    }

    override fun destroy() {
        ticker.removeCallbacks(tick)
        player.removeListener(listener)
        player.removeAnalyticsListener(formats)
        player.release()
    }
}

private fun isContainerError(code: Int) =
    code == PlaybackException.ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED ||
        code == PlaybackException.ERROR_CODE_PARSING_CONTAINER_MALFORMED

/** The HTTP status behind a playback error, if a server answered one. */
private fun httpStatus(error: PlaybackException): Int? {
    var cause: Throwable? = error.cause
    while (cause != null) {
        if (cause is HttpDataSource.InvalidResponseCodeException) return cause.responseCode
        cause = cause.cause
    }
    return null
}

/**
 * Sizes and centres the video surface for a picture-size mode; whatever
 * exceeds the frame is clipped by it. Layout only (the SurfaceView is never
 * scaled or translated), so the video surface composites normally.
 */
private class PictureFrame(context: Context) : FrameLayout(context) {
    var videoAspect = 16f / 9f
        set(value) {
            if (value > 0f && value != field) {
                field = value
                requestLayout()
            }
        }
    var mode = "normal"
        set(value) {
            if (value != field) {
                field = value
                requestLayout()
            }
        }

    override fun onMeasure(widthSpec: Int, heightSpec: Int) {
        val pw = MeasureSpec.getSize(widthSpec)
        val ph = MeasureSpec.getSize(heightSpec)
        setMeasuredDimension(pw, ph)
        if (pw <= 0 || ph <= 0) return
        val (w, h) = pictureSize(mode, pw.toFloat(), ph.toFloat(), videoAspect)
        for (i in 0 until childCount) {
            getChildAt(i).measure(
                MeasureSpec.makeMeasureSpec(w, MeasureSpec.EXACTLY),
                MeasureSpec.makeMeasureSpec(h, MeasureSpec.EXACTLY),
            )
        }
    }

    override fun onLayout(changed: Boolean, left: Int, top: Int, right: Int, bottom: Int) {
        val pw = right - left
        val ph = bottom - top
        for (i in 0 until childCount) {
            val c = getChildAt(i)
            val x = (pw - c.measuredWidth) / 2
            val y = (ph - c.measuredHeight) / 2
            c.layout(x, y, x + c.measuredWidth, y + c.measuredHeight)
        }
    }
}

/** The surface size for `mode` in a `pw`×`ph` frame showing video of aspect `va`. */
internal fun pictureSize(mode: String, pw: Float, ph: Float, va: Float): Pair<Int, Int> {
    val pa = pw / ph
    fun fit(a: Float) = if (a > pa) pw to pw / a else ph * a to ph
    fun cover(a: Float) = if (a > pa) ph * a to ph else pw to pw / a
    fun scaled(p: Pair<Float, Float>, k: Float) = p.first * k to p.second * k
    val (w, h) = when (mode) {
        "full" -> pw to ph
        "zoom" -> cover(va)
        // A wide film letterboxed in the video: zoom until it fills the frame.
        "cinema" -> scaled(fit(va), max(1f, 2.35f / va))
        "reduced" -> scaled(fit(va), 0.85f)
        "4:3" -> fit(4f / 3f)
        "16:9" -> fit(16f / 9f)
        "2.35:1" -> fit(2.35f)
        else -> fit(va)
    }
    return w.roundToInt() to h.roundToInt()
}
