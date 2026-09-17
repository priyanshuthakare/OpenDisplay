using System.Diagnostics;
using System.Security.Principal;

namespace USBDisplay.ControlApp.Services;

public interface IElevationService
{
    bool IsElevated { get; }
    bool IsAdminUser { get; }

    /// <summary>
    /// Runs an executable elevated via UAC. The caller must have shown the
    /// user WHY elevation is needed before calling this.
    /// </summary>
    int RunElevated(string exePath, string[] args, string? workingDir = null);
}

public sealed class ElevationService : IElevationService
{
    public bool IsElevated
    {
        get
        {
            using var id = WindowsIdentity.GetCurrent();
            return new WindowsPrincipal(id).IsInRole(WindowsBuiltInRole.Administrator);
        }
    }

    public bool IsAdminUser => new WindowsPrincipal(WindowsIdentity.GetCurrent())
        .IsInRole(WindowsBuiltInRole.Administrator)
        || IsElevated;

    public int RunElevated(string exePath, string[] args, string? workingDir = null)
    {
        var psi = new ProcessStartInfo
        {
            FileName = exePath,
            UseShellExecute = true,
            Verb = "runas",
        };
        foreach (var a in args)
        {
            psi.ArgumentList.Add(a);
        }
        if (!string.IsNullOrWhiteSpace(workingDir))
        {
            psi.WorkingDirectory = workingDir;
        }
        using var process = Process.Start(psi);
        process?.WaitForExit();
        return process?.ExitCode ?? -1;
    }
}
