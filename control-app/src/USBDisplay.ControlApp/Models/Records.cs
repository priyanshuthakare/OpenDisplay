using System;
using System.Collections.Generic;

namespace USBDisplay.ControlApp.Models;

public sealed record DeviceInfo(
    string Serial,
    AdbDeviceState State,
    string? Model,
    string? Product,
    string? TransportId);

public sealed record DriverInfo(
    bool PackageInstalled,
    string? PublishedName,
    bool DevicePresent,
    string? InstanceId,
    string Status,
    string? Service,
    int? ProblemCode,
    bool Loaded,
    bool MonitorPresent,
    string DriverVersion);

public sealed record DisplayInfo(
    string Name,
    string Status,
    int Width,
    int Height,
    int RefreshHz,
    bool IsUsbDisplay,
    bool IsPrimary);

public sealed record StreamTelemetry(
    long StreamedFrames,
    long StreamedPackets,
    double WriteStallMsMax,
    long InputEventsInjected,
    double Fps,
    string Codec,
    string Resolution,
    string EncoderBackend,
    long BitrateBps,
    long DroppedFrames,
    long CrcFailures,
    long Reconnects,
    DateTimeOffset UpdatedAt);

public sealed record LogEntry(DateTimeOffset Time, LogLevel Level, string Source, string Message);

public sealed record DiagnosticResult(
    string Name,
    DiagnosticStatus Status,
    string Detail,
    string? Remediation);

public sealed record ServiceEntry(
    string Name,
    string Description,
    string Status,
    string? Executable,
    int? Pid,
    TimeSpan? Uptime,
    string? LastError);

public sealed record ManagedProcessInfo(
    string Component,
    int Pid,
    string Executable,
    string CommandLine,
    DateTimeOffset StartTime,
    double CpuPercent,
    long WorkingSetBytes,
    bool Responding);

public sealed record FeatureAvailability(string Name, ComponentState State, string Note);

public sealed record StartupStep(string Label, bool Done, bool Failed);

/// <summary>Persisted GUI settings. Transport/codec/display prefs become stream-capture flags.</summary>
public sealed class AppSettings
{
    public string StreamerPath { get; set; } = "";
    public string AdbPath { get; set; } = "";
    public string DriverScriptsDir { get; set; } = "";
    public string Codec { get; set; } = "h264";
    public int BitrateBps { get; set; } = 20_000_000;
    public int Fps { get; set; } = 60;
    public int Gop { get; set; } = 60;
    public int UsbPort { get; set; } = 27183;
    public int WifiPort { get; set; } = 27184;
    public int ConnectionTimeoutSeconds { get; set; } = 45;
    public string Transport { get; set; } = "usb";
    public string DeviceIp { get; set; } = "";
    public string Pin { get; set; } = "";
    public string PreferredResolution { get; set; } = "1920x1080";
    public int PreferredRefreshHz { get; set; } = 60;
    public string TopologyPreference { get; set; } = "Extend";
    public bool LaunchAtStartup { get; set; }
    public bool MinimizeToTray { get; set; } = true;
    public bool ConfirmBeforeStop { get; set; } = true;
    public bool ConfirmDestructive { get; set; } = true;
    public string LogLevel { get; set; } = "Info";
    public int LogRetentionDays { get; set; } = 14;
    public string ExportLocation { get; set; } = "";
    public bool DemoMode { get; set; }
    public bool AutoPurgeCapture { get; set; } = true;
    public int CapturePurgeAgeSeconds { get; set; } = 300;
    public int CaptureWarnMb { get; set; } = 512;
    public bool FirstRunDone { get; set; }
    public bool AutoStartStreaming { get; set; }
    public int DevicePollSeconds { get; set; } = 5;
}
