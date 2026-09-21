package org.usbdisplay.client.pair

import org.json.JSONException
import org.json.JSONObject

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
        // Parse as JSON rather than substring-matching. A `contains("\"v\":1")`
        // check also accepts `"v":10`, and index scanning can latch onto a key
        // name that appears inside another value.
        val obj = try {
            JSONObject(json)
        } catch (_: JSONException) {
            return null
        }
        if (obj.optInt("v", -1) != VERSION) return null
        val ip = obj.optString("ip", "")
        if (ip.isEmpty()) return null
        val port = obj.optInt("port", -1)
        if (port !in 1..65535) return null
        val fp = obj.optString("fp", "").takeIf { it.isNotEmpty() && it != "null" }
        if (fp != null && !fp.startsWith("SHA256:")) return null
        return PairInfo(ip, port, fp)
    }
}

data class PairInfo(
    val ip: String,
    val port: Int,
    val fingerprint: String?,
)
