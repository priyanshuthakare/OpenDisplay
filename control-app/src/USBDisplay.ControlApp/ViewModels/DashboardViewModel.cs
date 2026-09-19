using System;
using System.Collections.ObjectModel;
using System.Linq;
using System.Threading.Tasks;
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
        StartUsb = new AsyncRelayCommand(async _ => await ConnectAsync("usb"));
        StartWifi = new AsyncRelayCommand(async _ => await ConnectAsync("wifi"));
        SwitchToUsb = new AsyncRelayCommand(async _ => await _gateway.SwitchTransportAsync(TransportKind.Usb));
        SwitchToWifi = new AsyncRelayCommand(async _ => await _gateway.SwitchTransportAsync(TransportKind.Wifi));
        TogglePairingQr = new RelayCommand(_ => TogglePairingCode());
        OpenNode = new RelayCommand(p => NodeHint = p is string s ? $"Selected {s} — open Diagnostics for detail." : "");
        ForgetTrust = new RelayCommand(_ =>
        {
            try { new WifiTrustStore().Forget(); TrustNote = "Trust cleared. Next Wi-Fi connect needs the PIN + fingerprint again."; }
            catch (Exception ex) { TrustNote = $"Forget failed: {ex.Message}"; }
            Refresh();
        });
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
    public AsyncRelayCommand StartUsb { get; }
    public AsyncRelayCommand StartWifi { get; }
    public AsyncRelayCommand SwitchToUsb { get; }
    public AsyncRelayCommand SwitchToWifi { get; }
    public RelayCommand TogglePairingQr { get; }
    public RelayCommand OpenNode { get; }
    public RelayCommand ForgetTrust { get; }

    public AppSettings Settings => _gateway.Settings;

    private string _connectionNote = "";
    public string ConnectionNote { get => _connectionNote; private set => SetProperty(ref _connectionNote, value); }

    private string _nodeHint = "Select a pipeline node for its diagnostics page.";
    public string NodeHint { get => _nodeHint; private set => SetProperty(ref _nodeHint, value); }

    private string _transportSummary = "";
    public string TransportSummary { get => _transportSummary; private set => SetProperty(ref _transportSummary, value); }

    private string _usbStatus = "";
    public string UsbStatus { get => _usbStatus; private set => SetProperty(ref _usbStatus, value); }

    private string _wifiStatus = "";
    public string WifiStatus { get => _wifiStatus; private set => SetProperty(ref _wifiStatus, value); }

    private string _wifiTrust = "";
    public string WifiTrust { get => _wifiTrust; private set => SetProperty(ref _wifiTrust, value); }

    private string _pinNote = "";
    public string PinNote { get => _pinNote; private set => SetProperty(ref _pinNote, value); }

    private string _trustNote = "";
    public string TrustNote { get => _trustNote; private set => SetProperty(ref _trustNote, value); }

    private string _errorUser = "";
    public string ErrorUser { get => _errorUser; private set => SetProperty(ref _errorUser, value); }

    private string _errorExplanation = "";
    public string ErrorExplanation { get => _errorExplanation; private set => SetProperty(ref _errorExplanation, value); }

    private string _errorTechnical = "";
    public string ErrorTechnical { get => _errorTechnical; private set => SetProperty(ref _errorTechnical, value); }

    public bool HasError => !string.IsNullOrWhiteSpace(ErrorUser);

    private System.Windows.Media.Imaging.BitmapImage? _pairingQr;
    public System.Windows.Media.Imaging.BitmapImage? PairingQr
    {
        get => _pairingQr;
        private set => SetProperty(ref _pairingQr, value);
    }

    private bool _showPairingQr;
    public bool ShowPairingQr { get => _showPairingQr; private set => SetProperty(ref _showPairingQr, value); }

    private string _pairingQrCaption = "";
    public string PairingQrCaption { get => _pairingQrCaption; private set => SetProperty(ref _pairingQrCaption, value); }

    public void TogglePairingCode()
    {
        if (ShowPairingQr)
        {
            ShowPairingQr = false;
            PairingQr = null;
            return;
        }
        try
        {
            var hostId = Services.HostIdentity.StableHostId();
            var payload = Services.HostIdentity.BuildPairingQrPayload(hostId);
            PairingQr = Services.QrCodeService.ToBitmapImage(Services.QrCodeService.RenderPng(payload));
            PairingQrCaption = $"Scan with the tablet (WiFi Pair → Scan PC). Trusts {hostId} — then CONNECT WI-FI needs no PIN.";
            ShowPairingQr = true;
            ConnectionNote = "";
        }
        catch (Exception ex)
        {
            ConnectionNote = $"Could not generate pairing code: {ex.Message}";
        }
    }

    public async Task ConnectAsync(string transport)
    {
        try
        {
            ConnectionNote = "";
            if (transport == "wifi" && string.IsNullOrWhiteSpace(Settings.DeviceIp))
            {
                ConnectionNote = "Enter the tablet LAN IP (or paste the QR JSON) first.";
                return;
            }
            Settings.Transport = transport;
            _gateway.SaveSettings();
            RaisePropertyChanged(nameof(Settings));
            await _gateway.StartAsync().ConfigureAwait(true);
        }
        catch (Exception ex)
        {
            ConnectionNote = ex.Message;
        }
    }

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

        var isWifi = string.Equals(g.Settings.Transport, "wifi", StringComparison.OrdinalIgnoreCase);
        var usbConnected = g.Devices.Any(d => d.State == AdbDeviceState.Device);
        UsbStatus = usbConnected ? "● USB — ADB bridge connected" : "○ USB — available (connect tablet to use)";
        WifiStatus = string.IsNullOrWhiteSpace(g.Settings.DeviceIp)
            ? "○ Wi-Fi — TLS 1.3 available (enter tablet IP to pair)"
            : (g.WifiLink?.Trusted == true ? $"● Wi-Fi — {g.Settings.DeviceIp} trusted" : $"○ Wi-Fi — {g.Settings.DeviceIp} (unpaired)");
        TransportSummary = isWifi ? $"Active transport: Wi-Fi ({g.Settings.DeviceIp}:{g.Settings.WifiPort} TLS 1.3)" : $"Active transport: USB (127.0.0.1:{g.Settings.UsbPort} ADB)";

        try
        {
            var raw = (g.Settings.DeviceIp ?? "").Trim();
            if (!string.IsNullOrWhiteSpace(raw))
            {
                var payload = WifiPairPayload.Parse(raw, g.Settings.WifiPort > 0 ? g.Settings.WifiPort : 27184);
                var store = new WifiTrustStore();
                var trusted = store.FindByIp(payload.Ip);
                WifiTrust = trusted != null
                    ? $"Device trusted — fp {FingerprintUtil.Short(trusted.Fingerprint)} — host {HostIdentity.StableHostId()} — PIN verified (fingerprint still enforced)."
                    : (!string.IsNullOrWhiteSpace(payload.Fingerprint)
                        ? $"Unpaired — QR fp {FingerprintUtil.Short(payload.Fingerprint)} — first connect needs the 6-digit PIN."
                        : "Unpaired — first connect needs the 6-digit PIN from the tablet pair screen.");
                PinNote = string.IsNullOrWhiteSpace(g.Settings.Pin)
                    ? "PIN omitted — allowed only for trusted-host reconnect (fingerprint still enforced)."
                    : FingerprintUtil.IsValidPin(g.Settings.Pin) ? "PIN format OK (6 digits). Attempts remaining: 3." : "PIN must be six digits.";
            }
            else
            {
                WifiTrust = "Not paired — open Device → Wi-Fi Pairing or enter the tablet IP above.";
                PinNote = "6-digit PIN from the tablet pair screen. 3 strikes → 30 s lockout (tablet-enforced).";
            }
        }
        catch (Exception ex)
        {
            WifiTrust = $"Target error: {ex.Message}";
            PinNote = "";
        }

        var err = g.LastError;
        ErrorUser = err?.UserMessage ?? "";
        ErrorExplanation = err?.Explanation ?? "";
        ErrorTechnical = err?.TechnicalDetail ?? "";
        RaisePropertyChanged(nameof(HasError));

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
        var wifiExtra = isWifi && g.WifiLink != null
            ? $"  TLS {g.WifiLink.Security} peer={g.WifiLink.Peer} adapt={g.WifiLink.Adaptation}"
            : "";
        TelemetryLine = active || t.StreamedFrames > 0
            ? $"SEQ {t.StreamedFrames:000000}  {t.Codec}  {t.Resolution}  {t.Fps} FPS  pkts={t.StreamedPackets}  stall={t.WriteStallMsMax:F1}ms  input={t.InputEventsInjected}{wifiExtra}"
            : (g.State == SystemState.Starting
                ? string.Join("  →  ", g.StartupSteps.Select(s => (s.Failed ? "✕ " : s.Done ? "✓ " : "● ") + s.Label))
                : "N/A — start the display to stream telemetry.");
        RaisePropertyChanged(nameof(StatusText));
        RaisePropertyChanged(nameof(TelemetryLine));
        RaisePropertyChanged(nameof(TransportSummary));
        RaisePropertyChanged(nameof(UsbStatus));
        RaisePropertyChanged(nameof(WifiStatus));
        RaisePropertyChanged(nameof(WifiTrust));
        RaisePropertyChanged(nameof(PinNote));
        RaisePropertyChanged(nameof(ErrorUser));
    }
}
