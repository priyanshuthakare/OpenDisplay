using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace USBDisplay.ControlApp.Services;

/// <summary>
/// Tablet QR / device-ip input. Accepts bare IP, ip:port, or the tablet
/// QR JSON {"v":1,"ip":"…","port":27184,"fp":"SHA256:…"} per protocol.
/// Never invents a format: this is the only accepted shape.
/// </summary>
public sealed record WifiPairPayload(string Ip, int Port, string? Fingerprint)
{
    public static WifiPairPayload Parse(string raw, int defaultPort = 27184)
    {
        raw = (raw ?? "").Trim();
        if (string.IsNullOrWhiteSpace(raw)) throw new ArgumentException("Empty device target.");
        if (raw.StartsWith("{", StringComparison.Ordinal))
        {
            using var doc = JsonDocument.Parse(raw);
            var root = doc.RootElement;
            if (!root.TryGetProperty("ip", out var ipEl)) throw new ArgumentException("QR JSON missing \"ip\".");
            var ip = ipEl.GetString()?.Trim() ?? "";
            var port = defaultPort;
            if (root.TryGetProperty("port", out var portEl) && portEl.TryGetInt32(out var p)) port = p;
            string? fp = null;
            if (root.TryGetProperty("fp", out var fpEl)) fp = fpEl.GetString();
            ValidateIp(ip);
            return new WifiPairPayload(ip, port, string.IsNullOrWhiteSpace(fp) ? null : fp.Trim());
        }
        // ip:port or bare ip.
        var host = raw;
        var port2 = defaultPort;
        var colon = raw.LastIndexOf(':');
        if (colon > 0 && raw.IndexOf(':') == colon) // single colon = ipv4:port
        {
            host = raw.Substring(0, colon).Trim();
            if (!int.TryParse(raw.Substring(colon + 1).Trim(), out port2)) throw new ArgumentException($"Bad port in \"{raw}\".");
        }
        ValidateIp(host);
        if (port2 is < 1 or > 65535) throw new ArgumentException($"Port out of range: {port2}.");
        return new WifiPairPayload(host, port2, null);
    }

    private static void ValidateIp(string ip)
    {
        if (string.IsNullOrWhiteSpace(ip)) throw new ArgumentException("Empty IP.");
        // Accept IPv4 dotted quad or hostname; reject shell metacharacters.
        if (ip.IndexOfAny(new[] { ' ', ';', '&', '|', '$', '`', '"', '\'', '\n', '\r' }) >= 0)
            throw new ArgumentException($"Illegal characters in IP: {ip}");
        if (Regex.IsMatch(ip, @"^\d+\.\d+\.\d+\.\d+$"))
        {
            var parts = ip.Split('.');
            if (parts.Any(p => !byte.TryParse(p, out _))) throw new ArgumentException($"Bad IPv4 address: {ip}");
            return;
        }
        if (!Regex.IsMatch(ip, @"^[A-Za-z0-9_.\-]+$")) throw new ArgumentException($"Bad host: {ip}");
    }
}

