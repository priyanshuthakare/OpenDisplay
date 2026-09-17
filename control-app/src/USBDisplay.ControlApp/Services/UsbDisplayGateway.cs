using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using System.Timers;
using USBDisplay.ControlApp.Models;

namespace USBDisplay.ControlApp.Services;

public interface IUsbDisplayGateway
{
    AppSettings Settings { get; }
    ILogService Log { get; }
    bool DemoMode { get; }

    SystemState State { get; }
    HealthState Health { get; }
    string StatusMessage { get; }
    IReadOnlyList<StartupStep> StartupSteps { get; }
    StreamTelemetry Telemetry { get; }
    DriverInfo? Driver { get; }
    IReadOnlyList<DisplayInfo> Displays { get; }
    IReadOnlyList<DeviceInfo> Devices { get; }
    IReadOnlyList<ManagedProcessInfo> Processes { get; }
    IReadOnlyList<ServiceEntry> ServiceEntries { get; }
    IReadOnlyList<DiagnosticResult> LastDiagnostics { get; }
    Dictionary<string, string> Capabilities { get; }
    bool IsElevated { get; }

    event EventHandler? Changed;

    Task RefreshAllAsync(CancellationToken ct = default);
    Task StartAsync(CancellationToken ct = default);
    Task StopAsync();
    Task RestartAsync(CancellationToken ct = default);
    Task RestartComponentAsync(string component);
    Task<IReadOnlyList<DiagnosticResult>> RunDiagnosticsAsync(
        IProgress<DiagnosticResult>? progress = null, CancellationToken ct = default);
    Task DriverInstallAsync();
    Task DriverUninstallAsync();
    Task DriverRestartAsync();
    Task DriverSetEnabledAsync(bool enable);
    void OpenDisplaySettings();
    void ExtendDisplays();
    Task RunAdbMaintenanceAsync(string what);
    void SaveSettings();
    void NotifyChanged();
}

public sealed class RealUsbDisplayGateway : IUsbDisplayGateway, IDisposable
{
    private readonly IConfigurationService _config;
    private readonly IElevationService _elevation;
    private readonly IDriverManager _drivers;
    private readonly IStreamerCli _streamer;
    private readonly IAdbClient _adb;
    private readonly IDisplayManager _displays;
    private readonly IProcessManager _processes;
    private readonly IDiagnosticsService _diagnostics;
    private readonly System.Timers.Timer _pollTimer;
    private readonly System.Timers.Timer _telemetryTimer;

    private IStreamSession? _session;
    private StreamTelemetryAccumulator _accumulator = new();
    private CancellationTokenSource? _startCts;
    private readonly TimeSpan _firstFrameTimeout;
    private bool _disposed;

    public RealUsbDisplayGateway(
        IConfigurationService config, IElevationService elevation, IDriverManager drivers,
        IStreamerCli streamer, IAdbClient adb, IDisplayManager displays,
        IProcessManager processes, IDiagnosticsService diagnostics, ILogService log,
        TimeSpan? firstFrameTimeout = null)
    {
        _config = config;
        _elevation = elevation;
        _drivers = drivers;
        _streamer = streamer;
        _adb = adb;
        _displays = displays;
        _processes = processes;
        _diagnostics = diagnostics;
        _firstFrameTimeout = firstFrameTimeout ?? TimeSpan.FromSeconds(45);
        Log = log;
        _pollTimer = new System.Timers.Timer(TimeSpan.FromSeconds(5).TotalMilliseconds) { AutoReset = true };
        _pollTimer.Elapsed += async (_, _) => await PollDevicesAsync().ConfigureAwait(false);
        _telemetryTimer = new System.Timers.Timer(1000) { AutoReset = true };
        _telemetryTimer.Elapsed += (_, _) => RefreshTelemetrySnapshot();
    }

