using System;
using System.Collections.Generic;
using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Parsing;

namespace USBDisplay.ControlApp.Services;

/// <summary>
/// Folds streamer stdout key=value lines (and --stats-json lines) into live
/// telemetry. Pure and unit testable. FPS is measured from stats frame
/// deltas; latency is never synthesized — fields the streamer does not emit
/// surface as N/A in the UI.
/// </summary>
public sealed class StreamTelemetryAccumulator
{
    private long _frames;
    private long _packets;
    private double _stallMax;
    private long _inputEvents;
    private long _dropped;
    private long _crc;
    private long _reconnects;
    private long _bitrate;
    private string _codec = "—";
    private string _resolution = "—";
    private string _backend = "—";
    private bool _connected;
    private long _lastFrames = -1;
    private DateTimeOffset _lastTime = DateTimeOffset.MinValue;
    private double _fps;
    private string _wifiPeer = "";
    private string _wifiFingerprint = "";
    private string _wifiEncryption = "";
    private string _adaptation = "Stable";
    private readonly List<string> _adaptationEvents = new();

    public bool Connected => _connected;
    public string WifiPeer => _wifiPeer;
    public string WifiFingerprint => _wifiFingerprint;
    public string WifiEncryption => _wifiEncryption;
    public string Adaptation => _adaptation;
    public IReadOnlyList<string> AdaptationEvents => _adaptationEvents;

    public void FeedLine(string line, IDictionary<string, string>? extra = null)
    {
        if (string.IsNullOrWhiteSpace(line))
        {
            return;
        }
        var map = CliOutputParser.ParseKeyValues(line);
        if (extra != null)
        {
            foreach (var kv in extra)
            {
                map[kv.Key] = kv.Value;
            }
        }
        FeedMap(map, line);
    }

    public void FeedMap(IDictionary<string, string> map, string? rawLine = null)
    {
        if (map.TryGetValue("android_connection", out var conn) && conn == "established")
        {
            _connected = true;
        }
        if (map.TryGetValue("streamed_frames", out var sf) && long.TryParse(sf, out var f))
        {
            var now = DateTimeOffset.Now;
            if (_lastFrames >= 0 && (now - _lastTime).TotalSeconds > 0.05)
            {
                _fps = (f - _lastFrames) / (now - _lastTime).TotalSeconds;
            }
            _lastFrames = f;
            _lastTime = now;
            _frames = f;
        }
        if (map.TryGetValue("streamed_packets", out var sp) && long.TryParse(sp, out var p)) _packets = p;
        if (map.TryGetValue("write_stall_ms_max", out var ws) && double.TryParse(ws, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var w)) _stallMax = w;
        if (map.TryGetValue("input_events_injected", out var ie) && long.TryParse(ie, out var i)) _inputEvents = i;
        if (map.TryGetValue("stream_encoder_backend", out var be)) _backend = be;
        if (map.TryGetValue("stream_bitrate", out var br) && long.TryParse(br, out var b)) _bitrate = b;
        if (map.TryGetValue("stream_frame_size", out var fs)) _resolution = fs.Replace('x', '×');
        if (map.TryGetValue("wifi_device", out var wd)) _wifiPeer = wd;
        if (map.TryGetValue("wifi_fingerprint", out var wf)) _wifiFingerprint = wf;
        if (map.TryGetValue("wifi_encryption", out var we)) _wifiEncryption = we;
        var raw = rawLine ?? string.Join(" ", map.Select(kv => $"{kv.Key}={kv.Value}"));
        foreach (var key in new[] { "bitrate_step_down", "bitrate_step_up" })
        {
            if (raw.IndexOf(key, StringComparison.OrdinalIgnoreCase) >= 0)
            {
                var evt = raw.Trim();
                _adaptationEvents.Add($"{DateTimeOffset.Now:HH:mm:ss} {evt}");
                while (_adaptationEvents.Count > 20) _adaptationEvents.RemoveAt(0);
                if (map.TryGetValue("new_bitrate", out var nb) && long.TryParse(nb, out var nbr)) _bitrate = nbr;
                _adaptation = key == "bitrate_step_down" ? "Stepped down (network stall)" : "Stepped up (recovered)";
            }
        }
        if (map.TryGetValue("wifi_defaults_applied", out _) && _bitrate == 0) { }
    }

    public void SetStatic(string codec, string resolution, long bitrateBps)
    {
        _codec = codec;
        _resolution = resolution.Replace('x', '×');
        _bitrate = bitrateBps;
    }

    public void MarkDroppedFrame() => _dropped++;
    public void MarkCrcFailure() => _crc++;
    public void MarkReconnect() => _reconnects++;
    public void MarkDisconnected() => _connected = false;

    public StreamTelemetry Snapshot() => new(
        _frames, _packets, _stallMax, _inputEvents, Math.Round(_fps, 1),
        _codec, _resolution, _backend, _bitrate, _dropped, _crc, _reconnects,
        DateTimeOffset.Now);
}
