package org.usbdisplay.client.pair

/**
 * WiFi pairing handshake JSON (PR-3), transported inside a Handshake (kind 6)
 * packet over the already-established TLS-1.3 session.
 *
 * Hello (host → tablet):
 *   {"v":1,"pin":"123456","host_id":"host-123","codecs":["h264","h265"]}
 * Welcome accept (tablet → host):
 *   {"v":1,"accept":true,"tablet_id":"tablet-…","fp":"SHA256:…"}
 * Welcome reject:
 *   {"v":1,"accept":false,"reason":"bad-pin|lockout|version"}
 *
 * Manual string parsing (no org.json) so JVM unit tests run without Robolectric.
 */
object Handshake {
    const val VERSION = 1

    data class Hello(
        val pin: String,
        val hostId: String,
        val codecs: List<String>,
    )

    fun buildHello(pin: String, hostId: String): String {
        val safePin = pin.replace("\"", "")
        val safeHost = hostId.replace("\"", "")
        return """{"v":1,"pin":"$safePin","host_id":"$safeHost","codecs":["h264","h265"]}"""
    }

    fun parseHello(json: String): Hello? {
        if (!json.contains("\"v\":1")) return null
        val pin = extractString(json, "\"pin\"") ?: return null
        val hostId = extractString(json, "\"host_id\"") ?: ""
        // codecs optional for forward-compat; default to h264.
        return Hello(pin = pin, hostId = hostId, codecs = listOf("h264", "h265"))
    }

    fun buildWelcomeAccept(tabletId: String, fp: String): String {
        val safeId = tabletId.replace("\"", "")
        return """{"v":1,"accept":true,"tablet_id":"$safeId","fp":"$fp"}"""
    }

    fun buildWelcomeReject(reason: String): String {
        val safe = reason.replace("\"", "")
        return """{"v":1,"accept":false,"reason":"$safe"}"""
    }

    data class Welcome(val accept: Boolean, val reason: String?, val fp: String?)

    fun parseWelcome(json: String): Welcome? {
        if (!json.contains("\"v\":")) return null
        val accept = when {
            json.contains("\"accept\":true") -> true
            json.contains("\"accept\":false") -> false
            else -> return null
        }
        val reason = extractString(json, "\"reason\"")
        val fp = extractString(json, "\"fp\"")
        return Welcome(accept, reason, fp)
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
