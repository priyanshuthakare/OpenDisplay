using System;
using System.Collections.ObjectModel;
using System.Linq;
using System.Threading.Tasks;
using System.Windows;
using Microsoft.Win32;
using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Mvvm;
using USBDisplay.ControlApp.Services;

namespace USBDisplay.ControlApp.ViewModels;

public sealed class DisplayViewModel : ObservableObject
{
    private readonly IUsbDisplayGateway _gateway;
    public DisplayViewModel(IUsbDisplayGateway gateway)
    {
        _gateway = gateway;
        _gateway.Changed += (_, _) => Refresh();
        OpenSettings = new RelayCommand(_ => Safe(() => _gateway.OpenDisplaySettings()));
        Extend = new RelayCommand(_ => Safe(() => _gateway.ExtendDisplays()));
        PurgeCapture = new AsyncRelayCommand(async _ => await PurgeCaptureAsync());
        Refresh();
    }

    private async Task PurgeCaptureAsync()
    {
        try
        {
            var age = TimeSpan.FromSeconds(Math.Max(60, _gateway.Settings.CapturePurgeAgeSeconds));
            var result = await Task.Run(() => CaptureMonitor.PurgeOlderThan(age)).ConfigureAwait(true);
            CaptureNote = result.DeletedFiles == 0
                ? CaptureSummary() + " Nothing old enough to purge."
                : $"Purged {result.DeletedFiles} stale frame(s) ({result.DeletedBytes / 1024} KB). " + CaptureSummary();
        }
        catch (Exception ex)
        {
            CaptureNote = $"Purge failed: {ex.Message}";
        }
    }

    private string CaptureSummary()
    {
        var status = CaptureMonitor.GetStatus();
        if (!status.Exists || status.FileCount == 0)
        {
            return "Capture folder: empty (driver is in-memory; nothing is written to disk).";
        }
        var mb = status.TotalBytes / (1024.0 * 1024.0);
        var age = status.NewestAge.HasValue ? $", newest {status.NewestAge.Value.TotalMinutes:F0} min ago" : "";
        var warn = mb > _gateway.Settings.CaptureWarnMb ? " — OVER BUDGET, purge recommended." : "";
        return $"Capture folder: {status.FileCount} file(s), {mb:F1} MB{age}.{warn}";
    }

    public ObservableCollection<DisplayInfo> Displays { get; } = new();
    public RelayCommand OpenSettings { get; }
    public RelayCommand Extend { get; }
    public AsyncRelayCommand PurgeCapture { get; }

    private string _captureNote = "";
    public string CaptureNote { get => _captureNote; private set => SetProperty(ref _captureNote, value); }

    private static void Safe(Action a)
    {
        try { a(); }
        catch (Exception ex) { MessageBox.Show(ex.Message, "USBDisplay", MessageBoxButton.OK, MessageBoxImage.Warning); }
    }

    public void Refresh()
    {
        Application.Current?.Dispatcher.Invoke(() =>
        {
            Displays.Clear();
            foreach (var d in _gateway.Displays) Displays.Add(d);
        });
        CaptureNote = CaptureSummary();
    }
}

public sealed class DeviceViewModel : ObservableObject
{
    private readonly IUsbDisplayGateway _gateway;
    public DeviceViewModel(IUsbDisplayGateway gateway)
    {
        _gateway = gateway;
        _gateway.Changed += (_, _) => Refresh();
        RefreshDevices = new AsyncRelayCommand(async _ => await _gateway.RefreshAllAsync());
        Reconnect = new AsyncRelayCommand(async _ => await RunAdbAsync("reconnect"));
        RestartServer = new AsyncRelayCommand(async _ => await RunAdbAsync("restart"));
        ConnectWifi = new AsyncRelayCommand(async _ => await ConnectWifiAsync());
        ForgetTrust = new RelayCommand(_ =>
        {
            try { new WifiTrustStore().Forget(); WifiTrustDetail = "Trust cleared. Next Wi-Fi connect needs the PIN + fingerprint again."; }
            catch (Exception ex) { WifiTrustDetail = $"Forget failed: {ex.Message}"; }
            Refresh();
        });
        Refresh();
    }

    public ObservableCollection<DeviceInfo> Devices { get; } = new();
    public AsyncRelayCommand RefreshDevices { get; }
    public AsyncRelayCommand Reconnect { get; }
    public AsyncRelayCommand RestartServer { get; }
    public AsyncRelayCommand ConnectWifi { get; }
    public RelayCommand ForgetTrust { get; }
    public AppSettings Settings => _gateway.Settings;

