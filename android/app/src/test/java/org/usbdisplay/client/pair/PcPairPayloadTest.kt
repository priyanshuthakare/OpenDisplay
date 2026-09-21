package org.usbdisplay.client.pair

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PcPairPayloadTest {
    @Test
    fun parsesValidPayload() {
        assertEquals("host-acer-pc", PcPairPayload.parseHostId("""{"v":1,"host_id":"host-acer-pc"}"""))
    }

    @Test
    fun rejectsWrongVersion() {
        assertNull(PcPairPayload.parseHostId("""{"v":2,"host_id":"host-acer-pc"}"""))
    }

    @Test
    fun rejectsMissingHostId() {
        assertNull(PcPairPayload.parseHostId("""{"v":1}"""))
    }

    @Test
    fun rejectsNonHostPrefix() {
        assertNull(PcPairPayload.parseHostId("""{"v":1,"host_id":"evil-box"}"""))
    }

    @Test
    fun rejectsBadCharacters() {
        assertNull(PcPairPayload.parseHostId("""{"v":1,"host_id":"host-a/b"}"""))
        assertNull(PcPairPayload.parseHostId("""{"v":1,"host_id":"HOST-ABC"}"""))
    }

    @Test
    fun validatesHostIds() {
        assertTrue(PcPairPayload.isValidHostId("host-a1_-b"))
        assertFalse(PcPairPayload.isValidHostId(null))
        assertFalse(PcPairPayload.isValidHostId(""))
        assertFalse(PcPairPayload.isValidHostId("host-" + "a".repeat(100)))
    }

    @Test
    fun rejectsAVersionThatMerelyPrefixesTheExpectedOne() {
        // Regression: a `contains("\"v\":1")` check accepted this, because the
        // string `"v":10` starts with `"v":1`.
        assertNull(PcPairPayload.parseHostId("""{"v":10,"host_id":"host-acer-pc"}"""))
    }

    @Test
    fun rejectsPayloadsThatAreNotJson() {
        assertNull(PcPairPayload.parseHostId("not json at all"))
        assertNull(PcPairPayload.parseHostId(""))
        assertNull(PcPairPayload.parseHostId("""{"v":1,"host_id":"""))
    }
}
