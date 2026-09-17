package org.usbdisplay.client.transport

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class TransportPacketTest {
    @Test
    fun packetRoundTrips() {
        val packet = TransportPacket.frameFragment(
            packetSequence = 7,
            frameSequence = 99,
            fragmentIndex = 2,
            fragmentTotal = 5,
            payload = byteArrayOf(1, 2, 3, 4),
        )

        val decoded = TransportPacket.decode(packet.encode())
        assertEquals(packet.header, decoded.header)
        assertArrayEquals(packet.payload, decoded.payload)
    }

    @Test
    fun rejectsBadMagic() {
        val bytes = TransportPacket.heartbeat(1).encode()
        bytes[0] = 0

        assertThrows(TransportDecodeException::class.java) {
            TransportPacket.decode(bytes)
        }
    }

    @Test
    fun handshakeRoundTrips() {
        val packet = TransportPacket.handshake(
            packetSequence = 7,
            payload = byteArrayOf(9, 8, 7),
        )

        val decoded = TransportPacket.decode(packet.encode())
        assertEquals(packet.header, decoded.header)
        assertArrayEquals(packet.payload, decoded.payload)
    }

    @Test
    fun rejectsCorruptPayload() {
        val bytes = TransportPacket.frameFragment(
            packetSequence = 1,
            frameSequence = 2,
            fragmentIndex = 0,
            fragmentTotal = 1,
            payload = byteArrayOf(10, 20, 30),
        ).encode()
        bytes[bytes.lastIndex] = bytes[bytes.lastIndex].toInt().xor(0xff).toByte()

        assertThrows(TransportDecodeException::class.java) {
            TransportPacket.decode(bytes)
        }
    }
}

