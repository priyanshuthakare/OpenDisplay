using USBDisplay.ControlApp.Services;

namespace USBDisplay.ControlApp.Tests;

[TestClass]
public sealed class CaptureMonitorTests
{
    private static string NewTempDir()
    {
        var dir = Path.Combine(Path.GetTempPath(), $"usbdisplay-cap-{Guid.NewGuid()}");
        Directory.CreateDirectory(dir);
        return dir;
    }

    private static void WriteBmp(string dir, string name, int bytes, TimeSpan age)
    {
        var path = Path.Combine(dir, name);
        File.WriteAllBytes(path, new byte[bytes]);
        File.SetLastWriteTimeUtc(path, DateTime.UtcNow - age);
    }

    [TestMethod]
    public void Status_MissingDirIsEmpty()
    {
        var status = CaptureMonitor.GetStatus(Path.Combine(Path.GetTempPath(), $"usbdisplay-nope-{Guid.NewGuid()}"));
        Assert.IsFalse(status.Exists);
        Assert.AreEqual(0, status.FileCount);
    }

    [TestMethod]
    public void Status_CountsOnlyCaptureBmps()
    {
        var dir = NewTempDir();
        try
        {
            WriteBmp(dir, "capture_0001.bmp", 100, TimeSpan.FromMinutes(1));
            WriteBmp(dir, "capture_0002.bmp", 200, TimeSpan.FromMinutes(2));
            File.WriteAllText(Path.Combine(dir, "notes.txt"), "not a frame");
            File.WriteAllBytes(Path.Combine(dir, "other.bmp"), new byte[9999]);
            var status = CaptureMonitor.GetStatus(dir);
            Assert.IsTrue(status.Exists);
            Assert.AreEqual(2, status.FileCount);
            Assert.AreEqual(300, status.TotalBytes);
        }
        finally
        {
            Directory.Delete(dir, recursive: true);
        }
    }

    [TestMethod]
    public void Purge_DeletesOnlyOldBmps()
    {
        var dir = NewTempDir();
        try
        {
            WriteBmp(dir, "capture_old.bmp", 100, TimeSpan.FromHours(1));
            WriteBmp(dir, "capture_new.bmp", 200, TimeSpan.FromSeconds(5));
            File.WriteAllText(Path.Combine(dir, "keep.txt"), "x");
            var result = CaptureMonitor.PurgeOlderThan(TimeSpan.FromMinutes(30), dir);
            Assert.AreEqual(1, result.DeletedFiles);
            Assert.AreEqual(100, result.DeletedBytes);
            Assert.IsFalse(File.Exists(Path.Combine(dir, "capture_old.bmp")));
            Assert.IsTrue(File.Exists(Path.Combine(dir, "capture_new.bmp")));
            Assert.IsTrue(File.Exists(Path.Combine(dir, "keep.txt")));
        }
        finally
        {
            Directory.Delete(dir, recursive: true);
        }
    }

    [TestMethod]
    public void Purge_MissingDirIsNoop()
    {
        var result = CaptureMonitor.PurgeOlderThan(
            TimeSpan.FromMinutes(1),
            Path.Combine(Path.GetTempPath(), $"usbdisplay-nope-{Guid.NewGuid()}"));
        Assert.AreEqual(0, result.DeletedFiles);
    }
}
