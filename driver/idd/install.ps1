param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",
    [switch]$EnableTestSigning
)

$ErrorActionPreference = "Stop"

if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Run this installer from an elevated PowerShell session."
}

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$inf = Join-Path $root "Driver.inf"
$hardwareId = "ROOT\USBDisplayIdd"

if ($EnableTestSigning) {
    & bcdedit /set testsigning on
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to enable Windows test signing."
    }
}

& "$root\build.ps1" -Configuration $Configuration

$packageRoot = Join-Path $root "bin\x64\$Configuration"
$builtDll = Join-Path $packageRoot "USBDisplayIdd.dll"
if (-not (Test-Path $builtDll)) {
    throw "Built driver DLL was not found at $builtDll."
}

Copy-Item -LiteralPath $builtDll -Destination (Join-Path $root "USBDisplayIdd.dll") -Force
& pnputil /add-driver $inf /install
if ($LASTEXITCODE -ne 0) {
    throw "pnputil failed to add the USBDisplay driver package."
}

Add-Type -Language CSharp @"
using System;
using System.Runtime.InteropServices;

public static class UsbDisplayRootDevice
{
    private const int CR_SUCCESS = 0;
    private const int CM_CREATE_DEVNODE_NORMAL = 0;
    private const int CM_REENUMERATE_NORMAL = 0;

    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode)]
    private static extern int CM_Locate_DevNode(out IntPtr pdnDevInst, string pDeviceID, int ulFlags);

    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode)]
    private static extern int CM_Create_DevNode(out IntPtr pdnDevInst, string pDeviceID, IntPtr dnParent, int ulFlags);

    [DllImport("cfgmgr32.dll")]
    private static extern int CM_Reenumerate_DevNode(IntPtr dnDevInst, int ulFlags);

    public static void Ensure(string hardwareId)
    {
        IntPtr devInst;
        int locate = CM_Locate_DevNode(out devInst, hardwareId, 0);
        if (locate == CR_SUCCESS)
        {
            CM_Reenumerate_DevNode(devInst, CM_REENUMERATE_NORMAL);
            return;
        }

        int create = CM_Create_DevNode(out devInst, hardwareId, IntPtr.Zero, CM_CREATE_DEVNODE_NORMAL);
        if (create != CR_SUCCESS)
        {
            throw new InvalidOperationException("CM_Create_DevNode failed with ConfigManager code " + create);
        }
        CM_Reenumerate_DevNode(devInst, CM_REENUMERATE_NORMAL);
    }
}
"@

[UsbDisplayRootDevice]::Ensure($hardwareId)
Start-Sleep -Seconds 2
& "$root\verify.ps1"

