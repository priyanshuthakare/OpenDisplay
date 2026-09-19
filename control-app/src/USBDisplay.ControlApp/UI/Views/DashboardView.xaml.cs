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
                Monitor.NodeClicked += (_, node) =>
                {
                    if (vm.OpenNode.CanExecute(node)) vm.OpenNode.Execute(node);
                };
                // PropertyChanged arrives on background threads (orchestrator
                // awaits with ConfigureAwait(false), timers). Dependency
                // properties must be touched on the UI thread — skipping the
                // marshal throws InvalidOperationException and masks the real
                // connection result.
                vm.PropertyChanged += (_, args) =>
                {
                    if (args.PropertyName == nameof(DashboardViewModel.StatusText))
                    {
                        Dispatcher.Invoke(() =>
                        {
                            Monitor.IsActive = vm.StatusText is "ACTIVE" or "STARTING";
                            Monitor.Nodes = vm.Nodes;
                        });
                    }
                };
                Monitor.IsActive = vm.StatusText is "ACTIVE" or "STARTING";
            }
        };
    }
}
