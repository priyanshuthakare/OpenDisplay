using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Services;
using USBDisplay.ControlApp.ViewModels;

namespace USBDisplay.ControlApp.Tests;

[TestClass]
public sealed class DashboardTests
{
    private static (MockUsbDisplayGateway Gateway, DashboardViewModel Vm) Create()
    {
        var path = Path.Combine(Path.GetTempPath(), $"usbdisplay-test-{Guid.NewGuid()}.json");
        var config = new ConfigurationService(path);
        var gateway = new MockUsbDisplayGateway(config, new LogService());
        return (gateway, new DashboardViewModel(gateway));
    }

    [TestMethod]
    public async Task ConnectWifi_RequiresDeviceIp()
    {
        var (_, vm) = Create();
        vm.Settings.DeviceIp = "";
        await vm.ConnectAsync("wifi");
        Assert.IsFalse(string.IsNullOrWhiteSpace(vm.ConnectionNote));
    }

    [TestMethod]
    public async Task ConnectWifi_StartsWithTransportWifi()
    {
        var (gateway, vm) = Create();
        vm.Settings.DeviceIp = "192.168.1.42";
        vm.Settings.Pin = "123456";
        await vm.ConnectAsync("wifi");
        Assert.AreEqual(SystemState.Active, gateway.State);
        Assert.AreEqual("wifi", gateway.Settings.Transport);
    }

    [TestMethod]
    public async Task ConnectUsb_SwitchesTransport()
    {
        var (gateway, vm) = Create();
        gateway.Settings.Transport = "wifi";
        await vm.ConnectAsync("usb");
        Assert.AreEqual(SystemState.Active, gateway.State);
        Assert.AreEqual("usb", gateway.Settings.Transport);
    }
}
