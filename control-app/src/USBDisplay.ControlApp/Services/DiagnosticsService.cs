using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using USBDisplay.ControlApp.Models;

namespace USBDisplay.ControlApp.Services;

public interface IDiagnosticsService
{
    Task<IReadOnlyList<DiagnosticResult>> RunFullDiagnosticAsync(
        IProgress<DiagnosticResult>? progress = null, CancellationToken ct = default);
}

public sealed class DiagnosticsService : IDiagnosticsService
{
    private readonly IElevationService _elevation;
    private readonly IDriverManager _drivers;
    private readonly IStreamerCli _streamer;
    private readonly IAdbClient _adb;
    private readonly IDisplayManager _displays;
    private readonly IConfigurationService _config;

    public DiagnosticsService(
        IElevationService elevation, IDriverManager drivers, IStreamerCli streamer,
        IAdbClient adb, IDisplayManager displays, IConfigurationService config)
    {
        _elevation = elevation;
        _drivers = drivers;
        _streamer = streamer;
        _adb = adb;
        _displays = displays;
        _config = config;
    }

    public async Task<IReadOnlyList<DiagnosticResult>> RunFullDiagnosticAsync(
        IProgress<DiagnosticResult>? progress = null, CancellationToken ct = default)
    {
        var results = new List<DiagnosticResult>();
        async Task Add(string name, Func<Task<(bool, string, string?)>> check)
        {
            DiagnosticResult r;
            try
            {
                var (ok, detail, fix) = await check().ConfigureAwait(false);
                r = new DiagnosticResult(name, ok ? DiagnosticStatus.Pass : DiagnosticStatus.Fail, detail, fix);
            }
            catch (Exception ex)
            {
                r = new DiagnosticResult(name, DiagnosticStatus.Fail, ex.Message, null);
            }
            results.Add(r);
            progress?.Report(r);
        }

        await Add("Administrator privileges", () => Task.FromResult(
            _elevation.IsElevated
                ? (true, "Process is elevated.", null)
                : (true, "Running in user mode; driver operations will prompt for elevation.",
                    (string?)"Some actions will show a UAC prompt. This is expected."))).ConfigureAwait(false);

        await Add("Driver package installed", async () =>
        {
            var d = await _drivers.GetDriverInfoAsync(ct).ConfigureAwait(false);
            return d.PackageInstalled
                ? (true, $"Published as {d.PublishedName}.", null)
                : (false, "No USBDisplay package in the driver store.",
                    "Driver page → Install Driver (requires administrator).");
        }).ConfigureAwait(false);

        await Add("ROOT device exists", async () =>
        {
            var d = await _drivers.GetDriverInfoAsync(ct).ConfigureAwait(false);
            return d.DevicePresent
                ? (true, d.InstanceId ?? "", null)
                : (false, "ROOT\\USBDisplayIdd node absent.", "Install the driver first.");
        }).ConfigureAwait(false);

        await Add("Driver loaded (problem 0, WUDFRd)", async () =>
        {
            var d = await _drivers.GetDriverInfoAsync(ct).ConfigureAwait(false);
            return d.Loaded
                ? (true, $"Status={d.Status}, service={d.Service}.", null)
                : (false, $"Status={d.Status}, problem={d.ProblemCode}, service={d.Service ?? "<none>"}.",
                    "Open Driver page for the PnP problem-code meaning; try Restart Driver.");
        }).ConfigureAwait(false);

        await Add("Virtual monitor available", () => Task.FromResult<(bool, string, string?)>(
            _displays.GetUsbDisplay() is { } m
                ? (true, $"{m.Name} {m.Width}×{m.Height}@{m.RefreshHz}Hz.", null)
                : (false, "No USBDisplay adapter enumerated.",
                    "Restart the driver, then DisplaySwitch.exe /extend."))).ConfigureAwait(false);

        await Add("Streamer executable found", () => Task.FromResult(
            _streamer.Available
                ? (true, _streamer.ExePath, null)
                : (false, $"Not found: {_streamer.ExePath}.",
                    (string?)"Build the Rust workspace (cargo build) or set the path in Settings → Advanced."))).ConfigureAwait(false);

        await Add("Streamer capabilities", async () =>
        {
            if (!_streamer.Available) return (false, "Skipped: no streamer binary.", (string?)null);
            var caps = await _streamer.GetCapabilitiesAsync(ct).ConfigureAwait(false);
            caps.TryGetValue("transport", out var t);
            return (true, $"transport={t ?? "?"}.", null);
        }).ConfigureAwait(false);

        await Add("ADB installed", () => Task.FromResult(
            _adb.Available
                ? (true, _adb.ExePath, null)
                : (false, "adb not found.", (string?)"Install platform-tools or set the ADB path in Settings."))).ConfigureAwait(false);

        await Add("Android device detected", async () =>
        {
            if (!_adb.Available) return (false, "Skipped: no adb.", (string?)null);
            var devs = await _adb.ListDevicesAsync(ct).ConfigureAwait(false);
            var d = devs.FirstOrDefault();
            return d == null
                ? (false, "No device visible to adb.", "Connect the tablet over USB and enable USB debugging.")
                : (true, $"{d.Serial} [{d.State}].", null);
        }).ConfigureAwait(false);

        await Add("Android authorized", async () =>
        {
            if (!_adb.Available) return (false, "Skipped: no adb.", (string?)null);
            var devs = await _adb.ListDevicesAsync(ct).ConfigureAwait(false);
            var d = devs.FirstOrDefault();
            if (d == null) return (false, "No device.", (string?)null);
            return d.State == AdbDeviceState.Device
                ? (true, $"{d.Serial} authorized.", null)
                : (false, $"Device state: {d.State}.",
                    "Unlock the tablet and accept the USB debugging prompt, then Refresh.");
        }).ConfigureAwait(false);

        await Add("Capture directory", () => Task.FromResult(
            Directory.Exists(CaptureDir())
                ? (true, CaptureDir(), null)
                : (false, $"Missing: {CaptureDir()}.", (string?)"Start the driver; it creates the capture folder."))).ConfigureAwait(false);

        await Add("Encoder probe (synthetic frame)", async () =>
        {
            if (!_streamer.Available) return (false, "Skipped: no streamer binary.", (string?)null);
            try
            {
                var r = await _streamer.ProbeFrameAsync(ct).ConfigureAwait(false);
                return r.ExitCode == 0
                    ? (true, FirstLine(r.StdOut), null)
                    : (false, FirstLine(r.StdErr), "Run encode-capture manually for detail.");
            }
            catch (Exception ex) { return (false, ex.Message, (string?)null); }
        }).ConfigureAwait(false);

        await Add("Transport probe (synthetic packets)", async () =>
        {
            if (!_streamer.Available) return (false, "Skipped: no streamer binary.", (string?)null);
            try
            {
                var r = await _streamer.TransportProbeAsync(ct).ConfigureAwait(false);
                return r.ExitCode == 0
                    ? (true, FirstLine(r.StdOut), null)
                    : (false, FirstLine(r.StdErr), (string?)null);
            }
            catch (Exception ex) { return (false, ex.Message, (string?)null); }
        }).ConfigureAwait(false);

        await Add("WiFi pairing store", () => Task.FromResult<(bool, string, string?)>(
            File.Exists(PairedJson())
                ? (true, PairedJson(), null)
                : (true, "No pairings yet (normal before first WiFi use).", null))).ConfigureAwait(false);

        return results;
    }

    private static string CaptureDir()
    {
        var baseDir = Environment.GetEnvironmentVariable("ProgramData") ?? @"C:\ProgramData";
        return Path.Combine(baseDir, "USBDisplay", "capture");
    }

    private static string PairedJson()
    {
        return Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            "USBDisplay", "paired.json");
    }

    private static string FirstLine(string text)
    {
        foreach (var l in text.Split(new[] { '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries))
        {
            return l.Trim();
        }
        return "";
    }
}
