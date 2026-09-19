using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Parsing;

namespace USBDisplay.ControlApp.Tests;

[TestClass]
public sealed class ParsingTests
{
    [TestMethod]
    public void KeyValues_ParsesStreamerLines()
    {
        const string output = "wifi_device=192.168.1.42:27184\nwifi_encryption=tls-1.3\nandroid_connection=established\n";
        var map = CliOutputParser.ParseKeyValues(output);
        Assert.AreEqual("192.168.1.42:27184", map["wifi_device"]);
        Assert.AreEqual("tls-1.3", map["wifi_encryption"]);
        Assert.AreEqual("established", map["android_connection"]);
    }

    [TestMethod]
    public void KeyValues_IgnoresGarbageAndFoldsStatsJson()
    {
        const string output = "backend MediaFoundation: SELECTED\n{\"streamed_frames\":60,\"streamed_packets\":240,\"write_stall_ms_max\":3.21,\"input_events_injected\":7}\n";
        var map = CliOutputParser.ParseKeyValues(output);
        Assert.AreEqual("60", map["streamed_frames"]);
        Assert.AreEqual("240", map["streamed_packets"]);
        Assert.AreEqual("7", map["input_events_injected"]);
        Assert.IsFalse(map.ContainsKey("backend MediaFoundation: SELECTED"));
    }

    [TestMethod]
    public void KeyValues_EmptyIsEmpty()
    {
        Assert.AreEqual(0, CliOutputParser.ParseKeyValues("").Count);
        Assert.AreEqual(0, CliOutputParser.ParseKeyValues("   \n").Count);
    }

    [TestMethod]
    public void AdbDevices_ParsesAllStates()
    {
        const string output = "List of devices attached\n" +
            "ABCD1234\tdevice product:marlin model:Pixel_XL device:marlin transport_id:1\n" +
            "EFGH5678\tunauthorized\n" +
            "IJKL9012\toffline\n";
        var devs = CliOutputParser.ParseAdbDevices(output);
        Assert.AreEqual(3, devs.Count);
        Assert.AreEqual(AdbDeviceState.Device, devs[0].State);
        Assert.AreEqual("Pixel_XL", devs[0].Model);
        Assert.AreEqual(AdbDeviceState.Unauthorized, devs[1].State);
        Assert.AreEqual(AdbDeviceState.Offline, devs[2].State);
    }

    [TestMethod]
    public void AdbDevices_EmptyList()
    {
        Assert.AreEqual(0, CliOutputParser.ParseAdbDevices("List of devices attached\n\n").Count);
    }

    [TestMethod]
    public void Pnputil_FindsPublishedNameByToken()
    {
        const string output = "Published Name :     oem42.inf\n" +
            "Original Name  :     Driver.inf\n" +
            "Provider Name  :     USBDisplay\n" +
            "Class Name     :     Display\n" +
            "\n" +
            "Published Name :     oem7.inf\n" +
            "Provider Name  :     Other\n" +
            "\n";
        var names = CliOutputParser.ParsePnputilPublishedNames(output, "USBDisplay");
        CollectionAssert.AreEqual(new[] { "oem42.inf" }, names);
    }

    [TestMethod]
    public void Pnputil_NoMatchIsEmpty()
    {
        Assert.AreEqual(0, CliOutputParser.ParsePnputilPublishedNames("Published Name : oem1.inf\n\n", "USBDisplay").Count);
    }
}