    private async Task ConnectWifiAsync()
    {
        try
        {
            if (string.IsNullOrWhiteSpace(Settings.DeviceIp))
            {
                Note = "Enter the tablet LAN IP (or paste the full QR JSON from the tablet pair screen) first.";
                return;
            }
            Settings.Transport = "wifi";
            _gateway.SaveSettings();
            RaisePropertyChanged(nameof(Settings));
            await _gateway.StartAsync().ConfigureAwait(true);
        }
        catch (Exception ex)
        {
            Note = ex.Message;
        }
    }

    private string _note = "Connect your Android device over USB and enable USB debugging.";
    public string Note { get => _note; private set => SetProperty(ref _note, value); }

    private string _wifiTrustDetail = "";
    public string WifiTrustDetail { get => _wifiTrustDetail; private set => SetProperty(ref _wifiTrustDetail, value); }

    private string _wifiSecurity = "TLS 1.3 — no plaintext fallback.";
    public string WifiSecurity { get => _wifiSecurity; private set => SetProperty(ref _wifiSecurity, value); }

    private async Task RunAdbAsync(string what)
    {
        try
        {
            await _gateway.RunAdbMaintenanceAsync(what).ConfigureAwait(true);
            await _gateway.RefreshAllAsync().ConfigureAwait(true);
        }
        catch (Exception ex)
        {
            Note = ex.Message;
        }
    }

    public void Refresh()
    {
        Application.Current?.Dispatcher.Invoke(() =>
        {
            Devices.Clear();
            foreach (var d in _gateway.Devices) Devices.Add(d);
        });
        if (_gateway.Devices.Count > 0)
        {
            var d = _gateway.Devices[0];
            Note = d.State == AdbDeviceState.Device
                ? $"{d.Serial} authorized. Transport: {(_gateway.Settings.Transport == "wifi" ? "WiFi TLS" : "ADB compatibility")}."
                : d.State == AdbDeviceState.Unauthorized
                    ? "ADB sees the device, but Android has not authorized this computer. Unlock the tablet and accept the USB debugging prompt."
                    : $"Device state: {d.State}.";
        }
        // Wi-Fi trust panel: explicit, never silent.
        try
        {
            var raw = (_gateway.Settings.DeviceIp ?? "").Trim();
            if (string.IsNullOrWhiteSpace(raw))
            {
                WifiTrustDetail = "Wi-Fi: not paired. Enter the tablet IP or paste its QR JSON to pair.";
            }
            else
            {
                var payload = WifiPairPayload.Parse(raw, _gateway.Settings.WifiPort > 0 ? _gateway.Settings.WifiPort : 27184);
                var store = new WifiTrustStore();
                var trusted = store.FindByIp(payload.Ip);
                var link = _gateway.WifiLink;
                WifiTrustDetail = trusted != null
                    ? $"Device {payload.Ip}:{payload.Port} — Certificate Trusted — fp {FingerprintUtil.Short(trusted.Fingerprint)} — host {HostIdentity.StableHostId()} — PIN verified (reconnect skips PIN, fingerprint still enforced)."
                    : $"Device {payload.Ip}:{payload.Port} — not trusted yet — {(string.IsNullOrWhiteSpace(payload.Fingerprint) ? "no fingerprint on file" : $"QR fp {FingerprintUtil.Short(payload.Fingerprint)}")} — first connect needs the 6-digit PIN.";
                _ = link;
            }
        }
        catch (Exception ex)
        {
            WifiTrustDetail = $"Wi-Fi target error: {ex.Message}";
        }
        WifiSecurity = "TLS 1.3 only — plaintext is never attempted; a changed certificate is never accepted silently (Forget + re-pair). Certificate regen happens on the tablet (pair screen → New certificate, with confirm; resets trust).";
        RaisePropertyChanged(nameof(Note));
        RaisePropertyChanged(nameof(WifiTrustDetail));
        RaisePropertyChanged(nameof(WifiSecurity));
    }
}

public sealed class ServicesViewModel : ObservableObject
{
    private readonly IUsbDisplayGateway _gateway;
    public ServicesViewModel(IUsbDisplayGateway gateway)
    {
        _gateway = gateway;
        _gateway.Changed += (_, _) => Refresh();
        RestartStreamer = new AsyncRelayCommand(async _ => await _gateway.RestartComponentAsync("streamer"));
        Refresh();
    }

