using USBDisplay.ControlApp.Services;

namespace USBDisplay.ControlApp.Tests;

[TestClass]
public sealed class TelemetryTests
{
    [TestMethod]
    public void Accumulator_TracksConnectionAndCounts()
    {
        var acc = new StreamTelemetryAccumulator();
        Assert.IsFalse(acc.Connected);
        acc.FeedLine("android_connection=established");
        Assert.IsTrue(acc.Connected);
        acc.FeedLine("{\"streamed_frames\":60,\"streamed_packets\":240,\"write_stall_ms_max\":3.21,\"input_events_injected\":7}");
        var snap = acc.Snapshot();
        Assert.AreEqual(60, snap.StreamedFrames);
        Assert.AreEqual(240, snap.StreamedPackets);
        Assert.AreEqual(3.21, snap.WriteStallMsMax, 0.001);
        Assert.AreEqual(7, snap.InputEventsInjected);
    }

    [TestMethod]
    public void Accumulator_IgnoresGarbage()
    {
        var acc = new StreamTelemetryAccumulator();
        acc.FeedLine("");
        acc.FeedLine("   ");
        acc.FeedLine("backend MediaFoundation: SELECTED");
        var snap = acc.Snapshot();
        Assert.AreEqual(0, snap.StreamedFrames);
        Assert.IsFalse(acc.Connected);
    }

    [TestMethod]
    public void Accumulator_StaticInfoAndDisconnect()
    {
        var acc = new StreamTelemetryAccumulator();
        acc.SetStatic("h264", "1920x1080", 12_000_000);
        acc.FeedLine("stream_encoder_backend=MediaFoundation");
        var snap = acc.Snapshot();
        Assert.AreEqual("H264", snap.Codec.ToUpperInvariant());
        Assert.AreEqual("MediaFoundation", snap.EncoderBackend);
        acc.MarkDisconnected();
        Assert.IsFalse(acc.Connected);
    }
}
