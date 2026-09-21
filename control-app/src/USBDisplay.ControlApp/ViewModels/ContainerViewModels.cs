using USBDisplay.ControlApp.Mvvm;

namespace USBDisplay.ControlApp.ViewModels;

/// <summary>
/// Groups the driver install and device-connection surfaces under one "Setup"
/// page so the shell has a simple three-item navigation (Home / Setup /
/// Advanced) instead of eight top-level pages. The leaf view models are
/// unchanged and constructed once by <see cref="MainViewModel"/>; this type
/// only re-hosts them as tabs.
/// </summary>
public sealed class SetupViewModel
{
    public SetupViewModel(DriverViewModel driver, DeviceViewModel device)
    {
        Driver = driver;
        Device = device;
    }

    public DriverViewModel Driver { get; }
    public DeviceViewModel Device { get; }
}

/// <summary>
/// Groups the display, services, logs, diagnostics, and settings surfaces under
/// one "Advanced" page (tabs). <see cref="SelectedIndex"/> lets the shell jump
/// straight to a tab — e.g. the tray "Diagnostics" action.
/// </summary>
public sealed class AdvancedViewModel : ObservableObject
{
    /// <summary>Tab index of the Diagnostics surface, for programmatic jumps.</summary>
    public const int DiagnosticsTabIndex = 3;

    private int _selectedIndex;

    public AdvancedViewModel(
        DisplayViewModel display,
        ServicesViewModel services,
        LogsViewModel logs,
        DiagnosticsViewModel diagnostics,
        SettingsViewModel settings)
    {
        Display = display;
        Services = services;
        Logs = logs;
        Diagnostics = diagnostics;
        Settings = settings;
    }

    public DisplayViewModel Display { get; }
    public ServicesViewModel Services { get; }
    public LogsViewModel Logs { get; }
    public DiagnosticsViewModel Diagnostics { get; }
    public SettingsViewModel Settings { get; }

    public int SelectedIndex
    {
        get => _selectedIndex;
        set => SetProperty(ref _selectedIndex, value);
    }
}
