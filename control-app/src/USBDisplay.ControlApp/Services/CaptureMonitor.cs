using System;
using System.IO;
using System.Linq;

namespace USBDisplay.ControlApp.Services;

public sealed record CaptureStatus(
    bool Exists, int FileCount, long TotalBytes, TimeSpan? NewestAge);

public sealed record PurgeResult(int DeletedFiles, long DeletedBytes);

/// <summary>
/// Hygiene for the legacy disk-backed capture handoff
/// (<c>%ProgramData%\USBDisplay\capture\capture_*.bmp</c>).
///
/// The production driver is in-memory only and writes nothing here, so this
/// folder should normally sit empty. If stale BMPs ever accumulate (old
/// builds, aborted runs), this bounds them. Purge is age-gated and only
/// touches <c>capture_*.bmp</c>: the live streamer consumes frames within
/// seconds, so a multi-minute threshold can never eat a frame it still
/// needs — including ordered replay, which never seeks backwards.
/// </summary>
public static class CaptureMonitor
{
    public static string DefaultDir()
    {
        var baseDir = Environment.GetEnvironmentVariable("ProgramData") ?? @"C:\ProgramData";
        return Path.Combine(baseDir, "USBDisplay", "capture");
    }

    public static CaptureStatus GetStatus(string? dir = null)
    {
        dir ??= DefaultDir();
        if (!Directory.Exists(dir))
        {
            return new CaptureStatus(false, 0, 0, null);
        }
        var files = BmpFiles(dir);
        if (files.Length == 0)
        {
            return new CaptureStatus(true, 0, 0, null);
        }
        var total = files.Sum(f => SafeLength(f));
        var newest = files.Max(f => SafeWriteTime(f));
        var age = newest == DateTime.MinValue ? (TimeSpan?)null : DateTime.UtcNow - newest.ToUniversalTime();
        return new CaptureStatus(true, files.Length, total, age);
    }

    public static PurgeResult PurgeOlderThan(TimeSpan maxAge, string? dir = null)
    {
        dir ??= DefaultDir();
        if (!Directory.Exists(dir))
        {
            return new PurgeResult(0, 0);
        }
        var cutoff = DateTime.UtcNow - maxAge;
        var deleted = 0;
        long bytes = 0;
        foreach (var file in BmpFiles(dir))
        {
            try
            {
                var wt = File.GetLastWriteTimeUtc(file);
                if (wt < cutoff)
                {
                    bytes += SafeLength(file);
                    File.Delete(file);
                    deleted++;
                }
            }
            catch
            {
                // Locked / vanishing files are the streamer's business; skip.
            }
        }
        return new PurgeResult(deleted, bytes);
    }

    private static string[] BmpFiles(string dir)
    {
        try
        {
            return Directory.GetFiles(dir, "capture_*.bmp");
        }
        catch
        {
            return Array.Empty<string>();
        }
    }

    private static long SafeLength(string file)
    {
        try { return new FileInfo(file).Length; } catch { return 0; }
    }

    private static DateTime SafeWriteTime(string file)
    {
        try { return File.GetLastWriteTimeUtc(file); } catch { return DateTime.MinValue; }
    }
}
