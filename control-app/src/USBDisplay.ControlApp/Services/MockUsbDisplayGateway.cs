using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using System.Timers;
using USBDisplay.ControlApp.Models;

namespace USBDisplay.ControlApp.Services;

/// <summary>
/// Scripted in-memory gateway for UI development and demonstration without
/// hardware, driver, or admin. The UI shows DEMO MODE whenever this is active.
/// </summary>
public sealed class MockUsbDisplayGateway : IUsbDisplayGateway, IDisposable
{
    private readonly System.Timers.Timer _tick;
    private long _frames;
    private bool _disposed;

    public MockUsbDisplayGateway(IConfigurationService config, ILogService log)
    {
        Settings = config.Settings;
        Log = log;
        Driver = new DriverInfo(true, "oem42.inf", true, @"ROOT\DEVGEN\DEMO", "OK", "WUDFRd", 0, true, true, "0.2.0.0");
        Displays = new List<DisplayInfo>
        {
            new("USBDisplay Virtual Monitor", "ACTIVE", 1920, 1080, 60, true, false),
            new("Generic PnP Monitor", "ACTIVE", 2560, 1440, 60, false, true),
        };
        Devices = new List<DeviceInfo> { new("DEMO123", AdbDeviceState.Device, "DemoTablet", "demo", "1") };
        Capabilities = new Dictionary<string, string>
        {
            ["codecs"] = "h264,h265,av1",
            ["transport"] = "adb-compat,native-usb-bulk,wifi-tls",
        };
        _tick = new System.Timers.Timer(500) { AutoReset = true };
        _tick.Elapsed += (_, _) => OnTick();
    }

    public AppSettings Settings { get; }
    public ILogService Log { get; }
    public bool DemoMode => true;
    public SystemState State { get; private set; } = SystemState.Stopped;
    public HealthState Health { get; private set; } = HealthState.Unknown;
    public string StatusMessage { get; private set; } = "Stopped (demo).";
    public IReadOnlyList<StartupStep> StartupSteps { get; private set; } = Array.Empty<StartupStep>();
    public StreamTelemetry Telemetry { get; private set; } = DemoTelemetry(0);
    public WifiLinkInfo? WifiLink => Settings.Transport == "wifi"
        ? new WifiLinkInfo("192.168.1.42:27184 (demo)", "TLS 1.3 (demo)", "SHA256:demo", true, "host-demo", true, 12_000_000, "Stable (demo)", null, 0)
        : null;
    public ErrorReport? LastError => null;
    public DriverInfo? Driver { get; private set; }
    public IReadOnlyList<DisplayInfo> Displays { get; private set; }
    public IReadOnlyList<DeviceInfo> Devices { get; private set; }
    public IReadOnlyList<ManagedProcessInfo> Processes { get; private set; } = Array.Empty<ManagedProcessInfo>();
    public IReadOnlyList<ServiceEntry> ServiceEntries { get; private set; } = new List<ServiceEntry>
    {
        new("USBDisplay Streamer (demo)", "Simulated.", "RUNNING", "demo.exe", 4242, TimeSpan.FromMinutes(3), null),
    };
    public IReadOnlyList<DiagnosticResult> LastDiagnostics { get; private set; } = Array.Empty<DiagnosticResult>();
    public Dictionary<string, string> Capabilities { get; private set; }
    public bool IsElevated => false;

    public event EventHandler? Changed;
    public void NotifyChanged() => Changed?.Invoke(this, EventArgs.Empty);

    public UsbDisplayState GetStateSnapshot() => UsbDisplayState.Empty(Telemetry) with
    {
        OverallState = State,
        SelectedTransport = Settings.Transport == "wifi" ? TransportKind.Wifi : TransportKind.Usb,
    };

    public Task SwitchTransportAsync(TransportKind kind)
    {
        Settings.Transport = kind == TransportKind.Wifi ? "wifi" : "usb";
        NotifyChanged();
        return Task.CompletedTask;
    }

    public Task RefreshAllAsync(CancellationToken ct = default)
    {
        NotifyChanged();
        return Task.CompletedTask;
    }

    public async Task StartAsync(CancellationToken ct = default)
    {
        State = SystemState.Starting;
        var steps = new List<string>
        {
            "Running elevated", "Driver detected", "Virtual display driver loaded",
            "Virtual display ready (1920×1080)", "Streamer binary found",
            "Android device: DEMO123", "Transport process started",
        };
        StartupSteps = Array.Empty<StartupStep>();
        NotifyChanged();
        var done = new List<StartupStep>();
        foreach (var s in steps)
        {
            await Task.Delay(350, ct).ConfigureAwait(false);
            done.Add(new StartupStep(s, true, false));
            StartupSteps = done.ToArray();
            Log.Log(LogLevel.Info, "demo", s);
            NotifyChanged();
        }
        State = SystemState.Active;
        StatusMessage = "USB DISPLAY ACTIVE (demo)";
        Health = HealthState.Healthy;
        _tick.Start();
        NotifyChanged();
    }

