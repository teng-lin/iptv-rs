package com.hantekllc.iptvrs.host

import org.junit.Assert.*
import org.junit.Test
import java.io.IOException

class RelayGuardTest {
    private val token = "0123456789abcdef".repeat(3)
    private val prefix = "http://127.0.0.1:40123/$token/"

    @Test fun theRelayAddressYieldsItsCapabilityPrefix() {
        assertEquals(prefix, RelayGuard.prefixOf("${prefix}http://origin.test/live/list.m3u8"))
        assertEquals(prefix, RelayGuard.prefixOf(prefix))
        for (local in listOf(
            "http://localhost:40123/$token/http://origin.test/a.m3u8",
            "https://127.0.0.1:40123/$token/http://origin.test/a.m3u8",
            "http://127.0.0.1/$token/http://origin.test/a.m3u8",
            "http://127.0.0.1:0/$token/http://origin.test/a.m3u8",
            "http://127.0.0.1:65536/$token/http://origin.test/a.m3u8",
            "http://127.0.0.1:40123/short/http://origin.test/a.m3u8",
            "http://127.0.0.1:40123/${token.uppercase()}/http://origin.test/a.m3u8",
            "http://origin.test/a.m3u8",
            "",
        )) {
            assertNull(local, RelayGuard.prefixOf(local))
        }
    }

    @Test fun loadsThroughTheOpenRelayAreAllowed() {
        for (uri in listOf(
            "${prefix}http://origin.test/live/list.m3u8",
            "${prefix}https://origin.test/live/seg001.ts?token=synthetic&n=1",
            "${prefix}key/https://origin.test/live/key.bin",
            "${prefix}http://origin.test/dash/manifest.mpd",
            "${prefix}http://origin.test/dash/chunk-\$Number\$.m4s",
            "${prefix}http://origin.test/a/b?path=../x",
        )) {
            assertTrue(uri, RelayGuard.allows(prefix, uri))
        }
    }

    @Test fun everythingElseIsRefused() {
        val other = "fedcba9876543210".repeat(3)
        for (uri in listOf(
            // Not through the relay at all.
            "http://origin.test/live/list.m3u8",
            "https://origin.test/live/list.m3u8",
            // Another port or token (another player's relay, a closed one).
            "http://127.0.0.1:40124/$token/http://origin.test/a.m3u8",
            "http://127.0.0.1:4012/$token/http://origin.test/a.m3u8",
            "http://127.0.0.1:401234/$token/http://origin.test/a.m3u8",
            "http://127.0.0.1:40123/$other/http://origin.test/a.m3u8",
            "http://127.0.0.1:40123/${token}0/http://origin.test/a.m3u8",
            "http://127.0.0.1:40123/http://origin.test/a.m3u8",
            // Another host for the same port and token.
            "http://localhost:40123/$token/http://origin.test/a.m3u8",
            "http://127.0.0.2:40123/$token/http://origin.test/a.m3u8",
            "http://[::1]:40123/$token/http://origin.test/a.m3u8",
            "http://origin.test:40123/$token/http://origin.test/a.m3u8",
            "http://127.0.0.1:40123@origin.test/$token/http://origin.test/a.m3u8",
            // Leaving the token by its path.
            "${prefix}../http://origin.test/a.m3u8",
            "${prefix}./http://origin.test/a.m3u8",
            "${prefix}%2e%2e/http://origin.test/a.m3u8",
            "${prefix}http://origin.test/a/../../../x",
            "${prefix}http://origin.test/a/%2E%2E/x",
            "${prefix}http://origin.test/./a",
            "${prefix}key/../http://origin.test/a",
            // Not an address the relay fetches.
            prefix,
            "${prefix}key/",
            "${prefix}http:/origin.test/a.m3u8",
            "${prefix}file:///data/local/tmp/a.ts",
            "${prefix}rtmp://origin.test/live",
            "${prefix}HTTP://origin.test/a.m3u8",
            // Other schemes for the whole address.
            "https://127.0.0.1:40123/$token/http://origin.test/a.m3u8",
            "HTTP://127.0.0.1:40123/$token/http://origin.test/a.m3u8",
            "file:///data/local/tmp/a.ts",
            "content://media/external/video/1",
            "asset:///a.ts",
            "data:application/x-mpegurl;base64,I0VYVE0zVQ==",
            "rtsp://127.0.0.1:40123/$token/http://origin.test/a",
        )) {
            assertFalse(uri, RelayGuard.allows(prefix, uri))
        }
        // No relay open: nothing loads.
        assertFalse(RelayGuard.allows(null, "${prefix}http://origin.test/a.m3u8"))
    }

    @Test fun refusalsAreFoundInTheErrorChainWithoutTheAddress() {
        val refused = RelayGuard.Refused()
        assertFalse(refused.message!!.contains("http"))
        assertTrue(RelayGuard.refusedIn(refused))
        assertTrue(RelayGuard.refusedIn(RuntimeException(IOException(refused))))
        assertFalse(RelayGuard.refusedIn(IOException("connection reset")))
        assertFalse(RelayGuard.refusedIn(null))
    }
}
