package org.usbdisplay.client.pair

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class HandshakeTest {
    @Test
    fun helloRoundTrips() {
        val json = Handshake.buildHello("123456", "host-42")
        val hello = Handshake.parseHello(json)
        assertNotNull(hello)
        assertEquals("123456", hello?.pin)
        assertEquals("host-42", hello?.hostId)
    }

    @Test
    fun rejectsWrongVersion() {
        assertNull(Handshake.parseHello("""{"v":2,"pin":"123456","host_id":"h"}"""))
    }

    @Test
    fun rejectsMissingPin() {
        assertNull(Handshake.parseHello("""{"v":1,"host_id":"h"}"""))
    }

    @Test
    fun welcomeAcceptRoundTrips() {
        val json = Handshake.buildWelcomeAccept("tablet-1", "SHA256:abcd")
        val w = Handshake.parseWelcome(json)
        assertNotNull(w)
        assertTrue(w?.accept == true)
        assertEquals("SHA256:abcd", w?.fp)
    }

    @Test
    fun welcomeRejectCarriesReason() {
        val json = Handshake.buildWelcomeReject("bad-pin")
        val w = Handshake.parseWelcome(json)
        assertNotNull(w)
        assertFalse(w?.accept == true)
        assertEquals("bad-pin", w?.reason)
    }

    @Test
    fun welcomeRejectsMissingAccept() {
        assertNull(Handshake.parseWelcome("""{"v":1,"reason":"x"}"""))
    }
}
