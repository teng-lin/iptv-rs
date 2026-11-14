package com.hantekllc.iptvrs.host

import org.junit.Assert.*
import org.junit.Test
import java.io.IOException

class PlaybackTransportTest {
    /** Records the calls the Rust relay would get; relay N listens on port 40000 + N. */
    private class FakeRelay : PlaybackTransport.Relay {
        val calls = mutableListOf<String>()
        var created = 0
        var address: (Int, String) -> String = { n, url -> "http://127.0.0.1:${40000 + n}/${token(n)}/$url" }
        var error: String? = null
        var failCreate = false

        override fun create(url: String, headers: Map<String, String>, publicOnly: Boolean): String {
            if (failCreate) throw IOException("Playback transport unavailable")
            created += 1
            calls += "create $url $headers $publicOnly"
            return address(created, url)
        }
        override fun updateHeaders(local: String, headers: Map<String, String>) {
            calls += "update ${port(local)} $headers"
        }
        override fun release(local: String) {
            calls += "release ${port(local)}"
        }
        override fun takeErrorCode(local: String) = error.also { error = null }
    }

    companion object {
        fun token(n: Int) = n.toString(16).padStart(48, '0')
        /** The relay's port, or the whole address when it is not a relay's. */
        fun port(local: String) =
            if (local.startsWith("http://127.0.0.1:")) local.removePrefix("http://127.0.0.1:").substringBefore("/") else local
        const val A = "http://origin.test/a.m3u8"
        const val B = "http://origin.test/b.m3u8"
    }

    @Test fun oneRelayPerAddressAndANewOneForTheNext() {
        val relay = FakeRelay()
        val transport = PlaybackTransport(relay)
        assertNull(transport.prefix)
        val first = transport.open(A, mapOf("User-Agent" to "ua"), true)
        assertEquals("http://127.0.0.1:40001/${token(1)}/$A", first)
        assertEquals("http://127.0.0.1:40001/${token(1)}/", transport.prefix)
        // The same address again (a retry, a format fallback): the same relay,
        // with the current headers.
        assertEquals(first, transport.open(A, mapOf("User-Agent" to "other"), true))
        assertEquals(1, relay.created)
        // Another address: the old relay goes first.
        val second = transport.open(B, emptyMap(), false)
        assertEquals("http://127.0.0.1:40002/${token(2)}/$B", second)
        assertEquals("http://127.0.0.1:40002/${token(2)}/", transport.prefix)
        assertFalse(RelayGuard.allows(transport.prefix, first))
        assertTrue(RelayGuard.allows(transport.prefix, second))
        assertEquals(listOf(
            "create $A {User-Agent=ua} true",
            "update 40001 {User-Agent=other}",
            "release 40001",
            "create $B {} false",
        ), relay.calls)
    }

    @Test fun closeReleasesTheRelayAndRefusesEveryLoad() {
        val relay = FakeRelay()
        val transport = PlaybackTransport(relay)
        val local = transport.open(A, emptyMap(), true)
        transport.close()
        assertNull(transport.prefix)
        assertFalse(RelayGuard.allows(transport.prefix, local))
        assertNull(transport.takeError())
        transport.update(mapOf("User-Agent" to "ua"))
        transport.close()
        assertEquals(listOf("create $A {} true", "release 40001"), relay.calls)
        // The same address after a close: a new relay.
        transport.open(A, emptyMap(), true)
        assertEquals(2, relay.created)
    }

    @Test fun errorsAreTheOpenRelays() {
        val relay = FakeRelay()
        val transport = PlaybackTransport(relay)
        relay.error = "ERROR_CODE_IO_NETWORK_CONNECTION_FAILED"
        assertNull(transport.takeError())
        transport.open(A, emptyMap(), true)
        assertEquals("ERROR_CODE_IO_NETWORK_CONNECTION_FAILED", transport.takeError())
        assertNull(transport.takeError())
    }

    @Test fun anAddressThatIsNotARelayIsReleasedAndFails() {
        val relay = FakeRelay()
        val transport = PlaybackTransport(relay)
        relay.address = { _, url -> url }
        assertThrows(IOException::class.java) { transport.open(A, emptyMap(), true) }
        assertNull(transport.prefix)
        assertEquals(listOf("create $A {} true", "release $A"), relay.calls)
        // Nothing stays open: the next open creates again.
        relay.address = { n, url -> "http://127.0.0.1:${40000 + n}/${token(n)}/$url" }
        transport.open(A, emptyMap(), true)
        assertEquals(2, relay.created)
    }

    @Test fun aFailedCreateLeavesNothingOpen() {
        val relay = FakeRelay()
        val transport = PlaybackTransport(relay)
        transport.open(A, emptyMap(), true)
        relay.failCreate = true
        assertThrows(IOException::class.java) { transport.open(B, emptyMap(), true) }
        assertNull(transport.prefix)
        assertNull(transport.takeError())
        assertEquals(listOf("create $A {} true", "release 40001"), relay.calls)
    }
}
