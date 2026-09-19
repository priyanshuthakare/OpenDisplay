using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Services;
using USBDisplay.ControlApp.Transport;

namespace USBDisplay.ControlApp.Tests;

[TestClass]
public sealed class WifiPairingTests
{
    [TestMethod]
    public void PairPayload_AcceptsBareIp()
    {
        var p = WifiPairPayload.Parse("192.168.1.42");
        Assert.AreEqual("192.168.1.42", p.Ip);
        Assert.AreEqual(27184, p.Port);
        Assert.IsNull(p.Fingerprint);
    }

    [TestMethod]
    public void PairPayload_AcceptsIpPort()
    {
        var p = WifiPairPayload.Parse("192.168.1.42:27184");
        Assert.AreEqual("192.168.1.42", p.Ip);
        Assert.AreEqual(27184, p.Port);
    }

    [TestMethod]
    public void PairPayload_AcceptsTabletQrJson()
    {
        var fp = "SHA256:" + new string('a', 64);
        var p = WifiPairPayload.Parse($"{{\"v\":1,\"ip\":\"192.168.1.42\",\"port\":27184,\"fp\":\"{fp}\"}}");
        Assert.AreEqual("192.168.1.42", p.Ip);
        Assert.AreEqual(fp, p.Fingerprint);
        Assert.IsTrue(FingerprintUtil.IsValid(p.Fingerprint));
    }

    [TestMethod]
    public void PairPayload_RejectsShellInjection()
    {
        Assert.ThrowsException<ArgumentException>(() => WifiPairPayload.Parse("192.168.1.1; rm -rf /"));
        Assert.ThrowsException<ArgumentException>(() => WifiPairPayload.Parse(""));
    }

    [TestMethod]
    public void Fingerprint_ValidatesShape()
    {
        Assert.IsTrue(FingerprintUtil.IsValid("SHA256:" + new string('A', 64)));
        Assert.IsTrue(FingerprintUtil.IsValid("SHA256:" + new string('a', 64)));
        Assert.IsFalse(FingerprintUtil.IsValid("SHA256:xyz"));
        Assert.IsFalse(FingerprintUtil.IsValid(null));
        Assert.AreEqual("SHA256:" + new string('a', 64), FingerprintUtil.Normalize("SHA256:" + new string('A', 64)));
    }

    [TestMethod]
    public void Pin_ValidatesSixDigits_ConstantTime()
    {
        Assert.IsTrue(FingerprintUtil.IsValidPin("481203"));
        Assert.IsFalse(FingerprintUtil.IsValidPin("12345"));
        Assert.IsFalse(FingerprintUtil.IsValidPin("abcdef"));
        Assert.IsTrue(FingerprintUtil.ConstantTimeEquals("123456", "123456"));
        Assert.IsFalse(FingerprintUtil.ConstantTimeEquals("123456", "123457"));
        Assert.IsFalse(FingerprintUtil.ConstantTimeEquals("123456", "12345"));
    }

    [TestMethod]
    public void PinTracker_ThreeStrikesThenLockout()
    {
        var t = new PinAttemptTracker();
        Assert.AreEqual(3, t.AttemptsRemaining);
        t.NoteFailure();
        t.NoteFailure();
        Assert.AreEqual(1, t.AttemptsRemaining);
        Assert.IsFalse(t.IsLockedOut);
        t.NoteFailure();
        Assert.IsTrue(t.IsLockedOut);
        StringAssert.Contains(t.Display(), "locked");
        t.NoteSuccess();
        Assert.IsFalse(t.IsLockedOut);
        Assert.AreEqual(3, t.AttemptsRemaining);
    }

    [TestMethod]
    public void TrustStore_ReadsAndForgets()
    {
        var path = Path.Combine(Path.GetTempPath(), $"usbdisplay-paired-{Guid.NewGuid()}.json");
        try
        {
            File.WriteAllText(path, "{\"tablet_id\":\"t1\",\"ip\":\"192.168.1.42\",\"cert_fingerprint\":\"SHA256:" + new string('b', 64) + "\",\"paired_at\":\"2026-01-01\"}");
            var store = new WifiTrustStore(path);
            Assert.IsTrue(store.HasTrust);
            var found = store.FindByIp("192.168.1.42");
            Assert.IsNotNull(found);
            Assert.IsTrue(FingerprintUtil.IsValid(found!.Fingerprint));
            store.Forget();
            Assert.IsFalse(store.HasTrust);
        }
        finally
        {
            try { File.Delete(path); } catch { }
        }
    }
}

[TestClass]
public sealed class TransportPolicyTests
{
    [TestMethod]
    public void NoSilentFallback_MessageRequiresExplicitChoice()
    {
        var usbFail = TransportPolicy.NoSilentFallbackMessage(TransportKind.Usb, TransportKind.Wifi);
        StringAssert.Contains(usbFail, "never switches automatically");
        StringAssert.Contains(TransportPolicy.WifiUnreachableMessage(), "AP-isolated");
        Assert.IsTrue(TransportPolicy.IsExplicitSwitch(TransportKind.Usb, TransportKind.Wifi));
        Assert.IsFalse(TransportPolicy.IsExplicitSwitch(TransportKind.Usb, TransportKind.Usb));
        Assert.AreEqual(27183, TransportPolicy.UsbPort);
        Assert.AreEqual(27184, TransportPolicy.WifiPort);
    }

