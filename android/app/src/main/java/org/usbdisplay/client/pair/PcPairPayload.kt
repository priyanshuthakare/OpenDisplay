package org.usbdisplay.client.pair

/**
 * PC pairing-code payload shown by the Windows control app and scanned by
 * the tablet: `{"v":1,"host_id":"host-…"}`.
 *
 * Scanning authorizes by physical proximity: the person holding the tablet
 * sanctions that PC, so its host id joins the trusted set and the next
 * connect from it skips the PIN (TLS fingerprint still enforced).
 * Pure object — unit tested without camera hardware.
 */
object PcPairPayload {
    private const val VERSION = 1
    private const val PREFIX = "host-"
    private const val MAX_LEN = 64

    fun parseHostId(json: String): String? {
        if (!json.contains("\"v\":$VERSION")) return null
        val hostId = extractString(json, "\"host_id\"") ?: return null
        if (!isValidHostId(hostId)) return null
        return hostId
    }

    fun isValidHostId(hostId: String?): Boolean {
        if (hostId.isNullOrBlank()) return false
        if (!hostId.startsWith(PREFIX)) return false
        if (hostId.length > MAX_LEN) return false
        return hostId.drop(PREFIX.length).all { it in 'a'..'z' || it in '0'..'9' || it == '-' || it == '_' }
    }

    private fun extractString(json: String, key: String): String? {
        val keyIndex = json.indexOf(key)
        if (keyIndex < 0) return null
        val afterKey = json.substring(keyIndex + key.length)
        val colon = afterKey.indexOf(':')
        if (colon < 0) return null
        val afterColon = afterKey.substring(colon + 1).trimStart()
        if (!afterColon.startsWith('"')) return null
        val end = afterColon.indexOf('"', 1)
        if (end < 0) return null
        return afterColon.substring(1, end)
    }
}
