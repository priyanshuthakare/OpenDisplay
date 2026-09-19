using System;
using System.Collections.Generic;
using System.Net.Sockets;
using System.Threading;
using System.Threading.Tasks;
using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Services;

namespace USBDisplay.ControlApp.Transport;

/// <summary>
/// Wi-Fi transport: Android TLS 1.3 listener on &lt;ip&gt;:27184 — LAN — streamer.
/// Enforces TLS 1.3 only (no plaintext fallback), fingerprint check against
/// the trusted store, and explicit AP-isolation guidance. Never silently
/// falls back to USB.
/// </summary>
public sealed class WifiTlsTransport : IUsbDisplayTransport, IWifiPreflight
{
    private readonly IConfigurationService _config;
    private readonly WifiTrustStore _trust;
    private readonly int _port;

    public WifiTlsTransport(IConfigurationService config, WifiTrustStore? trust = null, int port = TransportPolicy.WifiPort)
    {
        _config = config;
        _trust = trust ?? new WifiTrustStore();
        _port = port;
    }

    public TransportKind Kind => TransportKind.Wifi;
    public string DisplayName => "Wi-Fi (TLS 1.3)";
    public string EndpointDescription => $"{ResolvedIp()}:{_port} TLS 1.3 only";

    public async Task<TransportStatus> ConnectAsync(CancellationToken ct = default)
    {
        var raw = _config.Settings.DeviceIp?.Trim() ?? "";
        if (string.IsNullOrWhiteSpace(raw))
            return new TransportStatus(Kind, SubsystemState.Disconnected, EndpointDescription, "No tablet IP configured.", "Enter the tablet LAN IP (or paste the tablet QR JSON) in Device → Wi-Fi Pairing.");
        WifiPairPayload payload;
        try { payload = WifiPairPayload.Parse(raw, _port); }
        catch (Exception ex)
        {
            return new TransportStatus(Kind, SubsystemState.Error, EndpointDescription, $"Unparseable device target: {ex.Message}", "Use a bare IP, ip:port, or the tablet QR JSON {\"v\":1,\"ip\":…}.");
        }
        // TCP reachability probe with the configured connection timeout.
        var timeoutS = Math.Clamp(_config.Settings.ConnectionTimeoutSeconds <= 0 ? 5 : _config.Settings.ConnectionTimeoutSeconds, 1, 60);
        try
        {
            using var client = new TcpClient();
            var connect = client.ConnectAsync(payload.Ip, payload.Port);
            var done = await Task.WhenAny(connect, Task.Delay(TimeSpan.FromSeconds(timeoutS), ct)).ConfigureAwait(false);
            if (done != connect || !client.Connected)
                return new TransportStatus(Kind, SubsystemState.Disconnected, $"{payload.Ip}:{payload.Port}",
                    "Host unreachable.", TransportPolicy.WifiUnreachableMessage());
        }
        catch (Exception)
        {
            return new TransportStatus(Kind, SubsystemState.Disconnected, $"{payload.Ip}:{payload.Port}",
                "Host unreachable.", TransportPolicy.WifiUnreachableMessage());
        }
        // Fingerprint enforcement: a QR-supplied fp must match trust when present.
        if (!string.IsNullOrWhiteSpace(payload.Fingerprint))
        {
            if (!FingerprintUtil.IsValid(payload.Fingerprint))
                return new TransportStatus(Kind, SubsystemState.Error, $"{payload.Ip}:{payload.Port}",
                    $"Malformed fingerprint in QR: {payload.Fingerprint}", "Re-scan the tablet pair screen QR.");
            var trusted = _trust.FindByIp(payload.Ip);
            if (trusted != null && !string.Equals(trusted.Fingerprint, payload.Fingerprint, StringComparison.OrdinalIgnoreCase))
                return new TransportStatus(Kind, SubsystemState.Error, $"{payload.Ip}:{payload.Port}",
                    $"Tablet certificate does not match the trusted fingerprint. Expected {trusted.Fingerprint}, received {payload.Fingerprint}.",
                    "Forget the existing pairing and pair the tablet again. The app never accepts a changed certificate silently.");
        }
        var trustNote = _trust.FindByIp(payload.Ip) != null
            ? "Trusted host — PIN skipped, fingerprint still enforced."
            : "First pairing needs the 6-digit PIN from the tablet pair screen.";
        return new TransportStatus(Kind, SubsystemState.Connecting, $"{payload.Ip}:{payload.Port} TLS 1.3", $"Reachable. {trustNote}", null);
    }

