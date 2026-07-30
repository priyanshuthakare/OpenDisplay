package org.usbdisplay.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class FramePacerTest {
    private val ms = 1_000_000L
    private val frameNs = 16_666_666L // ~60 fps

    @Test
    fun firstFramePresentsImmediately() {
        val pacer = FramePacer()
        assertEquals(1_000L, pacer.deadlineNs(ptsNs = 500_000_000L, nowNs = 1_000L))
    }

    @Test
    fun secondFrameScheduledOneIntervalAfterAnchor() {
        val pacer = FramePacer()
        val now = 10_000L
        pacer.deadlineNs(ptsNs = 0L, nowNs = now)
        // A frame one interval later, arriving early, is scheduled one interval
        // after the anchor wall time — not presented immediately.
        val deadline = pacer.deadlineNs(ptsNs = frameNs, nowNs = now + 2 * ms)
        assertEquals(now + frameNs, deadline)
    }

    @Test
    fun burstArrivalIsSpreadAcrossIntervals() {
        val pacer = FramePacer()
        val now = 0L
        // Three frames all arrive at once (burst), but carry increasing PTS.
        val d0 = pacer.deadlineNs(0L, now)
        val d1 = pacer.deadlineNs(frameNs, now)
        val d2 = pacer.deadlineNs(2 * frameNs, now)
        assertEquals(0L, d0)
        assertEquals(frameNs, d1)
        assertEquals(2 * frameNs, d2)
    }

    @Test
    fun lateFrameNeverScheduledInThePast() {
        val pacer = FramePacer()
        val now = 0L
        pacer.deadlineNs(0L, now)
        // The next frame's ideal deadline is one interval out, but the clock has
        // already advanced past it; it must present now, never in the past.
        val deadline = pacer.deadlineNs(frameNs, now + 50 * ms)
        assertTrue(deadline >= now + 50 * ms)
    }

    @Test
    fun farBehindReanchorsToNow() {
        val pacer = FramePacer(maxLagNs = 100 * ms)
        pacer.deadlineNs(0L, 0L)
        // A frame whose ideal deadline is far in the past (we fell way behind):
        // re-anchor and present now rather than accumulate unbounded lag.
        val now = 5_000 * ms
        val deadline = pacer.deadlineNs(frameNs, now)
        assertEquals(now, deadline)
    }

    @Test
    fun backwardsPtsJumpReanchors() {
        val pacer = FramePacer(resetThresholdNs = 1_000 * ms)
        pacer.deadlineNs(10_000 * ms, 0L)
        // A looped/reset source restarts PTS near zero; treat it as a new
        // timeline and present immediately instead of waiting.
        val now = 100L
        val deadline = pacer.deadlineNs(0L, now)
        assertEquals(now, deadline)
    }
}
