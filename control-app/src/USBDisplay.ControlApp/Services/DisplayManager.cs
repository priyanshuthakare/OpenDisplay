using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using System.Windows.Forms;
using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Native;

namespace USBDisplay.ControlApp.Services;

public interface IDisplayManager
{
    IReadOnlyList<DisplayInfo> GetDisplays();
    DisplayInfo? GetUsbDisplay();
    void OpenDisplaySettings();
    void ExtendDisplays();
}

public sealed class DisplayManager : IDisplayManager
{
    public IReadOnlyList<DisplayInfo> GetDisplays()
    {
        var result = new List<DisplayInfo>();
        try
        {
            var adapters = DisplayApi.EnumerateAdapters();
            var screens = Screen.AllScreens;
            foreach (var a in adapters)
            {
                var isUsb = a.DeviceString.IndexOf("USBDisplay", StringComparison.OrdinalIgnoreCase) >= 0
                    || a.DeviceId.IndexOf("USBDisplay", StringComparison.OrdinalIgnoreCase) >= 0;
                var status = a.Active ? "ACTIVE" : "Inactive";
                result.Add(new DisplayInfo(
                    string.IsNullOrWhiteSpace(a.DeviceString) ? a.DeviceName : a.DeviceString,
                    status, (int)a.Width, (int)a.Height, (int)a.Frequency, isUsb, a.Primary));
            }
            // Fallback: if enumeration yields nothing, report WinForms screens.
            if (result.Count == 0)
            {
                foreach (var s in screens)
                {
                    result.Add(new DisplayInfo(s.DeviceName, "ACTIVE",
                        s.Bounds.Width, s.Bounds.Height, 0, false, s.Primary));
                }
            }
        }
        catch (Exception ex)
        {
            result.Add(new DisplayInfo("Display enumeration failed", ex.Message, 0, 0, 0, false, false));
        }
        return result;
    }

    public DisplayInfo? GetUsbDisplay() => GetDisplays().FirstOrDefault(d => d.IsUsbDisplay);

    public void OpenDisplaySettings()
    {
        Process.Start(new ProcessStartInfo("ms-settings:display") { UseShellExecute = true });
    }

    public void ExtendDisplays()
    {
        Process.Start(new ProcessStartInfo("DisplaySwitch.exe", "/extend") { UseShellExecute = true });
    }
}
