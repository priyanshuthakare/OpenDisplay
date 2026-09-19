using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Services;

namespace USBDisplay.ControlApp.Transport;

/// <summary>
/// Wired transport: Android —ADB— adb forward 127.0.0.1:27183 — streamer.
/// Owns forward lifecycle: detect, verify, create, remove stale, reconnect.
/// UI never builds adb command lines; all args are fixed arrays.
/// </summary>
public sealed class UsbAdbTransport : IUsbDisplayTransport
{
    private readonly IAdbClient _adb;
    private readonly int _port;

    public UsbAdbTransport(IAdbClient adb, int port = TransportPolicy.UsbPort)
    {
        _adb = adb;
        _port = port;
    }

    public TransportKind Kind => TransportKind.Usb;
    public string DisplayName => "USB (ADB bridge)";
    public string EndpointDescription => $"127.0.0.1:{_port} via adb forward tcp:{_port}";

    public async Task<TransportStatus> ConnectAsync(CancellationToken ct = default)
    {
        if (!_adb.Available)
            return new TransportStatus(Kind, SubsystemState.Unavailable, EndpointDescription, "adb not found.", "Install platform-tools or set ADB path in Settings → Advanced.");
        var devices = await _adb.ListDevicesAsync(ct).ConfigureAwait(false);
        var dev = devices.FirstOrDefault(d => d.State == AdbDeviceState.Device);
        if (dev == null)
        {
            var first = devices.FirstOrDefault();
            var detail = first == null ? "No Android device visible to adb."
                : first.State == AdbDeviceState.Unauthorized ? $"Device {first.Serial} is unauthorized."
                : $"Device {first.Serial} is {first.State}.";
            var fix = first?.State == AdbDeviceState.Unauthorized
                ? "Unlock the tablet and accept the USB debugging prompt."
                : "Connect the tablet over USB with USB debugging enabled.";
            return new TransportStatus(Kind, SubsystemState.Disconnected, EndpointDescription, detail, fix);
        }
        var forward = await _adb.EnsureForwardAsync(_port, dev.Serial, ct).ConfigureAwait(false);
        if (!forward.Ok)
            return new TransportStatus(Kind, SubsystemState.Error, EndpointDescription, forward.Detail, forward.Remediation);
        return new TransportStatus(Kind, SubsystemState.Connecting, EndpointDescription, $"ADB {dev.Serial}; forward tcp:{_port} verified.", null);
    }

    public async Task DisconnectAsync(CancellationToken ct = default)
    {
        try { await _adb.RemoveForwardAsync(_port, null, ct).ConfigureAwait(false); }
        catch { }
    }

    public async Task<TransportStatus> ReconnectAsync(CancellationToken ct = default)
    {
        await DisconnectAsync(ct).ConfigureAwait(false);
        return await ConnectAsync(ct).ConfigureAwait(false);
    }

    public async Task<TransportStatus> GetStatusAsync(CancellationToken ct = default)
        => await ConnectAsync(ct).ConfigureAwait(false);

    public async Task<TransportDiagnostics> GetDiagnosticsAsync(CancellationToken ct = default)
    {
        var checks = new List<DiagnosticResult>();
        checks.Add(new DiagnosticResult("ADB present",
            _adb.Available ? Models.DiagnosticStatus.Pass : Models.DiagnosticStatus.Fail,
            _adb.Available ? _adb.ExePath : "adb not found.",
            _adb.Available ? null : "Install platform-tools or set the ADB path in Settings."));
        if (_adb.Available)
        {
            try
            {
                var devs = await _adb.ListDevicesAsync(ct).ConfigureAwait(false);
                var dev = devs.FirstOrDefault(d => d.State == AdbDeviceState.Device);
                checks.Add(new DiagnosticResult("Android device",
                    dev != null ? Models.DiagnosticStatus.Pass : Models.DiagnosticStatus.Fail,
                    dev != null ? $"{dev.Serial} authorized." : "No authorized device.",
                    dev != null ? null : "Connect via USB; accept the debugging prompt."));
                var forwards = await _adb.ListForwardsAsync(ct).ConfigureAwait(false);
                var want = $"tcp:{_port}";
                var has = forwards.Any(f => f.LocalSpec == want || f.LocalSpec.EndsWith($":{_port}"));
                checks.Add(new DiagnosticResult($"ADB forward {want}",
                    has ? Models.DiagnosticStatus.Pass : Models.DiagnosticStatus.Fail,
                    has ? string.Join("; ", forwards.Select(f => $"{f.Serial} {f.LocalSpec}->{f.RemoteSpec}")) : $"No forward for {want}.",
                    has ? null : $"The streamer creates adb forward tcp:{_port} on Start; stale forwards are removed on Stop."));
            }
            catch (Exception ex)
            {
                checks.Add(new DiagnosticResult("ADB query", Models.DiagnosticStatus.Fail, ex.Message, null));
            }
        }
        return new TransportDiagnostics(Kind, checks);
    }

    // Upper-layer frame flow goes through the streamer child process;
    // transports expose status/diagnostics only — they never reimplement
    // protocol, encode, or packet framing.
    public Task SendFrameAsync(byte[] frame, CancellationToken ct = default) => throw new NotSupportedException("Frames flow through the streamer process; transports manage the link only.");
    public Task<byte[]> ReceiveControlAsync(CancellationToken ct = default) => throw new NotSupportedException("Control flows through the streamer process; transports manage the link only.");
}
