package com.longbridge.nsh

import java.io.File
import org.junit.Assert.*
import org.junit.Test

/** Uses the same decoder that passes Rust props to PlayerComponent.setProp. */
class PlayerContractTest {
    @Test fun rustPlayerPropsDecodeAsTheHostsExpectedJavaTypes() {
        val props = Json.parse(File("../../../crates/iptv-player-mpv/fixtures/player-props.json").readText()) as Map<*, *>
        val source = props["source"] as Map<*, *>
        assertEquals("http://127.0.0.1:18080/synthetic/index.m3u8", source["url"] as String)
        assertEquals(37L, (source["token"] as Number).toLong())
        assertEquals(true, source["live"])
        assertEquals(false, source["reconnect"])
        assertEquals(false, source["publicOnly"])
        assertEquals(mapOf("X-Synthetic" to "fixture"), source["headers"] as Map<*, *>)
        assertEquals(false, props["paused"])
        assertEquals("normal", props["pictureSize"])
        assertEquals("auto", props["videoTrack"])
        assertEquals("auto", props["audioTrack"])
        assertEquals("off", props["textTrack"])
        assertEquals(80, (props["volume"] as Number).toInt())
        assertEquals(false, props["muted"])
        assertEquals(125, (props["audioDelay"] as Number).toInt())
        val pip = props["pip"] as Map<*, *>
        assertEquals(true, pip["auto"])
        assertEquals(3L, (pip["enter"] as Number).toLong())
        assertEquals(false, pip["window"])
    }
}
