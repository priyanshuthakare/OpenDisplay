using USBDisplay.ControlApp.Services;

namespace USBDisplay.ControlApp.Tests;

[TestClass]
public sealed class PairingQrTests
{
    [TestMethod]
    public void StableHostId_HasHostPrefixAndIsStable()
    {
        var a = HostIdentity.StableHostId();
        var b = HostIdentity.StableHostId();
        Assert.AreEqual(a, b);
        Assert.IsTrue(HostIdentity.IsValidHostId(a));
    }

    [TestMethod]
    public void PairingPayload_MatchesTabletContract()
    {
        // Must stay parseable by Android PcPairPayload.parseHostId:
        // {"v":1,"host_id":"host-…"} with [a-z0-9-_] charset.
        var payload = HostIdentity.BuildPairingQrPayload("host-acer-pc");
        Assert.AreEqual("{\"v\":1,\"host_id\":\"host-acer-pc\"}", payload);
    }

    [TestMethod]
    public void PairingPayload_RejectsBadIds()
    {
        Assert.ThrowsException<ArgumentException>(() => HostIdentity.BuildPairingQrPayload("evil-box"));
        Assert.ThrowsException<ArgumentException>(() => HostIdentity.BuildPairingQrPayload(""));
        Assert.IsFalse(HostIdentity.IsValidHostId("HOST-ABC"));
    }

    [TestMethod]
    public void QrCode_RendersPngBytes()
    {
        var png = QrCodeService.RenderPng("{\"v\":1,\"host_id\":\"host-test\"}");
        Assert.IsTrue(png.Length > 100);
        // PNG signature.
        Assert.AreEqual(0x89, png[0]);
        Assert.AreEqual((byte)'P', png[1]);
        Assert.AreEqual((byte)'N', png[2]);
        Assert.AreEqual((byte)'G', png[3]);
    }
}