    public Task StopAsync()
    {
        _tick.Stop();
        State = SystemState.Stopped;
        StatusMessage = "Stopped (demo).";
        Health = HealthState.Unknown;
        NotifyChanged();
        return Task.CompletedTask;
    }

    public async Task RestartAsync(CancellationToken ct = default)
    {
        await StopAsync().ConfigureAwait(false);
        await StartAsync(ct).ConfigureAwait(false);
    }

    public Task RestartComponentAsync(string component) => RestartAsync();

    public Task<IReadOnlyList<DiagnosticResult>> RunDiagnosticsAsync(
        IProgress<DiagnosticResult>? progress = null, CancellationToken ct = default)
    {
        var names = new[] { "Administrator privileges", "Driver installed", "ROOT device exists",
            "Driver loaded (problem 0, WUDFRd)", "Virtual monitor available", "Streamer executable found",
            "Android device detected", "Android authorized", "Transport handshake", "First frame received" };
        var list = names.Select(n =>
        {
            var r = new DiagnosticResult(n, DiagnosticStatus.Pass, "demo ok", null);
            progress?.Report(r);
            return r;
        }).ToArray();
        LastDiagnostics = list;
        NotifyChanged();
        return Task.FromResult<IReadOnlyList<DiagnosticResult>>(list);
    }

    public void OpenDisplaySettings() => Log.Log(LogLevel.Info, "demo", "Open display settings (simulated).");
    public void ExtendDisplays() => Log.Log(LogLevel.Info, "demo", "Extend displays (simulated).");
    public Task RunAdbMaintenanceAsync(string what) { Log.Log(LogLevel.Info, "demo", $"ADB {what} (simulated)."); return Task.CompletedTask; }
    public void SaveSettings() => Log.Log(LogLevel.Info, "demo", "Settings saved (simulated).");
    public Task DriverSetEnabledAsync(bool enable) { Log.Log(LogLevel.Info, "demo", $"Driver {(enable ? "enabled" : "disabled")} (simulated)."); return Task.CompletedTask; }
    public Task DriverInstallAsync() { Log.Log(LogLevel.Info, "demo", "Install (simulated)."); return Task.CompletedTask; }
    public Task DriverUninstallAsync() { Log.Log(LogLevel.Warning, "demo", "Uninstall (simulated)."); return Task.CompletedTask; }
    public Task DriverRestartAsync() { Log.Log(LogLevel.Info, "demo", "Restart (simulated)."); return Task.CompletedTask; }

    private void OnTick()
    {
        _frames += 30;
        Telemetry = DemoTelemetry(_frames);
        NotifyChanged();
    }

    private static StreamTelemetry DemoTelemetry(long frames) => new(
        frames, frames * 4, 3.2, 128, 59.8, "H.264", "1920×1080",
        "MediaFoundation", 12_000_000, 0, 0, 0, DateTimeOffset.Now);

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        _tick.Dispose();
    }
}

/// <summary>Honest feature-availability matrix (§24). Never imply stubs work.</summary>
public static class FeatureMatrix
{
    public static IReadOnlyList<FeatureAvailability> Get() => new List<FeatureAvailability>
    {
        new("H.264 encode (Media Foundation)", ComponentState.Ready, "Implemented; selected via backend chain."),
        new("H.265 encode", ComponentState.Ready, "Implemented where the backend supports it."),
        new("USB transport (adb-forward)", ComponentState.Ready, "Implemented; port 27183."),
        new("WiFi transport (TLS 1.3 + PIN)", ComponentState.Ready, "Implemented; port 27184, QR pairing."),
        new("Adaptive bitrate", ComponentState.Ready, "RateController 20→12→8→4 Mbps."),
        new("Input return (USB/WiFi)", ComponentState.Ready, "Pointer + keyboard via Control packets."),
        new("Native USB bulk transport", ComponentState.ComingSoon, "Backends are honest stubs."),
        new("Pen pressure / tilt", ComponentState.ComingSoon, "Coordinates only today."),
        new("Dedicated HID device", ComponentState.ComingSoon, "SendInput drives the system cursor today."),
        new("AV1", ComponentState.ComingSoon, "Negotiated in caps; encoder pending."),
    };
}
