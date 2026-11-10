package com.hantekllc.iptvrs.host

import android.content.Context
import android.graphics.Color
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.SurfaceView
import android.view.View
import android.widget.FrameLayout
import kotlin.math.max
import kotlin.math.roundToInt
import androidx.annotation.OptIn
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.VideoSize
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.datasource.HttpDataSource
import androidx.media3.datasource.ResolvingDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import com.longbridge.nsh.NshComponent
import com.longbridge.nsh.NshComponentContext

/**
 * `tv.player`: one persistent Media3 player on a SurfaceView, aspect-fit in a
 * black frame. The Rust app lays it out like any view and draws its UI above
 * it; changing layout never recreates the decoder.
 *
 * Props:
 * - `source`: `{url, headers: {name: value}, token}` or null (stops). A new
 *   url or token reopens the stream (the token forces a retry of the same
 *   url). Headers go with every request; identity headers (Cookie,
 *   Authorization, SN, X-User-Agent) only to the stream's own host.
 * - `paused` (bool), `userAgent` (fallback when the source sends none),
 * - `pictureSize` ("normal", "full", "zoom", "cinema", "reduced", "4:3",
 *   "16:9", "2.35:1"; sizes the surface by layout, never by transform).
 *
 * Events: `state` {state: idle|buffering|ready|ended}, `error` {code, http?}
 * (code is Media3's error code name; no URLs or messages), `size` {width,
 * height}, `position` {ms} (from where the stream opened; once a second
 * while playing). Recovers by itself from BEHIND_LIVE_WINDOW, and retries once with
 * the other kind of stream (a container it could not parse as HLS, a
 * playlist it could not parse as progressive); everything else goes to the
 * app, which owns the retry policy. The picture is black from a new source
 * or an error until the next first frame.
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
    private val root = PictureFrame(context).apply {
        setBackgroundColor(Color.BLACK)
        addView(surface, FrameLayout.LayoutParams(-1, -1, Gravity.CENTER))
        addView(shutter, FrameLayout.LayoutParams(-1, -1, Gravity.CENTER))
    }

    private var userAgent = DEFAULT_UA
    private var url: String? = null
    private var token: Long = 0
    private var sourceHeaders: Map<String, String> = emptyMap()
    @Volatile private var headers = Headers(null, emptyMap())
    /** Set after the one format fallback (HLS or progressive) for the current stream. */
    private var triedOtherFormat = false
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
        return ExoPlayer.Builder(context)
            .setMediaSourceFactory(
                DefaultMediaSourceFactory(context).setDataSourceFactory(withHeaders),
            )
            .build()
            .also { p ->
                p.setVideoSurfaceView(surface)
                // Live streams that end are reconnected by the app.
                p.repeatMode = Player.REPEAT_MODE_OFF
                p.playWhenReady = true
                p.addListener(listener)
            }
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
            "userAgent" -> {
                userAgent = (value as? String)?.takeIf { it.isNotBlank() } ?: DEFAULT_UA
                applyHeaders()
            }
        }
    }

    private fun setSource(source: Map<*, *>?) {
        val next = source?.get("url") as? String
        val nextToken = (source?.get("token") as? Number)?.toLong() ?: 0
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
        shutter.visibility = View.VISIBLE
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

    private fun start(target: String, mime: String?) {
        val item = MediaItem.Builder().setUri(target)
        if (mime != null) item.setMimeType(mime)
        player.setMediaItem(item.build())
        player.prepare()
    }

    override fun destroy() {
        ticker.removeCallbacks(tick)
        player.removeListener(listener)
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
