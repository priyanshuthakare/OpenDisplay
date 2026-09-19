using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using USBDisplay.ControlApp.Models;

namespace USBDisplay.ControlApp.Services;

public interface ILogService
{
    event EventHandler<LogEntry>? EntryAdded;
    IReadOnlyList<LogEntry> Entries { get; }
    void Log(LogLevel level, string source, string message);
    void Clear();
    IEnumerable<LogEntry> Query(LogLevel? minLevel, string? search);
    void Export(string path);
}

public sealed class LogService : ILogService
{
    private const int Capacity = 5000;
    private readonly ConcurrentQueue<LogEntry> _entries = new();

    public event EventHandler<LogEntry>? EntryAdded;

    public IReadOnlyList<LogEntry> Entries => _entries.ToArray();

    public void Log(LogLevel level, string source, string message)
    {
        var entry = new LogEntry(DateTimeOffset.Now, level, source, message ?? "");
        _entries.Enqueue(entry);
        while (_entries.Count > Capacity && _entries.TryDequeue(out _)) { }
        EntryAdded?.Invoke(this, entry);
    }

    public void Clear()
    {
        while (_entries.TryDequeue(out _)) { }
    }

    public IEnumerable<LogEntry> Query(LogLevel? minLevel, string? search)
    {
        var snapshot = _entries.ToArray();
        return snapshot.Where(e =>
            (!minLevel.HasValue || e.Level >= minLevel.Value) &&
            (string.IsNullOrWhiteSpace(search) ||
             e.Message.Contains(search, StringComparison.OrdinalIgnoreCase) ||
             e.Source.Contains(search, StringComparison.OrdinalIgnoreCase)));
    }

    public void Export(string path)
    {
        var lines = _entries.ToArray()
            .Select(e => $"{e.Time:HH:mm:ss} {e.Level.ToString().ToUpperInvariant(),-7} [{e.Source}] {e.Message}");
        File.WriteAllLines(path, lines);
    }
}
