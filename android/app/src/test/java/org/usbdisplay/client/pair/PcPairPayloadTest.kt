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
}
