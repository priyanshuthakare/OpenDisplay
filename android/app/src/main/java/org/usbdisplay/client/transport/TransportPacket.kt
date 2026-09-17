package org.usbdisplay.client.transport

import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.zip.CRC32

private val MAGIC = byteArrayOf('U'.code.toByte(), 'S'.code.toByte(), 'B'.code.toByte(), 'T'.code.toByte())
private const val VERSION: Int = 1
const val PACKET_HEADER_LEN: Int = 40
const val DEFAULT_MAX_PACKET_PAYLOAD: Int = 64 * 1024

enum class PacketKind(val wireValue: Int) {
    FrameFragment(1),
    Ack(2),
    Heartbeat(3),
    KeyframeRequest(4),
    Control(5),
    Handshake(6);

    companion object {
        fun fromWire(value: Int): PacketKind =
            entries.firstOrNull { it.wireValue == value }
                ?: throw TransportDecodeException("unknown packet kind $value")
    }
}

data class PacketHeader(
    val kind: PacketKind,
    val packetSequence: Long,
    val frameSequence: Long,
    val fragmentIndex: Int,
    val fragmentTotal: Int,
    val payloadLength: Int,
    val payloadCrc32: Long,
)

data class TransportPacket(
    val header: PacketHeader,
    val payload: ByteArray,
) {
    fun encode(): ByteArray {
        val buffer = ByteBuffer.allocate(PACKET_HEADER_LEN + payload.size)
            .order(ByteOrder.LITTLE_ENDIAN)
        buffer.put(MAGIC)
        buffer.putShort(VERSION.toShort())
        buffer.putShort(PACKET_HEADER_LEN.toShort())
        buffer.put(header.kind.wireValue.toByte())
        buffer.put(byteArrayOf(0, 0, 0))
        buffer.putLong(header.packetSequence)
        buffer.putLong(header.frameSequence)
        buffer.putShort(header.fragmentIndex.toShort())
        buffer.putShort(header.fragmentTotal.toShort())
        buffer.putInt(payload.size)
        buffer.putInt(header.payloadCrc32.toInt())
        buffer.put(payload)
        return buffer.array()
    }

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is TransportPacket) return false
        return header == other.header && payload.contentEquals(other.payload)
    }

    override fun hashCode(): Int = 31 * header.hashCode() + payload.contentHashCode()

    companion object {
        fun frameFragment(
            packetSequence: Long,
            frameSequence: Long,
            fragmentIndex: Int,
            fragmentTotal: Int,
            payload: ByteArray,
        ): TransportPacket =
            create(PacketKind.FrameFragment, packetSequence, frameSequence, fragmentIndex, fragmentTotal, payload)

        fun heartbeat(packetSequence: Long): TransportPacket =
            create(PacketKind.Heartbeat, packetSequence, 0, 0, 0, ByteArray(0))

        fun handshake(packetSequence: Long, payload: ByteArray): TransportPacket =
            create(PacketKind.Handshake, packetSequence, 0, 0, 0, payload)

        fun create(
            kind: PacketKind,
            packetSequence: Long,
            frameSequence: Long,
            fragmentIndex: Int,
            fragmentTotal: Int,
            payload: ByteArray,
        ): TransportPacket {
            val crc = crc32(payload)
            return TransportPacket(
                PacketHeader(
                    kind = kind,
                    packetSequence = packetSequence,
                    frameSequence = frameSequence,
                    fragmentIndex = fragmentIndex,
                    fragmentTotal = fragmentTotal,
                    payloadLength = payload.size,
                    payloadCrc32 = crc,
                ),
                payload,
            )
        }

        fun decode(bytes: ByteArray, maxPayloadLength: Int = DEFAULT_MAX_PACKET_PAYLOAD): TransportPacket {
            if (bytes.size < PACKET_HEADER_LEN) {
                throw TransportDecodeException("packet buffer is shorter than the transport header")
            }

            for (index in MAGIC.indices) {
                if (bytes[index] != MAGIC[index]) {
                    throw TransportDecodeException("invalid transport magic")
                }
            }

            val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            buffer.position(4)
            val version = buffer.short.toInt() and 0xffff
            if (version != VERSION) {
                throw TransportDecodeException("unsupported transport version $version")
            }

            val headerLength = buffer.short.toInt() and 0xffff
            if (headerLength != PACKET_HEADER_LEN) {
                throw TransportDecodeException("invalid transport header length $headerLength")
            }

            val kind = PacketKind.fromWire(buffer.get().toInt() and 0xff)
            buffer.position(12)
            val packetSequence = buffer.long
            val frameSequence = buffer.long
            val fragmentIndex = buffer.short.toInt() and 0xffff
            val fragmentTotal = buffer.short.toInt() and 0xffff
            val payloadLength = buffer.int
            val payloadCrc32 = buffer.int.toLong() and 0xffffffffL

            if (payloadLength < 0 || payloadLength > maxPayloadLength) {
                throw TransportDecodeException("transport payload length $payloadLength exceeds configured maximum $maxPayloadLength")
            }
            if (bytes.size < headerLength + payloadLength) {
                throw TransportDecodeException("packet buffer does not contain the declared payload")
            }

            val payload = bytes.copyOfRange(headerLength, headerLength + payloadLength)
            val actualCrc = crc32(payload)
            if (actualCrc != payloadCrc32) {
                throw TransportDecodeException("packet payload CRC mismatch: expected $payloadCrc32, got $actualCrc")
            }

            return TransportPacket(
                PacketHeader(
                    kind = kind,
                    packetSequence = packetSequence,
                    frameSequence = frameSequence,
                    fragmentIndex = fragmentIndex,
                    fragmentTotal = fragmentTotal,
                    payloadLength = payloadLength,
                    payloadCrc32 = payloadCrc32,
                ),
                payload,
            )
        }
    }
}

class TransportDecodeException(message: String) : Exception(message)

private fun crc32(bytes: ByteArray): Long {
    val crc = CRC32()
    crc.update(bytes)
    return crc.value
}

