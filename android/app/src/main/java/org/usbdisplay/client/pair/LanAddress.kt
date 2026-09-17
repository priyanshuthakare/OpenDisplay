package org.usbdisplay.client.pair

import android.content.Context
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Build
import java.net.InetAddress

/**
 * Get the LAN IP address of the device without requiring location permissions.
 * Uses LinkProperties from the active network, which doesn't need ACCESS_FINE_LOCATION.
 */
object LanAddress {
    fun getLanIp(context: Context): String? {
        val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            val network = cm.activeNetwork ?: return null
            val linkProperties = cm.getLinkProperties(network) ?: return null
            return linkProperties.linkAddresses
                .map { it.address }
                .filter { isRoutable(it) }
                .map { it.hostAddress }
                .firstOrNull { isRoutable(it) }
        }
        return null
    }

    private fun isRoutable(address: InetAddress): Boolean {
        val bytes = address.address
        if (bytes.size != 4) return false // IPv4 only
        val first = bytes[0].toInt() and 0xff
        return when (first) {
            10 -> true // 10.0.0.0/8
            172 -> (bytes[1].toInt() and 0xff) in 16..31 // 172.16.0.0/12
            192 -> bytes[1].toInt() and 0xff == 168 // 192.168.0.0/16
            else -> false
        }
    }

    fun isRoutable(address: String?): Boolean {
        if (address == null) return false
        try {
            return isRoutable(InetAddress.getByName(address))
        } catch (e: Exception) {
            return false
        }
    }
}
