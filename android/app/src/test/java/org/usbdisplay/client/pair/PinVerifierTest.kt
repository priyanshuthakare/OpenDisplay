package org.usbdisplay.client.pair

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.security.SecureRandom

class PinVerifierTest {
    @Test
    fun acceptsCorrectPin() {
        val v = PinVerifier()
        val result = v.verify("123456", "123456")
        assertTrue(result is PinVerifier.Result.Ok)
    }

    @Test
    fun rejectsWrongPinWithRetriesLeft() {
        val v = PinVerifier()
        val r1 = v.verify("000000", "123456")
        assertTrue(r1 is PinVerifier.Result.Wrong)
        assertEquals(2, (r1 as PinVerifier.Result.Wrong).retriesLeft)
        val r2 = v.verify("000001", "123456")
        assertEquals(1, (r2 as PinVerifier.Result.Wrong).retriesLeft)
    }

    @Test
    fun locksOutAfterThreeStrikes() {
        var now = 1_000_000L
        val v = PinVerifier(clockMs = { now })
        v.verify("a", "123456")
        v.verify("b", "123456")
        val third = v.verify("c", "123456")
        assertTrue(third is PinVerifier.Result.Locked)
        // Still locked immediately after.
        val again = v.verify("123456", "123456")
        assertTrue(again is PinVerifier.Result.Locked)
        // After 30s, correct PIN works again.
        now += 31_000
        assertTrue(v.verify("123456", "123456") is PinVerifier.Result.Ok)
    }

    @Test
    fun constantTimeEqualsMatches() {
        assertTrue(PinVerifier.constantTimeEquals("123456", "123456"))
        assertFalse(PinVerifier.constantTimeEquals("123456", "123457"))
        assertFalse(PinVerifier.constantTimeEquals("12345", "123456"))
    }

    @Test
    fun pinFormatValidation() {
        assertTrue(PinVerifier.isValidFormat("123456"))
        assertFalse(PinVerifier.isValidFormat("12345"))
        assertFalse(PinVerifier.isValidFormat("abcdef"))
        assertFalse(PinVerifier.isValidFormat("1234567"))
    }

    @Test
    fun generatedPinIsSixDigits() {
        val pin = PinVerifier.generate(SecureRandom())
        assertTrue(PinVerifier.isValidFormat(pin))
    }
}
