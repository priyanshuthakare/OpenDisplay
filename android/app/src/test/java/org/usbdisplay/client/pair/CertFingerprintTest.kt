package org.usbdisplay.client.pair

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CertFingerprintTest {
    @Test
    fun fingerprintIsFullSha256() {
        val fp = CertFingerprint.ofDer("test-cert-bytes".toByteArray())
        assertTrue(fp.startsWith("SHA256:"))
        assertEquals("SHA256:".length + 64, fp.length)
        assertTrue(CertFingerprint.isValidFormat(fp))
    }

    @Test
    fun shortDisplayShowsFirst16() {
        val fp = "SHA256:" + "ab".repeat(32)
        val short = CertFingerprint.shortDisplay(fp)
        assertTrue(short.startsWith("SHA256:"))
        assertTrue(short.contains("…"))
    }

    @Test
    fun rejectsBadFormat() {
        assertFalse(CertFingerprint.isValidFormat(null))
        assertFalse(CertFingerprint.isValidFormat("abcd"))
        assertFalse(CertFingerprint.isValidFormat("SHA256:short"))
    }

    @Test
    fun equalsIgnoresCase() {
        val a = "SHA256:" + "AB".repeat(32)
        val b = "SHA256:" + "ab".repeat(32)
        assertTrue(CertFingerprint.equalsIgnoreCase(a, b))
        assertFalse(CertFingerprint.equalsIgnoreCase(a, null))
    }
}
