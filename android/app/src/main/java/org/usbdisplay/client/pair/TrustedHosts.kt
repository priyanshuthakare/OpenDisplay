package org.usbdisplay.client.pair

/**
 * Trusted host_ids that have completed PIN pairing once (PR-3).
 *
 * Reconnect from a known host_id skips PIN but still requires the TLS
 * fingerprint match (enforced at the TLS layer + QR fp).
 * Pure in-memory core + thin SharedPreferences wrapper in [TrustedHostsStore].
 */
class TrustedHosts(initial: Set<String> = emptySet()) {
    private val hosts = initial.toMutableSet()

    fun isTrusted(hostId: String): Boolean = hosts.contains(hostId)

    fun trust(hostId: String) {
        if (hostId.isNotBlank()) hosts.add(hostId)
    }

    fun clear() = hosts.clear()

    fun snapshot(): Set<String> = hosts.toSet()
}
