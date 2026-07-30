package org.usbdisplay.client.transport

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Test

class InputEventTest {
    @Test
    fun encodesFixedWidth() {
        val event = InputEvent.pointer(InputAction.Down, PointerButton.Left, 0, 40000, 12345)
        assertEquals(INPUT_EVENT_LEN, event.encode().size)
    }

    @Test
    fun encodesPointerLayoutLittleEndian() {
        // kind=Pointer(1), action=Down(1), button=Left(0), pointer=2,
        // x=0x9C40 (40000), y=0x3039 (12345), scroll_x=0, scroll_y=0, reserved.
        // Must match the Rust `matches_kotlin_pointer_reference_bytes` literal.
        val event = InputEvent.pointer(InputAction.Down, PointerButton.Left, 2, 40000, 12345)
        val expected = byteArrayOf(
            1,
            1, 0, 2,
            0x40.toByte(), 0x9C.toByte(),
            0x39.toByte(), 0x30.toByte(),
            0, 0,
            0, 0,
            0, 0, 0, 0,
        )
        assertArrayEquals(expected, event.encode())
    }

    @Test
    fun encodesCharKeyLayout() {
        // kind=Key(2), action=Down(1), named=Char(0), reserved, unicode='A' (0x41).
        // Must match the Rust `matches_kotlin_key_reference_bytes` literal.
        val event = InputEvent.keyChar(KeyAction.Down, 'A'.code)
        val expected = byteArrayOf(
            2,
            1,
            0,
            0,
            0x41.toByte(), 0x00.toByte(),
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        )
        assertArrayEquals(expected, event.encode())
    }

    @Test
    fun encodesNamedKeyLayout() {
        val event = InputEvent.keyNamed(KeyAction.Up, NamedKey.Enter)
        val bytes = event.encode()
        assertEquals(InputKind.Key.wireValue.toByte(), bytes[0])
        assertEquals(KeyAction.Up.wireValue.toByte(), bytes[1])
        assertEquals(NamedKey.Enter.wireValue.toByte(), bytes[2])
        assertEquals(0x00.toByte(), bytes[4]) // unicode low byte unused
        assertEquals(0x00.toByte(), bytes[5])
    }

    @Test
    fun encodesScrollWithSignedDeltas() {
        // scroll_x = -3 -> 0xFFFD, scroll_y = 5 -> 0x0005 (little endian)
        val event = InputEvent.scroll(100, 200, -3, 5)
        val bytes = event.encode()
        assertEquals(InputKind.Pointer.wireValue.toByte(), bytes[0])
        assertEquals(InputAction.Scroll.wireValue.toByte(), bytes[1])
        assertEquals(PointerButton.None.wireValue.toByte(), bytes[2])
        assertEquals(0xFD.toByte(), bytes[8])
        assertEquals(0xFF.toByte(), bytes[9])
        assertEquals(0x05.toByte(), bytes[10])
        assertEquals(0x00.toByte(), bytes[11])
    }

    @Test
    fun normalizeMapsExtentEndpoints() {
        assertEquals(0, InputEvent.normalize(0f, 1920))
        assertEquals(65535, InputEvent.normalize(1919f, 1920))
        assertEquals(0, InputEvent.normalize(-50f, 1920)) // clamps below range
        assertEquals(65535, InputEvent.normalize(5000f, 1920)) // clamps above range
    }
}
