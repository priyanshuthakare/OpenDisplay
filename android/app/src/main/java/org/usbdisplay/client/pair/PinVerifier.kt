package org.usbdisplay.client.pair

import java.security.MessageDigest
import java.security.SecureRandom

/**
 * PIN verification with constant-time compare + 3-strikes 30s lockout (PR-3).
 *
 * Pure logic (injectable clock) so `testDebugUnitTest` can cover it without
 * Android framework. Storage of the expected PIN lives in [PinStore].
 */
class PinVerifier(
    private val clockMs: () -> Long = { System.currentTimeMillis() },
) {
    private var failures = 0
    private var lockedUntilMs = 0L

    sealed class Result {
        data object Ok : Result()
        data class Wrong(val retriesLeft: Int) : Result()
        data class Locked(val retryAfterMs: Long) : Result()
    }

    fun verify(input: String, expected: String): Result {
        val now = clockMs()
        if (now < lockedUntilMs) {
            return Result.Locked(lockedUntilMs - now)
        }
        val match_ = constantTimeEquals(input, expected)
        return if (match_) {
            failures = 0
            Result.Ok
        } else {
            failures += 1
            if (failures >= MAX_ATTEMPTS) {
                lockedUntilMs = now + LOCKOUT_MS
                failures = 0
                Result.Locked(LOCKOUT_MS)
            } else {
                Result.Wrong(MAX_ATTEMPTS - failures)
            }
        }
    }

    fun isLocked(nowMs: Long = clockMs()): Boolean = nowMs < lockedUntilMs

    companion object {
        const val MAX_ATTEMPTS = 3
        const val LOCKOUT_MS = 30_000L
        const val PIN_LENGTH = 6

        fun constantTimeEquals(a: String, b: String): Boolean {
            // PINs are short ASCII; MessageDigest.isEqual is constant-time
            // over the byte arrays (length mismatch => false fast, acceptable
            // since length is fixed 6).
            return MessageDigest.isEqual(
                a.toByteArray(Charsets.US_ASCII),
                b.toByteArray(Charsets.US_ASCII),
            )
        }

        fun isValidFormat(pin: String): Boolean =
            pin.length == PIN_LENGTH && pin.all { it in '0'..'9' }

        fun generate(random: SecureRandom = SecureRandom()): String {
            val n = random.nextInt(1_000_000)
            return "%06d".format(n)
        }
    }
}
