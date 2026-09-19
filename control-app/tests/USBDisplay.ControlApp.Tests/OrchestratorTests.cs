using System.Diagnostics;
using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Services;

namespace USBDisplay.ControlApp.Tests;

#region Fakes (no hardware, no admin, no driver)

internal sealed class FakeElevation : IElevationService
{
    public bool IsElevated => false;
    public bool IsAdminUser => true;
    public int RunElevated(string exePath, string[] args, string? workingDir = null) => 0;
}

internal sealed class FakeDriver : IDriverManager
{
    public string ScriptsDir => Path.GetTempPath();
    public string DriverVersion => "0.2.0.0";
    public DriverInfo Info { get; set; } = Loaded();

    public static DriverInfo Loaded() => new(true, "oem42.inf", true, @"ROOT\FAKE\0",
        "OK", "WUDFRd", 0, true, true, "0.2.0.0");

    public Task<DriverInfo> GetDriverInfoAsync(CancellationToken ct = default) => Task.FromResult(Info);
    public Task<ProcessResult> InstallAsync(CancellationToken ct = default) => Task.FromResult(new ProcessResult(0, "", ""));
    public Task<ProcessResult> UninstallAsync(CancellationToken ct = default) => Task.FromResult(new ProcessResult(0, "", ""));
    public Task RestartAsync(CancellationToken ct = default) => Task.CompletedTask;
    public Task SetEnabledAsync(string instanceId, bool enable, CancellationToken ct = default) => Task.CompletedTask;
}

internal sealed class FakeSession : IStreamSession
{
#pragma warning disable CS0067 // interface event; raised by real sessions only
    public event EventHandler<string>? LineReceived;
#pragma warning restore CS0067
    public event EventHandler<int>? Exited;
    public int Pid => 4242;
    public string CommandLine => "fake stream-capture";
    public DateTimeOffset StartTime => DateTimeOffset.Now;
    public bool HasExited { get; private set; }
    public void Stop(TimeSpan gracefulTimeout) => HasExited = true;
    public void Dispose() => HasExited = true;
    public void FireExit(int code)
    {
        HasExited = true;
        Exited?.Invoke(this, code);
    }
}

internal sealed class FakeStreamer : IStreamerCli
{
    public FakeSession? LastSession;
    public StreamStartOptions? LastOptions;
    public string ExePath => "fake-streamer.exe";
    public bool Available => true;
    public Task<IReadOnlyList<DeviceInfo>> GetDevicesAsync(CancellationToken ct = default) =>
        Task.FromResult<IReadOnlyList<DeviceInfo>>(Array.Empty<DeviceInfo>());
    public Task<Dictionary<string, string>> GetCapabilitiesAsync(CancellationToken ct = default) =>
        Task.FromResult(new Dictionary<string, string> { ["transport"] = "adb-compat" });
    public Task<ProcessResult> ProbeFrameAsync(CancellationToken ct = default) =>
        Task.FromResult(new ProcessResult(0, "probe_frame_bytes=1", ""));
    public Task<ProcessResult> TransportProbeAsync(CancellationToken ct = default) =>
        Task.FromResult(new ProcessResult(0, "transport_packets=1", ""));
    public IStreamSession StartStream(StreamStartOptions options, DataReceivedEventHandler onOut, DataReceivedEventHandler onErr)
    {
        LastOptions = options;
        LastSession = new FakeSession();
        return LastSession;
    }
}