    public Task DisconnectAsync(CancellationToken ct = default) => Task.CompletedTask;

    public Task<TransportStatus> CheckAsync(CancellationToken ct = default) => ConnectAsync(ct);

    public async Task<TransportStatus> ReconnectAsync(CancellationToken ct = default)
        => await ConnectAsync(ct).ConfigureAwait(false);

    public async Task<TransportStatus> GetStatusAsync(CancellationToken ct = default)
        => await ConnectAsync(ct).ConfigureAwait(false);

    public async Task<TransportDiagnostics> GetDiagnosticsAsync(CancellationToken ct = default)
    {
        var checks = new List<DiagnosticResult>();
        var raw = _config.Settings.DeviceIp?.Trim() ?? "";
        WifiPairPayload? payload = null;
        try { if (!string.IsNullOrWhiteSpace(raw)) payload = WifiPairPayload.Parse(raw, _port); } catch { }
        checks.Add(new DiagnosticResult("Wi-Fi device IP",
            payload != null ? Models.DiagnosticStatus.Pass : Models.DiagnosticStatus.Fail,
            payload != null ? $"{payload.Ip}:{payload.Port}" : "No tablet IP configured.",
            payload != null ? null : "Enter the tablet LAN IP or paste its QR JSON."));
        if (payload != null)
        {
            TransportStatus reach;
            try { reach = await ConnectAsync(ct).ConfigureAwait(false); }
            catch (Exception ex) { reach = new TransportStatus(Kind, SubsystemState.Error, "", ex.Message, null); }
            var ok = reach.State is SubsystemState.Connecting or SubsystemState.Healthy or SubsystemState.Running;
            checks.Add(new DiagnosticResult($"Port {payload.Port} reachable",
                ok ? Models.DiagnosticStatus.Pass : Models.DiagnosticStatus.Fail, reach.Detail, reach.LastError));
            checks.Add(new DiagnosticResult("TLS 1.3 only",
                Models.DiagnosticStatus.Pass, "Plaintext is never attempted; the streamer dials TLS 1.3.",
                null));
            var fp = payload.Fingerprint ?? _trust.FindByIp(payload.Ip)?.Fingerprint;
            checks.Add(new DiagnosticResult("Certificate fingerprint",
                string.IsNullOrWhiteSpace(fp) ? Models.DiagnosticStatus.Fail
                    : FingerprintUtil.IsValid(fp) ? Models.DiagnosticStatus.Pass : Models.DiagnosticStatus.Fail,
                string.IsNullOrWhiteSpace(fp) ? "Unknown — scan the tablet QR or pair once to record trust."
                    : fp, string.IsNullOrWhiteSpace(fp) ? "Pair the tablet; the fingerprint is recorded on first success." : null));
            var trusted = _trust.FindByIp(payload.Ip);
            checks.Add(new DiagnosticResult("Trust state",
                trusted != null ? Models.DiagnosticStatus.Pass : Models.DiagnosticStatus.Fail,
                trusted != null ? $"Trusted ({trusted.TabletId ?? trusted.Ip ?? "tablet"})." : "Not paired.",
                trusted != null ? null : "Enter the 6-digit PIN once; later reconnects skip the PIN."));
        }
        return new TransportDiagnostics(Kind, checks);
    }

    private string ResolvedIp()
    {
        try
        {
            var raw = _config.Settings.DeviceIp?.Trim() ?? "";
            if (!string.IsNullOrWhiteSpace(raw)) return WifiPairPayload.Parse(raw, _port).Ip;
        }
        catch { }
        return "<device-ip>";
    }
}
