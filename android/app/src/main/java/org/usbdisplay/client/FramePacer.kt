package org.usbdisplay.client

/**
 * Turns a decoded frame's presentation timestamp into a wall-clock render
 * deadline in the [System.nanoTime] domain, so frames are presented on the
 * cadence the host encoded them at rather than as fast as they decode.
 *
 * The first frame anchors the timeline: its render deadline is "now", and every
 * later frame is scheduled at `anchorWall + (pts - anchorPts)`. This absorbs
 * bursty USB arrival — a clump of frames that decode together still hand off to
 * SurfaceFlinger spread across their intended intervals.
 *
 * The class is pure (no Android framework types) so it can be unit tested; the
 * decoder passes the result to `MediaCodec.releaseOutputBuffer(index, ns)`.
 */
internal class FramePacer(
    /** Present a frame immediately if its deadline is more than this far past. */
    private val maxLagNs: Long = 100_000_000L, // 100 ms
    /** Re-anchor if a PTS jumps backwards by more than this (stream reset). */
    private val resetThresholdNs: Long = 1_000_000_000L, // 1 s
) {
    private var anchored = false
    private var anchorWallNs = 0L
    private var anchorPtsNs = 0L
    private var lastPtsNs = Long.MIN_VALUE

    /**
     * Compute the render deadline for a frame.
     *
     * @param ptsNs frame presentation timestamp in nanoseconds
     * @param nowNs current time in the [System.nanoTime] domain
     * @return the nanoTime-domain instant to present the frame
     */
    fun deadlineNs(ptsNs: Long, nowNs: Long): Long {
        // Anchor on the first frame, or re-anchor if the timeline jumped
        // (new stream / looped source / PTS wrap) so we don't stall for seconds.
        if (!anchored ||
            ptsNs < lastPtsNs - resetThresholdNs ||
            ptsNs > lastPtsNs + resetThresholdNs
        ) {
            anchored = true
            anchorWallNs = nowNs
            anchorPtsNs = ptsNs
        }
        lastPtsNs = ptsNs

        val target = anchorWallNs + (ptsNs - anchorPtsNs)
        // Never schedule in the past; if we've fallen far behind, drop the
        // backlog by re-anchoring to now so we don't accumulate lag forever.
        if (target < nowNs - maxLagNs) {
            anchorWallNs = nowNs
            anchorPtsNs = ptsNs
            return nowNs
        }
        return maxOf(target, nowNs)
    }

    /** Forget the timeline so the next frame re-anchors (e.g. after a stall). */
    fun reset() {
        anchored = false
        lastPtsNs = Long.MIN_VALUE
    }
}
