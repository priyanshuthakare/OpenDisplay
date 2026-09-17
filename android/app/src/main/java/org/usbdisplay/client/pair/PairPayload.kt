package org.usbdisplay.client.pair

/**
 * QR payload encoder/decoder for WiFi pairing.
 * Format: {"v":1,"ip":"192.168.1.42","port":27184,"fp":"SHA256:..."}
 */
object PairPayload {
    private const val VERSION = 1

    fun encode(ip: String, port: Int, fingerprint: String? = null): String {
        val fp = fingerprint?.let { """, "fp":"$it"""" } ?: ""
        return """{"v":$VERSION,"ip":"$ip","port":$port$fp}"""
    }

    fun decode(json: String): PairInfo? {
        if (!json.contains("\"v\":$VERSION")) return null
        val ip = extractString(json, "\"ip\"") ?: return null
        val port = extractInt(json, "\"port\"") ?: return null
        if (port !in 1..65535) return null
        val fp = extractString(json, "\"fp\"")
        if (fp != null && fp.isNotEmpty() && !fp.startsWith("SHA256:")) return null
        return PairInfo(ip, port, fp)
    }

    private fun extractString(json: String, key: String): String? {
        val keyIndex = json.indexOf(key) ?: return null
        val afterKey = json.substring(keyIndex + key.length)
        val colonIndex = afterKey.indexOf(':') ?: return null
        val afterColon = afterKey.substring(colonIndex + 1).trimStart()
        if (!afterColon.startsWith('"')) return null
        val endQuote = afterColon.indexOf('"', 1) ?: return null
        return afterColon.substring(1, endQuote)
    }

    private fun extractInt(json: String, key: String): Int? {
        val keyIndex = json.indexOf(key) ?: return null
        val afterKey = json.substring(keyIndex + key.length)
        val colonIndex = afterKey.indexOf(':') ?: return null
        val afterColon = afterKey.substring(colonIndex + 1).trimStart()
        val digits = afterColon.takeWhile { it.isDigit() }
        return digits.toIntOrNull()
    }
}

data class PairInfo(
    val ip: String,
    val port: Int,
    val fingerprint: String?,
)
