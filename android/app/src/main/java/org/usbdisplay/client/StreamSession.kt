package org.usbdisplay.client

import android.media.MediaCodec
import android.media.MediaFormat
import android.util.Log
import android.view.Surface
import org.usbdisplay.client.transport.InputEvent
import org.usbdisplay.client.transport.PacketKind
import org.usbdisplay.client.transport.TransportPacket
import org.usbdisplay.client.transport.TransportDecodeException
import java.io.BufferedInputStream
import java.io.EOFException
import java.io.OutputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.zip.CRC32

private const val TAG = "USBDisplay"
private const val STREAM_PORT = 27183
private const val PROTOCOL_HEADER_LEN = 50
private val PROTOCOL_MAGIC = byteArrayOf('U'.code.toByte(), 'S'.code.toByte(), 'B'.code.toByte(), 'D'.code.toByte())

internal class StreamSession(
    private val surface: Surface,
) {
    private val running = AtomicBoolean(false)
    private var serverSocket: ServerSocket? = null
    private var clientSocket: Socket? = null
    private var thread: Thread? = null
    private val pipeline = StreamPipeline(surface)

    // Reverse input channel. `output` is set once a host connects and guarded by
    // `outputLock` because the UI thread writes input while the session thread
    // owns the read loop. `packetSequence` numbers outbound Control packets.
    private val outputLock = Any()
    private var output: OutputStream? = null
    private val packetSequence = AtomicLong(1)

    fun start() {
        if (!running.compareAndSet(false, true)) return
        thread = Thread(::runLoop, "usbdisplay-stream-session").also { it.start() }
    }

    fun stop() {
        running.set(false)
        synchronized(outputLock) { output = null }
        clientSocket?.close()
        serverSocket?.close()
        thread?.join(2000)
        thread = null
        pipeline.release()
    }

    /**
     * Send a pointer/scroll event back to the host inside a Control transport
     * packet. Safe to call from the UI thread; a no-op until a host connects.
     */
    fun sendInput(event: InputEvent) {
        val stream = synchronized(outputLock) { output } ?: return
        val packet = TransportPacket.create(
            kind = PacketKind.Control,
            packetSequence = packetSequence.getAndIncrement(),
            frameSequence = 0,
            fragmentIndex = 0,
            fragmentTotal = 0,
            payload = event.encode(),
        )
        val bytes = packet.encode()
        val framed = ByteBuffer.allocate(4 + bytes.size).order(ByteOrder.LITTLE_ENDIAN)
        framed.putInt(bytes.size)
        framed.put(bytes)
        try {
            synchronized(outputLock) {
                stream.write(framed.array())
                stream.flush()
            }
        } catch (e: Exception) {
            Log.w(TAG, "Failed to send input event: ${e.message}")
        }
    }

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
        synchronized(outputLock) { output = socket.getOutputStream() }
        try {
            pipeline.handleClient(input, socket.getOutputStream())
        } finally {
            synchronized(outputLock) { output = null }
        }
    }
}


