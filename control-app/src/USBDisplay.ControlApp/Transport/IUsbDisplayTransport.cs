using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using USBDisplay.ControlApp.Models;

namespace USBDisplay.ControlApp.Transport;

/// <summary>
/// One abstraction for both link types. Upper layers never branch on
/// USB-vs-WiFi except to display the active transport; in particular
/// they must never silently fall back from one to the other.
/// </summary>
public interface IUsbDisplayTransport
{
    TransportKind Kind { get; }
    string DisplayName { get; }
    string EndpointDescription { get; }

    Task<TransportStatus> ConnectAsync(CancellationToken ct = default);
    Task DisconnectAsync(CancellationToken ct = default);
    Task<TransportStatus> ReconnectAsync(CancellationToken ct = default);
    Task<TransportStatus> GetStatusAsync(CancellationToken ct = default);
    Task<TransportDiagnostics> GetDiagnosticsAsync(CancellationToken ct = default);
}

public sealed record TransportStatus(
    TransportKind Kind,
    SubsystemState State,
    string Endpoint,
    string Detail,
    string? LastError);

public sealed record TransportDiagnostics(
    TransportKind Kind,
    IReadOnlyList<DiagnosticResult> Checks);

/// <summary>
/// Explicit user-confirmed transport selection. There is no automatic
/// USB↔WiFi downgrade anywhere in the app; callers must surface both
/// options and wait for the user to pick.
/// </summary>
public static class TransportPolicy
{
    public const int UsbPort = 27183;
    public const int WifiPort = 27184;

    public static string NoSilentFallbackMessage(TransportKind failed, TransportKind alternative) =>
        failed == TransportKind.Usb
            ? "USB connection failed. Wi-Fi is available. Choose Retry USB or Switch to Wi-Fi — the app never switches automatically."
            : "Wi-Fi connection failed. USB is available. Choose Retry Wi-Fi or Switch to USB — the app never switches automatically.";

    public static string WifiUnreachableMessage() =>
        "Wi-Fi unavailable. Possible cause: the network is AP-isolated or the host is unreachable. Use USB to connect this tablet.";

    public static bool IsExplicitSwitch(TransportKind from, TransportKind to) => from != to;
}

/// <summary>Test-seam for the Wi-Fi TCP reachability probe (real network in prod, fake in tests).</summary>
public interface IWifiPreflight
{
    Task<TransportStatus> CheckAsync(CancellationToken ct = default);
}
