using System.Threading.Tasks;
using System.Windows;
using System.Windows.Input;
using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Mvvm;
using USBDisplay.ControlApp.Services;

namespace USBDisplay.ControlApp.ViewModels;

public sealed class MainViewModel : ObservableObject
{
    private readonly IUsbDisplayGateway _gateway;
    private string _currentPage = "Dashboard";

    public MainViewModel(IUsbDisplayGateway gateway)
    {
        _gateway = gateway;
        _gateway.Changed += (_, _) => RefreshFromGateway();
        Dashboard = new DashboardViewModel(gateway);
        Driver = new DriverViewModel(gateway);
        Display = new DisplayViewModel(gateway);
        Device = new DeviceViewModel(gateway);
        Services = new ServicesViewModel(gateway);
        Logs = new LogsViewModel(gateway);
        Diagnostics = new DiagnosticsViewModel(gateway);
        Settings = new SettingsViewModel(gateway);
        Navigate = new RelayCommand(p => CurrentPage = p as string ?? "Dashboard");
        Start = new AsyncRelayCommand(async _ => await _gateway.StartAsync());
        Stop = new AsyncRelayCommand(async _ => await _gateway.StopAsync());
        Restart = new AsyncRelayCommand(async _ => await _gateway.RestartAsync());
        Refresh = new AsyncRelayCommand(async _ => await _gateway.RefreshAllAsync());
        CloseFirstRun = new RelayCommand(_ =>
        {
            _gateway.Settings.FirstRunDone = true;
            RaisePropertyChanged(nameof(ShowFirstRun));
        });
        RefreshFromGateway();
        _ = _gateway.RefreshAllAsync();
    }

    public DashboardViewModel Dashboard { get; }
    public DriverViewModel Driver { get; }
    public DisplayViewModel Display { get; }
    public DeviceViewModel Device { get; }
    public ServicesViewModel Services { get; }
    public LogsViewModel Logs { get; }
    public DiagnosticsViewModel Diagnostics { get; }
    public SettingsViewModel Settings { get; }

    public RelayCommand Navigate { get; }
    public AsyncRelayCommand Start { get; }
    public AsyncRelayCommand Stop { get; }
    public AsyncRelayCommand Restart { get; }
    public AsyncRelayCommand Refresh { get; }
    public RelayCommand CloseFirstRun { get; }

    public string CurrentPage
    {
        get => _currentPage;
        set
        {
            if (SetProperty(ref _currentPage, value))
            {
                RaisePropertyChanged(nameof(CurrentViewModel));
            }
        }
    }

    public object CurrentViewModel => CurrentPage switch
    {
        "Driver" => Driver,
        "Display" => Display,
        "Device" => Device,
        "Services" => Services,
        "Logs" => Logs,
        "Diagnostics" => Diagnostics,
        "Settings" => Settings,
        _ => Dashboard,
    };

    public bool ShowFirstRun => !_gateway.Settings.FirstRunDone;
    public bool DemoMode => _gateway.DemoMode;
    public string AdminBadge => _gateway.IsElevated ? "✓ Elevated" : "⚠ User mode";
    public string AdminDetail => _gateway.IsElevated
        ? "ADMINISTRATOR"
        : "USER MODE — admin actions will prompt";

    public string SystemBadge => _gateway.State switch
    {
        SystemState.Active => "● DISPLAY ACTIVE",
        SystemState.Starting => "● STARTING",
        SystemState.Stopping => "● STOPPING",
        SystemState.Error => "● ERROR",
        _ => DeviceConnected ? "● SYSTEM READY" : "● DEVICE NOT CONNECTED",
    };

    private bool DeviceConnected =>
        _gateway.State == SystemState.Active ||
        System.Linq.Enumerable.Any(_gateway.Devices,
            d => d.State == AdbDeviceState.Device);

    private void RefreshFromGateway()
    {
        RaisePropertyChanged(nameof(SystemBadge));
        RaisePropertyChanged(nameof(AdminBadge));
        RaisePropertyChanged(nameof(AdminDetail));
        RaisePropertyChanged(nameof(ShowFirstRun));
        RaisePropertyChanged(nameof(DemoMode));
        if (System.Windows.Application.Current?.Dispatcher is { } dispatcher && !dispatcher.CheckAccess())
        {
            dispatcher.Invoke(() => CommandManager.InvalidateRequerySuggested());
        }
        else
        {
            CommandManager.InvalidateRequerySuggested();
        }
    }
}
