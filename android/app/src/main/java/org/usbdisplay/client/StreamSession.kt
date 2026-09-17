package org.usbdisplay.client

import android.util.Log
import android.view.Surface
import org.usbdisplay.client.transport.InputEvent
import java.io.BufferedInputStream
import java.io.EOFException
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.atomic.AtomicBoolean

private const val TAG = "USBDisplay"
private const val STREAM_PORT = 27183

internal class StreamSession(
    private val surface: Surface,
    val pipeline: StreamPipeline = StreamPipeline(surface),
) {
    private val running = AtomicBoolean(false)
    private var serverSocket: ServerSocket? = null
    private var clientSocket: Socket? = null
    private var thread: Thread? = null

    fun start() {
        if (!running.compareAndSet(false, true)) return
        thread = Thread(::runLoop, "usbdisplay-stream-session").also { it.start() }
    }

    fun stop() {
        running.set(false)
        clientSocket?.close()
        serverSocket?.close()
        thread?.join(2000)
        thread = null
        // Do not release shared pipeline here; MainActivity owns it when shared.
    }

    /**
     * Send input back to the host via the shared pipeline.
     * Safe to call from the UI thread; no-op until a host connects.
     */
    fun sendInput(event: InputEvent) = pipeline.sendInput(event)

    private fun runLoop() {
        try {
            serverSocket = ServerSocket(STREAM_PORT, 1, InetAddress.getByName("127.0.0.1"))
            Log.i(TAG, "Listening on 127.0.0.1:$STREAM_PORT for adb-forwarded host stream")
            while (running.get()) {
                val accepted = try {
                    serverSocket!!.accept()
                } catch (e: Exception) {
                    if (!running.get()) break
                    throw e
                }
                clientSocket = accepted
                try {
                    Log.i(TAG, "Host connected, starting decode")
                    handleClient(accepted)
                    Log.i(TAG, "Host disconnected")
                } finally {
                    accepted.close()
                    clientSocket = null
                }
            }
        } catch (e: EOFException) {
            Log.i(TAG, "Host stream ended")
        } catch (e: Exception) {
            if (running.get()) {
                Log.e(TAG, "Stream session failed", e)
            }
        } finally {
            clientSocket?.close()
            serverSocket?.close()
        }
    }

    private fun handleClient(socket: Socket) {
        val input = BufferedInputStream(socket.getInputStream())
        pipeline.handleClient(input, socket.getOutputStream())
    }
}
