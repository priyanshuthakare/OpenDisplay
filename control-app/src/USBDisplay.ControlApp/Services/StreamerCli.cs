using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Parsing;

namespace USBDisplay.ControlApp.Services;

public sealed record StreamStartOptions(
    TransportKind Transport,
    string Codec,
    int BitrateBps,
    int Fps,
    int Gop,
    int UsbPort,
    string? Serial,
    string? DeviceIp,
    string? Pin,
    bool Live,
    bool Loop);

/// <summary>Handle to a running stream-capture child process.</summary>
public interface IStreamSession : IDisposable
{
    event EventHandler<string>? LineReceived;
    event EventHandler<int>? Exited;
    int Pid { get; }
    string CommandLine { get; }
    DateTimeOffset StartTime { get; }
    bool HasExited { get; }
    void Stop(TimeSpan gracefulTimeout);
}

public interface IStreamerCli
{
    string ExePath { get; }
    bool Available { get; }
    Task<IReadOnlyList<DeviceInfo>> GetDevicesAsync(CancellationToken ct = default);
    Task<Dictionary<string, string>> GetCapabilitiesAsync(CancellationToken ct = default);
    Task<ProcessResult> ProbeFrameAsync(CancellationToken ct = default);
    Task<ProcessResult> TransportProbeAsync(CancellationToken ct = default);
    IStreamSession StartStream(StreamStartOptions options, DataReceivedEventHandler onOut, DataReceivedEventHandler onErr);
}

public sealed class StreamerCli : IStreamerCli
{
    private readonly IProcessRunner _runner;
    private readonly IConfigurationService _config;

    public StreamerCli(IProcessRunner runner, IConfigurationService config)
    {
        _runner = runner;
        _config = config;
    }

    public string ExePath => ResolveExe();
    public bool Available => File.Exists(ExePath);

