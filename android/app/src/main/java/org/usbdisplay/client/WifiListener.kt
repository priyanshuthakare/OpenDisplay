package org.usbdisplay.client

import android.util.Log
import org.usbdisplay.client.transport.PacketKind
import org.usbdisplay.client.transport.TransportPacket
import java.io.BufferedInputStream
import java.io.OutputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.atomic.AtomicBoolean

private const val TAG = "USBDisplay-WiFi"
private const val WIFI_STREAM_PORT = 27184

/**
 * WiFi listener that accepts connections on all interfaces (not loopback only).
 * Mirrors the USB StreamSession logic but uses port 27184 and accepts from any LAN IP.
 */
class WifiListener(
    private val pipeline: StreamPipeline,
) {
    private val running = AtomicBoolean(false)
    private var serverSocket: ServerSocket? = null
    private var thread: Thread? = null

    fun start() {
        if (!running.compareAndSet(false, true)) return
        thread = Thread(::runLoop, "usbdisplay-wifi-listener").also { it.start() }
    }

    fun stop() {
        running.set(false)
        serverSocket?.close()
        thread?.join()
    }

    private fun runLoop() {
        try {
            serverSocket = ServerSocket(WIFI_STREAM_PORT, 0, InetAddress.getByName("0.0.0.0"))
            Log.i(TAG, "WiFi listener listening on 0.0.0.0:$WIFI_STREAM_PORT")
            while (running.get()) {
                val client = serverSocket?.accept() ?: break
                Log.i(TAG, "WiFi client connected from ${client.inetAddress}")
                try {
                    handleClient(client)
                } catch (e: Exception) {
                    Log.e(TAG, "WiFi client error", e)
                } finally {
                    client.close()
                }
            }
        } catch (e: Exception) {
            if (running.get()) {
                Log.e(TAG, "WiFi listener error", e)
            }
        }
    }

    private fun handleClient(socket: Socket) {
        val input = BufferedInputStream(socket.getInputStream())
        val output = socket.getOutputStream()
        pipeline.handleClient(input, output)
    }
}
