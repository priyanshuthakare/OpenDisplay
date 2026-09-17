using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using USBDisplay.ControlApp.Models;

namespace USBDisplay.ControlApp.Services;

public interface IProcessManager
{
    IReadOnlyList<ManagedProcessInfo> Snapshot();
    void Track(string component, Process process, string commandLine);
    void Untrack(string component);
    bool Restart(string component, Func<Process> starter, string commandLine);
    int CrashCount(string component);
    void NoteCrash(string component);
    void ResetCrashCount(string component);
}

/// <summary>
/// Registry of USBDisplay-owned processes. Graceful stop first (wait +
/// close), force-kill only after a timeout. Bounded crash counting prevents
/// infinite restart loops (see orchestrator).
/// </summary>
public sealed class ProcessManager : IProcessManager
{
    public const int MaxAutoRestarts = 3;

    private sealed record Entry(Process Process, string CommandLine, DateTimeOffset Start, TimeSpan LastCpu, DateTimeOffset LastSample);
    private readonly ConcurrentDictionary<string, Entry> _tracked = new(StringComparer.OrdinalIgnoreCase);
    private readonly ConcurrentDictionary<string, int> _crashes = new(StringComparer.OrdinalIgnoreCase);

    public void Track(string component, Process process, string commandLine)
    {
        TimeSpan cpu = TimeSpan.Zero;
        try { cpu = process.TotalProcessorTime; } catch { }
        _tracked[component] = new Entry(process, commandLine, DateTimeOffset.Now, cpu, DateTimeOffset.Now);
    }

    public void Untrack(string component) => _tracked.TryRemove(component, out _);

    public IReadOnlyList<ManagedProcessInfo> Snapshot()
    {
        var list = new List<ManagedProcessInfo>();
        foreach (var kv in _tracked)
        {
            var p = kv.Value.Process;
            bool alive = true;
            try { alive = !p.HasExited; } catch { alive = false; }
            if (!alive)
            {
                continue;
            }
            double cpu = 0;
            try
            {
                var now = DateTimeOffset.Now;
                var cur = p.TotalProcessorTime;
                var dt = (now - kv.Value.LastSample).TotalSeconds;
                if (dt > 0.2)
                {
                    cpu = Math.Max(0, (cur - kv.Value.LastCpu).TotalSeconds / dt / Environment.ProcessorCount * 100.0);
                    _tracked[kv.Key] = kv.Value with { LastCpu = cur, LastSample = now };
                }
            }
            catch { }
            int pid = -1; long ws = 0; bool responding = true; string exe = "";
            try { pid = p.Id; } catch { }
            try { ws = p.WorkingSet64; } catch { }
            try { responding = p.Responding; } catch { }
            try { exe = p.MainModule?.FileName ?? ""; } catch { }
            list.Add(new ManagedProcessInfo(kv.Key, pid, exe, kv.Value.CommandLine, kv.Value.Start, Math.Round(cpu, 1), ws, responding));
        }
        return list;
    }

    public bool Restart(string component, Func<Process> starter, string commandLine)
    {
        StopInternal(component, TimeSpan.FromSeconds(5));
        try
        {
            var fresh = starter();
            Track(component, fresh, commandLine);
            return true;
        }
        catch
        {
            return false;
        }
    }

    public int CrashCount(string component) => _crashes.TryGetValue(component, out var n) ? n : 0;
    public void NoteCrash(string component) => _crashes.AddOrUpdate(component, 1, (_, n) => n + 1);
    public void ResetCrashCount(string component) => _crashes.TryRemove(component, out _);

    private void StopInternal(string component, TimeSpan timeout)
    {
        if (!_tracked.TryRemove(component, out var entry))
        {
            return;
        }
        try
        {
            var p = entry.Process;
            if (p.HasExited) return;
            try { p.CloseMainWindow(); } catch { }
            if (!p.WaitForExit((int)timeout.TotalMilliseconds) && !p.HasExited)
            {
                p.Kill(entireProcessTree: true);
            }
        }
        catch { }
    }
}