    public async Task<IReadOnlyList<DeviceInfo>> GetDevicesAsync(CancellationToken ct = default)
    {
        // The streamer `devices` command prints `no_android_devices=true` when
        // empty, else one `serial=… state=… …` line per device.
        var result = await _runner.RunAsync(ExePath, new[] { "devices" }, null, TimeSpan.FromSeconds(30), ct).ConfigureAwait(false);
        if (result.ExitCode != 0)
        {
            throw new InvalidOperationException($"streamer devices failed: {result.StdErr.Trim()}");
        }
        var map = CliOutputParser.ParseKeyValues(result.StdOut);
        if (map.TryGetValue("no_android_devices", out var none) && none == "true")
        {
            return Array.Empty<DeviceInfo>();
        }
        var devices = new List<DeviceInfo>();
        foreach (var raw in result.StdOut.Split(new[] { '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries))
        {
            var line = raw.Trim();
            if (!line.StartsWith("serial=", StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }
            var kv = CliOutputParser.ParseKeyValues(line);
            kv.TryGetValue("serial", out var serial);
            kv.TryGetValue("state", out var state);
            kv.TryGetValue("model", out var model);
            kv.TryGetValue("product", out var product);
            kv.TryGetValue("transport_id", out var tid);
            devices.Add(new DeviceInfo(
                serial ?? "", ParseState(state), model, product, tid));
        }
        return devices;
    }

    public async Task<Dictionary<string, string>> GetCapabilitiesAsync(CancellationToken ct = default)
    {
        var result = await _runner.RunAsync(ExePath, new[] { "capabilities" }, null, TimeSpan.FromSeconds(30), ct).ConfigureAwait(false);
        if (result.ExitCode != 0)
        {
            throw new InvalidOperationException($"streamer capabilities failed: {result.StdErr.Trim()}");
        }
        return CliOutputParser.ParseKeyValues(result.StdOut);
    }

    public Task<ProcessResult> ProbeFrameAsync(CancellationToken ct = default) =>
        _runner.RunAsync(ExePath, new[] { "probe-frame" }, null, TimeSpan.FromSeconds(60), ct);

    public Task<ProcessResult> TransportProbeAsync(CancellationToken ct = default) =>
        _runner.RunAsync(ExePath, new[] { "transport-probe" }, null, TimeSpan.FromSeconds(60), ct);

    public IStreamSession StartStream(StreamStartOptions o, DataReceivedEventHandler onOut, DataReceivedEventHandler onErr)
    {
        var args = new List<string>
        {
            "stream-capture",
            "--codec", o.Codec,
            "--bitrate", o.BitrateBps.ToString(),
            "--fps", o.Fps.ToString(),
            "--gop", o.Gop.ToString(),
            "--port", o.UsbPort.ToString(),
            "--transport", o.Transport == TransportKind.Wifi ? "wifi" : "usb",
            "--stats-json",
        };
        if (!string.IsNullOrWhiteSpace(o.Serial)) { args.Add("--serial"); args.Add(o.Serial); }
        if (o.Transport == TransportKind.Wifi)
        {
            if (string.IsNullOrWhiteSpace(o.DeviceIp)) throw new ArgumentException("WiFi transport needs --device-ip.");
            args.Add("--device-ip"); args.Add(o.DeviceIp);
        }
        if (!o.Live) args.Add("--no-live");
        if (o.Loop) args.Add("--loop");
        IReadOnlyDictionary<string, string?>? environment = null;
        if (o.Transport == TransportKind.Wifi && !string.IsNullOrWhiteSpace(o.Pin))
        {
            environment = new Dictionary<string, string?> { ["USBDISPLAY_WIFI_PIN"] = o.Pin };
        }
        var process = _runner.StartLongRunning(ExePath, args.ToArray(), null, onOut, onErr, environment);
        return new StreamSession(process, $"{ExePath} {string.Join(" ", args)}");
    }

    private static AdbDeviceState ParseState(string? state) => state switch
    {
        "Device" => AdbDeviceState.Device,
        "Unauthorized" => AdbDeviceState.Unauthorized,
        "Offline" => AdbDeviceState.Offline,
        _ => AdbDeviceState.Other,
    };

    private string ResolveExe()
    {
        var configured = _config.Settings.StreamerPath;
        if (!string.IsNullOrWhiteSpace(configured) && File.Exists(configured))
        {
            return configured;
        }
        var appDir = AppContext.BaseDirectory;
        foreach (var name in new[] { "usbdisplay-streamer.exe", "usbdisplay-streamer" })
        {
            var local = Path.Combine(appDir, name);
            if (File.Exists(local)) return local;
        }
        // Repo layout fallback: control-app/../.. -> host/streamer target dir.
        var repoRoot = Path.GetFullPath(Path.Combine(appDir, "..", "..", "..", ".."));
        foreach (var rel in new[] {
            @"target\debug\usbdisplay-streamer.exe",
            @"target\release\usbdisplay-streamer.exe" })
        {
            var candidate = Path.Combine(repoRoot, rel);
            if (File.Exists(candidate)) return candidate;
        }
        return Path.Combine(appDir, "usbdisplay-streamer.exe");
    }

    private sealed class StreamSession : IStreamSession
    {
        private readonly Process _process;
        private bool _disposed;

        public StreamSession(Process process, string commandLine)
        {
            _process = process;
            CommandLine = commandLine;
            StartTime = DateTimeOffset.Now;
            _process.Exited += (_, _) => Exited?.Invoke(this, _process.ExitCode);
        }

        public event EventHandler<string>? LineReceived;
        public event EventHandler<int>? Exited;
        public int Pid { get { try { return _process.Id; } catch { return -1; } } }
        public string CommandLine { get; }
        public DateTimeOffset StartTime { get; }
        public bool HasExited { get { try { return _process.HasExited; } catch { return true; } } }

        public void RaiseLine(string line) => LineReceived?.Invoke(this, line);

        public void Stop(TimeSpan gracefulTimeout)
        {
            try
            {
                if (_process.HasExited) return;
                // Console child has no window to close gracefully; give it a
                // moment (it polls its run flag on the input channel), then kill.
                if (!_process.WaitForExit((int)gracefulTimeout.TotalMilliseconds))
                {
                    _process.Kill(entireProcessTree: true);
                }
            }
            catch { }
        }

        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;
            try { if (!_process.HasExited) _process.Kill(entireProcessTree: true); } catch { }
            _process.Dispose();
        }
    }
}