    public AppSettings Settings => _config.Settings;
    public ILogService Log { get; }
    public bool DemoMode => false;
    public SystemState State { get; private set; } = SystemState.Stopped;
    public HealthState Health { get; private set; } = HealthState.Unknown;
    public string StatusMessage { get; private set; } = "Stopped.";
    public IReadOnlyList<StartupStep> StartupSteps { get; private set; } = Array.Empty<StartupStep>();
    public StreamTelemetry Telemetry { get; private set; } = new StreamTelemetry(0, 0, 0, 0, 0, "—", "—", "—", 0, 0, 0, 0, DateTimeOffset.Now);
    public DriverInfo? Driver { get; private set; }
    public IReadOnlyList<DisplayInfo> Displays { get; private set; } = Array.Empty<DisplayInfo>();
    public IReadOnlyList<DeviceInfo> Devices { get; private set; } = Array.Empty<DeviceInfo>();
    public IReadOnlyList<ManagedProcessInfo> Processes { get; private set; } = Array.Empty<ManagedProcessInfo>();
    public IReadOnlyList<ServiceEntry> ServiceEntries { get; private set; } = Array.Empty<ServiceEntry>();
    public IReadOnlyList<DiagnosticResult> LastDiagnostics { get; private set; } = Array.Empty<DiagnosticResult>();
    public Dictionary<string, string> Capabilities { get; private set; } = new();
    public bool IsElevated => _elevation.IsElevated;

    public event EventHandler? Changed;
    public void NotifyChanged() => Changed?.Invoke(this, EventArgs.Empty);

    /// <summary>Test hook: feeds one streamer stdout line through the live path.</summary>
    internal void TestFeedLine(string line)
    {
        try { _accumulator.FeedLine(line); } catch { }
        RefreshTelemetrySnapshot();
    }

    public async Task RefreshAllAsync(CancellationToken ct = default)
    {
        try { Driver = await _drivers.GetDriverInfoAsync(ct).ConfigureAwait(false); }
        catch (Exception ex) { Log.Log(LogLevel.Warning, "driver", $"Driver query failed: {ex.Message}"); }
        try { Displays = _displays.GetDisplays(); } catch { }
        await PollDevicesAsync().ConfigureAwait(false);
        Processes = _processes.Snapshot();
        BuildServiceEntries();
        EvaluateHealth();
        NotifyChanged();
    }

