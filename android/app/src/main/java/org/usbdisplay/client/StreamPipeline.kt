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
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.atomic.AtomicLong
import java.util.zip.CRC32

private const val TAG = "USBDisplay-Pipeline"
private const val PROTOCOL_HEADER_LEN = 50
private val PROTOCOL_MAGIC = byteArrayOf('U'.code.toByte(), 'S'.code.toByte(), 'B'.code.toByte(), 'D'.code.toByte())

/**
 * Shared streaming pipeline used by both USB (StreamSession) and WiFi (WifiListener).
 * Handles frame reassembly, decoding, and pacing.
 */
class StreamPipeline(
    private val surface: Surface,
) {
    private var codec: MediaCodec? = null
    private val frameReassembler = FrameReassembler()
    private val presenter = PipelinePresenter()
    private val outputLock = Any()
    private var output: OutputStream? = null
    private val packetSequence = AtomicLong(1)

    /**
     * Handle a client connection by reading length-prefixed transport packets,
     * reassembling frames, decoding, and presenting on the surface.
     */
    fun handleClient(input: BufferedInputStream, output: OutputStream) {
        synchronized(outputLock) { this.output = output }
        try {
            while (true) {
                val packet = readPacket(input) ?: break
                when (packet.header.kind) {
                    PacketKind.FrameFragment -> {
                        val fragment = parseFragment(packet)
                        val frame = frameReassembler.push(fragment)
                        if (frame != null) {
                            decodeAndPresent(frame)
                        }
                    }
                    PacketKind.Heartbeat -> continue
                    PacketKind.KeyframeRequest -> {
                        Log.d(TAG, "Keyframe request ignored (client only)")
                    }
                    else -> {
                        Log.d(TAG, "Ignoring packet kind: ${packet.header.kind}")
                    }
                }
            }
        } catch (e: EOFException) {
            Log.i(TAG, "Client disconnected")
        } catch (e: Exception) {
            Log.e(TAG, "Pipeline error", e)
        } finally {
            synchronized(outputLock) { this.output = null }
        }
    }

    /**
     * Send input event back to host. Used by MainActivity for touch/keyboard events.
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

    private fun readPacket(input: BufferedInputStream): TransportPacket? {
        val lenBytes = readExactly(input, 4)
        if (lenBytes == null) return null
        val packetLen = ByteBuffer.wrap(lenBytes).order(ByteOrder.LITTLE_ENDIAN).int
        if (packetLen <= 0 || packetLen > 64 * 1024 + 64) {
            throw TransportDecodeException("Invalid packet length: $packetLen")
        }
        val packetBytes = readExactly(input, packetLen) ?: return null
        return TransportPacket.decode(packetBytes)
    }

    private fun parseFragment(packet: TransportPacket): Fragment {
        return Fragment(
            frameSequence = packet.header.frameSequence,
            index = packet.header.fragmentIndex,
            total = packet.header.fragmentTotal,
            bytes = packet.payload,
        )
    }

    private fun decodeAndPresent(frame: Frame) {
        val buffer = ByteBuffer.wrap(frame.payload)
        if (buffer.remaining() < PROTOCOL_HEADER_LEN) {
            Log.w(TAG, "Frame too short for header")
            return
        }

        val magic = ByteArray(4)
        buffer.get(magic)
        if (!magic.contentEquals(PROTOCOL_MAGIC)) {
            Log.w(TAG, "Invalid frame magic")
            return
        }

        buffer.position(4)
        val version = buffer.short.toInt() and 0xffff
        if (version != 1) {
            Log.w(TAG, "Unsupported protocol version: $version")
            return
        }

        buffer.position(12)
        val sequence = buffer.long
        val timestampNs = buffer.long
        val codecId = buffer.get().toInt() and 0xff
        val flags = buffer.get().toInt() and 0xff
        val width = buffer.short.toInt() and 0xffff
        val height = buffer.short.toInt() and 0xffff
        val refreshMillihz = buffer.int
        val payloadLen = buffer.int
        val payloadCrc = buffer.int

        if (buffer.remaining() < payloadLen) {
            Log.w(TAG, "Frame payload too short")
            return
        }

        val payload = ByteArray(payloadLen)
        buffer.get(payload)

        val crc = CRC32()
        crc.update(payload)
        if (crc.value != payloadCrc.toLong() and 0xffffffffL) {
            Log.w(TAG, "Frame payload CRC mismatch")
            return
        }

        if (codec == null || width != codecWidth || height != codecHeight) {
            recreateCodec(width, height, codecId)
        }

        val inputIndex = codec!!.dequeueInputBuffer(10000)
        if (inputIndex >= 0) {
            val inputBuffer = codec!!.getInputBuffer(inputIndex)!!
            inputBuffer.clear()
            inputBuffer.put(payload)
            codec!!.queueInputBuffer(
                inputIndex,
                0,
                payloadLen,
                timestampNs / 1000,
                if ((flags and 1) != 0) MediaCodec.BUFFER_FLAG_KEY_FRAME else 0
            )
        }

        val bufferInfo = MediaCodec.BufferInfo()
        val outputIndex = codec!!.dequeueOutputBuffer(bufferInfo, 10000)
        if (outputIndex >= 0) {
            presenter.present(codec!!, outputIndex, bufferInfo, surface)
        }
    }

    private var codecWidth = 0
    private var codecHeight = 0

    private fun recreateCodec(width: Int, height: Int, codecId: Int) {
        codec?.release()
        val mimeType = when (codecId) {
            1 -> "video/avc"
            2 -> "video/hevc"
            3 -> "video/av01"
            else -> {
                Log.w(TAG, "Unknown codec ID: $codecId")
                return
            }
        }
        codec = MediaCodec.createDecoderByType(mimeType)
        val format = MediaFormat.createVideoFormat(mimeType, width, height)
        codec!!.configure(format, surface, null, 0)
        codec!!.start()
        codecWidth = width
        codecHeight = height
        Log.i(TAG, "Codec recreated: $mimeType ${width}x$height")
    }

    fun release() {
        codec?.release()
        codec = null
    }
}

data class Fragment(
    val frameSequence: Long,
    val index: Int,
    val total: Int,
    val bytes: ByteArray,
)

data class Frame(
    val sequence: Long,
    val payload: ByteArray,
)

class FrameReassembler {
    private var currentSequence: Long? = null
    private var fragments: MutableMap<Int, ByteArray> = mutableMapOf()
    private var totalFragments: Int = 0

    fun push(fragment: Fragment): Frame? {
        if (currentSequence == null || fragment.frameSequence != currentSequence) {
            currentSequence = fragment.frameSequence
            fragments.clear()
            totalFragments = fragment.total
        }

        fragments[fragment.index] = fragment.bytes

        if (fragments.size == totalFragments && totalFragments > 0) {
            val payload = ByteArray(fragments.values.sumOf { it.size })
            var offset = 0
            for (i in 0 until totalFragments) {
                val frag = fragments[i] ?: return null
                System.arraycopy(frag, 0, payload, offset, frag.size)
                offset += frag.size
            }
            return Frame(fragment.frameSequence, payload)
        }
        return null
    }
}

private class PipelinePresenter {
    private var lastPresentTimeUs: Long = 0

    fun present(codec: MediaCodec, index: Int, info: MediaCodec.BufferInfo, surface: Surface) {
        val nowUs = System.nanoTime() / 1000
        if (lastPresentTimeUs == 0L) {
            lastPresentTimeUs = nowUs
        }

        val targetTimeUs = info.presentationTimeUs
        val delayUs = targetTimeUs - lastPresentTimeUs
        if (delayUs > 0) {
            Thread.sleep(delayUs / 1000)
        }

        codec.releaseOutputBuffer(index, true)
        lastPresentTimeUs = targetTimeUs
    }
}

private fun readExactly(input: BufferedInputStream, len: Int): ByteArray? {
    val buffer = ByteArray(len)
    var offset = 0
    while (offset < len) {
        val read = input.read(buffer, offset, len - offset)
        if (read == -1) return null
        offset += read
    }
    return buffer
}

internal fun readU32LE(input: BufferedInputStream): Int {
    val bytes = readExactly(input, 4) ?: throw java.io.EOFException("eof reading u32")
    return ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN).int
}
