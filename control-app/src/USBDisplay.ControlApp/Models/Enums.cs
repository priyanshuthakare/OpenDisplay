namespace USBDisplay.ControlApp.Models;

/// <summary>Overall orchestration state. ACTIVE means health-verified, never just "process started".</summary>
public enum SystemState
{
    Stopped,
    Starting,
    Active,
    Stopping,
    Error,
}

public enum ComponentState
{
    Unknown,
    Ready,
    Running,
    Connecting,
    Warning,
    Failed,
    Stopped,
    NotInstalled,
    ComingSoon,
}

public enum HealthState
{
    Unknown,
    Healthy,
    Degraded,
    Error,
}

public enum LogLevel
{
    Debug,
    Info,
    Warning,
    Error,
}

public enum TransportKind
{
    Usb,
    Wifi,
}

public enum DiagnosticStatus
{
    NotRun,
    Pass,
    Fail,
    Skipped,
}

public enum AdbDeviceState
{
    Device,
    Unauthorized,
    Offline,
    Other,
    Absent,
}