    [TestMethod]
    public async Task Gateway_SwitchTransport_IsExplicitAndPersisted()
    {
        var path = Path.Combine(Path.GetTempPath(), $"usbdisplay-test-{Guid.NewGuid()}.json");
        try
        {
            var config = new ConfigurationService(path);
            var gateway = new RealUsbDisplayGateway(
                config, new FakeElevation(), new FakeDriver(), new FakeStreamer(), new FakeAdb(),
                new FakeDisplays(), new ProcessManager(), new FakeDiagnostics(),
                new LogService(), TimeSpan.FromSeconds(5));
            Assert.AreEqual("usb", config.Settings.Transport);
            await gateway.SwitchTransportAsync(TransportKind.Wifi);
            Assert.AreEqual("wifi", config.Settings.Transport);
            await gateway.SwitchTransportAsync(TransportKind.Usb);
            Assert.AreEqual("usb", config.Settings.Transport);
            gateway.Dispose();
        }
        finally
        {
            try { File.Delete(path); } catch { }
        }
    }

    [TestMethod]
    public async Task Gateway_UsbFailure_NeverAutoSwitchesToWifi()
    {
        var path = Path.Combine(Path.GetTempPath(), $"usbdisplay-test-{Guid.NewGuid()}.json");
        try
        {
            var config = new ConfigurationService(path);
            config.Settings.Transport = "usb";
            var adb = new FakeAdb();
            adb.Devices.Clear();
            var gateway = new RealUsbDisplayGateway(
                config, new FakeElevation(), new FakeDriver(), new FakeStreamer(), adb,
                new FakeDisplays(), new ProcessManager(), new FakeDiagnostics(),
                new LogService(), TimeSpan.FromSeconds(5));
            await gateway.StartAsync();
            Assert.AreEqual(SystemState.Error, gateway.State);
            // Still USB — no silent downgrade happened.
            Assert.AreEqual("usb", config.Settings.Transport);
            gateway.Dispose();
        }
        finally
        {
            try { File.Delete(path); } catch { }
        }
    }
}

[TestClass]
public sealed class UsbDisplayStateTests
{
    [TestMethod]
    public void EmptyState_HasBothTransports_AndIsNotActive()
    {
        var s = UsbDisplayState.Empty();
        Assert.IsFalse(s.IsActive);
        CollectionAssert.Contains(s.AvailableTransports.ToList(), TransportKind.Usb);
        CollectionAssert.Contains(s.AvailableTransports.ToList(), TransportKind.Wifi);
        Assert.AreEqual(TransportKind.Usb, s.SelectedTransport);
    }

    [TestMethod]
    public void ErrorReport_CarriesThreeLayers()
    {
        var e = new ErrorReport("Tablet connection failed.", "Listener timed out.", "android_connection=not_established", "[ Retry ]", DateTimeOffset.Now);
        Assert.AreEqual("Tablet connection failed.", e.UserMessage);
        StringAssert.Contains(e.TechnicalDetail, "android_connection");
    }
}

[TestClass]
public sealed class ConfigToleranceTests
{
    [TestMethod]
    public void Settings_IgnoresUnknownFields_AndKeepsNewDefaults()
    {
        var path = Path.Combine(Path.GetTempPath(), $"usbdisplay-test-{Guid.NewGuid()}.json");
        try
        {
            File.WriteAllText(path, "{\"codec\":\"h265\",\"future_field_xyz\":123,\"transport\":\"wifi\"}");
            var config = new ConfigurationService(path);
            Assert.AreEqual("h265", config.Settings.Codec);
            Assert.AreEqual("wifi", config.Settings.Transport);
            // New fields keep compiled defaults when absent.
            Assert.AreEqual(27184, config.Settings.WifiPort);
            Assert.AreEqual(45, config.Settings.ConnectionTimeoutSeconds);
            Assert.AreEqual("Extend", config.Settings.TopologyPreference);
        }
        finally
        {
            try { File.Delete(path); } catch { }
        }
    }

    [TestMethod]
    public void Telemetry_ParsesWifiAndAdaptation()
    {
        var acc = new StreamTelemetryAccumulator();
        acc.FeedLine("wifi_device=192.168.1.42:27184");
        acc.FeedLine("wifi_fingerprint=SHA256:" + new string('c', 64));
        acc.FeedLine("wifi_encryption=tls-1.3");
        Assert.AreEqual("192.168.1.42:27184", acc.WifiPeer);
        Assert.AreEqual("tls-1.3", acc.WifiEncryption);
        acc.FeedLine("bitrate_step_down old_bitrate=12000000 new_bitrate=8000000");
        Assert.AreEqual(1, acc.AdaptationEvents.Count);
        StringAssert.Contains(acc.Adaptation, "down");
    }
}