    public async Task StartAsync(CancellationToken ct = default)
    {
        if (State is SystemState.Starting or SystemState.Active)
        {
            return;
        }
        _startCts?.Cancel();
        _startCts = CancellationTokenSource.CreateLinkedTokenSource(ct);
        var token = _startCts.Token;
        var steps = new List<StartupStep>();
        void SetSteps() { StartupSteps = steps.ToArray(); NotifyChanged(); }
        void Step(string label, bool done, bool failed = false)
        {
            steps.Add(new StartupStep(label, done, failed));
            SetSteps();
        }

        State = SystemState.Starting;
        StatusMessage = "Starting…";
        SetSteps();
        Log.Log(LogLevel.Info, "orchestrator", "Start requested.");
        _processes.ResetCrashCount("streamer");

        try
        {
            Step(_elevation.IsElevated ? "Running elevated" : "User mode (driver ops will prompt)", done: true);
            var driver = await _drivers.GetDriverInfoAsync(token).ConfigureAwait(false);
            Driver = driver;
            if (!driver.PackageInstalled) throw new FriendlyException("Driver is not installed.", "Open the Driver page and install it (administrator required).");
            Step("Driver detected", done: true);
            if (!driver.Loaded) throw new FriendlyException($"Driver not loaded (status={driver.Status}, problem={driver.ProblemCode}).", "Open the Driver page; try Restart Driver.");
            Step("Virtual display driver loaded", done: true);

            var monitor = _displays.GetUsbDisplay();
            if (monitor == null) throw new FriendlyException("Virtual monitor not enumerated.", "Restart the driver, then extend displays (DisplaySwitch.exe /extend).");
            Step($"Virtual display ready ({monitor.Width}×{monitor.Height})", done: true);

            if (!_streamer.Available) throw new FriendlyException($"Streamer not found: {_streamer.ExePath}.", "Build the Rust workspace or set the path in Settings → Advanced.");
            Step("Streamer binary found", done: true);

            var s = Settings;
            if (s.Transport == "wifi")
            {
                if (string.IsNullOrWhiteSpace(s.DeviceIp)) throw new FriendlyException("WiFi needs a device IP.", "Enter the tablet IP (or QR JSON) in Settings → Transport.");
                Step($"Tablet target: {s.DeviceIp} (WiFi TLS)", done: true);
            }
            else
            {
                var devs = await _adb.ListDevicesAsync(token).ConfigureAwait(false);
                Devices = devs;
                var dev = devs.FirstOrDefault(d => d.State == AdbDeviceState.Device);
                if (dev == null)
                {
                    var unauth = devs.FirstOrDefault();
                    throw new FriendlyException(
                        unauth == null ? "No Android device visible to adb." : $"Device {unauth.Serial} is {unauth.State}.",
                        unauth?.State == AdbDeviceState.Unauthorized
                            ? "Unlock the tablet and accept the USB debugging prompt."
                            : "Connect the tablet over USB with USB debugging enabled.");
                }
                Step($"Android device: {dev.Serial}", done: true);
            }

            _accumulator = new StreamTelemetryAccumulator();
            _accumulator.SetStatic(s.Codec.ToUpperInvariant(), "—", s.BitrateBps);
            var options = new StreamStartOptions(
                s.Transport == "wifi" ? TransportKind.Wifi : TransportKind.Usb,
                s.Codec, s.BitrateBps, s.Fps, s.Gop, s.UsbPort,
                null, string.IsNullOrWhiteSpace(s.DeviceIp) ? null : s.DeviceIp,
                null, Live: true, Loop: true);
            var session = _streamer.StartStream(options, OnStreamerOut, OnStreamerErr);
            _session = session;
            session.Exited += OnSessionExited;
            try
            {
                _processes.Track("streamer", GetProcess(session), session.CommandLine);
            }
            catch (Exception ex)
            {
                // Tracking is observability only; never fail a start over it.
                Log.Log(LogLevel.Debug, "processes", $"Process tracking unavailable: {ex.Message}");
            }
            Log.Log(LogLevel.Info, "streamer", $"Started pid {session.Pid}.");
            Step("Transport process started", done: true);

            // Health-verified ACTIVE: connection + increasing frame count.
            var deadline = DateTimeOffset.Now.Add(_firstFrameTimeout);
            long lastFrames = -1;
            var framesSeen = false;
            while (DateTimeOffset.Now < deadline)
            {
                token.ThrowIfCancellationRequested();
                if (_session == null || _session.HasExited)
                {
                    throw new FriendlyException("Streamer exited during startup.", "See Logs; run Diagnostics.");
                }
                var snap = _accumulator.Snapshot();
                if (_accumulator.Connected && snap.StreamedFrames > lastFrames && snap.StreamedFrames > 0)
                {
                    framesSeen = true;
                    break;
                }
                lastFrames = Math.Max(lastFrames, snap.StreamedFrames);
                await Task.Delay(500, token).ConfigureAwait(false);
            }
            if (!framesSeen)
            {
                throw new FriendlyException("No frames received within 45 s.", "Check capture dir, encoder backend in Logs, and tablet screen.");
            }
            Step("First frames received — USB DISPLAY ACTIVE", done: true);
            State = SystemState.Active;
            StatusMessage = "USB DISPLAY ACTIVE";
            Health = HealthState.Healthy;
            Log.Log(LogLevel.Info, "orchestrator", "System ACTIVE (verified).");
            _pollTimer.Start();
            _telemetryTimer.Start();
        }
        catch (OperationCanceledException)
        {
            await StopInternalAsync("Start cancelled.").ConfigureAwait(false);
        }
        catch (FriendlyException ex)
        {
            Fail(ex.Message, ex.Remediation);
        }
        catch (Exception ex)
        {
            Fail($"Start failed: {ex.Message}", "See Logs; run Diagnostics.");
        }
        NotifyChanged();
    }

    public async Task StopAsync()
    {
        if (State is SystemState.Stopped or SystemState.Stopping)
        {
            return;
        }
        State = SystemState.Stopping;
        StatusMessage = "Stopping…";
        NotifyChanged();
        Log.Log(LogLevel.Info, "orchestrator", "Stop requested (session only; driver stays installed).");
        await StopInternalAsync("Stopped.").ConfigureAwait(false);
        NotifyChanged();
    }

    public async Task RestartAsync(CancellationToken ct = default)
    {
        await StopAsync().ConfigureAwait(false);
        await StartAsync(ct).ConfigureAwait(false);
    }

