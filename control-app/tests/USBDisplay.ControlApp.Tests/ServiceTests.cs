using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.Services;

namespace USBDisplay.ControlApp.Tests;

[TestClass]
public sealed class ConfigTests
{
    [TestMethod]
    public void Settings_RoundTrip()
    {
        var path = Path.Combine(Path.GetTempPath(), $"usbdisplay-test-{Guid.NewGuid()}.json");
        try
        {
            var config = new ConfigurationService(path);
            Assert.AreEqual("h264", config.Settings.Codec);
            config.Settings.Codec = "h265";
            config.Settings.BitrateBps = 8_000_000;
            config.Save();

            var reloaded = new ConfigurationService(path);
            Assert.AreEqual("h265", reloaded.Settings.Codec);
            Assert.AreEqual(8_000_000, reloaded.Settings.BitrateBps);
        }
        finally
        {
            File.Delete(path);
        }
    }

    [TestMethod]
    public void Settings_CorruptFileFallsBackToDefaults()
    {
        var path = Path.Combine(Path.GetTempPath(), $"usbdisplay-test-{Guid.NewGuid()}.json");
        try
        {
            File.WriteAllText(path, "{not valid json!!!");
            var config = new ConfigurationService(path);
            Assert.AreEqual("h264", config.Settings.Codec);
        }
        finally
        {
            File.Delete(path);
        }
    }
}

[TestClass]
public sealed class LogServiceTests
{
    [TestMethod]
    public void Log_QueryFiltersByLevelAndSearch()
    {
        var log = new LogService();
        log.Log(LogLevel.Info, "a", "hello world");
        log.Log(LogLevel.Error, "b", "boom happened");
        log.Log(LogLevel.Debug, "a", "hello debug");

        Assert.AreEqual(3, log.Query(null, null).Count());
        Assert.AreEqual(1, log.Query(LogLevel.Error, null).Count());
        Assert.AreEqual(2, log.Query(null, "hello").Count());
        Assert.AreEqual(1, log.Query(LogLevel.Warning, "boom").Count());
    }

    [TestMethod]
    public void Log_ExportWritesLines()
    {
        var log = new LogService();
        log.Log(LogLevel.Info, "a", "line one");
        var path = Path.Combine(Path.GetTempPath(), $"usbdisplay-log-{Guid.NewGuid()}.txt");
        try
        {
            log.Export(path);
            var text = File.ReadAllText(path);
            StringAssert.Contains(text, "line one");
        }
        finally
        {
            File.Delete(path);
        }
    }

    [TestMethod]
    public void Log_EntryAddedFires()
    {
        var log = new LogService();
        LogEntry? seen = null;
        log.EntryAdded += (_, e) => seen = e;
        log.Log(LogLevel.Warning, "src", "msg");
        Assert.IsNotNull(seen);
        Assert.AreEqual("msg", seen!.Message);
    }
}

[TestClass]
public sealed class ProcessManagerTests
{
    [TestMethod]
    public void CrashCounting_BoundsRestarts()
    {
        var pm = new ProcessManager();
        Assert.AreEqual(0, pm.CrashCount("streamer"));
        pm.NoteCrash("streamer");
        pm.NoteCrash("streamer");
        pm.NoteCrash("streamer");
        pm.NoteCrash("streamer");
        Assert.AreEqual(4, pm.CrashCount("streamer"));
        Assert.IsTrue(pm.CrashCount("streamer") > ProcessManager.MaxAutoRestarts);
        pm.ResetCrashCount("streamer");
        Assert.AreEqual(0, pm.CrashCount("streamer"));
    }

    [TestMethod]
    public void Snapshot_EmptyWhenNothingTracked()
    {
        var pm = new ProcessManager();
        Assert.AreEqual(0, pm.Snapshot().Count);
    }
}
