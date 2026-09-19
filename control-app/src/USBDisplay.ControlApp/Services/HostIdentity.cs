using System;
using System.Linq;

namespace USBDisplay.ControlApp.Services;

/// <summary>
/// Stable PC identity for pairing. MUST match the Rust streamer's
/// <c>pairing::stable_host_id()</c> exactly: the tablet trusts this id when
/// it scans the PC pairing code, and the streamer sends the same id in its
/// Hello. If either side changes the scheme, scan-to-trust silently stops
/// matching — keep the two implementations in lockstep.
/// </summary>
public static class HostIdentity
{
    public const string Prefix = "host-";

    public static string StableHostId()
    {
        var computer = Sanitize(Environment.GetEnvironmentVariable("COMPUTERNAME"));
        if (computer.Length > 0)
        {
            return Prefix + computer;
        }
        var user = Sanitize(Environment.GetEnvironmentVariable("USERNAME"));
        if (user.Length > 0)
        {
            return Prefix + user;
        }
        return Prefix + Environment.ProcessId;
    }

    /// <summary>
    /// QR payload the tablet scans: <c>{"v":1,"host_id":"host-…"}</c>.
    /// Scanned via the tablet pair screen, which trusts the id (physical
    /// proximity is the authorization) so the PC can then connect PIN-free.
    /// </summary>
    public static string BuildPairingQrPayload(string hostId)
    {
        if (string.IsNullOrWhiteSpace(hostId) || !IsValidHostId(hostId))
        {
            throw new ArgumentException("Invalid host id for pairing QR.", nameof(hostId));
        }
        var safe = hostId.Replace("\"", "");
        return "{\"v\":1,\"host_id\":\"" + safe + "\"}";
    }

    public static bool IsValidHostId(string? hostId)
    {
        if (string.IsNullOrWhiteSpace(hostId) || !hostId.StartsWith(Prefix, StringComparison.Ordinal))
        {
            return false;
        }
        if (hostId.Length > 64)
        {
            return false;
        }
        return hostId.Skip(Prefix.Length).All(c =>
            (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '-' || c == '_');
    }

    private static string Sanitize(string? raw)
    {
        if (string.IsNullOrEmpty(raw))
        {
            return "";
        }
        return new string(raw.ToLowerInvariant()
            .Where(c => (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '-' || c == '_')
            .ToArray());
    }
}
