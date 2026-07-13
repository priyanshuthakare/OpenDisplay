#requires -Version 5.1
<#
    Forces a fresh USBDisplay driver load while capturing the driver's own
    TraceLogging events, so the callback sequence (and the last call before any
    failure) is visible. A device stuck in FAILED_ADD does not re-run driver load
    on Disable/Enable, so this removes and recreates the ROOT devnode instead.

    Run elevated. Prints the ordered [USBDisplay] callback trace, plus only the
    crash records that match the CURRENTLY built DLL (older stale crashes are
    filtered out by module timestamp).
#>

param(
    [int]$WaitSeconds = 8
)

$ErrorActionPreference = "Continue"

if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Run capture-crash.ps1 from an elevated PowerShell session."
}

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$providerGuid = "{B9A2F0C4-3E7D-4C1A-9F2B-7A6E5D4C3B21}"
$session = "UsbDisplayCap"
$etl = Join-Path $env:TEMP "usbdisplay_trace.etl"
$xml = Join-Path $env:TEMP "usbdisplay_trace.xml"
$hardwareId = "Root\USBDisplayIdd"

function Get-DevGen {
    Get-ChildItem "C:\Program Files (x86)\Windows Kits" -Recurse -Filter "devgen.exe" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "\\x64\\devgen.exe$" } |
        Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
}

# Determine the timestamp (PE TimeDateStamp, hex) of the DLL now in the driver store,
# so we can distinguish a fresh crash from an old one.
$curStamp = $null
try {
    $storeDll = Get-ChildItem "C:\Windows\System32\DriverStore\FileRepository\driver.inf_amd64_*\USBDisplayIdd.dll" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($storeDll) {
        $bytes = [System.IO.File]::ReadAllBytes($storeDll.FullName)
        $peOff = [BitConverter]::ToInt32($bytes, 0x3C)
        $curStamp = ('{0:x8}' -f [BitConverter]::ToUInt32($bytes, $peOff + 8))
        Write-Host "Current driver-store DLL PE timestamp: 0x$curStamp"
    }
} catch { }

# Clean any prior session / files.
logman stop $session -ets 2>$null | Out-Null
Remove-Item $etl, $xml -Force -ErrorAction SilentlyContinue

Write-Host "Starting ETW capture on $providerGuid ..."
logman create trace $session -p $providerGuid 0xFFFFFFFFFFFFFFFF 0xFF -o $etl -ets | Out-Null

# Force a fresh load: remove the existing devnode, then recreate it. PnP will
# start the new node and (re)run the driver load + AddDevice while we capture.
$dev = Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
    ((Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName 'DEVPKEY_Device_HardwareIds' -ErrorAction SilentlyContinue).Data -join ';') -match 'USBDisplayIdd'
} | Select-Object -First 1

if ($dev) {
    Write-Host "Removing existing node $($dev.InstanceId) ..."
    & pnputil /remove-device $dev.InstanceId 2>&1 | Out-Null
    Start-Sleep -Seconds 1
}

$devgen = Get-DevGen
Write-Host "Recreating ROOT node to trigger a fresh driver load ..."
& $devgen /add /bus ROOT /hardwareid $hardwareId 2>&1 | ForEach-Object { Write-Host "  $_" }
Start-Sleep -Seconds 2

# Bind the driver onto the freshly created node WHILE tracing, so DeviceAdd/D0Entry
# run inside the capture window. (A bare devgen node has no driver until installed.)
$pkgInf = Join-Path $root "package\x64\Release\Driver.inf"
if (Test-Path $pkgInf) {
    Write-Host "Installing driver onto the new node (under trace) ..."
    & pnputil /add-driver $pkgInf /install 2>&1 | ForEach-Object { Write-Host "  $_" }
} else {
    Write-Host "WARN: $pkgInf not found; run build first. Trying /scan-devices ..."
    & pnputil /scan-devices 2>&1 | Out-Null
}

Write-Host "Waiting $WaitSeconds s for driver load + callbacks ..."
Start-Sleep -Seconds $WaitSeconds

logman stop $session -ets | Out-Null
Write-Host "Formatting trace ..."
tracerpt $etl -o $xml -of XML -y | Out-Null

