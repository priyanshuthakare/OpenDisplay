using System.Windows.Controls;
using USBDisplay.ControlApp.ViewModels;

namespace USBDisplay.ControlApp.UI.Views;

public partial class DashboardView : UserControl
{
    public DashboardView()
    {
        InitializeComponent();
        DataContextChanged += (_, e) =>
        {
            if (e.NewValue is DashboardViewModel vm)
            {
                Monitor.Nodes = vm.Nodes;
                vm.PropertyChanged += (_, args) =>
                {
                    if (args.PropertyName == nameof(DashboardViewModel.StatusText))
                    {
                        Monitor.IsActive = vm.StatusText is "ACTIVE" or "STARTING";
                        Monitor.Nodes = vm.Nodes;
                    }
                };
                Monitor.IsActive = vm.StatusText is "ACTIVE" or "STARTING";
            }
        };
    }
}
