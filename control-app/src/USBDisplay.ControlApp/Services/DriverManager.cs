using System;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Native;
using USBDisplay.ControlApp.Parsing;

namespace USBDisplay.ControlApp.Services;

public interface IDriverManager
{
    string ScriptsDir { get; }
    Task<DriverInfo> GetDriverInfoAsync(CancellationToken ct = default);
    Task<ProcessResult> InstallAsync(CancellationToken ct = default);
    Task<ProcessResult> UninstallAsync(CancellationToken ct = default);
    Task RestartAsync(CancellationToken ct = default);
    Task SetEnabledAsync(string instanceId, bool enable, CancellationToken ct = default);
    string DriverVersion { get; }
}

public sealed class DriverManager : IDriverManager
{
    public const string HardwareIdToken = "USBDisplayIdd";
    public const string DriverVersionValue = "0.2.0.1";

    private readonly IProcessRunner _runner;
    private readonly IElevationService _elevation;
    private readonly IConfigurationService _config;

    public DriverManager(IProcessRunner runner, IElevationService elevation, IConfigurationService config)
    {
        _runner = runner;
        _elevation = elevation;
        _config = config;
    }

    public string DriverVersion => DriverVersionValue;

    public string ScriptsDir
    {
        get
        {
            var configured = _config.Settings.DriverScriptsDir;
            if (!string.IsNullOrWhiteSpace(configured) && Directory.Exists(configured))
            {
                return configured;
            }
            var repoRoot = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "..", "..", "..", ".."));
            var candidate = Path.Combine(repoRoot, "driver", "idd");
            if (File.Exists(Path.Combine(candidate, "install.ps1")))
            {
                return candidate;
            }
            return Path.Combine(AppContext.BaseDirectory, "driver");
        }
    }

    public async Task<DriverInfo> GetDriverInfoAsync(CancellationToken ct = default)
    {
        string? published = null;
        try
        {
            var result = await _runner.RunAsync("pnputil", new[] { "/enum-drivers" }, null, TimeSpan.FromSeconds(30), ct).ConfigureAwait(false);
            if (result.ExitCode == 0)
            {
                published = CliOutputParser.ParsePnputilPublishedNames(result.StdOut, "USBDisplay").FirstOrDefault();
            }
        }
        catch { }

        SetupApi.PnpDevice? match = null;
        try
        {
            match = SetupApi.EnumeratePresent()
                .FirstOrDefault(d => d.HardwareIds.IndexOf(HardwareIdToken, StringComparison.OrdinalIgnoreCase) >= 0);
        }
        catch { }

        var devicePresent = match != null;
        var status = "Absent";
        string? service = match?.Service;
        int? problem = null;
        if (devicePresent)
        {
            try
            {
                var (flags, prob) = SetupApi.GetLiveStatus(match!.InstanceId);
                if (prob != uint.MaxValue)
                {
                    problem = unchecked((int)prob);
                }
                var started = (flags & SetupApi.DN_STARTED) != 0;
                var hasProblem = (flags & SetupApi.DN_HAS_PROBLEM) != 0;
                status = started && !hasProblem ? "OK" : hasProblem ? "Problem" : "Present";
            }
            catch
            {
                status = "Present";
            }
        }

        var loaded = devicePresent && status == "OK" && problem is 0 or null
            && string.Equals(service, "WUDFRd", StringComparison.OrdinalIgnoreCase);

        bool monitor = false;
        try
        {
            monitor = DisplayApi.EnumerateAdapters().Any(a =>
                a.DeviceString.IndexOf("USBDisplay", StringComparison.OrdinalIgnoreCase) >= 0);
        }
        catch { }

        return new DriverInfo(
            published != null, published, devicePresent, match?.InstanceId,
            status, service, problem, loaded, monitor, DriverVersionValue);
    }

    public Task<ProcessResult> InstallAsync(CancellationToken ct = default)
    {
        var script = Path.Combine(ScriptsDir, "install.ps1");
        if (!File.Exists(script)) throw new FileNotFoundException($"Driver install script not found: {script}");
        // install.ps1 builds + signs unless -SkipBuild; the GUI ships no WDK,
        // so prefer a prebuilt package when present, else build.
        var packageInf = Path.Combine(ScriptsDir, "package", "x64", "Release", "Driver.inf");
        var args = File.Exists(packageInf)
            ? new[] { "-ExecutionPolicy", "Bypass", "-File", script, "-SkipBuild" }
            : new[] { "-ExecutionPolicy", "Bypass", "-File", script };
        return RunElevatedScriptAsync("powershell.exe", args, ct);
    }

    public Task<ProcessResult> UninstallAsync(CancellationToken ct = default)
    {
        var script = Path.Combine(ScriptsDir, "uninstall.ps1");
        if (!File.Exists(script)) throw new FileNotFoundException($"Driver uninstall script not found: {script}");
        return RunElevatedScriptAsync("powershell.exe",
            new[] { "-ExecutionPolicy", "Bypass", "-File", script }, ct);
    }

    public async Task RestartAsync(CancellationToken ct = default)
    {
        var info = await GetDriverInfoAsync(ct).ConfigureAwait(false);
        if (info.InstanceId == null) throw new InvalidOperationException("No USBDisplay device node to restart.");
        await SetEnabledAsync(info.InstanceId, false, ct).ConfigureAwait(false);
        await Task.Delay(1500, ct).ConfigureAwait(false);
        await SetEnabledAsync(info.InstanceId, true, ct).ConfigureAwait(false);
    }

    public async Task SetEnabledAsync(string instanceId, bool enable, CancellationToken ct = default)
    {
        if (string.IsNullOrWhiteSpace(instanceId)) throw new ArgumentException("Empty instance id.", nameof(instanceId));
        var verb = enable ? "/enable-device" : "/disable-device";
        var result = await _runner.RunAsync("pnputil", new[] { verb, instanceId }, null, TimeSpan.FromSeconds(60), ct).ConfigureAwait(false);
        if (result.ExitCode != 0)
        {
            throw new InvalidOperationException($"pnputil {verb} failed: {(result.StdOut + result.StdErr).Trim()}");
        }
    }

    private Task<ProcessResult> RunElevatedScriptAsync(string exe, string[] args, CancellationToken ct)
    {
        // Elevated work runs synchronously through UAC; wrap it so callers stay async.
        return Task.Run(() =>
        {
            ct.ThrowIfCancellationRequested();
            var exit = _elevation.RunElevated(exe, args, ScriptsDir);
            return new ProcessResult(exit, "", exit == 0 ? "" : $"Elevated step exited with code {exit}.");
        }, ct);
    }
}
