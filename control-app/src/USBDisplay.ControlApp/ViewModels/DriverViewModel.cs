using System.Threading.Tasks;
using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Mvvm;
using USBDisplay.ControlApp.Services;

namespace USBDisplay.ControlApp.ViewModels;

/// <summary>
/// Driver lifecycle. Stop ≠ Disable ≠ Uninstall — separate commands, and the
/// view confirms danger-zone actions before invoking them.
/// </summary>
public sealed class DriverViewModel : ObservableObject
{
    private readonly IUsbDisplayGateway _gateway;

    public DriverViewModel(IUsbDisplayGateway gateway)
    {
        _gateway = gateway;
        _gateway.Changed += (_, _) => Refresh();
        Install = new AsyncRelayCommand(async _ => await _gateway.DriverInstallAsync());
        Restart = new AsyncRelayCommand(async _ => await _gateway.DriverRestartAsync());
        Enable = new AsyncRelayCommand(async _ => await _gateway.DriverSetEnabledAsync(true));
        Disable = new AsyncRelayCommand(async _ => await _gateway.DriverSetEnabledAsync(false));
        Uninstall = new AsyncRelayCommand(async _ => await _gateway.DriverUninstallAsync());
        RefreshDisplay = new AsyncRelayCommand(async _ => await _gateway.RefreshAllAsync());
        Refresh();
    }

    public AsyncRelayCommand Install { get; }
    public AsyncRelayCommand Restart { get; }
    public AsyncRelayCommand Enable { get; }
    public AsyncRelayCommand Disable { get; }
    public AsyncRelayCommand Uninstall { get; }
    public AsyncRelayCommand RefreshDisplay { get; }

    private DriverInfo? _driver;
    public DriverInfo? Driver { get => _driver; private set => SetProperty(ref _driver, value); }

    public string ProblemMeaning => Driver?.ProblemCode switch
    {
        null or 0 => "OK (no problem)",
        28 => "CM_PROB_FAILED_INSTALL (28) — no driver installed / device not bound",
        31 => "CM_PROB_FAILED_ADD (31) — driver present but failed to start",
        37 => "CM_PROB_FAILED_DRIVER_ENTRY (37) — DriverEntry/DeviceAdd failed",
        39 => "CM_PROB_FAILED_LOAD (39) — driver could not be loaded",
        41 => "CM_PROB_FAILED_START (41) — loaded but device not started",
        52 => "CM_PROB_UNSIGNED_DRIVER (52) — signature not trusted (test signing on + rebooted?)",
        var c => $"PnP problem code {c}",
    };

    public void Refresh()
    {
        Driver = _gateway.Driver;
        RaisePropertyChanged(nameof(ProblemMeaning));
    }

    public Task RefreshAsync() => _gateway.RefreshAllAsync();
}
