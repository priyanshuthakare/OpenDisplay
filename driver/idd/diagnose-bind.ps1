#requires -Version 5.1
<#
    Framework-level bind diagnostics for the "DLL loads, DriverEntry never called,
    DLL unloads, Problem 31" failure. Gathers the concrete reasons WUDFHost rejects
    the driver before DriverEntry:

      1. Our installed service's WDF/UMDF/extension registry values.
      2. Whether the IddCx0102 UMDF class extension is registered on this machine.
      3. The WUDF operational events for OUR device instance during a fresh load
         (full message text, incl. any bind/extension errors).
      4. The tail of setupapi.dev.log for the device start attempt.

    Run elevated.
#>
$ErrorActionPreference = "Continue"
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Run diagnose-bind.ps1 elevated."
}
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$hardwareId = "Root\USBDisplayIdd"

function Section($t) { Write-Host ""; Write-Host "== $t ==" -ForegroundColor Cyan }

Section "1. Installed service registry (WUDF\Services\USBDisplayIdd)"
$svcKey = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\WUDF\Services\USBDisplayIdd"
if (Test-Path $svcKey) {
    Get-ItemProperty $svcKey | Format-List WdfMajorVersion,WdfMinorVersion,UmdfExtensions,DriverCLSID,ComCLSID,ImagePath
} else {
    Write-Host "  (service key not present - driver not installed onto a device yet)"
}

Section "2. IddCx0102 UMDF class extension registration"
$found = $false
foreach ($base in @(
    "HKLM:\SYSTEM\CurrentControlSet\Control\WUDF\Services",
    "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\WUDF\Services")) {
    if (Test-Path $base) {
        Get-ChildItem $base -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -match 'IddCx' } | ForEach-Object {
            $found = $true
            Write-Host "  Registered extension key: $($_.PSChildName)"
            Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue | Format-List *
        }
    }
}
Write-Host ("  IddCx extension binary: {0}" -f (Test-Path "C:\Windows\System32\drivers\UMDF\IddCx.dll"))
if (-not $found) { Write-Host "  NOTE: no IddCx* extension service registration found in WUDF\Services." }

Section "3. Fresh load - capturing WUDF events for our device"
$dev = Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
    ((Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName 'DEVPKEY_Device_HardwareIds' -ErrorAction SilentlyContinue).Data -join ';') -match 'USBDisplayIdd'
} | Select-Object -First 1
if ($dev) { & pnputil /remove-device $dev.InstanceId 2>&1 | Out-Null; Start-Sleep 1 }
$devgen = Get-ChildItem "C:\Program Files (x86)\Windows Kits" -Recurse -Filter "devgen.exe" -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match "\\x64\\devgen.exe$" } | Select-Object -First 1 -ExpandProperty FullName
$t0 = Get-Date
& $devgen /add /bus ROOT /hardwareid $hardwareId 2>&1 | Out-Null
Start-Sleep 2
& pnputil /add-driver (Join-Path $root "package\x64\Release\Driver.inf") /install 2>&1 | Out-Null
Start-Sleep 4

$node = Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
    ((Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName 'DEVPKEY_Device_HardwareIds' -ErrorAction SilentlyContinue).Data -join ';') -match 'USBDisplayIdd'
} | Select-Object -First 1
$nodeId = if ($node) { $node.InstanceId } else { "" }
$short = if ($nodeId) { ($nodeId -split '\\')[-1] } else { "" }
Write-Host "  New node: $nodeId"

Write-Host "  --- WUDF operational events mentioning this node (full text) ---"
try {
    Get-WinEvent -LogName "Microsoft-Windows-DriverFrameworks-UserMode/Operational" -MaxEvents 300 -ErrorAction Stop |
        Where-Object { $_.TimeCreated -ge $t0 -and ($_.Message -match [regex]::Escape($short) -or $_.LevelDisplayName -in @('Error','Critical','Warning')) } |
        Select-Object -First 25 |
        ForEach-Object {
            Write-Host ("  {0} [{1}] id={2}" -f $_.TimeCreated.ToString('HH:mm:ss'), $_.LevelDisplayName, $_.Id)
            Write-Host ("      {0}" -f ($_.Message -replace "`r?`n"," "))
        }
} catch { Write-Host "  (log error: $($_.Exception.Message))" }

Section "4. setupapi.dev.log tail for this install"
try {
    $log = Get-Content "C:\Windows\INF\setupapi.dev.log" -Tail 120 -ErrorAction Stop
    $log | Where-Object { $_ -match 'USBDisplay|IndirectKmd|UMDF|dvi:|! ' } | Select-Object -Last 40 | ForEach-Object { Write-Host "  $_" }
} catch { Write-Host "  (could not read setupapi.dev.log: $($_.Exception.Message))" }

Section "5. Final device state"
if ($node) {
    $p = (Get-PnpDeviceProperty -InstanceId $node.InstanceId -KeyName 'DEVPKEY_Device_ProblemCode' -ErrorAction SilentlyContinue).Data
    $ps = (Get-PnpDeviceProperty -InstanceId $node.InstanceId -KeyName 'DEVPKEY_Device_ProblemStatus' -ErrorAction SilentlyContinue).Data
    Write-Host ("  {0}  Status={1}  Problem={2}  ProblemStatus={3}" -f $node.InstanceId, $node.Status, $p, $ps)
}