    public Task RestartComponentAsync(string component)
    {
        if (!string.Equals(component, "streamer", StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException($"Restart is only supported for the streamer (asked: {component}).");
        }
        return RestartAsync();
    }

    public async Task<IReadOnlyList<DiagnosticResult>> RunDiagnosticsAsync(
        IProgress<DiagnosticResult>? progress = null, CancellationToken ct = default)
    {
        Log.Log(LogLevel.Info, "diagnostics", "Full diagnostic started.");
        var results = await _diagnostics.RunFullDiagnosticAsync(progress, ct).ConfigureAwait(false);
        LastDiagnostics = results;
        var fails = results.Count(r => r.Status == DiagnosticStatus.Fail);
        Log.Log(fails == 0 ? LogLevel.Info : LogLevel.Warning, "diagnostics",
            fails == 0 ? "Diagnostic: SYSTEM READY." : $"Diagnostic: {fails} check(s) failed.");
        NotifyChanged();
        return results;
    }

    public async Task DriverInstallAsync()
    {
        Log.Log(LogLevel.Info, "driver", "Install requested (elevated).");
        var r = await _drivers.InstallAsync().ConfigureAwait(false);
        Log.Log(r.ExitCode == 0 ? LogLevel.Info : LogLevel.Error, "driver", $"Install exit={r.ExitCode}. {(r.StdOut + r.StdErr).Trim()}");
        await RefreshAllAsync().ConfigureAwait(false);
    }

    public async Task DriverUninstallAsync()
    {
        Log.Log(LogLevel.Warning, "driver", "Uninstall requested (elevated).");
        var r = await _drivers.UninstallAsync().ConfigureAwait(false);
        Log.Log(r.ExitCode == 0 ? LogLevel.Info : LogLevel.Error, "driver", $"Uninstall exit={r.ExitCode}.");
        await RefreshAllAsync().ConfigureAwait(false);
    }

    public async Task DriverRestartAsync()
    {
        await _drivers.RestartAsync().ConfigureAwait(false);
        await RefreshAllAsync().ConfigureAwait(false);
    }

    public async Task DriverSetEnabledAsync(bool enable)
    {
        if (Driver?.InstanceId == null)
        {
            throw new InvalidOperationException("No USBDisplay device node found.");
        }
        await _drivers.SetEnabledAsync(Driver.InstanceId, enable).ConfigureAwait(false);
        Log.Log(LogLevel.Info, "driver", enable ? "Driver enabled." : "Driver disabled (virtual monitor removed until re-enabled).");
        await RefreshAllAsync().ConfigureAwait(false);
    }

    public void OpenDisplaySettings() => _displays.OpenDisplaySettings();
    public void ExtendDisplays() => _displays.ExtendDisplays();

    public async Task RunAdbMaintenanceAsync(string what)
    {
        // Direct ADB access lives behind the app's AdbClient so UI strings
        // never become shell commands.
        var adb = _adb as AdbClient;
        if (adb == null)
        {
            throw new InvalidOperationException("ADB maintenance is unavailable in this mode.");
        }
        if (what == "restart")
        {
            await adb.RestartServerAsync().ConfigureAwait(false);
        }
        else
        {
            await adb.ReconnectAsync().ConfigureAwait(false);
        }
        Log.Log(LogLevel.Info, "adb", $"Maintenance action '{what}' done.");
    }

    public void SaveSettings()
    {
        if (_config is ConfigurationService cs)
        {
            cs.Save();
            Log.Log(LogLevel.Info, "config", $"Settings saved to {cs.SettingsPath}.");
        }
    }

    private async Task StopInternalAsync(string message)
    {
        _pollTimer.Stop();
        _telemetryTimer.Stop();
        try
        {
            if (_session != null)
            {
                _session.Exited -= OnSessionExited;
                _session.Stop(TimeSpan.FromSeconds(5));
                _session.Dispose();
                _session = null;
            }
        }
        catch { }
        _processes.Untrack("streamer");
        _accumulator.MarkDisconnected();
        State = SystemState.Stopped;
        StatusMessage = message;
        EvaluateHealth();
        await RefreshAllAsync().ConfigureAwait(false);
    }

    private void Fail(string message, string? remediation)
    {
        State = SystemState.Error;
        StatusMessage = message;
        Health = HealthState.Error;
        Log.Log(LogLevel.Error, "orchestrator", remediation == null ? message : $"{message} {remediation}");
        try
        {
            if (_session != null)
            {
                _session.Exited -= OnSessionExited;
                _session.Stop(TimeSpan.FromSeconds(3));
                _session.Dispose();
                _session = null;
            }
        }
        catch { }
        _processes.Untrack("streamer");
    }

    private void OnStreamerOut(object? sender, System.Diagnostics.DataReceivedEventArgs e)
    {
        if (e.Data == null) return;
        Log.Log(LogLevel.Debug, "streamer", e.Data);
        try { _accumulator.FeedLine(e.Data); } catch { }
        RefreshTelemetrySnapshot();
    }

    private void OnStreamerErr(object? sender, System.Diagnostics.DataReceivedEventArgs e)
    {
        if (e.Data == null) return;
        Log.Log(LogLevel.Warning, "streamer:stderr", e.Data);
    }

    private void OnSessionExited(object? sender, int code)
    {
        // Unexpected exit while Active = crash. Bounded auto-restart.
        if (State != SystemState.Active)
        {
            return;
        }
        _processes.NoteCrash("streamer");
        var n = _processes.CrashCount("streamer");
        Log.Log(LogLevel.Error, "orchestrator", $"Streamer stopped unexpectedly (exit {code}, crash #{n}).");
        if (n <= ProcessManager.MaxAutoRestarts)
        {
            Log.Log(LogLevel.Info, "orchestrator", "Attempting bounded restart…");
            _ = Task.Run(async () =>
            {
                await Task.Delay(2000).ConfigureAwait(false);
                if (State == SystemState.Active) await StartAsync().ConfigureAwait(false);
            });
        }
        else
        {
            State = SystemState.Error;
            StatusMessage = $"Streamer crashed {n} times; auto-restart paused. See Logs.";
            Health = HealthState.Error;
            NotifyChanged();
        }
    }

    private async Task PollDevicesAsync()
    {
        try
        {
            Devices = await _adb.ListDevicesAsync().ConfigureAwait(false);
            Processes = _processes.Snapshot();
            EvaluateHealth();
            NotifyChanged();
        }
        catch { }
    }

    private void RefreshTelemetrySnapshot()
    {
        try
        {
            Telemetry = _accumulator.Snapshot();
            NotifyChanged();
        }
        catch { }
    }

    private void EvaluateHealth()
    {
        if (State == SystemState.Active)
        {
            var snap = _accumulator.Snapshot();
            Health = snap.StreamedFrames > 0 ? HealthState.Healthy : HealthState.Degraded;
            return;
        }
        if (State == SystemState.Error)
        {
            Health = HealthState.Error;
            return;
        }
        Health = HealthState.Unknown;
    }

    private void BuildServiceEntries()
    {
        var list = new List<ServiceEntry>();
        var streamer = Processes.FirstOrDefault(p => p.Component == "streamer");
        list.Add(new ServiceEntry("USBDisplay Streamer",
            "Rust streamer process (capture → encode → transport). Managed by this app; not a Windows service.",
            streamer != null ? "RUNNING" : "STOPPED",
            streamer?.Executable, streamer?.Pid,
            streamer != null ? DateTimeOffset.Now - streamer.StartTime : null, null));
        var svc = Driver?.Service;
        list.Add(new ServiceEntry("WUDFRd reflector binding",
            "UMDF reflector the IDD binds through (part of Windows).",
            Driver?.Loaded == true ? "BOUND" : "NOT BOUND",
            null, null, null,
            Driver?.Loaded == true ? null : "Driver not loaded."));
        list.Add(new ServiceEntry("ADB server",
            "Android Debug Bridge daemon used for device discovery and USB transport.",
            "AVAILABLE",
            null, null, null, null));
        _ = svc;
        ServiceEntries = list;
    }

    private static System.Diagnostics.Process GetProcess(IStreamSession session)
    {
        // IStreamSession wraps the Process; Track needs the live object for
        // CPU/memory. StreamerCli's session owns it — unwrap via reflection-free
        // path: re-resolve by pid.
        try { return System.Diagnostics.Process.GetProcessById(session.Pid); }
        catch { throw new InvalidOperationException("Streamer process already exited."); }
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        _pollTimer.Dispose();
        _telemetryTimer.Dispose();
        _startCts?.Cancel();
        _startCts?.Dispose();
    }
}

public sealed class FriendlyException : Exception
{
    public string? Remediation { get; }
    public FriendlyException(string message, string? remediation = null) : base(message)
    {
        Remediation = remediation;
    }
}
