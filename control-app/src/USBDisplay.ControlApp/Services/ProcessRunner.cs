using System;
using System.Diagnostics;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace USBDisplay.ControlApp.Services;

public sealed record ProcessResult(int ExitCode, string StdOut, string StdErr);

/// <summary>
/// Runs executables with direct process APIs. Arguments are passed as an
/// array — never through a shell — so UI strings cannot become injection.
/// </summary>
public interface IProcessRunner
{
    Task<ProcessResult> RunAsync(
        string exePath, string[] args, string? workingDir = null,
        TimeSpan? timeout = null, CancellationToken ct = default);

    Process StartLongRunning(
        string exePath, string[] args, string? workingDir,
        DataReceivedEventHandler? onOut, DataReceivedEventHandler? onErr);
}

public sealed class ProcessRunner : IProcessRunner
{
    public async Task<ProcessResult> RunAsync(
        string exePath, string[] args, string? workingDir = null,
        TimeSpan? timeout = null, CancellationToken ct = default)
    {
        ValidateExe(exePath);
        using var process = new Process();
        process.StartInfo = BaseStartInfo(exePath, args, workingDir);
        process.StartInfo.RedirectStandardOutput = true;
        process.StartInfo.RedirectStandardError = true;
        var stdout = new StringBuilder();
        var stderr = new StringBuilder();
        process.OutputDataReceived += (_, e) => { if (e.Data != null) stdout.AppendLine(e.Data); };
        process.ErrorDataReceived += (_, e) => { if (e.Data != null) stderr.AppendLine(e.Data); };
        process.Start();
        process.BeginOutputReadLine();
        process.BeginErrorReadLine();
        using var cts = CancellationTokenSource.CreateLinkedTokenSource(ct);
        if (timeout.HasValue)
        {
            cts.CancelAfter(timeout.Value);
        }
        try
        {
            await process.WaitForExitAsync(cts.Token).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            TryKill(process);
            throw new TimeoutException($"Timed out running {exePath}.");
        }
        return new ProcessResult(process.ExitCode, stdout.ToString(), stderr.ToString());
    }

    public Process StartLongRunning(
        string exePath, string[] args, string? workingDir,
        DataReceivedEventHandler? onOut, DataReceivedEventHandler? onErr)
    {
        ValidateExe(exePath);
        var process = new Process();
        process.StartInfo = BaseStartInfo(exePath, args, workingDir);
        process.StartInfo.RedirectStandardOutput = true;
        process.StartInfo.RedirectStandardError = true;
        process.EnableRaisingEvents = true;
        if (onOut != null) process.OutputDataReceived += onOut;
        if (onErr != null) process.ErrorDataReceived += onErr;
        process.Start();
        process.BeginOutputReadLine();
        process.BeginErrorReadLine();
        return process;
    }

    private static ProcessStartInfo BaseStartInfo(string exePath, string[] args, string? workingDir)
    {
        var psi = new ProcessStartInfo
        {
            FileName = exePath,
            UseShellExecute = false,
            CreateNoWindow = true,
        };
        foreach (var a in args)
        {
            psi.ArgumentList.Add(a);
        }
        if (!string.IsNullOrWhiteSpace(workingDir))
        {
            psi.WorkingDirectory = workingDir;
        }
        return psi;
    }

    private static void ValidateExe(string exePath)
    {
        if (string.IsNullOrWhiteSpace(exePath))
        {
            throw new ArgumentException("Executable path is empty.", nameof(exePath));
        }
        if (exePath.IndexOfAny(new[] { '\n', '\r', '"', ';', '&', '|' }) >= 0)
        {
            throw new ArgumentException("Executable path contains illegal characters.", nameof(exePath));
        }
        if (!System.IO.File.Exists(exePath) && !IsBareCommand(exePath))
        {
            throw new System.IO.FileNotFoundException($"Executable not found: {exePath}");
        }
    }

    private static bool IsBareCommand(string exePath) =>
        exePath.IndexOf(System.IO.Path.DirectorySeparatorChar) < 0
        && exePath.IndexOf(System.IO.Path.AltDirectorySeparatorChar) < 0;

    private static void TryKill(Process process)
    {
        try
        {
            if (!process.HasExited) process.Kill(entireProcessTree: true);
        }
        catch { }
    }
}