    public ObservableCollection<ServiceEntry> ServiceEntries { get; } = new();
    public ObservableCollection<ManagedProcessInfo> Processes { get; } = new();
    public AsyncRelayCommand RestartStreamer { get; }

    public string Explainer => "USBDisplay runs as managed processes, not Windows services. " +
        "SERVICE RUNNING ≠ PROCESS RUNNING ≠ PIPELINE HEALTHY — the dashboard shows verified pipeline health.";

    public void Refresh()
    {
        Application.Current?.Dispatcher.Invoke(() =>
        {
            ServiceEntries.Clear();
            foreach (var s in _gateway.ServiceEntries) ServiceEntries.Add(s);
            Processes.Clear();
            foreach (var p in _gateway.Processes) Processes.Add(p);
        });
    }
}

public sealed class LogsViewModel : ObservableObject
{
    private readonly IUsbDisplayGateway _gateway;
    private string _search = "";
    private string _levelFilter = "ALL";
    private string _subsystemFilter = "All";
    private bool _paused;

    public LogsViewModel(IUsbDisplayGateway gateway)
    {
        _gateway = gateway;
        _gateway.Log.EntryAdded += (_, _) => { if (!_paused) Refresh(); };
        Pause = new RelayCommand(_ => { _paused = !_paused; RaisePropertyChanged(nameof(PauseLabel)); });
        Clear = new RelayCommand(_ => { _gateway.Log.Clear(); Refresh(); });
        Copy = new RelayCommand(_ =>
        {
            try { Clipboard.SetText(string.Join(Environment.NewLine, Visible.Select(Format))); } catch { }
        });
        Export = new RelayCommand(_ =>
        {
            var dlg = new SaveFileDialog
            {
                FileName = $"usbdisplay-log-{DateTime.Now:yyyy-MM-dd}.txt",
                Filter = "Text files (*.txt)|*.txt",
            };
            if (dlg.ShowDialog() == true)
            {
                try { _gateway.Log.Export(dlg.FileName); } catch (Exception ex) { MessageBox.Show(ex.Message); }
            }
        });
        Refresh();
    }

    public ObservableCollection<string> Visible { get; } = new();
    public string[] Levels { get; } = { "ALL", "INFO", "WARNING", "ERROR", "DEBUG" };
    public string[] Subsystems { get; } = { "All", "Driver", "Capture", "Encoder", "USB", "Wi-Fi", "Android", "Protocol", "Control Center", "Streamer", "Transport", "Diagnostics" };
    public RelayCommand Pause { get; }
    public RelayCommand Clear { get; }
    public RelayCommand Copy { get; }
    public RelayCommand Export { get; }
    public string PauseLabel => _paused ? "Resume" : "Pause";

    public string Search { get => _search; set { SetProperty(ref _search, value); Refresh(); } }
    public string LevelFilter { get => _levelFilter; set { SetProperty(ref _levelFilter, value); Refresh(); } }
    public string SubsystemFilter { get => _subsystemFilter; set { SetProperty(ref _subsystemFilter, value); Refresh(); } }

    private static string Format(string s) => s;

    public void Refresh()
    {
        LogLevel? min = LevelFilter switch
        {
            "INFO" => LogLevel.Info,
            "WARNING" => LogLevel.Warning,
            "ERROR" => LogLevel.Error,
            "DEBUG" => LogLevel.Debug,
            _ => null,
        };
        var subsystem = SubsystemFilter;
        var items = _gateway.Log.Query(min, Search)
            .Where(e => MatchesSubsystem(e.Source, subsystem))
            .Select(e => $"{e.Time:HH:mm:ss} {e.Level.ToString().ToUpperInvariant(),-7} [{e.Source}] {e.Message}")
            .ToArray();
        Application.Current?.Dispatcher.Invoke(() =>
        {
            Visible.Clear();
            foreach (var i in items.TakeLast(500)) Visible.Add(i);
        });
    }

