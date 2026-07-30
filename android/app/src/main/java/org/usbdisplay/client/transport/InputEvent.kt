package org.usbdisplay.client.transport

import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Fixed on-wire size of an [InputEvent], in bytes. Mirrors the Rust side. */
const val INPUT_EVENT_LEN: Int = 16

/** Record tag stored in byte 0 of every event. */
enum class InputKind(val wireValue: Int) {
    Pointer(1),
    Key(2),
}

/** What happened to the pointer. Wire values match the Rust `InputAction`. */
enum class InputAction(val wireValue: Int) {
    Down(1),
    Move(2),
    Up(3),
    Scroll(4),
}

/** Logical button. Wire values match the Rust `PointerButton`. */
enum class PointerButton(val wireValue: Int) {
    Left(0),
    Right(1),
    Middle(2),
    None(255),
}

/** Whether a key was pressed or released. Matches the Rust `KeyAction`. */
enum class KeyAction(val wireValue: Int) {
    Down(1),
    Up(2),
}

/** A non-character key with no Unicode code point. Matches the Rust `NamedKey`. */
enum class NamedKey(val wireValue: Int) {
    Char(0),
    Enter(1),
    Backspace(2),
    Tab(3),
    Escape(4),
    Delete(5),
    ArrowLeft(6),
    ArrowRight(7),
    ArrowUp(8),
    ArrowDown(9),
    Home(10),
    End(11),
}

/**
 * An input event travelling device -> host, carried in a transport `Control`
 * packet. Every event serializes to [INPUT_EVENT_LEN] bytes whose first byte is
 * an [InputKind] tag, byte-for-byte matching `usbdisplay_protocol::InputEvent`.
 *
 * Pointer coordinates are normalized to `0..65535` across the surface so the
 * host can map them onto the virtual monitor without knowing the tablet's pixel
 * size. Key events carry a Unicode code point for text or a [NamedKey] for
 * editing keys.
 */
sealed class InputEvent {
    abstract fun encode(): ByteArray

    data class Pointer(
        val action: InputAction,
        val button: PointerButton,
        val pointerId: Int,
        val x: Int,
        val y: Int,
        val scrollX: Int = 0,
        val scrollY: Int = 0,
    ) : InputEvent() {
        override fun encode(): ByteArray {
            val buffer = ByteBuffer.allocate(INPUT_EVENT_LEN).order(ByteOrder.LITTLE_ENDIAN)
            buffer.put(InputKind.Pointer.wireValue.toByte())
            buffer.put(action.wireValue.toByte())
            buffer.put(button.wireValue.toByte())
            buffer.put((pointerId and 0xff).toByte())
            buffer.putShort(x.coerceIn(0, 0xffff).toShort())
            buffer.putShort(y.coerceIn(0, 0xffff).toShort())
            buffer.putShort(scrollX.toShort())
            buffer.putShort(scrollY.toShort())
            // remaining bytes are reserved and left zero
            return buffer.array()
        }
    }

    data class Key(
        val action: KeyAction,
        val named: NamedKey,
        val unicode: Int,
    ) : InputEvent() {
        override fun encode(): ByteArray {
            val buffer = ByteBuffer.allocate(INPUT_EVENT_LEN).order(ByteOrder.LITTLE_ENDIAN)
            buffer.put(InputKind.Key.wireValue.toByte())
            buffer.put(action.wireValue.toByte())
            buffer.put(named.wireValue.toByte())
            buffer.put(0) // reserved (modifiers)
            buffer.putShort((unicode and 0xffff).toShort())
            // remaining bytes are reserved and left zero
            return buffer.array()
        }
    }

    companion object {
        /** Build a pointer event at a normalized position. */
        fun pointer(
            action: InputAction,
            button: PointerButton,
            pointerId: Int,
            x: Int,
            y: Int,
        ): Pointer = Pointer(action, button, pointerId, x, y)

        /** Build a scroll event at a normalized position. */
        fun scroll(x: Int, y: Int, scrollX: Int, scrollY: Int): Pointer =
            Pointer(InputAction.Scroll, PointerButton.None, 0, x, y, scrollX, scrollY)

        /** Build a character key event carrying a Unicode code point. */
        fun keyChar(action: KeyAction, unicode: Int): Key =
            Key(action, NamedKey.Char, unicode)

        /** Build a named (non-character) key event. */
        fun keyNamed(action: KeyAction, named: NamedKey): Key =
            Key(action, named, 0)

        /**
         * Normalize a raw pixel coordinate against a surface dimension into the
         * `0..65535` range the host expects.
         */
        fun normalize(value: Float, extent: Int): Int {
            if (extent <= 1) return 0
            val clamped = value.coerceIn(0f, (extent - 1).toFloat())
            return Math.round(clamped / (extent - 1) * 65535f)
        }
    }
}
