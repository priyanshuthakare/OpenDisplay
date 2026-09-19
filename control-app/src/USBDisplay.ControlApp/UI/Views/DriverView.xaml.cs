using System.Windows;
using System.Windows.Controls;
using USBDisplay.ControlApp.ViewModels;

namespace USBDisplay.ControlApp.UI.Views;

public partial class DriverView : UserControl
{
    public DriverView()
    {
        InitializeComponent();
    }

    private void Disable_Click(object sender, RoutedEventArgs e)
    {
        var answer = MessageBox.Show(
            "Disabling the USBDisplay driver will remove the virtual monitor from Windows until the driver is enabled again.\n\nDisable the driver?",
            "USBDisplay", MessageBoxButton.YesNo, MessageBoxImage.Warning);
        if (answer == MessageBoxResult.Yes && DataContext is DriverViewModel vm)
        {
            vm.Disable.Execute(null);
        }
    }

    private void Uninstall_Click(object sender, RoutedEventArgs e)
    {
        var answer = MessageBox.Show(
            "Remove USBDisplay driver?\n\nThis will remove the USBDisplay virtual display driver from Windows. Your Android device will no longer appear as a USBDisplay monitor until the driver is installed again.",
            "USBDisplay", MessageBoxButton.YesNo, MessageBoxImage.Warning);
        if (answer == MessageBoxResult.Yes && DataContext is DriverViewModel vm)
        {
            vm.Uninstall.Execute(null);
        }
    }
}
