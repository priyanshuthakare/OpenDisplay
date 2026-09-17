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
}

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
