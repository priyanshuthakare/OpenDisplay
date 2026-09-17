using System;
using System.Collections.ObjectModel;
using System.Linq;
using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Mvvm;
using USBDisplay.ControlApp.Services;

namespace USBDisplay.ControlApp.ViewModels;

public sealed record PipelineNode(string Name, string Detail, ComponentState State);

public sealed class DashboardViewModel : ObservableObject
{
    private readonly IUsbDisplayGateway _gateway;

    public DashboardViewModel(IUsbDisplayGateway gateway)
    {
        _gateway = gateway;
        _gateway.Changed += (_, _) => Refresh();
        _gateway.Log.EntryAdded += (_, e) =>
        {
            System.Windows.Application.Current?.Dispatcher.Invoke(() =>
            {
                RecentActivity.Insert(0, $"{e.Time:HH:mm:ss}  {e.Message}");
                while (RecentActivity.Count > 8) RecentActivity.RemoveAt(RecentActivity.Count - 1);
            });
        };
        Refresh();
    }

    public ObservableCollection<PipelineNode> Nodes { get; } = new();
    public ObservableCollection<string> RecentActivity { get; } = new();

    private string _statusText = "STOPPED";
    public string StatusText { get => _statusText; private set => SetProperty(ref _statusText, value); }

    private string _telemetryLine = "N/A";
    public string TelemetryLine { get => _telemetryLine; private set => SetProperty(ref _telemetryLine, value); }

    public void Refresh()
    {
        var g = _gateway;
        StatusText = g.State switch
        {
            SystemState.Active => "ACTIVE",
            SystemState.Starting => "STARTING",
            SystemState.Stopping => "STOPPING",
            SystemState.Error => "ERROR",
            _ => "STOPPED",
        };

        var monitor = g.Driver?.MonitorPresent == true;
        var active = g.State == SystemState.Active;
        var connected = g.Telemetry.StreamedFrames > 0;
        var starting = g.State == SystemState.Starting;
        var error = g.State == SystemState.Error;

        ComponentState Map(bool good, string? detailIfBad = null)
        {
            if (error) return ComponentState.Failed;
            if (active) return good ? ComponentState.Running : ComponentState.Warning;
            if (starting) return ComponentState.Connecting;
            return good ? ComponentState.Ready : ComponentState.Stopped;
        }

        var deviceOk = g.Devices.Any(d => d.State == AdbDeviceState.Device) || g.Settings.Transport == "wifi";
        var nodes = new[]
        {
            new PipelineNode("WINDOWS", "Host", ComponentState.Ready),
            new PipelineNode("VIRTUAL DISPLAY", monitor ? "Available" : "Not detected", Map(monitor)),
            new PipelineNode("CAPTURE", active ? $"{g.Telemetry.Fps} FPS" : "Idle", Map(active && connected)),
            new PipelineNode("ENCODER", string.IsNullOrWhiteSpace(g.Telemetry.EncoderBackend) || g.Telemetry.EncoderBackend == "—" ? g.Settings.Codec.ToUpperInvariant() : g.Telemetry.EncoderBackend, Map(active && connected)),
            new PipelineNode("USB TRANSPORT", g.Settings.Transport == "wifi" ? "WiFi TLS" : "ADB", Map(active && g.Telemetry.StreamedPackets > 0)),
            new PipelineNode("ANDROID", deviceOk ? "Connected" : "Not connected", Map(deviceOk && (!active || connected))),
        };
        System.Windows.Application.Current?.Dispatcher.Invoke(() =>
        {
            Nodes.Clear();
            foreach (var n in nodes) Nodes.Add(n);
        });

        var t = g.Telemetry;
        TelemetryLine = active || t.StreamedFrames > 0
            ? $"SEQ {t.StreamedFrames:000000}  {t.Codec}  {t.Resolution}  {t.Fps} FPS  pkts={t.StreamedPackets}  stall={t.WriteStallMsMax:F1}ms  input={t.InputEventsInjected}"
            : (g.State == SystemState.Starting
                ? string.Join("  →  ", g.StartupSteps.Select(s => (s.Failed ? "✕ " : s.Done ? "✓ " : "● ") + s.Label))
                : "N/A — start the display to stream telemetry.");
        RaisePropertyChanged(nameof(StatusText));
        RaisePropertyChanged(nameof(TelemetryLine));
    }
}
