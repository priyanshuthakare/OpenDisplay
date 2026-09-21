package org.usbdisplay.client.pair

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class PairPayloadTest {
    @Test
    fun encodesBasicPayload() {
        val json = PairPayload.encode("192.168.1.42", 27184)
        assertEquals("""{"v":1,"ip":"192.168.1.42","port":27184}""", json)
    }

    @Test
    fun encodesPayloadWithFingerprint() {
        val json = PairPayload.encode("192.168.1.42", 27184, "SHA256:abcd1234")
        assertEquals("""{"v":1,"ip":"192.168.1.42","port":27184, "fp":"SHA256:abcd1234"}""", json)
    }

    @Test
    fun decodesBasicPayload() {
        val json = """{"v":1,"ip":"192.168.1.42","port":27184}"""
        val info = PairPayload.decode(json)
        assertNotNull(info)
        assertEquals("192.168.1.42", info?.ip)
        assertEquals(27184, info?.port)
        assertEquals(null, info?.fingerprint)
    }

    @Test
    fun decodesPayloadWithFingerprint() {
        val json = """{"v":1,"ip":"192.168.1.42","port":27184,"fp":"SHA256:abcd1234"}"""
        val info = PairPayload.decode(json)
        assertNotNull(info)
        assertEquals("192.168.1.42", info?.ip)
        assertEquals(27184, info?.port)
        assertEquals("SHA256:abcd1234", info?.fingerprint)
    }

    @Test
    fun rejectsWrongVersion() {
        val json = """{"v":2,"ip":"192.168.1.42","port":27184}"""
        assertNull(PairPayload.decode(json))
    }

    @Test
    fun rejectsMissingIp() {
        val json = """{"v":1,"port":27184}"""
        assertNull(PairPayload.decode(json))
    }

    @Test
    fun rejectsMissingPort() {
        val json = """{"v":1,"ip":"192.168.1.42"}"""
        assertNull(PairPayload.decode(json))
    }

    @Test
    fun rejectsAnOutOfRangePort() {
        val json = """{"v":1,"ip":"10.0.0.5","port":70000}"""
        val info = PairPayload.decode(json)
        // Port extraction fails for out-of-range values
        assertEquals(null, info?.port)
    }

    @Test
    fun rejectsAVersionThatMerelyPrefixesTheExpectedOne() {
        // Regression: a `contains("\"v\":1")` check accepted this, because the
        // string `"v":10` starts with `"v":1`.
        val json = """{"v":10,"ip":"192.168.1.42","port":27184}"""
        assertNull(PairPayload.decode(json))
    }

    @Test
    fun rejectsPayloadsThatAreNotJson() {
        assertNull(PairPayload.decode("not json at all"))
        assertNull(PairPayload.decode(""))
        assertNull(PairPayload.decode("""{"v":1,"ip":"""))
    }
}
