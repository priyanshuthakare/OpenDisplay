using System;
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

    [TestMethod]
    public void WifiPayload_ParsesQrJson()
    {
        var p = WifiPairPayload.Parse(
            "{\"v\":1,\"ip\":\"192.168.1.42\",\"port\":27184,\"fp\":\"SHA256:abcd\"}");
        Assert.AreEqual("192.168.1.42", p.Ip);
        Assert.AreEqual(27184, p.Port);
        Assert.AreEqual("SHA256:abcd", p.Fingerprint);
    }

    [TestMethod]
    public void WifiPayload_ParsesBareIpAndIpPort()
    {
        Assert.AreEqual("192.168.1.42", WifiPairPayload.Parse("192.168.1.42").Ip);
        Assert.AreEqual(1234, WifiPairPayload.Parse("192.168.1.42:1234").Port);
    }

    [TestMethod]
    public void WifiPayload_RejectsUnsupportedOrMissingVersion()
    {
        // The tablet only speaks v1; a different or absent version must not be
        // silently accepted.
        Assert.ThrowsException<ArgumentException>(
            () => WifiPairPayload.Parse("{\"v\":2,\"ip\":\"192.168.1.42\"}"));
        Assert.ThrowsException<ArgumentException>(
            () => WifiPairPayload.Parse("{\"ip\":\"192.168.1.42\"}"));
    }

    [TestMethod]
    public void WifiPayload_RejectsMalformedJsonAndBadPort()
    {
        Assert.ThrowsException<ArgumentException>(() => WifiPairPayload.Parse("{not json"));
        Assert.ThrowsException<ArgumentException>(
            () => WifiPairPayload.Parse("{\"v\":1,\"ip\":\"10.0.0.5\",\"port\":70000}"));
    }
}
