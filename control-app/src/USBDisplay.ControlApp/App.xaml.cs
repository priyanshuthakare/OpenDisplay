using System;
using System.Linq;
using System.Windows;
using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Services;
using USBDisplay.ControlApp.UI;
using USBDisplay.ControlApp.ViewModels;

namespace USBDisplay.ControlApp;

/// <summary>
/// Composition root. Manual wiring (no DI container dependency):
/// config → log → runner/elevation → cli/adb/drivers/display/processes →
/// diagnostics → gateway (real or mock) → MainViewModel → MainWindow + tray.
/// </summary>
public partial class App : Application
{
    private IUsbDisplayGateway? _gateway;
    private System.Windows.Forms.NotifyIcon? _tray;
    private MainViewModel? _main;

    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);
        var args = e.Args ?? Array.Empty<string>();
        var forceDemo = args.Any(a => a.Equals("--demo", StringComparison.OrdinalIgnoreCase));
        var minimized = args.Any(a => a.Equals("--minimized", StringComparison.OrdinalIgnoreCase));

        var config = new ConfigurationService();
        if (forceDemo)
        {
            config.Settings.DemoMode = true;
        }
        var log = new LogService();
        log.Log(LogLevel.Info, "app", "USBDisplay Control Center starting.");

        IUsbDisplayGateway gateway;
        if (config.Settings.DemoMode)
        {
            gateway = new MockUsbDisplayGateway(config, log);
            log.Log(LogLevel.Warning, "app", "DEMO MODE — all status is simulated.");
        }
        else
        {
            var runner = new ProcessRunner();
            var elevation = new ElevationService();
            var streamer = new StreamerCli(runner, config);
            var adb = new AdbClient(runner, config);
            var drivers = new DriverManager(runner, elevation, config);
            var displays = new DisplayManager();
            var processes = new ProcessManager();
            var diagnostics = new DiagnosticsService(elevation, drivers, streamer, adb, displays, config);
            gateway = new RealUsbDisplayGateway(config, elevation, drivers, streamer, adb, displays, processes, diagnostics, log);
        }
        _gateway = gateway;
        _main = new MainViewModel(gateway);

        var window = new MainWindow(_main);
        MainWindow = window;
        SetupTray(window);
        if (minimized && config.Settings.MinimizeToTray)
        {
            window.Hide();
        }
        else
        {
            window.Show();
        }
    }

    private void SetupTray(Window window)
    {
        if (_gateway == null || _main == null)
        {
            return;
        }
        var gateway = _gateway;
        _tray = new System.Windows.Forms.NotifyIcon
        {
            Icon = System.Drawing.SystemIcons.Application,
            Visible = true,
        };
        var menu = new System.Windows.Forms.ContextMenuStrip();
        menu.Items.Add("Open Dashboard", null, (_, _) => { window.Show(); window.Activate(); });
        menu.Items.Add("Stop Display", null, async (_, _) => await gateway.StopAsync());
        menu.Items.Add("Restart", null, async (_, _) => await gateway.RestartAsync());
        menu.Items.Add("Diagnostics", null, async (_, _) =>
        {
            window.Show();
            _main.CurrentPage = "Diagnostics";
            await gateway.RunDiagnosticsAsync();
        });
        menu.Items.Add("Exit", null, (_, _) =>
        {
            if (window is MainWindow mw) mw.ExitExplicit();
            else Shutdown();
        });
        _tray.ContextMenuStrip = menu;
        _tray.DoubleClick += (_, _) => { window.Show(); window.Activate(); };
        gateway.Changed += (_, _) =>
        {
            if (_tray != null)
            {
                _tray.Text = $"USBDisplay — {gateway.State}"; // 63-char limit respected
            }
        };
    }

    protected override void OnExit(ExitEventArgs e)
    {
        try
        {
            if (_tray != null)
            {
                _tray.Visible = false;
                _tray.Dispose();
            }
            (_gateway as IDisposable)?.Dispose();
        }
        catch { }
        base.OnExit(e);
    }
}
