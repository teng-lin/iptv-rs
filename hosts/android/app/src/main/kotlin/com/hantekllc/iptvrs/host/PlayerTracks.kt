package com.hantekllc.iptvrs.host

import androidx.annotation.OptIn
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.TrackSelectionParameters
import androidx.media3.common.Tracks
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.audio.AudioSink
import androidx.media3.exoplayer.audio.ForwardingAudioSink

/**
 * The `tv.player` contract's tracks on Media3: the `tracks` event from
 * [Tracks], and the `videoTrack` / `audioTrack` / `textTrack` props as
 * [TrackSelectionParameters]. A track's id is `"<group>:<track>"`, its
 * group's index in [Tracks.getGroups] and its index in the group: valid for
 * the stream that reported it (the app starts again from `auto`, `auto`,
 * `off` with each new source). Same shape as the Linux host's (mpv).
 */
internal object PlayerTracks {
    const val AUTO = "auto"
    const val OFF = "off"

    /** What the viewer chose for the open stream. */
    data class Choice(val video: String = AUTO, val audio: String = AUTO, val text: String = OFF)

    private fun positive(v: Int): Int? = v.takeIf { it > 0 }
    private fun positive(v: Float): Float? = v.takeIf { it > 0f && it.isFinite() }

    /** The `tracks` event: supported tracks of each kind and what plays. */
    fun event(tracks: Tracks, choice: Choice): Map<String, Any?> {
        val video = mutableListOf<Map<String, Any?>>()
        val audio = mutableListOf<Map<String, Any?>>()
        val text = mutableListOf<Map<String, Any?>>()
        var audioSelected: String? = null
        var textSelected: String? = null
        tracks.groups.forEachIndexed { g, group ->
            for (t in 0 until group.length) {
                if (!group.isTrackSupported(t)) continue
                val f = group.getTrackFormat(t)
                val id = "$g:$t"
                when (group.type) {
                    C.TRACK_TYPE_VIDEO -> video += mapOf(
                        "id" to id,
                        "label" to (f.label ?: ""),
                        "width" to positive(f.width),
                        "height" to positive(f.height),
                        "fps" to positive(f.frameRate),
                        "bitrate" to positive(f.bitrate),
                    )
                    C.TRACK_TYPE_AUDIO -> {
                        audio += mapOf(
                            "id" to id,
                            "label" to (f.label ?: ""),
                            "language" to (f.language ?: ""),
                            "channels" to positive(f.channelCount),
                            "codec" to (f.sampleMimeType ?: ""),
                        )
                        if (group.isTrackSelected(t) && audioSelected == null) audioSelected = id
                    }
                    C.TRACK_TYPE_TEXT -> {
                        text += mapOf(
                            "id" to id,
                            "label" to (f.label ?: ""),
                            "language" to (f.language ?: ""),
                        )
                        if (group.isTrackSelected(t) && textSelected == null) textSelected = id
                    }
                }
            }
        }
        // An adaptive video group selects all its tracks: "auto" unless the
        // viewer fixed one.
        val videoSelected = choice.video.takeIf { it != AUTO && find(tracks, C.TRACK_TYPE_VIDEO, it) != null }
        return mapOf(
            "video" to video,
            "audio" to audio,
            "text" to text,
            "selected" to mapOf(
                "video" to (videoSelected ?: AUTO),
                "audio" to audioSelected,
                "text" to (textSelected ?: OFF),
            ),
        )
    }

    /** The override for track [id] of [type], if the stream has it. */
    fun find(tracks: Tracks, type: Int, id: String): TrackSelectionOverride? {
        val parts = id.split(':')
        if (parts.size != 2) return null
        val g = parts[0].toIntOrNull() ?: return null
        val t = parts[1].toIntOrNull() ?: return null
        val group = tracks.groups.getOrNull(g) ?: return null
        if (group.type != type || t !in 0 until group.length) return null
        return TrackSelectionOverride(group.mediaTrackGroup, t)
    }

    /** [base] with [choice] applied to the stream's [tracks]. */
    fun parameters(base: TrackSelectionParameters, tracks: Tracks, choice: Choice): TrackSelectionParameters {
        val b = base.buildUpon()
            .clearOverridesOfType(C.TRACK_TYPE_VIDEO)
            .clearOverridesOfType(C.TRACK_TYPE_AUDIO)
            .clearOverridesOfType(C.TRACK_TYPE_TEXT)
        find(tracks, C.TRACK_TYPE_VIDEO, choice.video)?.let { b.setOverrideForType(it) }
        find(tracks, C.TRACK_TYPE_AUDIO, choice.audio)?.let { b.setOverrideForType(it) }
        val text = find(tracks, C.TRACK_TYPE_TEXT, choice.text)
        b.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, text == null)
        text?.let { b.setOverrideForType(it) }
        return b.build()
    }

    /** The `format` event's fields Media3 knows for [video] and [audio]. */
    fun format(video: Format?, audio: Format?): Map<String, Any> {
        val out = mutableMapOf<String, Any>()
        video?.frameRate?.let { positive(it) }?.let { out["fps"] = it }
        audio?.channelCount?.let { positive(it) }?.let { out["channels"] = it }
        return out
    }
}

/**
 * The audio sink with the `audioDelay` prop applied: Media3 has no audio
 * offset, but its video follows the audio clock, so a clock reported
 * [delayUs] ahead shows each frame that much earlier against the sound
 * (positive: the sound comes later). Changing it makes the clock jump once
 * (frames are dropped, or wait, until they meet it again).
 */
@OptIn(UnstableApi::class)
internal class DelayedAudioSink(sink: AudioSink) : ForwardingAudioSink(sink) {
    @Volatile var delayUs = 0L

    override fun getCurrentPositionUs(sourceEnded: Boolean): Long {
        val position = super.getCurrentPositionUs(sourceEnded)
        return if (position == AudioSink.CURRENT_POSITION_NOT_SET) position else position + delayUs
    }
}
