package com.hantekllc.iptvrs.host

import android.net.Uri
import androidx.annotation.OptIn
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DataSource
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.TransferListener
import java.io.IOException

/**
 * Media3 loads only through its player's playback relay, as mpv does on the
 * desktops: every address the player opens is `http://127.0.0.1:<port>/<token>/`
 * (the relay's capability, [PlaybackTransport.prefix]) followed by the
 * upstream address the relay fetches, or `key/` and that address for an HLS
 * key. Anything else (an address a manifest rewriter missed, a relative
 * reference that climbed out of the capability, a closed relay's) fails with
 * [Refused] instead of reaching the network without the relay's checks.
 */
internal object RelayGuard {
    private val PREFIX = Regex("""^http://127\.0\.0\.1:([0-9]{1,5})/([0-9a-f]{32,})/""")

    /** A load outside the relay; carries no address (it must never be logged). */
    class Refused : IOException("Load outside the playback relay")

    /** The capability prefix of relay address `local`, or null if it is not one. */
    fun prefixOf(local: String): String? {
        val match = PREFIX.find(local) ?: return null
        val port = match.groupValues[1].toInt()
        return match.value.takeIf { port in 1..65535 }
    }

    /** Whether the player may open `uri` while the relay at `prefix` (null: none) is open. */
    fun allows(prefix: String?, uri: String): Boolean {
        if (prefix == null || !uri.startsWith(prefix)) return false
        val target = uri.substring(prefix.length).removePrefix("key/")
        if (!target.startsWith("http://") && !target.startsWith("https://")) return false
        // The relay's own addresses never have dot segments (it and Media3
        // resolve them away), so one here is an attempt to leave the token.
        val path = target.substringAfter("://").substringBefore('?').substringBefore('#')
        return path.split('/').none { segment ->
            val decoded = segment.replace("%2e", ".", ignoreCase = true)
            decoded == "." || decoded == ".."
        }
    }

    /** Whether a playback error comes from a load [allows] refused. */
    fun refusedIn(error: Throwable?): Boolean =
        generateSequence(error) { it.cause }.take(16).any { it is Refused }
}

/** [RelayGuard] around each of `upstream`'s loads; `prefix` is read on loader threads. */
@OptIn(UnstableApi::class)
internal class RelayOnlyDataSource(
    private val upstream: DataSource,
    private val prefix: () -> String?,
) : DataSource {
    class Factory(
        private val upstream: DataSource.Factory,
        private val prefix: () -> String?,
    ) : DataSource.Factory {
        override fun createDataSource(): DataSource =
            RelayOnlyDataSource(upstream.createDataSource(), prefix)
    }

    override fun addTransferListener(transferListener: TransferListener) =
        upstream.addTransferListener(transferListener)

    override fun open(dataSpec: DataSpec): Long {
        if (!RelayGuard.allows(prefix(), dataSpec.uri.toString())) throw RelayGuard.Refused()
        val length = upstream.open(dataSpec)
        // The relay follows redirects itself and never answers one; should
        // a load still have been redirected, where it ended must pass too.
        val opened = upstream.uri
        if (opened != null && !RelayGuard.allows(prefix(), opened.toString())) {
            try {
                upstream.close()
            } finally {
                throw RelayGuard.Refused()
            }
        }
        return length
    }

    override fun read(buffer: ByteArray, offset: Int, length: Int): Int =
        upstream.read(buffer, offset, length)

    override fun getUri(): Uri? = upstream.uri

    override fun getResponseHeaders(): Map<String, List<String>> = upstream.responseHeaders

    override fun close() = upstream.close()
}
