package org.usbdisplay.client.pair

import org.json.JSONException
import org.json.JSONObject

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
        // Real JSON parsing: a `contains("\"v\":1")` check would also accept
        // `"v":10`, and index scanning can match a key name nested in a value.
        val obj = try {
            JSONObject(json)
        } catch (_: JSONException) {
            return null
        }
        if (obj.optInt("v", -1) != VERSION) return null
        val hostId = obj.optString("host_id", "").takeIf { it.isNotEmpty() && it != "null" }
            ?: return null
        if (!isValidHostId(hostId)) return null
        return hostId
    }

    fun isValidHostId(hostId: String?): Boolean {
        if (hostId.isNullOrBlank()) return false
        if (!hostId.startsWith(PREFIX)) return false
        if (hostId.length > MAX_LEN) return false
        return hostId.drop(PREFIX.length).all { it in 'a'..'z' || it in '0'..'9' || it == '-' || it == '_' }
    }
}