/// <summary>Fingerprint helpers. Format: SHA256:&lt;64 lower hex&gt; (colons tolerated on display).</summary>
public static class FingerprintUtil
{
    public static bool IsValid(string? fp)
    {
        if (string.IsNullOrWhiteSpace(fp)) return false;
        var s = fp.Trim();
        if (s.StartsWith("SHA256:", StringComparison.OrdinalIgnoreCase)) s = s.Substring(7);
        s = s.Replace(":", "").ToLowerInvariant();
        return s.Length == 64 && s.All(c => (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'));
    }

    public static string Normalize(string fp)
    {
        var s = fp.Trim();
        if (s.StartsWith("SHA256:", StringComparison.OrdinalIgnoreCase)) s = s.Substring(7);
        return "SHA256:" + s.Replace(":", "").ToLowerInvariant();
    }

    public static string Short(string? fp)
    {
        if (string.IsNullOrWhiteSpace(fp)) return "—";
        var n = Normalize(fp);
        return n.Length > 16 ? n.Substring(0, 16) + "…" : n;
    }

    /// <summary>Constant-time comparison for PINs (never early-exit on content).</summary>
    public static bool ConstantTimeEquals(string a, string b)
    {
        var ab = System.Text.Encoding.UTF8.GetBytes(a ?? "");
        var bb = System.Text.Encoding.UTF8.GetBytes(b ?? "");
        if (ab.Length != bb.Length) return false;
        var diff = 0;
        for (var i = 0; i < ab.Length; i++) diff |= ab[i] ^ bb[i];
        return diff == 0;
    }

    public static bool IsValidPin(string? pin) =>
        !string.IsNullOrWhiteSpace(pin) && Regex.IsMatch(pin.Trim(), @"^\d{6}$");
}

/// <summary>
/// PIN attempt tracking surfaced in UI. The tablet enforces 3 strikes +
/// 30 s lockout; this tracker mirrors failures observed locally so the UI
/// can show "Attempts remaining: N" and the lockout countdown honestly.
/// </summary>
public sealed class PinAttemptTracker
{
    public const int MaxAttempts = 3;
    public static readonly TimeSpan LockoutDuration = TimeSpan.FromSeconds(30);

    private int _failures;
    private DateTimeOffset _lockedUntil = DateTimeOffset.MinValue;

    public int AttemptsRemaining => Math.Max(0, MaxAttempts - _failures);
    public bool IsLockedOut => DateTimeOffset.Now < _lockedUntil;
    public TimeSpan LockoutRemaining => IsLockedOut ? _lockedUntil - DateTimeOffset.Now : TimeSpan.Zero;

    public void NoteFailure()
    {
        _failures++;
        if (_failures >= MaxAttempts)
        {
            _lockedUntil = DateTimeOffset.Now.Add(LockoutDuration);
            _failures = 0; // reset after lockout triggers
        }
    }

    public void NoteSuccess()
    {
        _failures = 0;
        _lockedUntil = DateTimeOffset.MinValue;
    }

    public string Display() => IsLockedOut
        ? $"Pairing temporarily locked. Try again in {(int)Math.Ceiling(LockoutRemaining.TotalSeconds)} seconds."
        : $"Attempts remaining: {AttemptsRemaining}";
}

/// <summary>
/// Local view of tablet trust. The Rust streamer owns
/// %AppData%\USBDisplay\paired.json (TOFU); this store reads it and
/// exposes lookup for fingerprint enforcement. Forget is an explicit
/// user action that removes the file so the next connect re-pairs.
/// </summary>
public sealed class WifiTrustStore
{
    public sealed record TrustedTablet(string? TabletId, string? Ip, string? Fingerprint, string? PairedAt);

    private readonly string _path;

    public WifiTrustStore(string? path = null)
    {
        _path = path ?? System.IO.Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            "USBDisplay", "paired.json");
    }

    public string StorePath => _path;

    public TrustedTablet? FindByIp(string ip)
    {
        foreach (var t in List()) 
            if (string.Equals(t.Ip, ip, StringComparison.OrdinalIgnoreCase)) return t;
        return null;
    }

    public IReadOnlyList<TrustedTablet> List()
    {
        try
        {
            if (!File.Exists(_path)) return Array.Empty<TrustedTablet>();
            var json = File.ReadAllText(_path);
            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            // Support both single-object and array shapes.
            var list = new List<TrustedTablet>();
            if (root.ValueKind == JsonValueKind.Array)
            {
                foreach (var el in root.EnumerateArray()) list.Add(ReadOne(el));
            }
            else list.Add(ReadOne(root));
            return list;
        }
        catch { return Array.Empty<TrustedTablet>(); }
    }

    public bool HasTrust => List().Count > 0;

    /// <summary>Explicit forget: removes local trust so next connect re-pairs. Requires user confirmation in UI.</summary>
    public void Forget()
    {
        try { if (File.Exists(_path)) File.Delete(_path); } catch { }
    }

    private static TrustedTablet ReadOne(JsonElement el)
    {
        string? Get(string name) => el.ValueKind == JsonValueKind.Object && el.TryGetProperty(name, out var v) ? v.GetString() : null;
        return new TrustedTablet(Get("tablet_id"), Get("ip"), Get("cert_fingerprint") ?? Get("fingerprint"), Get("paired_at"));
    }
}