internal sealed class FakeAdb : IAdbClient
{
    public string ExePath => "fake-adb.exe";
    public bool Available => true;
    public List<DeviceInfo> Devices { get; set; } = new()
    {
        new DeviceInfo("FAKE123", AdbDeviceState.Device, "FakeTablet", "fake", "1"),
    };
    public List<AdbForward> Forwards { get; set; } = new();
    public Task<IReadOnlyList<DeviceInfo>> ListDevicesAsync(CancellationToken ct = default) =>
        Task.FromResult<IReadOnlyList<DeviceInfo>>(Devices);
    public Task ReconnectAsync(CancellationToken ct = default) => Task.CompletedTask;
    public Task RestartServerAsync(CancellationToken ct = default) => Task.CompletedTask;
    public Task InstallApkAsync(string apkPath, CancellationToken ct = default) => Task.CompletedTask;
    public Task LaunchAppAsync(string package, CancellationToken ct = default) => Task.CompletedTask;
    public Task<IReadOnlyList<AdbForward>> ListForwardsAsync(CancellationToken ct = default) =>
        Task.FromResult<IReadOnlyList<AdbForward>>(Forwards);
    public Task<ForwardResult> EnsureForwardAsync(int port, string? serial = null, CancellationToken ct = default)
    {
        Forwards.Add(new AdbForward(serial ?? Devices.FirstOrDefault()?.Serial ?? "FAKE123", $"tcp:{port}", $"tcp:{port}"));
        return Task.FromResult(new ForwardResult(true, "fake forward ok", null));
    }
    public Task RemoveForwardAsync(int port, string? serial = null, CancellationToken ct = default)
    {
        Forwards.RemoveAll(f => f.LocalSpec == $"tcp:{port}");
        return Task.CompletedTask;
    }
    public Task<int> RemoveStaleForwardsAsync(int port, CancellationToken ct = default) => Task.FromResult(0);
}

internal sealed class FakeDisplays : IDisplayManager
{
    public bool HasUsbDisplay { get; set; } = true;
    public IReadOnlyList<DisplayInfo> GetDisplays() => HasUsbDisplay
        ? new[] { new DisplayInfo("USBDisplay Virtual Monitor", "ACTIVE", 1920, 1080, 60, true, false) }
        : Array.Empty<DisplayInfo>();
    public DisplayInfo? GetUsbDisplay() => GetDisplays().FirstOrDefault();
    public void OpenDisplaySettings() { }
    public void ExtendDisplays() { }
}

internal sealed class FakeDiagnostics : IDiagnosticsService
{
    public Task<IReadOnlyList<DiagnosticResult>> RunFullDiagnosticAsync(
        IProgress<DiagnosticResult>? progress = null, CancellationToken ct = default) =>
        Task.FromResult<IReadOnlyList<DiagnosticResult>>(Array.Empty<DiagnosticResult>());
}

internal sealed class FakeWifiPreflight : Transport.IWifiPreflight
{
    public Transport.TransportStatus Result { get; set; } = new Transport.TransportStatus(
        TransportKind.Wifi, SubsystemState.Connecting, "192.168.1.42:27184", "Reachable (fake).", null);
    public Task<Transport.TransportStatus> CheckAsync(CancellationToken ct = default) => Task.FromResult(Result);
}

#endregion

[TestClass]
public sealed class OrchestratorTests
{
    private static (RealUsbDisplayGateway Gateway, FakeStreamer Streamer) Create(
        TimeSpan? firstFrameTimeout = null,
        Action<FakeDriver, FakeAdb, FakeDisplays>? tweak = null,
        Transport.IWifiPreflight? preflight = null)
    {
        var path = Path.Combine(Path.GetTempPath(), $"usbdisplay-test-{Guid.NewGuid()}.json");
        var config = new ConfigurationService(path);
        var driver = new FakeDriver();
        var adb = new FakeAdb();
        var displays = new FakeDisplays();
        tweak?.Invoke(driver, adb, displays);
        var streamer = new FakeStreamer();
        var gateway = new RealUsbDisplayGateway(
            config, new FakeElevation(), driver, streamer, adb, displays,
            new ProcessManager(), new FakeDiagnostics(), new LogService(),
            firstFrameTimeout ?? TimeSpan.FromSeconds(30), preflight);
        return (gateway, streamer);
    }

    [TestMethod]
    public async Task Start_BecomesActiveOnlyAfterVerifiedFrames()
    {
        var (gateway, _) = Create();
        var start = gateway.StartAsync();
        // No frames yet: must NOT be Active.
        await Task.Delay(300);
        Assert.AreNotEqual(SystemState.Active, gateway.State);
        // Feed connection + frames through the live path.
        gateway.TestFeedLine("android_connection=established");
        gateway.TestFeedLine("{\"streamed_frames\":30,\"streamed_packets\":120,\"write_stall_ms_max\":2.5,\"input_events_injected\":0}");
        gateway.TestFeedLine("{\"streamed_frames\":60,\"streamed_packets\":240,\"write_stall_ms_max\":2.5,\"input_events_injected\":3}");
        await start;
        Assert.AreEqual(SystemState.Active, gateway.State);
        Assert.AreEqual(HealthState.Healthy, gateway.Health);
        Assert.AreEqual(60, gateway.Telemetry.StreamedFrames);
        await gateway.StopAsync();
        Assert.AreEqual(SystemState.Stopped, gateway.State);
    }

