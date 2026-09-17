package org.usbdisplay.client.pair

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LanAddressTest {
    @Test
    fun isRoutable10PrivateRange() {
        assertTrue(LanAddress.isRoutable("10.0.0.1"))
        assertTrue(LanAddress.isRoutable("10.255.255.254"))
    }

    @Test
    fun isRoutable172PrivateRange() {
        assertTrue(LanAddress.isRoutable("172.16.0.1"))
        assertTrue(LanAddress.isRoutable("172.31.255.254"))
    }

    @Test
    fun isRoutable192PrivateRange() {
        assertTrue(LanAddress.isRoutable("192.168.0.1"))
        assertTrue(LanAddress.isRoutable("192.168.255.254"))
    }

    @Test
    fun isNotRoutablePublicIP() {
        assertFalse(LanAddress.isRoutable("8.8.8.8"))
        assertFalse(LanAddress.isRoutable("1.1.1.1"))
    }

    @Test
    fun isNotRoutableLoopback() {
        assertFalse(LanAddress.isRoutable("127.0.0.1"))
    }

    @Test
    fun isNotRoutableLinkLocal() {
        assertFalse(LanAddress.isRoutable("169.254.1.1"))
    }

    @Test
    fun isNotRoutableInvalid() {
        assertFalse(LanAddress.isRoutable(""))
        assertFalse(LanAddress.isRoutable(null))
    }
}
