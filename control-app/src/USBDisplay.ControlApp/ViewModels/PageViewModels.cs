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
        Refresh();
    }

    public ObservableCollection<DisplayInfo> Displays { get; } = new();
    public RelayCommand OpenSettings { get; }
    public RelayCommand Extend { get; }

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
        Refresh();
    }

    public ObservableCollection<DeviceInfo> Devices { get; } = new();
    public AsyncRelayCommand RefreshDevices { get; }
    public AsyncRelayCommand Reconnect { get; }
    public AsyncRelayCommand RestartServer { get; }

    private string _note = "Connect your Android device over USB and enable USB debugging.";
    public string Note { get => _note; private set => SetProperty(ref _note, value); }

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
        RaisePropertyChanged(nameof(Note));
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
    public RelayCommand Pause { get; }
    public RelayCommand Clear { get; }
    public RelayCommand Copy { get; }
    public RelayCommand Export { get; }
    public string PauseLabel => _paused ? "Resume" : "Pause";

    public string Search { get => _search; set { SetProperty(ref _search, value); Refresh(); } }
    public string LevelFilter { get => _levelFilter; set { SetProperty(ref _levelFilter, value); Refresh(); } }

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
        var items = _gateway.Log.Query(min, Search)
            .Select(e => $"{e.Time:HH:mm:ss} {e.Level.ToString().ToUpperInvariant(),-7} [{e.Source}] {e.Message}")
            .ToArray();
        Application.Current?.Dispatcher.Invoke(() =>
        {
            Visible.Clear();
            foreach (var i in items.TakeLast(500)) Visible.Add(i);
        });
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
