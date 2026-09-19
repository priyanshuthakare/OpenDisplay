using System;
using System.Collections.Generic;
using USBDisplay.ControlApp.Models;

namespace USBDisplay.ControlApp.Parsing;

/// <summary>
/// Parses the stable machine-readable surfaces of the existing repo tooling.
/// Pure static functions so they are unit testable without hardware.
/// </summary>
public static class CliOutputParser
{
    /// <summary>Parses <c>key=value</c> lines (usbdisplay-streamer contract).</summary>
    public static Dictionary<string, string> ParseKeyValues(string output)
    {
        var map = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        if (string.IsNullOrWhiteSpace(output))
        {
            return map;
        }
        foreach (var raw in output.Split(new[] { '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries))
        {
            var line = raw.Trim();
            if (line.StartsWith("{", StringComparison.Ordinal))
            {
                var stats = ParseStatsJson(line);
                foreach (var kv in stats)
                {
                    map[kv.Key] = kv.Value;
                }
                continue;
            }
            var eq = line.IndexOf('=');
            if (eq <= 0)
            {
                continue;
            }
            map[line.Substring(0, eq).Trim()] = line.Substring(eq + 1).Trim();
        }
        return map;
    }

    /// <summary>Parses the --stats-json line into the same key=value map shape.</summary>
    public static Dictionary<string, string> ParseStatsJson(string jsonLine)
    {
        var map = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        var inner = jsonLine.Trim().TrimStart('{').TrimEnd('}');
        foreach (var part in inner.Split(','))
        {
            var kv = part.Split(new[] { ':' }, 2);
            if (kv.Length != 2)
            {
                continue;
            }
            map[kv[0].Trim().Trim('"')] = kv[1].Trim().Trim('"');
        }
        return map;
    }

    /// <summary>Parses <c>adb devices -l</c> output.</summary>
    public static List<DeviceInfo> ParseAdbDevices(string output)
    {
        var devices = new List<DeviceInfo>();
        if (string.IsNullOrWhiteSpace(output))
        {
            return devices;
        }
        var lines = output.Split(new[] { '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries);
        for (var i = 1; i < lines.Length; i++) // skip "List of devices attached"
        {
            var line = lines[i].Trim();
            if (line.Length == 0 || line.StartsWith("*", StringComparison.Ordinal))
            {
                continue;
            }
            var parts = line.Split(new[] { ' ', '\t' }, StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length < 2)
            {
                continue;
            }
            var state = parts[1] switch
            {
                "device" => AdbDeviceState.Device,
                "unauthorized" => AdbDeviceState.Unauthorized,
                "offline" => AdbDeviceState.Offline,
                _ => AdbDeviceState.Other,
            };
            string? model = null, product = null, transportId = null;
            foreach (var p in parts)
            {
                if (p.StartsWith("model:", StringComparison.Ordinal)) model = p.Substring(6);
                else if (p.StartsWith("product:", StringComparison.Ordinal)) product = p.Substring(8);
                else if (p.StartsWith("transport_id:", StringComparison.Ordinal)) transportId = p.Substring(13);
            }
            devices.Add(new DeviceInfo(parts[0], state, model, product, transportId));
        }
        return devices;
    }

    /// <summary>Finds published driver names whose pnputil block mentions the given token.</summary>
    public static List<string> ParsePnputilPublishedNames(string output, string token)
    {
        var names = new List<string>();
        if (string.IsNullOrWhiteSpace(output))
        {
            return names;
        }
        var block = new List<string>();
        void Flush()
        {
            if (block.Count == 0)
            {
                return;
            }
            var joined = string.Join("\n", block);
            if (joined.IndexOf(token, StringComparison.OrdinalIgnoreCase) >= 0)
            {
                foreach (var l in block)
                {
                    var idx = l.IndexOf("Published Name", StringComparison.OrdinalIgnoreCase);
                    if (idx >= 0)
                    {
                        var colon = l.IndexOf(':', idx);
                        if (colon >= 0)
                        {
                            var name = l.Substring(colon + 1).Trim();
                            if (name.Length > 0)
                            {
                                names.Add(name);
                            }
                        }
                    }
                }
            }
            block.Clear();
        }
        foreach (var raw in output.Split(new[] { '\r', '\n' }))
        {
            if (string.IsNullOrWhiteSpace(raw))
            {
                Flush();
            }
            else
            {
                block.Add(raw);
            }
        }
        Flush();
        return names;
    }
}