    [TestMethod]
    public async Task Start_FailsCleanlyWithoutDriver()
    {
        var (gateway, _) = Create(tweak: (driver, _, _) =>
            driver.Info = driver.Info with { PackageInstalled = false });
        await gateway.StartAsync();
        Assert.AreEqual(SystemState.Error, gateway.State);
        StringAssert.Contains(gateway.StatusMessage, "not installed");
    }

    [TestMethod]
    public async Task Start_FailsCleanlyWithoutDevice()
    {
        var (gateway, _) = Create(tweak: (_, adb, _) => adb.Devices.Clear());
        await gateway.StartAsync();
        Assert.AreEqual(SystemState.Error, gateway.State);
        // User-facing layer names the failure; technical detail carries adb state.
        Assert.AreEqual("Tablet connection failed.", gateway.StatusMessage);
        Assert.IsNotNull(gateway.LastError);
        StringAssert.Contains(gateway.LastError!.TechnicalDetail, "adb_serial");
    }

    [TestMethod]
    public async Task Start_TimesOutWithoutFrames()
    {
        var (gateway, _) = Create(firstFrameTimeout: TimeSpan.FromSeconds(2));
        await gateway.StartAsync();
        // Session started (connection never established, no frames) → Error, never ACTIVE.
        Assert.AreEqual(SystemState.Error, gateway.State);
    }

    [TestMethod]
    public async Task WifiStart_PassesPinDeviceIpAndTransport()
    {
        var path = Path.Combine(Path.GetTempPath(), $"usbdisplay-test-{Guid.NewGuid()}.json");
        var config = new ConfigurationService(path);
        config.Settings.Transport = "wifi";
        config.Settings.DeviceIp = "192.168.1.42";
        config.Settings.Pin = "123456";
        var streamer = new FakeStreamer();
        var gateway = new RealUsbDisplayGateway(
            config, new FakeElevation(), new FakeDriver(), streamer, new FakeAdb(),
            new FakeDisplays(), new ProcessManager(), new FakeDiagnostics(),
            new LogService(), TimeSpan.FromSeconds(30), new FakeWifiPreflight());
        var start = gateway.StartAsync();
        gateway.TestFeedLine("android_connection=established");
        gateway.TestFeedLine("{\"streamed_frames\":10,\"streamed_packets\":40,\"write_stall_ms_max\":1.0,\"input_events_injected\":0}");
        gateway.TestFeedLine("{\"streamed_frames\":20,\"streamed_packets\":80,\"write_stall_ms_max\":1.0,\"input_events_injected\":0}");
        await start;
        Assert.AreEqual(SystemState.Active, gateway.State);
        Assert.IsNotNull(streamer.LastOptions);
        Assert.AreEqual(TransportKind.Wifi, streamer.LastOptions!.Transport);
        Assert.AreEqual("192.168.1.42", streamer.LastOptions!.DeviceIp);
        Assert.AreEqual("123456", streamer.LastOptions!.Pin);
        await gateway.StopAsync();
        try { File.Delete(path); } catch { }
    }

    [TestMethod]
    public async Task Stop_MeansSessionOnly_DriverStays()
    {
        var (gateway, _) = Create();
        var start = gateway.StartAsync();
        gateway.TestFeedLine("android_connection=established");
        gateway.TestFeedLine("{\"streamed_frames\":10,\"streamed_packets\":40,\"write_stall_ms_max\":1.0,\"input_events_injected\":0}");
        gateway.TestFeedLine("{\"streamed_frames\":20,\"streamed_packets\":80,\"write_stall_ms_max\":1.0,\"input_events_injected\":0}");
        await start;
        Assert.AreEqual(SystemState.Active, gateway.State);
        await gateway.StopAsync();
        Assert.AreEqual(SystemState.Stopped, gateway.State);
        // Driver untouched by stop.
        Assert.IsTrue(gateway.Driver?.PackageInstalled == true);
    }
}
