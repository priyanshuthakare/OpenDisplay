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
    WifiLinkInfo? WifiLink { get; }
    ErrorReport? LastError { get; }
    DriverInfo? Driver { get; }
    IReadOnlyList<DisplayInfo> Displays { get; }
    IReadOnlyList<DeviceInfo> Devices { get; }
    IReadOnlyList<ManagedProcessInfo> Processes { get; }
    IReadOnlyList<ServiceEntry> ServiceEntries { get; }
    IReadOnlyList<DiagnosticResult> LastDiagnostics { get; }
    Dictionary<string, string> Capabilities { get; }
    bool IsElevated { get; }

    event EventHandler? Changed;

    UsbDisplayState GetStateSnapshot();
    Task SwitchTransportAsync(TransportKind kind);
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
    private readonly Transport.IWifiPreflight _preflight;
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
        TimeSpan? firstFrameTimeout = null,
        Transport.IWifiPreflight? preflight = null)
    {
        _config = config;
        _elevation = elevation;
        _drivers = drivers;
        _streamer = streamer;
        _adb = adb;
        _displays = displays;
        _processes = processes;
        _diagnostics = diagnostics;
        _preflight = preflight ?? new Transport.WifiTlsTransport(config);
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
    public WifiLinkInfo? WifiLink { get; private set; }
    public ErrorReport? LastError { get; private set; }
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

    public TransportKind SelectedTransport =>
        string.Equals(Settings.Transport, "wifi", StringComparison.OrdinalIgnoreCase) ? TransportKind.Wifi : TransportKind.Usb;

    /// <summary>Explicit transport selection. Never automatic — UI calls this from a user gesture.</summary>
    public Task SwitchTransportAsync(TransportKind kind)
    {
        Settings.Transport = kind == TransportKind.Wifi ? "wifi" : "usb";
        SaveSettings();
        Log.Log(LogLevel.Info, "transport", $"Transport explicitly switched to {Settings.Transport} by user. No auto-fallback is ever performed.");
        NotifyChanged();
        return Task.CompletedTask;
    }

    public UsbDisplayState GetStateSnapshot()
    {
        var active = State == SystemState.Active;
        var connected = _accumulator.Connected && Telemetry.StreamedFrames > 0;
        SubsystemState Map(bool good, bool running)
        {
            if (State == SystemState.Error) return SubsystemState.Error;
            if (active) return good ? SubsystemState.Healthy : SubsystemState.Warning;
            if (State == SystemState.Starting) return SubsystemState.Starting;
            if (State == SystemState.Stopping) return SubsystemState.Stopping;
            if (running) return SubsystemState.Running;
            return good ? SubsystemState.Ready : SubsystemState.Unknown;
        }
        var transport = SelectedTransport;
        return new UsbDisplayState(
            State,
            Driver == null ? SubsystemState.Unknown : Driver.Loaded ? SubsystemState.Healthy : Driver.PackageInstalled ? SubsystemState.Warning : SubsystemState.NotInstalled,
            Driver?.MonitorPresent == true ? SubsystemState.Healthy : SubsystemState.Unavailable,
            active && connected ? SubsystemState.Healthy : active ? SubsystemState.Warning : SubsystemState.Ready,
            active && connected ? SubsystemState.Healthy : active ? SubsystemState.Warning : SubsystemState.Ready,
            active ? (connected ? SubsystemState.Healthy : SubsystemState.Warning) : Map(transport == TransportKind.Usb ? Devices.Any(d => d.State == AdbDeviceState.Device) : !string.IsNullOrWhiteSpace(Settings.DeviceIp), false),
            transport == TransportKind.Usb
                ? Map(Devices.Any(d => d.State == AdbDeviceState.Device), false)
                : Map(!string.IsNullOrWhiteSpace(Settings.DeviceIp), false),
            active ? (connected ? SubsystemState.Healthy : SubsystemState.Warning) : SubsystemState.Ready,
            Telemetry.InputEventsInjected > 0 ? SubsystemState.Healthy : SubsystemState.Ready,
            transport,
            new[] { TransportKind.Usb, TransportKind.Wifi },
            Telemetry, WifiLink, LastError, LastDiagnostics, DateTimeOffset.Now);
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
            string? usbSerial = null;
            if (s.Transport == "wifi")
            {
                if (string.IsNullOrWhiteSpace(s.DeviceIp)) throw new FriendlyException("Tablet connection failed.", "Wi-Fi needs a device IP.", "transport=wifi device_ip=<empty>", "Enter the tablet LAN IP (or QR JSON) in Device → Wi-Fi Pairing.");
                WifiPairPayload payload;
                try { payload = WifiPairPayload.Parse(s.DeviceIp.Trim(), s.WifiPort > 0 ? s.WifiPort : 27184); }
                catch (Exception ex) { throw new FriendlyException("Tablet connection failed.", $"Unparseable Wi-Fi target: {ex.Message}", $"transport=wifi device_ip={s.DeviceIp}", "Use a bare IP, ip:port, or the tablet QR JSON."); }
                if (!string.IsNullOrWhiteSpace(payload.Fingerprint) && !FingerprintUtil.IsValid(payload.Fingerprint))
                    throw new FriendlyException("Tablet connection failed.", $"Malformed fingerprint in QR: {payload.Fingerprint}", $"wifi_fingerprint={payload.Fingerprint}", "Re-scan the tablet pair screen QR.");
                if (!string.IsNullOrWhiteSpace(s.Pin) && !FingerprintUtil.IsValidPin(s.Pin))
                    throw new FriendlyException("Pairing PIN invalid.", "The PIN is six digits.", $"pin_len={s.Pin.Trim().Length}", "Re-read the current PIN on the tablet Wi-Fi pair screen.");
                // Pre-flight reachability (TCP). TLS handshake + PIN/trust happen in the streamer.
                var pre = await _preflight.CheckAsync(token).ConfigureAwait(false);
                if (pre.State == SubsystemState.Disconnected)
                    throw new FriendlyException("Tablet connection failed.", $"The Android listener did not establish a connection: {pre.Detail}", $"transport=wifi peer={payload.Ip}:{payload.Port}", $"{pre.LastError} [ Retry Wi-Fi ] [ Switch to USB ] — the app never switches automatically.");
                if (pre.State == SubsystemState.Error)
                    throw new FriendlyException("Connection refused.", pre.Detail, $"transport=wifi peer={payload.Ip}:{payload.Port}", $"{pre.LastError} [ View Details ] [ Forget Trust ] — changed certificates are never accepted silently.");
                Step($"Tablet target: {payload.Ip}:{payload.Port} (Wi-Fi TLS 1.3) — {pre.Detail}", done: true);
                Step("Certificate/PIN pre-check passed (TLS handshake runs in streamer)", done: true);
            }
            else
            {
                var devs = await _adb.ListDevicesAsync(token).ConfigureAwait(false);
                Devices = devs;
                var dev = devs.FirstOrDefault(d => d.State == AdbDeviceState.Device);
                if (dev == null)
                {
                    var unauth = devs.FirstOrDefault();
                    var detail = unauth == null ? "adb_serial=<none>" : $"adb_serial={unauth.Serial} adb_state={unauth.State}";
                    throw new FriendlyException(
                        "Tablet connection failed.",
                        unauth == null ? "No Android device visible to adb." : $"Device {unauth.Serial} is {unauth.State}.",
                        $"transport=usb {detail} adb_forward=tcp:{s.UsbPort}",
                        unauth?.State == AdbDeviceState.Unauthorized
                            ? "Unlock the tablet and accept the USB debugging prompt. [ Retry USB ] [ Switch to Wi-Fi ]"
                            : "Connect the tablet over USB with USB debugging enabled. A paired Wi-Fi tablet can be used via [ Switch to Wi-Fi ].");
                }
                usbSerial = dev.Serial;
                Step($"Android device: {dev.Serial}", done: true);
                // ADB forward lifecycle: purge stale, create/verify ours.
                try { await _adb.RemoveStaleForwardsAsync(s.UsbPort, token).ConfigureAwait(false); } catch { }
                var fwd = await _adb.EnsureForwardAsync(s.UsbPort, dev.Serial, token).ConfigureAwait(false);
                if (!fwd.Ok)
                    throw new FriendlyException("Tablet connection failed.", $"ADB forward failed: {fwd.Detail}", $"transport=usb adb_serial={dev.Serial} adb_forward=tcp:{s.UsbPort} FAILED", $"{fwd.Remediation} [ Retry USB ] [ Switch to Wi-Fi ]");
                Step($"ADB forward verified ({fwd.Detail})", done: true);
            }

            // Legacy disk handoff hygiene: drop stale BMPs predating this
            // session so an aborted run can never flood storage. Age-gated —
            // never touches frames the streamer could still consume.
            if (s.AutoPurgeCapture)
            {
                try
                {
                    var purged = CaptureMonitor.PurgeOlderThan(
                        TimeSpan.FromSeconds(Math.Max(60, s.CapturePurgeAgeSeconds)));
                    if (purged.DeletedFiles > 0)
                    {
                        Log.Log(LogLevel.Info, "capture",
                            $"Purged {purged.DeletedFiles} stale frame(s) ({purged.DeletedBytes / 1024} KB).");
                    }
                }
                catch (Exception ex)
                {
                    Log.Log(LogLevel.Debug, "capture", $"Pre-start purge skipped: {ex.Message}");
                }
            }

            _accumulator = new StreamTelemetryAccumulator();
            _accumulator.SetStatic(s.Codec.ToUpperInvariant(), "—", s.BitrateBps);
            var options = new StreamStartOptions(
                s.Transport == "wifi" ? TransportKind.Wifi : TransportKind.Usb,
                s.Codec, s.BitrateBps, s.Fps, s.Gop, s.UsbPort,
                null, string.IsNullOrWhiteSpace(s.DeviceIp) ? null : s.DeviceIp,
                string.IsNullOrWhiteSpace(s.Pin) ? null : s.Pin,
                Live: true, Loop: true);
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
            // ACTIVE is never set from process liveness alone.
            var deadline = DateTimeOffset.Now.Add(_firstFrameTimeout);
            long lastFrames = -1;
            var framesSeen = false;
            while (DateTimeOffset.Now < deadline)
            {
                token.ThrowIfCancellationRequested();
                if (_session == null || _session.HasExited)
                {
                    throw new FriendlyException("Streamer stopped unexpectedly.",
                        "The transport process exited before the first verified frame.",
                        $"transport={s.Transport} streamer_pid={_session?.Pid ?? -1}",
                        "See Logs; run Diagnostics. [ Retry ] [ View Logs ] [ Run Diagnostics ]");
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
                var tech = s.Transport == "wifi"
                    ? $"transport=wifi peer={s.DeviceIp} android_connection=not_established streamed_frames={lastFrames}"
                    : $"transport=usb adb_serial={usbSerial ?? "<none>"} adb_forward=tcp:{s.UsbPort} android_connection=not_established streamed_frames={lastFrames}";
                var remedy = s.Transport == "wifi"
                    ? "Check the tablet screen, firewall for port 27184, and encoder backend in Logs. [ Retry Wi-Fi ] [ Switch to USB ] [ Run Diagnostics ]"
                    : "Check capture dir, encoder backend in Logs, and tablet screen. [ Retry USB ] [ Switch to Wi-Fi ] [ Run Diagnostics ]";
                throw new FriendlyException("Tablet connection failed.",
                    "The Android listener did not establish a connection within the configured timeout (max 45 s first frame).",
                    tech, remedy);
            }
            Step("First frames received — USB DISPLAY ACTIVE", done: true);
            State = SystemState.Active;
            StatusMessage = "USB DISPLAY ACTIVE";
            Health = HealthState.Healthy;
            LastError = null;
            Log.Log(LogLevel.Info, "orchestrator", "System ACTIVE (verified connection + increasing frames).");
            _pollTimer.Start();
            _telemetryTimer.Start();
        }
        catch (OperationCanceledException)
        {
            await StopInternalAsync("Start cancelled.").ConfigureAwait(false);
        }
        catch (FriendlyException ex)
        {
            Fail(ex.Message, ex.Explanation, ex.TechnicalDetail, ex.Remediation);
        }
        catch (Exception ex)
        {
            Fail("Start failed.", ex.Message, $"transport={Settings.Transport} detail={ex.GetType().Name}", "See Logs; run Diagnostics. [ Retry ] [ View Logs ]");
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
        var wasUsb = SelectedTransport == TransportKind.Usb;
        var usbPort = Settings.UsbPort;
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
        // Session stop removes our ADB forward; the driver stays installed.
        if (wasUsb)
        {
            try
            {
                await _adb.RemoveForwardAsync(usbPort).ConfigureAwait(false);
                Log.Log(LogLevel.Info, "transport", $"ADB forward tcp:{usbPort} removed (session stop; driver untouched).");
            }
            catch (Exception ex) { Log.Log(LogLevel.Debug, "transport", $"Forward cleanup skipped: {ex.Message}"); }
        }
        WifiLink = null;
        State = SystemState.Stopped;
        StatusMessage = message;
        EvaluateHealth();
        await RefreshAllAsync().ConfigureAwait(false);
    }

    private void Fail(string userMessage, string? explanation, string? technicalDetail = null, string? remediation = null)
    {
        State = SystemState.Error;
        StatusMessage = userMessage;
        Health = HealthState.Error;
        LastError = new ErrorReport(userMessage, explanation ?? userMessage,
            technicalDetail ?? $"transport={Settings.Transport} state=Error", remediation, DateTimeOffset.Now);
        Log.Log(LogLevel.Error, "orchestrator",
            remediation == null ? $"{userMessage} {explanation}" : $"{userMessage} {explanation} {remediation} [{technicalDetail}]");
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
            LastError = new ErrorReport("Streamer failed repeatedly.",
                "Automatic restart paused after 3 bounded attempts.",
                $"transport={Settings.Transport} streamer_crashes={n}",
                "[ Retry ] [ View Logs ] [ Run Diagnostics ]", DateTimeOffset.Now);
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
            WifiLink = SelectedTransport == TransportKind.Wifi ? BuildWifiLink() : null;
            NotifyChanged();
        }
        catch { }
    }

    private WifiLinkInfo BuildWifiLink()
    {
        var store = new WifiTrustStore();
        string? ip = null;
        try
        {
            var raw = (Settings.DeviceIp ?? "").Trim();
            if (!string.IsNullOrWhiteSpace(raw)) ip = WifiPairPayload.Parse(raw, Settings.WifiPort > 0 ? Settings.WifiPort : 27184).Ip;
        }
        catch { }
        var trusted = ip != null ? store.FindByIp(ip) : null;
        var fp = !string.IsNullOrWhiteSpace(_accumulator.WifiFingerprint) ? _accumulator.WifiFingerprint
            : trusted?.Fingerprint;
        return new WifiLinkInfo(
            string.IsNullOrWhiteSpace(_accumulator.WifiPeer) ? (ip != null ? $"{ip}:{Settings.WifiPort}" : null) : _accumulator.WifiPeer,
            string.IsNullOrWhiteSpace(_accumulator.WifiEncryption) ? "TLS 1.3" : _accumulator.WifiEncryption,
            fp, trusted != null, HostIdentity.StableHostId(),
            false, Telemetry.BitrateBps, _accumulator.Adaptation, null, Telemetry.Reconnects);
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
    public string? Explanation { get; }
    public string? TechnicalDetail { get; }
    public string? Remediation { get; }
    public FriendlyException(string message, string? remediation = null) : base(message)
    {
        Explanation = remediation;
        Remediation = remediation;
    }
    public FriendlyException(string userMessage, string explanation, string technicalDetail, string? remediation = null)
        : base(userMessage)
    {
        Explanation = explanation;
        TechnicalDetail = technicalDetail;
        Remediation = remediation;
    }
}
