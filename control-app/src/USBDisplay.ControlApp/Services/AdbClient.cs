using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Parsing;

namespace USBDisplay.ControlApp.Services;

public interface IAdbClient
{
    string ExePath { get; }
    bool Available { get; }
    Task<IReadOnlyList<DeviceInfo>> ListDevicesAsync(CancellationToken ct = default);
    Task ReconnectAsync(CancellationToken ct = default);
    Task RestartServerAsync(CancellationToken ct = default);
    Task InstallApkAsync(string apkPath, CancellationToken ct = default);
    Task LaunchAppAsync(string package, CancellationToken ct = default);
    Task<IReadOnlyList<AdbForward>> ListForwardsAsync(CancellationToken ct = default);
    Task<ForwardResult> EnsureForwardAsync(int port, string? serial = null, CancellationToken ct = default);
    Task RemoveForwardAsync(int port, string? serial = null, CancellationToken ct = default);
    Task<int> RemoveStaleForwardsAsync(int port, CancellationToken ct = default);
}

public sealed record AdbForward(string Serial, string LocalSpec, string RemoteSpec);

public sealed record ForwardResult(bool Ok, string Detail, string? Remediation);

public sealed class AdbClient : IAdbClient
{
    private readonly IProcessRunner _runner;
    private readonly IConfigurationService _config;

    public AdbClient(IProcessRunner runner, IConfigurationService config)
    {
        _runner = runner;
        _config = config;
    }

    public string ExePath => ResolveExe();
    public bool Available => File.Exists(ExePath);

    public async Task<IReadOnlyList<DeviceInfo>> ListDevicesAsync(CancellationToken ct = default)
    {
        var result = await _runner.RunAsync(ExePath, new[] { "devices", "-l" }, null, TimeSpan.FromSeconds(30), ct).ConfigureAwait(false);
        if (result.ExitCode != 0)
        {
            throw new InvalidOperationException($"adb devices failed: {result.StdErr.Trim()}");
        }
        return CliOutputParser.ParseAdbDevices(result.StdOut);
    }

    public async Task ReconnectAsync(CancellationToken ct = default)
    {
        var result = await _runner.RunAsync(ExePath, new[] { "reconnect" }, null, TimeSpan.FromSeconds(60), ct).ConfigureAwait(false);
        if (result.ExitCode != 0)
        {
            throw new InvalidOperationException($"adb reconnect failed: {result.StdErr.Trim()}");
        }
    }

    public async Task RestartServerAsync(CancellationToken ct = default)
    {
        await _runner.RunAsync(ExePath, new[] { "kill-server" }, null, TimeSpan.FromSeconds(30), ct).ConfigureAwait(false);
        await _runner.RunAsync(ExePath, new[] { "start-server" }, null, TimeSpan.FromSeconds(30), ct).ConfigureAwait(false);
    }

    public async Task InstallApkAsync(string apkPath, CancellationToken ct = default)
    {
        if (!File.Exists(apkPath)) throw new FileNotFoundException($"APK not found: {apkPath}");
        var result = await _runner.RunAsync(ExePath, new[] { "install", "-r", apkPath }, null, TimeSpan.FromMinutes(3), ct).ConfigureAwait(false);
        if (result.ExitCode != 0 || result.StdOut.IndexOf("Success", StringComparison.OrdinalIgnoreCase) < 0)
        {
            throw new InvalidOperationException($"adb install failed: {(result.StdOut + result.StdErr).Trim()}");
        }
    }

    public async Task LaunchAppAsync(string package, CancellationToken ct = default)
    {
        var result = await _runner.RunAsync(ExePath, new[] { "shell", "monkey", "-p", package, "1" }, null, TimeSpan.FromSeconds(30), ct).ConfigureAwait(false);
        if (result.ExitCode != 0)
        {
            throw new InvalidOperationException($"adb launch failed: {result.StdErr.Trim()}");
        }
    }

