package org.usbdisplay.client.pair

import java.security.MessageDigest
import java.util.Locale

/**
 * Certificate fingerprint helpers (PR-3).
 *
 * Fingerprint = SHA-256 of DER cert, formatted `SHA256:<64 lowercase hex>`.
 * Display short form = `SHA256:xxxx…` (first 16 hex chars).
 * Pure object so unit tests cover format without AndroidKeyStore.
 */
object CertFingerprint {
    const val PREFIX = "SHA256:"

    fun ofDer(certDer: ByteArray): String {
        val digest = MessageDigest.getInstance("SHA-256").digest(certDer)
        return PREFIX + digest.joinToString("") { "%02x".format(it) }
    }

    fun shortDisplay(fp: String): String {
        val hex = fp.removePrefix(PREFIX)
        val shown = hex.take(16)
        return "$PREFIX$shown…"
    }

    fun isValidFormat(fp: String?): Boolean {
        if (fp == null) return false
        if (!fp.startsWith(PREFIX)) return false
        val hex = fp.removePrefix(PREFIX)
        // Accept full 64-hex; also accept short display in logs (not for pinning).
        if (hex.length != 64) return false
        return hex.all { it in '0'..'9' || it.lowercaseChar() in 'a'..'f' }
    }

    fun equalsIgnoreCase(a: String?, b: String?): Boolean {
        if (a == null || b == null) return false
        return a.equals(b, ignoreCase = true)
    }

    private fun Char.lowercaseChar(): Char = this.lowercase(Locale.US)[0]
}