    private static bool MatchesSubsystem(string source, string filter)
    {
        if (string.Equals(filter, "All", StringComparison.OrdinalIgnoreCase)) return true;
        var s = source ?? "";
        return filter switch
        {
            "USB" => s.IndexOf("adb", StringComparison.OrdinalIgnoreCase) >= 0 || s.IndexOf("usb", StringComparison.OrdinalIgnoreCase) >= 0 || s.IndexOf("streamer", StringComparison.OrdinalIgnoreCase) >= 0,
            "Wi-Fi" => s.IndexOf("wifi", StringComparison.OrdinalIgnoreCase) >= 0 || s.IndexOf("tls", StringComparison.OrdinalIgnoreCase) >= 0 || s.IndexOf("pair", StringComparison.OrdinalIgnoreCase) >= 0,
            "Control Center" => s.IndexOf("orchestrator", StringComparison.OrdinalIgnoreCase) >= 0 || s.IndexOf("config", StringComparison.OrdinalIgnoreCase) >= 0 || s.IndexOf("app", StringComparison.OrdinalIgnoreCase) >= 0,
            "Encoder" => s.IndexOf("encod", StringComparison.OrdinalIgnoreCase) >= 0 || s.IndexOf("streamer", StringComparison.OrdinalIgnoreCase) >= 0,
            "Protocol" => s.IndexOf("protocol", StringComparison.OrdinalIgnoreCase) >= 0 || s.IndexOf("transport", StringComparison.OrdinalIgnoreCase) >= 0 || s.IndexOf("streamer", StringComparison.OrdinalIgnoreCase) >= 0,
            _ => s.IndexOf(filter, StringComparison.OrdinalIgnoreCase) >= 0,
        };
    }
}

public sealed class DiagnosticsViewModel : ObservableObject
{
    private readonly IUsbDisplayGateway _gateway;
    public DiagnosticsViewModel(IUsbDisplayGateway gateway)
    {
        _gateway = gateway;
        _gateway.Changed += (_, _) => Refresh();
        Run = new AsyncRelayCommand(async _ =>
        {
            var progress = new Progress<DiagnosticResult>(_ => Refresh());
            await _gateway.RunDiagnosticsAsync(progress).ConfigureAwait(true);
        });
        Refresh();
    }

    public ObservableCollection<DiagnosticResult> Results { get; } = new();
    public AsyncRelayCommand Run { get; }

    private string _summary = "Not run yet.";
    public string Summary { get => _summary; private set => SetProperty(ref _summary, value); }

    public void Refresh()
    {
        Application.Current?.Dispatcher.Invoke(() =>
        {
            Results.Clear();
            foreach (var r in _gateway.LastDiagnostics) Results.Add(r);
        });
        var fails = _gateway.LastDiagnostics.Count(r => r.Status == DiagnosticStatus.Fail);
        Summary = _gateway.LastDiagnostics.Count == 0
            ? "Not run yet."
            : fails == 0 ? "RESULT: SYSTEM READY" : $"RESULT: {fails} check(s) failed — see remediation below.";
        RaisePropertyChanged(nameof(Summary));
    }
}

public sealed class SettingsViewModel : ObservableObject
{
    private readonly IUsbDisplayGateway _gateway;
    public SettingsViewModel(IUsbDisplayGateway gateway)
    {
        _gateway = gateway;
        Save = new RelayCommand(_ =>
        {
            _gateway.SaveSettings();
            SavedNote = $"Saved {DateTime.Now:HH:mm:ss}.";
        });
        ToggleStartup = new RelayCommand(_ => SetStartup(Settings.LaunchAtStartup));
        Features = new ObservableCollection<FeatureAvailability>(FeatureMatrix.Get());
    }

    public AppSettings Settings => _gateway.Settings;
    public ObservableCollection<FeatureAvailability> Features { get; }
    public RelayCommand Save { get; }
    public RelayCommand ToggleStartup { get; }

    private string _savedNote = "";
    public string SavedNote { get => _savedNote; private set => SetProperty(ref _savedNote, value); }

    public string About =>
        $"Control App {AppVersion()}   Driver {(_gateway.Driver?.DriverVersion ?? "—")}   " +
        $"Protocol v1 (USBD/USBT)   Transport 27183 USB / 27184 WiFi-TLS";

    private static string AppVersion() =>
        typeof(SettingsViewModel).Assembly.GetName().Version?.ToString(3) ?? "0.3.0";

    private static void SetStartup(bool enable)
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(
                @"Software\Microsoft\Windows\CurrentVersion\Run", writable: true);
            if (enable)
            {
                var exe = Environment.ProcessPath ?? "";
                if (exe.Length > 0) key?.SetValue("USBDisplayControlCenter", $"\"{exe}\" --minimized");
            }
            else
            {
                key?.DeleteValue("USBDisplayControlCenter", throwOnMissingValue: false);
            }
        }
        catch (Exception ex)
        {
            MessageBox.Show($"Could not update startup setting: {ex.Message}");
        }
    }
}