    /// <summary>Lists active forwards via <c>adb forward --list</c> (serial local remote per line).</summary>
    public async Task<IReadOnlyList<AdbForward>> ListForwardsAsync(CancellationToken ct = default)
    {
        var result = await _runner.RunAsync(ExePath, new[] { "forward", "--list" }, null, TimeSpan.FromSeconds(30), ct).ConfigureAwait(false);
        var list = new List<AdbForward>();
        if (result.ExitCode != 0) return list;
        foreach (var raw in result.StdOut.Split(new[] { '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries))
        {
            var parts = raw.Trim().Split(new[] { ' ', '\t' }, StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length >= 3) list.Add(new AdbForward(parts[0], parts[1], parts[2]));
        }
        return list;
    }

    /// <summary>Verifies tcp:port forward; creates it if missing. Detects stale forwards for other serials.</summary>
    public async Task<ForwardResult> EnsureForwardAsync(int port, string? serial = null, CancellationToken ct = default)
    {
        var want = $"tcp:{port}";
        IReadOnlyList<AdbForward> forwards;
        try { forwards = await ListForwardsAsync(ct).ConfigureAwait(false); }
        catch (Exception ex) { return new ForwardResult(false, $"Could not list adb forwards: {ex.Message}", "Check the USB cable and run adb reconnect from the Device page."); }
        var match = System.Linq.Enumerable.FirstOrDefault(forwards,
            f => f.LocalSpec == want && (serial == null || f.Serial == serial));
        if (match != null)
            return new ForwardResult(true, $"adb forward {want}->{match.RemoteSpec} verified for {match.Serial}.", null);
        // Remove stale forwards on the same local port owned by other serials before recreating.
        foreach (var stale in System.Linq.Enumerable.Where(forwards, f => f.LocalSpec == want))
        {
            try { await RemoveForwardSpecAsync(stale.Serial, stale.LocalSpec, ct).ConfigureAwait(false); } catch { }
        }
        var args = serial != null
            ? new[] { "-s", serial, "forward", want, want }
            : new[] { "forward", want, want };
        var result = await _runner.RunAsync(ExePath, args, null, TimeSpan.FromSeconds(30), ct).ConfigureAwait(false);
        if (result.ExitCode != 0)
            return new ForwardResult(false, $"adb forward failed: {(result.StdOut + result.StdErr).Trim()}", "Reconnect the tablet; if the port is held by another app, free tcp:27183 and retry.");
        return new ForwardResult(true, $"adb forward {want}->{want} created.", null);
    }

    public async Task RemoveForwardAsync(int port, string? serial = null, CancellationToken ct = default)
    {
        var want = $"tcp:{port}";
        var forwards = await ListForwardsAsync(ct).ConfigureAwait(false);
        foreach (var f in forwards)
        {
            if (f.LocalSpec != want) continue;
            if (serial != null && f.Serial != serial) continue;
            try { await RemoveForwardSpecAsync(f.Serial, f.LocalSpec, ct).ConfigureAwait(false); } catch { }
        }
    }

    public async Task<int> RemoveStaleForwardsAsync(int port, CancellationToken ct = default)
    {
        // Stale = forward on our port whose device serial is no longer visible.
        var removed = 0;
        IReadOnlyList<DeviceInfo> devices;
        try { devices = await ListDevicesAsync(ct).ConfigureAwait(false); }
        catch { return 0; }
        var live = new HashSet<string>(System.Linq.Enumerable.Select(devices, d => d.Serial), StringComparer.Ordinal);
        var forwards = await ListForwardsAsync(ct).ConfigureAwait(false);
        foreach (var f in forwards)
        {
            if (f.LocalSpec != $"tcp:{port}") continue;
            if (live.Contains(f.Serial)) continue;
            try { await RemoveForwardSpecAsync(f.Serial, f.LocalSpec, ct).ConfigureAwait(false); removed++; } catch { }
        }
        return removed;
    }

    private async Task RemoveForwardSpecAsync(string serial, string localSpec, CancellationToken ct)
    {
        await _runner.RunAsync(ExePath, new[] { "-s", serial, "forward", "--remove", localSpec }, null, TimeSpan.FromSeconds(30), ct).ConfigureAwait(false);
    }

    private string ResolveExe()
    {
        var configured = _config.Settings.AdbPath;
        if (!string.IsNullOrWhiteSpace(configured) && File.Exists(configured))
        {
            return configured;
        }
        var platformTools = @"C:\platform-tools\adb.exe";
        if (File.Exists(platformTools))
        {
            return platformTools;
        }
        return "adb";
    }
}
