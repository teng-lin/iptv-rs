package com.hantekllc.iptvrs.host

import android.content.Context
import android.graphics.Color
import android.view.Gravity
import android.view.SurfaceView
import android.view.View
import android.widget.FrameLayout
import kotlin.math.max
import kotlin.math.roundToInt
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.VideoSize
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import com.longbridge.nsh.NshComponent
import com.longbridge.nsh.NshComponentContext

/**
 * `tv.player`: one persistent Media3 player on a SurfaceView, aspect-fit in a
 * black frame. The Rust app lays it out like any view and draws its UI above
 * it; changing layout never recreates the decoder.
 *
 * Props: `url` (string, null stops), `paused` (bool), `userAgent` (string),
 * `pictureSize` ("normal", "full", "zoom", "cinema", "reduced", "4:3",
 * "16:9", "2.35:1"; sizes the surface by layout, never by transform).
 * Events: `state` {state}, `error` {code, message}, `size` {width, height}.
 */
class PlayerComponent(
    context: Context,
    private val nsh: NshComponentContext,
) : NshComponent {
    companion object {
        const val NAME = "tv.player"
        private const val DEFAULT_UA = "iptv-rs/0.1 (Media3)"
    }

    private val surface = SurfaceView(context)
    private val root = PictureFrame(context).apply {
        setBackgroundColor(Color.BLACK)
        addView(surface, FrameLayout.LayoutParams(-1, -1, Gravity.CENTER))
    }

    private var userAgent = DEFAULT_UA
    private var url: String? = null
    private lateinit var player: ExoPlayer

    private fun build(context: Context): ExoPlayer {
        val http = DefaultHttpDataSource.Factory()
            .setUserAgent(userAgent)
            .setAllowCrossProtocolRedirects(true)
        return ExoPlayer.Builder(context)
            .setMediaSourceFactory(DefaultMediaSourceFactory(context).setDataSourceFactory(http))
            .build()
            .also { p ->
                p.setVideoSurfaceView(surface)
                p.repeatMode = Player.REPEAT_MODE_ALL
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

        override fun onVideoSizeChanged(size: VideoSize) {
            if (size.width > 0 && size.height > 0) {
                root.videoAspect = size.width * size.pixelWidthHeightRatio / size.height
                nsh.emit("size", mapOf("width" to size.width, "height" to size.height))
            }
        }

        override fun onPlayerError(error: PlaybackException) {
            nsh.emit("error", mapOf("code" to error.errorCodeName, "message" to (error.message ?: "")))
        }
    }

    // After `listener` is initialised: build() registers it.
    init {
        player = build(context)
    }

    override fun view(): View = root

    override fun setProp(name: String, value: Any?) {
        when (name) {
            "url" -> play(value as? String)
            "paused" -> player.playWhenReady = value != true
            "pictureSize" -> root.mode = (value as? String) ?: "normal"
            "userAgent" -> {
                val ua = (value as? String)?.takeIf { it.isNotBlank() } ?: DEFAULT_UA
                if (ua != userAgent) {
                    // The data source factory is fixed per player: rebuild once, keep the stream.
                    userAgent = ua
                    player.release()
                    player = build(root.context)
                    url?.let { start(it) }
                }
            }
        }
    }

    private fun play(next: String?) {
        if (next == url) return
        url = next
        if (next == null) {
            player.stop()
            player.clearMediaItems()
        } else {
            start(next)
        }
    }

    private fun start(target: String) {
        player.setMediaItem(MediaItem.fromUri(target))
        player.prepare()
    }

    override fun destroy() {
        player.removeListener(listener)
        player.release()
    }
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
