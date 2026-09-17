package org.usbdisplay.client

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class FrameReassemblerTest {
    @Test
    fun reassemblesSingleFragment() {
        val reassembler = FrameReassembler()
        val fragment = Fragment(
            frameSequence = 1,
            index = 0,
            total = 1,
            bytes = byteArrayOf(1, 2, 3)
        )
        val frame = reassembler.push(fragment)
        assertNotNull(frame)
        assertArrayEquals(byteArrayOf(1, 2, 3), frame?.payload)
    }

    @Test
    fun reassemblesMultipleFragments() {
        val reassembler = FrameReassembler()
        val fragment1 = Fragment(
            frameSequence = 1,
            index = 0,
            total = 2,
            bytes = byteArrayOf(1, 2)
        )
        val fragment2 = Fragment(
            frameSequence = 1,
            index = 1,
            total = 2,
            bytes = byteArrayOf(3, 4)
        )
        assertNull(reassembler.push(fragment1))
        val frame = reassembler.push(fragment2)
        assertNotNull(frame)
        assertArrayEquals(byteArrayOf(1, 2, 3, 4), frame?.payload)
    }

    @Test
    fun handlesOutOfOrderFragments() {
        val reassembler = FrameReassembler()
        val fragment2 = Fragment(
            frameSequence = 1,
            index = 1,
            total = 2,
            bytes = byteArrayOf(3, 4)
        )
        val fragment1 = Fragment(
            frameSequence = 1,
            index = 0,
            total = 2,
            bytes = byteArrayOf(1, 2)
        )
        assertNull(reassembler.push(fragment2))
        val frame = reassembler.push(fragment1)
        assertNotNull(frame)
        assertArrayEquals(byteArrayOf(1, 2, 3, 4), frame?.payload)
    }

    @Test
    fun resetsOnNewFrameSequence() {
        val reassembler = FrameReassembler()
        val oldFragment = Fragment(
            frameSequence = 1,
            index = 0,
            total = 1,
            bytes = byteArrayOf(1, 2)
        )
        val newFragment = Fragment(
            frameSequence = 2,
            index = 0,
            total = 1,
            bytes = byteArrayOf(3, 4)
        )
        val frame1 = reassembler.push(oldFragment)
        assertNotNull(frame1)
        assertArrayEquals(byteArrayOf(1, 2), frame1?.payload)
        val frame2 = reassembler.push(newFragment)
        assertNotNull(frame2)
        assertArrayEquals(byteArrayOf(3, 4), frame2?.payload)
    }
}
