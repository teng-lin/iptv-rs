package com.hantekllc.iptvrs.host

import android.content.Context
import android.graphics.Color
import android.view.Gravity
import android.view.SurfaceView
import android.view.View
import android.widget.FrameLayout
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.VideoSize
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.ui.AspectRatioFrameLayout
import com.longbridge.nsh.NshComponent
import com.longbridge.nsh.NshComponentContext

/**
 * `tv.player`: one persistent Media3 player on a SurfaceView, aspect-fit in a
 * black frame. The Rust app lays it out like any view and draws its UI above
 * it; changing layout never recreates the decoder.
 *
 * Props: `url` (string, null stops), `paused` (bool), `userAgent` (string).
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
    private val aspect = AspectRatioFrameLayout(context).apply {
        resizeMode = AspectRatioFrameLayout.RESIZE_MODE_FIT
        addView(surface, FrameLayout.LayoutParams(-1, -1))
    }
    private val root = FrameLayout(context).apply {
        setBackgroundColor(Color.BLACK)
        addView(aspect, FrameLayout.LayoutParams(-1, -1, Gravity.CENTER))
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
                aspect.setAspectRatio(size.width * size.pixelWidthHeightRatio / size.height)
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