Write-Host ""
Write-Host "==================== USBDisplay callback trace ====================" -ForegroundColor Cyan
if (Test-Path $xml) {
    [xml]$doc = Get-Content -Raw $xml
    $count = 0
    foreach ($e in $doc.Events.Event) {
        $fn = $null; $msg = $null
        foreach ($d in $e.EventData.Data) {
            if ($d.Name -eq "Function") { $fn = $d.'#text' }
            if ($d.Name -eq "Message")  { $msg = $d.'#text' }
        }
        if ($fn -or $msg) {
            Write-Host ("  {0}  {1}: {2}" -f $e.System.TimeCreated.SystemTime, $fn, $msg)
            $count++
        }
    }
    if ($count -eq 0) {
        Write-Host "  (no USBDisplay provider events captured)"
    } else {
        Write-Host ""
        Write-Host "  >>> The LAST line above is the last callback that ran; if it ends in a" -ForegroundColor Yellow
        Write-Host "      non-zero status or stops before MonitorArrival, that is the failure point." -ForegroundColor Yellow
    }
} else {
    Write-Host "  (tracerpt produced no XML)"
}

# Post-load device state.
Write-Host ""
Write-Host "==================== Device state after reload ====================" -ForegroundColor Cyan
$dev2 = Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
    ((Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName 'DEVPKEY_Device_HardwareIds' -ErrorAction SilentlyContinue).Data -join ';') -match 'USBDisplayIdd'
} | Select-Object -First 1
if ($dev2) {
    $p = (Get-PnpDeviceProperty -InstanceId $dev2.InstanceId -KeyName 'DEVPKEY_Device_ProblemCode' -ErrorAction SilentlyContinue).Data
    Write-Host ("  {0}  Status={1}  Problem={2}" -f $dev2.InstanceId, $dev2.Status, $p)
}

# Only crash records matching the current build.
Write-Host ""
Write-Host "==================== Fresh crash records (current build only) ====================" -ForegroundColor Cyan
$fresh = Get-WinEvent -FilterHashtable @{ LogName='Application'; Id=1000 } -MaxEvents 30 -ErrorAction SilentlyContinue |
    Where-Object { $_.Message -match 'usbdisplayidd\.dll' -and ($null -eq $curStamp -or $_.Message -match $curStamp) } | Select-Object -First 3
if ($fresh) {
    $fresh | ForEach-Object {
        $off = ($_.Message | Select-String 'Fault offset: (0x[0-9a-fA-F]+)').Matches.Groups[1].Value
        Write-Host ("  {0}  Fault offset: {1}" -f $_.TimeCreated, $off)
    }
    Write-Host "  (Resolve with: .\resolve-offset.ps1 -Dll bin\x64\Release\USBDisplayIdd.dll -Rva <offset>)"
} else {
    Write-Host "  No crash for the current build (0x$curStamp) -- the driver did NOT fault this load."
    Write-Host "  A FAILED_ADD without a crash means a callback returned a non-zero NTSTATUS (see trace above)."
}

Write-Host ""
Write-Host "ETL: $etl"

# Framework-level diagnosis (independent of our provider): the UMDF operational log
# and Kernel-PnP record WHY AddDevice/start failed, including HRESULTs.
Write-Host ""
Write-Host "==================== UMDF operational events (full text, last 20) ====================" -ForegroundColor Cyan
try {
    Get-WinEvent -LogName "Microsoft-Windows-DriverFrameworks-UserMode/Operational" -MaxEvents 60 -ErrorAction Stop |
        Where-Object { $_.TimeCreated -gt (Get-Date).AddMinutes(-5) } |
        Select-Object -First 20 |
        ForEach-Object {
            Write-Host ("  {0} [{1}] id={2}" -f $_.TimeCreated, $_.LevelDisplayName, $_.Id)
            Write-Host ("      {0}" -f ($_.Message -replace "`r?`n", " "))
        }
} catch { Write-Host "  (log unavailable: $($_.Exception.Message))" }

Write-Host ""
Write-Host "==================== Kernel-PnP config (full text, last 8) ====================" -ForegroundColor Cyan
try {
    Get-WinEvent -LogName "Microsoft-Windows-Kernel-PnP/Configuration" -MaxEvents 40 -ErrorAction Stop |
        Where-Object { $_.Message -match 'USBDisplay|DEVGEN' } |
        Select-Object -First 8 |
        ForEach-Object {
            Write-Host ("  {0} [{1}] id={2}" -f $_.TimeCreated, $_.LevelDisplayName, $_.Id)
            Write-Host ("      {0}" -f ($_.Message -replace "`r?`n", " "))
        }
} catch { Write-Host "  (log unavailable: $($_.Exception.Message))" }
