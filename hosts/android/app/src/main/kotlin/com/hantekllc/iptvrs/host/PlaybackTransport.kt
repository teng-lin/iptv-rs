package com.hantekllc.iptvrs.host

import org.json.JSONObject

/** Per-player bounded Rust transport; Media3 never receives identity headers. */
internal class PlaybackTransport {
    private var target: String? = null
    private var local: String? = null

    fun open(url: String, headers: Map<String, String>, publicOnly: Boolean): String {
        if (target != url) close()
        val active = local
        if (active != null) {
            update(headers)
            return active
        }
        val created = create(url, JSONObject(headers).toString(), publicOnly)
        target = url
        local = created
        return created
    }

    fun update(headers: Map<String, String>) {
        local?.let { updateHeaders(it, JSONObject(headers).toString()) }
    }

    fun takeError(): String? = local?.let { takeErrorCode(it) }

    fun close() {
        local?.let { release(it) }
        local = null
        target = null
    }

    private external fun create(url: String, headers: String, publicOnly: Boolean): String
    private external fun updateHeaders(local: String, headers: String)
    private external fun release(local: String)
    private external fun takeErrorCode(local: String): String?
}
