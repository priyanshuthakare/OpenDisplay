using System;
using System.ComponentModel;
using System.Windows;
using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.ViewModels;

namespace USBDisplay.ControlApp.UI;

public partial class MainWindow : Window
{
    private readonly MainViewModel _vm;
    private bool _explicitExit;

    public MainWindow(MainViewModel vm)
    {
        _vm = vm;
        DataContext = vm;
        InitializeComponent();
    }

    public void ExitExplicit()
    {
        _explicitExit = true;
        Close();
    }

    protected override void OnClosing(CancelEventArgs e)
    {
        var settings = _vm.Settings.Settings;
        // Minimize-to-tray instead of closing, unless explicitly exiting.
        if (!_explicitExit && settings.MinimizeToTray)
        {
            e.Cancel = true;
            Hide();
            return;
        }
        // Never allow accidental exit mid-stream: confirm, then stop first.
        if (_vm.Dashboard.StatusText is "ACTIVE" or "STARTING")
        {
            var answer = MessageBox.Show(
                "USBDisplay is currently streaming. Exiting stops the active session (the streamer is a child process).\n\nExit and stop the display?",
                "USBDisplay", MessageBoxButton.YesNo, MessageBoxImage.Warning);
            if (answer != MessageBoxResult.Yes)
            {
                e.Cancel = true;
                _explicitExit = false;
                return;
            }
            try { _vm.Stop.Execute(null); } catch { }
        }
        base.OnClosing(e);
    }
}
