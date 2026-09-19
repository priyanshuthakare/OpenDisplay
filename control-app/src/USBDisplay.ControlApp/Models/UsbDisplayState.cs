namespace USBDisplay.ControlApp.Models;

/// <summary>
/// Authoritative subsystem health. Architectural invariant:
/// process running != healthy, installed != loaded, loaded != monitor
/// present, monitor present != stream active, connected != frames flowing.
/// </summary>
public enum SubsystemState
{
    Unknown,
    NotInstalled,
    Unavailable,
    Ready,
    Starting,
    Connecting,
    Running,
    Healthy,
    Warning,
    Disconnected,
    Stopping,
    Error,
}

/// <summary>
/// Single authoritative snapshot of the whole USBDisplay stack.
/// Pages must render from this (via the gateway) and never keep
/// contradictory local copies.
/// </summary>
public sealed record UsbDisplayState(
    SystemState OverallState,
    SubsystemState DriverState,
    SubsystemState DisplayState,
    SubsystemState CaptureState,
    SubsystemState EncoderState,
    SubsystemState TransportState,
    SubsystemState AndroidState,
    SubsystemState StreamState,
    SubsystemState InputState,
    TransportKind SelectedTransport,
    IReadOnlyList<TransportKind> AvailableTransports,
    StreamTelemetry Metrics,
    WifiLinkInfo? WifiLink,
    ErrorReport? LastError,
    IReadOnlyList<DiagnosticResult> Diagnostics,
    DateTimeOffset UpdatedAt)
{
    public bool IsActive => OverallState == SystemState.Active
        && StreamState == SubsystemState.Healthy
        && TransportState == SubsystemState.Healthy;

    public static UsbDisplayState Empty(StreamTelemetry? metrics = null) => new(
        SystemState.Stopped,
        SubsystemState.Unknown, SubsystemState.Unknown, SubsystemState.Unknown,
        SubsystemState.Unknown, SubsystemState.Unknown, SubsystemState.Unknown,
        SubsystemState.Unknown, SubsystemState.Unknown,
        TransportKind.Usb,
        new[] { TransportKind.Usb, TransportKind.Wifi },
        metrics ?? new StreamTelemetry(0, 0, 0, 0, 0, "—", "—", "—", 0, 0, 0, 0, DateTimeOffset.Now),
        null, null,
        Array.Empty<DiagnosticResult>(),
        DateTimeOffset.Now);
}

/// <summary>Three-layer error: user-facing, explanation, technical detail.</summary>
public sealed record ErrorReport(
    string UserMessage,
    string Explanation,
    string TechnicalDetail,
    string? Remediation,
    DateTimeOffset At);

/// <summary>Wi-Fi link detail. Unknown metrics stay null, never fabricated.</summary>
public sealed record WifiLinkInfo(
    string? Peer,
    string Security,
    string? Fingerprint,
    bool Trusted,
    string? HostId,
    bool PinVerified,
    long BitrateBps,
    string Adaptation,
    long? PacketLoss,
    long Reconnects);
