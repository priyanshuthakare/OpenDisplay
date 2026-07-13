#requires -Version 5.1
<#
    Global OutputDebugString capturer + fresh driver load.

    The USBDisplay driver mirrors EVERY log line (DllMain, DriverEntry, DeviceAdd,
    D0Entry, adapter/monitor init, frame loop) to OutputDebugStringW. This script
    reads the global DBWIN debug channel (like Sysinternals DebugView's "Capture
    Global Win32"), so it sees the driver's output from WUDFHost regardless of ETW
    timing -- including DllMain, which fires before DriverEntry.

    Run elevated (needs SeCreateGlobalPrivilege to create the Global\ DBWIN objects).
    It starts the capture, forces a fresh driver load, and prints every [USBDisplay]
    line in order.
#>

param(
    [int]$WaitSeconds = 12,
    [switch]$AllProcesses   # print every debug line, not just [USBDisplay]
)

$ErrorActionPreference = "Continue"

if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Run capture-debug.ps1 from an elevated PowerShell session."
}

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$hardwareId = "Root\USBDisplayIdd"

Add-Type -TypeDefinition @"
using System;
using System.Collections.Concurrent;
using System.Runtime.InteropServices;
using System.Threading;

public static class DbwinCapture
{
    [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    static extern IntPtr CreateFileMapping(IntPtr hFile, IntPtr sa, uint prot, uint high, uint low, string name);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern IntPtr MapViewOfFile(IntPtr h, uint access, uint offHigh, uint offLow, UIntPtr bytes);
    [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    static extern IntPtr CreateEvent(IntPtr sa, bool manualReset, bool initialState, string name);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool SetEvent(IntPtr h);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern uint WaitForSingleObject(IntPtr h, uint ms);
    [DllImport("advapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    static extern bool ConvertStringSecurityDescriptorToSecurityDescriptor(string sddl, uint rev, out IntPtr psd, IntPtr size);

    const uint PAGE_READWRITE = 0x04;
    const uint FILE_MAP_READ = 0x0004;

    public static ConcurrentQueue<string> Lines = new ConcurrentQueue<string>();
    public static string Error = "";
    static volatile bool _running;
    static Thread _thread;

    [StructLayout(LayoutKind.Sequential)]
    struct SECURITY_ATTRIBUTES { public int nLength; public IntPtr lpSecurityDescriptor; public int bInheritHandle; }

    public static bool Start()
    {
        IntPtr psd;
        // Everyone full access + low integrity label so a session-0 low/system-IL
        // WUDFHost process can signal these objects.
        if (!ConvertStringSecurityDescriptorToSecurityDescriptor("D:(A;;GA;;;WD)S:(ML;;NW;;;LW)", 1, out psd, IntPtr.Zero))
        { Error = "SDDL convert failed: " + Marshal.GetLastWin32Error(); return false; }

        SECURITY_ATTRIBUTES sa = new SECURITY_ATTRIBUTES();
        sa.nLength = Marshal.SizeOf(typeof(SECURITY_ATTRIBUTES));
        sa.lpSecurityDescriptor = psd;
        sa.bInheritHandle = 0;
        IntPtr psa = Marshal.AllocHGlobal(sa.nLength);
        Marshal.StructureToPtr(sa, psa, false);

        IntPtr bufferReady = CreateEvent(psa, false, true,  "Global\\DBWIN_BUFFER_READY");
        IntPtr dataReady   = CreateEvent(psa, false, false, "Global\\DBWIN_DATA_READY");
        IntPtr mapping     = CreateFileMapping(new IntPtr(-1), psa, PAGE_READWRITE, 0, 4096, "Global\\DBWIN_BUFFER");
        if (bufferReady == IntPtr.Zero || dataReady == IntPtr.Zero || mapping == IntPtr.Zero)
        { Error = "Create DBWIN objects failed: " + Marshal.GetLastWin32Error(); return false; }
        IntPtr view = MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, (UIntPtr)512);
        if (view == IntPtr.Zero) { Error = "MapViewOfFile failed: " + Marshal.GetLastWin32Error(); return false; }

        _running = true;
        _thread = new Thread(delegate() {
            while (_running) {
                SetEvent(bufferReady);
                if (WaitForSingleObject(dataReady, 200) == 0) {
                    int pid = Marshal.ReadInt32(view, 0);
                    string s = Marshal.PtrToStringAnsi(new IntPtr(view.ToInt64() + 4));
                    if (s != null) Lines.Enqueue(pid + "\t" + s.TrimEnd());
                }
            }
        });
        _thread.IsBackground = true;
        _thread.Start();
        return true;
    }

    public static void Stop() { _running = false; if (_thread != null) _thread.Join(1500); }
}
"@

function Get-DevGen {
    Get-ChildItem "C:\Program Files (x86)\Windows Kits" -Recurse -Filter "devgen.exe" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "\\x64\\devgen.exe$" } |
        Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
}

Write-Host "Starting global OutputDebugString capture ..."
if (-not [DbwinCapture]::Start()) {
    throw "Could not start debug capture: $([DbwinCapture]::Error). (Is DebugView running? Close it and retry.)"
}
Start-Sleep -Milliseconds 500

# Force a fresh driver load under capture.
$dev = Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
    ((Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName 'DEVPKEY_Device_HardwareIds' -ErrorAction SilentlyContinue).Data -join ';') -match 'USBDisplayIdd'
} | Select-Object -First 1
if ($dev) {
    Write-Host "Removing existing node $($dev.InstanceId) ..."
    & pnputil /remove-device $dev.InstanceId 2>&1 | Out-Null
    Start-Sleep -Seconds 1
}
$devgen = Get-DevGen
Write-Host "Creating fresh ROOT node ..."
& $devgen /add /bus ROOT /hardwareid $hardwareId 2>&1 | ForEach-Object { Write-Host "  $_" }
Start-Sleep -Seconds 2
$pkgInf = Join-Path $root "package\x64\Release\Driver.inf"
Write-Host "Installing driver under capture ..."
& pnputil /add-driver $pkgInf /install 2>&1 | ForEach-Object { Write-Host "  $_" }

Write-Host "Capturing for $WaitSeconds s ..."
Start-Sleep -Seconds $WaitSeconds
[DbwinCapture]::Stop()

Write-Host ""
Write-Host "==================== Driver debug output ====================" -ForegroundColor Cyan
$all = @()
$line = ""
while ([DbwinCapture]::Lines.TryDequeue([ref]$line)) { $all += $line }
$shown = 0
foreach ($l in $all) {
    if ($AllProcesses -or $l -match '\[USBDisplay\]') {
        Write-Host ("  {0}" -f $l)
        $shown++
    }
}
if ($shown -eq 0) {
    Write-Host "  (no [USBDisplay] debug lines captured)"
    Write-Host "  -> If DllMain line is ALSO absent, the DLL never loaded into WUDFHost."
    Write-Host "  -> Total debug lines seen from all processes: $($all.Count) (run with -AllProcesses to view)"
}

# Device state.
Write-Host ""
Write-Host "==================== Device state ====================" -ForegroundColor Cyan
$dev2 = Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
    ((Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName 'DEVPKEY_Device_HardwareIds' -ErrorAction SilentlyContinue).Data -join ';') -match 'USBDisplayIdd'
} | Select-Object -First 1
if ($dev2) {
    $p = (Get-PnpDeviceProperty -InstanceId $dev2.InstanceId -KeyName 'DEVPKEY_Device_ProblemCode' -ErrorAction SilentlyContinue).Data
    Write-Host ("  {0}  Status={1}  Problem={2}" -f $dev2.InstanceId, $dev2.Status, $p)
}
