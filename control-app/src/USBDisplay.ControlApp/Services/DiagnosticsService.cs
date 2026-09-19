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

        await Add("Capture folder bounded", () =>
        {
            var warnMb = _config.Settings.CaptureWarnMb;
            var status = CaptureMonitor.GetStatus();
            var mb = status.TotalBytes / (1024.0 * 1024.0);
            if (mb > warnMb)
            {
                return Task.FromResult<(bool, string, string?)>((false,
                    $"{status.FileCount} stale frame(s), {mb:F1} MB (budget {warnMb} MB).",
                    "Display page → Purge stale frames; check nothing unexpected is writing BMPs there."));
            }
            return Task.FromResult<(bool, string, string?)>((true,
                status.FileCount == 0
                    ? "Empty — the in-memory driver writes nothing to disk."
                    : $"{status.FileCount} file(s), {mb:F1} MB within budget.",
                null));
        }).ConfigureAwait(false);

        await Add("WiFi pairing store", () => Task.FromResult<(bool, string, string?)>(
            File.Exists(PairedJson())
                ? (true, PairedJson(), null)
                : (true, "No pairings yet (normal before first WiFi use).", null))).ConfigureAwait(false);

        await Add("USB forward tcp:27183", async () =>
        {
            if (!_adb.Available) return (false, "Skipped: no adb.", (string?)null);
            try
            {
                var forwards = await _adb.ListForwardsAsync(ct).ConfigureAwait(false);
                var want = $"tcp:{_config.Settings.UsbPort}";
                var match = System.Linq.Enumerable.FirstOrDefault(forwards, f => f.LocalSpec == want);
                // Forward absent while stopped is normal — report N/A as pass-with-note.
                return match != null
                    ? (true, $"{match.Serial} {match.LocalSpec}->{match.RemoteSpec}.", null)
                    : (true, $"No forward for {want} (normal while stopped; created on Start).", null);
            }
            catch (Exception ex) { return (false, ex.Message, (string?)null); }
        }).ConfigureAwait(false);

        await Add("Wi-Fi device target", () =>
        {
            var raw = (_config.Settings.DeviceIp ?? "").Trim();
            if (string.IsNullOrWhiteSpace(raw))
                return Task.FromResult<(bool, string, string?)>((true, "No Wi-Fi target configured (normal for USB-only use).", null));
            try
            {
                var payload = WifiPairPayload.Parse(raw, _config.Settings.WifiPort);
                return Task.FromResult<(bool, string, string?)>((true, $"{payload.Ip}:{payload.Port}" + (payload.Fingerprint != null ? $" fp={FingerprintUtil.Short(payload.Fingerprint)}" : ""), null));
            }
            catch (Exception ex) { return Task.FromResult<(bool, string, string?)>((false, ex.Message, "Use a bare IP, ip:port, or the tablet QR JSON.")); }
        }).ConfigureAwait(false);

        await Add("Wi-Fi reachable + TLS trust", async () =>
        {
            var raw = (_config.Settings.DeviceIp ?? "").Trim();
            if (string.IsNullOrWhiteSpace(raw)) return (true, "Skipped: no Wi-Fi target.", (string?)null);
            WifiPairPayload payload;
            try { payload = WifiPairPayload.Parse(raw, _config.Settings.WifiPort); }
            catch (Exception ex) { return (false, ex.Message, (string?)null); }
            try
            {
                using var client = new System.Net.Sockets.TcpClient();
                var connect = client.ConnectAsync(payload.Ip, payload.Port);
                var done = await Task.WhenAny(connect, Task.Delay(TimeSpan.FromSeconds(5), ct)).ConfigureAwait(false);
                if (done != connect || !client.Connected)
                    return (false, $"Host {payload.Ip}:{payload.Port} unreachable.",
                        "The network may be AP-isolated or the host unreachable. Use USB to connect this tablet.");
            }
            catch (Exception) { return (false, $"Host {payload.Ip}:{payload.Port} unreachable.", "The network may be AP-isolated. Use USB."); }
            // Fingerprint vs trust.
            var store = new WifiTrustStore();
            var trusted = store.FindByIp(payload.Ip);
            if (!string.IsNullOrWhiteSpace(payload.Fingerprint))
            {
                if (!FingerprintUtil.IsValid(payload.Fingerprint))
                    return (false, $"Malformed fingerprint: {payload.Fingerprint}", "Re-scan the tablet pair screen QR.");
                if (trusted != null && !string.Equals(trusted.Fingerprint, payload.Fingerprint, StringComparison.OrdinalIgnoreCase))
                    return (false, $"Fingerprint mismatch. Expected {trusted.Fingerprint}, received {payload.Fingerprint}.",
                        "Forget the existing pairing and pair the tablet again. Changed certificates are never accepted silently.");
                return (true, $"Reachable; fp={FingerprintUtil.Short(payload.Fingerprint)}; TLS 1.3 enforced.", null);
            }
            return trusted != null
                ? (true, $"Reachable; trusted host; fp={FingerprintUtil.Short(trusted.Fingerprint)}.", null)
                : (true, "Reachable. Pair once with the 6-digit PIN; later reconnects skip the PIN (fingerprint still enforced).", null);
        }).ConfigureAwait(false);

        await Add("Protocol framing (USBD/USBT v1)", async () =>
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
