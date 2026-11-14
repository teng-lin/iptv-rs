package com.hantekllc.iptvrs.host

import org.json.JSONObject
import java.io.IOException

/** Per-player bounded Rust transport; Media3 never receives identity headers. */
internal class PlaybackTransport {
    private var target: String? = null
    private var local: String? = null

    /**
     * The open relay's capability prefix ([RelayGuard]), or null when none
     * is open: every load the player makes must start with it. Read on
     * Media3's loader threads.
     */
    @Volatile var prefix: String? = null
        private set

    fun open(url: String, headers: Map<String, String>, publicOnly: Boolean): String {
        if (target != url) close()
        val active = local
        if (active != null) {
            update(headers)
            return active
        }
        val created = create(url, JSONObject(headers).toString(), publicOnly)
        // Not a relay address: nothing could load through it.
        val relay = RelayGuard.prefixOf(created) ?: run {
            release(created)
            throw IOException("Playback transport unavailable")
        }
        target = url
        local = created
        prefix = relay
        return created
    }

    fun update(headers: Map<String, String>) {
        local?.let { updateHeaders(it, JSONObject(headers).toString()) }
    }

    fun takeError(): String? = local?.let { takeErrorCode(it) }

    fun close() {
        prefix = null
        local?.let { release(it) }
        local = null
        target = null
    }

    private external fun create(url: String, headers: String, publicOnly: Boolean): String
    private external fun updateHeaders(local: String, headers: String)
    private external fun release(local: String)
    private external fun takeErrorCode(local: String): String?
}
