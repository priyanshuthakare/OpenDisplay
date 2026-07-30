package org.usbdisplay.client

import android.media.MediaCodec
import android.media.MediaFormat
import android.util.Log
import android.view.Surface
import org.usbdisplay.client.transport.PacketKind
import org.usbdisplay.client.transport.TransportPacket
import org.usbdisplay.client.transport.TransportDecodeException
import java.io.BufferedInputStream
import java.io.EOFException
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.atomic.AtomicBoolean
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
        var decoder: FrameDecoder? = null
        val reassembler = FrameReassembler()
        val input = BufferedInputStream(socket.getInputStream())
        try {
            while (running.get()) {
                val packetLength = readU32LE(input)
                val packetBytes = readExactly(input, packetLength)
                val packet = try {
                    TransportPacket.decode(packetBytes)
                } catch (e: TransportDecodeException) {
                    Log.w(TAG, "Dropping invalid transport packet: ${e.message}")
                    continue
                }
                if (packet.header.kind != PacketKind.FrameFragment) {
                    continue
                }

                val frameBytes = reassembler.push(
                    frameSequence = packet.header.frameSequence,
                    fragmentIndex = packet.header.fragmentIndex,
                    fragmentTotal = packet.header.fragmentTotal,
                    fragmentPayload = packet.payload,
                ) ?: continue

                val frame = try {
                    ProtocolFrame.decode(frameBytes)
                } catch (e: ProtocolDecodeException) {
                    Log.w(TAG, "Dropping invalid protocol frame: ${e.message}")
                    continue
                }
                if (decoder == null || !decoder.matches(frame)) {
                    decoder?.close()
                    decoder = FrameDecoder(surface, frame)
                }
                decoder.queue(frame)
            }
        } finally {
            decoder?.close()
        }
    }
}

private data class ProtocolFrame(
    val sequence: Long,
    val timestampNs: Long,
    val codec: Int,
    val width: Int,
    val height: Int,
    val payload: ByteArray,
) {
    companion object {
        fun decode(bytes: ByteArray): ProtocolFrame {
            if (bytes.size < PROTOCOL_HEADER_LEN) {
                throw ProtocolDecodeException("frame shorter than protocol header")
            }
            for (i in PROTOCOL_MAGIC.indices) {
                if (bytes[i] != PROTOCOL_MAGIC[i]) {
                    throw ProtocolDecodeException("invalid protocol magic")
                }
            }

            val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            buffer.position(4)
            val version = buffer.short.toInt() and 0xffff
            if (version != 1) {
                throw ProtocolDecodeException("unsupported protocol version $version")
            }
            val headerLen = buffer.short.toInt() and 0xffff
            if (headerLen != PROTOCOL_HEADER_LEN || bytes.size < headerLen) {
                throw ProtocolDecodeException("invalid protocol header length $headerLen")
            }
            val sequence = buffer.long
            val timestampNs = buffer.long
            val codec = buffer.get().toInt() and 0xff
            buffer.get() // flags
            val width = buffer.short.toInt() and 0xffff
            val height = buffer.short.toInt() and 0xffff
            buffer.int // refresh mHz
            val payloadLen = buffer.int
            val payloadCrc = buffer.int.toLong() and 0xffffffffL
            buffer.position(PROTOCOL_HEADER_LEN)

            if (payloadLen < 0 || bytes.size < PROTOCOL_HEADER_LEN + payloadLen) {
                throw ProtocolDecodeException("invalid payload length $payloadLen")
            }
            val payload = ByteArray(payloadLen)
            buffer.get(payload)

            val crc = CRC32()
            crc.update(payload)
            if (crc.value != payloadCrc) {
                throw ProtocolDecodeException("payload crc mismatch")
            }

            return ProtocolFrame(
                sequence = sequence,
                timestampNs = timestampNs,
                codec = codec,
                width = width,
                height = height,
                payload = payload,
            )
        }
    }
}

private class ProtocolDecodeException(message: String) : Exception(message)

private class FrameDecoder(
    surface: Surface,
    firstFrame: ProtocolFrame,
) : AutoCloseable {
    private val codecType = firstFrame.codec
    private val width = firstFrame.width
    private val height = firstFrame.height
    private val decoder: MediaCodec = MediaCodec.createDecoderByType(mimeType(codecType)).apply {
        configure(MediaFormat.createVideoFormat(mimeType(codecType), width, height), surface, null, 0)
        start()
    }
    private val bufferInfo = MediaCodec.BufferInfo()

    fun matches(frame: ProtocolFrame): Boolean =
        frame.codec == codecType && frame.width == width && frame.height == height

    fun queue(frame: ProtocolFrame) {
        val inIndex = decoder.dequeueInputBuffer(10_000)
        if (inIndex >= 0) {
            val inputBuffer = decoder.getInputBuffer(inIndex) ?: return
            inputBuffer.clear()
            inputBuffer.put(frame.payload)
            decoder.queueInputBuffer(
                inIndex,
                0,
                frame.payload.size,
                frame.timestampNs / 1_000,
                0,
            )
        }

        while (true) {
            val outIndex = decoder.dequeueOutputBuffer(bufferInfo, 0)
            if (outIndex >= 0) {
                decoder.releaseOutputBuffer(outIndex, true)
            } else {
                break
            }
        }
    }

    override fun close() {
        try {
            decoder.stop()
        } catch (_: Exception) {
        }
        decoder.release()
    }

    private fun mimeType(codec: Int): String = when (codec) {
        1 -> MediaFormat.MIMETYPE_VIDEO_AVC
        2 -> MediaFormat.MIMETYPE_VIDEO_HEVC
        else -> throw IllegalArgumentException("unsupported codec id $codec")
    }
}

private class FrameReassembler {
    private data class State(
        var total: Int = 0,
        val fragments: MutableMap<Int, ByteArray> = mutableMapOf(),
    )

    private val pending = linkedMapOf<Long, State>()

    fun push(
        frameSequence: Long,
        fragmentIndex: Int,
        fragmentTotal: Int,
        fragmentPayload: ByteArray,
    ): ByteArray? {
        if (fragmentTotal <= 0 || fragmentIndex < 0 || fragmentIndex >= fragmentTotal) {
            return null
        }
        val state = pending.getOrPut(frameSequence) { State(total = fragmentTotal) }
        state.total = fragmentTotal
        state.fragments.putIfAbsent(fragmentIndex, fragmentPayload)
        if (state.fragments.size != state.total) {
            trimPending()
            return null
        }

        val output = ByteArray(state.fragments.values.sumOf { it.size })
        var offset = 0
        for (index in 0 until state.total) {
            val chunk = state.fragments[index] ?: return null
            System.arraycopy(chunk, 0, output, offset, chunk.size)
            offset += chunk.size
        }
        pending.remove(frameSequence)
        trimPending()
        return output
    }

    private fun trimPending() {
        while (pending.size > 16) {
            val oldest = pending.keys.firstOrNull() ?: return
            pending.remove(oldest)
        }
    }
}

private fun readExactly(input: BufferedInputStream, length: Int): ByteArray {
    if (length <= 0) throw EOFException("invalid packet length $length")
    val bytes = ByteArray(length)
    var read = 0
    while (read < length) {
        val n = input.read(bytes, read, length - read)
        if (n < 0) throw EOFException("unexpected EOF")
        read += n
    }
    return bytes
}

private fun readU32LE(input: BufferedInputStream): Int {
    val bytes = readExactly(input, 4)
    return ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN).int
}
