package org.usbdisplay.client.pair

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TrustedHostsTest {
    @Test
    fun unknownHostNotTrusted() {
        val t = TrustedHosts()
        assertFalse(t.isTrusted("host-1"))
    }

    @Test
    fun trustAndRecall() {
        val t = TrustedHosts()
        t.trust("host-1")
        assertTrue(t.isTrusted("host-1"))
        assertFalse(t.isTrusted("host-2"))
    }

    @Test
    fun clearForgetsAll() {
        val t = TrustedHosts(setOf("a", "b"))
        t.clear()
        assertFalse(t.isTrusted("a"))
    }

    @Test
    fun ignoresBlank() {
        val t = TrustedHosts()
        t.trust("")
        t.trust("   ")
        assertTrue(t.snapshot().isEmpty())
    }
}
