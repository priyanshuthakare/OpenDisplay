#requires -Version 5.1
<#
    USBDisplay indirect display driver -- runtime verification.

    Reports a clear PASS/FAIL summary plus detail for:
      * driver package installed (driver store)
      * UMDF reflector service bound (WUDFRd)
      * driver loaded (PnP problem code 0)
      * ROOT device exists
      * USBDisplay monitor exists
      * current display count / adapter count
      * current monitor EDID
      * recent driver / DriverFrameworks-UserMode / Display events

    Live per-callback driver traces come from the ETW provider USBDisplay.IddDriver
    (see Trace.h) -- view them in DebugView, or capture with:
      tracelog -start UsbDisplay -guid #B9A2F0C4-3E7D-4C1A-9F2B-7A6E5D4C3B21 -f usb.etl -level 5 -flags 0xff
#>

param(
    [int]$EventCount = 15
)

$ErrorActionPreference = "Continue"

$checks = [System.Collections.Generic.List[object]]::new()
function Add-Check {
    param([string]$Name, [bool]$Pass, [string]$Detail)
    $checks.Add([pscustomobject]@{ Name = $Name; Pass = $Pass; Detail = $Detail })
}

function Section($title) {
    Write-Host ""
    Write-Host "== $title ==" -ForegroundColor Cyan
}

# Common PnP problem codes worth calling out explicitly during bring-up.
$problemText = @{
    0  = "OK (no problem)"
    28 = "CM_PROB_FAILED_INSTALL (28) -- no driver installed / device not bound"
    31 = "CM_PROB_FAILED_ADD (31) -- driver present but failed to start"
    37 = "CM_PROB_FAILED_DRIVER_ENTRY (37) -- driver returned failure from DriverEntry/DeviceAdd"
    39 = "CM_PROB_FAILED_LOAD (39) -- driver could not be loaded"
    41 = "CM_PROB_FAILED_START (41) -- driver loaded but device not started"
    52 = "CM_PROB_UNSIGNED_DRIVER (52) -- signature not trusted (is test signing ON + rebooted?)"
}

# ---------------------------------------------------------------------------
# 1. Driver package in the driver store
# ---------------------------------------------------------------------------
Section "Driver package (driver store)"
$publishedNames = @()
try {
    $enum = & pnputil /enum-drivers 2>&1
    $block = @()
    foreach ($line in $enum) {
        if ([string]::IsNullOrWhiteSpace($line)) {
            if (($block -join "`n") -match "USBDisplay") {
                $pub = ($block | Where-Object { $_ -match "Published Name" }) -replace '.*:\s*', ''
                if ($pub) { $publishedNames += $pub.Trim() }
            }
            $block = @()
        } else {
            $block += $line
        }
    }
} catch { }
$pkgInstalled = $publishedNames.Count -gt 0
if ($pkgInstalled) {
    Write-Host ("  Published as: {0}" -f ($publishedNames -join ", "))
} else {
    Write-Host "  No USBDisplay driver package found in the driver store."
}
Add-Check "Driver package installed" $pkgInstalled ($publishedNames -join ", ")

# ---------------------------------------------------------------------------
# 2. ROOT device node + driver load state
# ---------------------------------------------------------------------------
Section "Device node"
# devgen names the node ROOT\DEVGEN\{guid} and sets Root\USBDisplayIdd as the
# *hardware* ID, so match on hardware IDs (works before AND after the driver binds).
$device = Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
    $hw = (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName 'DEVPKEY_Device_HardwareIds' -ErrorAction SilentlyContinue).Data
    ($hw -join ';') -match 'USBDisplayIdd'
} | Select-Object -First 1

$devicePresent = $null -ne $device
$driverLoaded = $false
$problemCode = $null
if ($devicePresent) {
    try {
        $problemCode = (Get-PnpDeviceProperty -InstanceId $device.InstanceId -KeyName 'DEVPKEY_Device_ProblemCode' -ErrorAction SilentlyContinue).Data
    } catch { }
    $svc = $null
    try {
        $svc = (Get-PnpDeviceProperty -InstanceId $device.InstanceId -KeyName 'DEVPKEY_Device_Service' -ErrorAction SilentlyContinue).Data
    } catch { }
    # A bare devgen node with no bound driver still reports Status=OK / Problem=0,
    # so "loaded" additionally requires the WUDFRd reflector service to be bound.
    $driverLoaded = ($device.Status -eq "OK") -and (($problemCode -eq 0) -or ($null -eq $problemCode)) -and ($svc -eq "WUDFRd")
    $ptext = if ($null -ne $problemCode -and $problemText.ContainsKey([int]$problemCode)) { $problemText[[int]$problemCode] } else { "problem=$problemCode" }
    $classText = if ([string]::IsNullOrWhiteSpace($device.Class)) { "<none - driver not bound>" } else { $device.Class }
    Write-Host ("  InstanceId : {0}" -f $device.InstanceId)
    Write-Host ("  Friendly   : {0}" -f $device.FriendlyName)
    Write-Host ("  Class      : {0}" -f $classText)
    Write-Host ("  Status     : {0}" -f $device.Status)
    Write-Host ("  Service    : {0}" -f $svc)
    Write-Host ("  Problem    : {0}" -f $ptext)
    if ([string]::IsNullOrWhiteSpace($svc)) {
        Write-Host "  NOTE: device present but NO driver bound (empty service). The INF did not install onto this node." -ForegroundColor Yellow
    }
    Add-Check "UMDF reflector service (WUDFRd)" ($svc -eq "WUDFRd") "Service=$svc"
} else {
    Write-Host "  ROOT\USBDisplayIdd device not found. Run install.ps1 (elevated) to create it."
    Add-Check "UMDF reflector service (WUDFRd)" $false "device absent"
}
$deviceDetail = ""
if ($device) { $deviceDetail = $device.InstanceId }
Add-Check "ROOT device exists" $devicePresent $deviceDetail
Add-Check "Driver loaded (problem 0)" $driverLoaded ("problem=" + $problemCode)

# ---------------------------------------------------------------------------
# 3. Adapter count / display count
# ---------------------------------------------------------------------------
Section "Adapters & displays"
$adapters = Get-PnpDevice -Class Display -PresentOnly -ErrorAction SilentlyContinue
$adapterCount = ($adapters | Measure-Object).Count
Write-Host ("  Display adapters present: {0}" -f $adapterCount)
$adapters | ForEach-Object { Write-Host ("    - {0} [{1}]" -f $_.FriendlyName, $_.Status) }

$displayCount = 0
try {
    Add-Type -AssemblyName System.Windows.Forms -ErrorAction SilentlyContinue
    $displayCount = [System.Windows.Forms.Screen]::AllScreens.Count
} catch { }
Write-Host ("  Active display outputs (screens): {0}" -f $displayCount)
Add-Check "Adapter count" ($adapterCount -ge 1) "$adapterCount adapters"
Add-Check "Display count" ($displayCount -ge 1) "$displayCount screens"

# ---------------------------------------------------------------------------
# 4. USBDisplay monitor + EDID
# ---------------------------------------------------------------------------
Section "Monitor"
function Decode-Name([byte[]]$bytes) {
    if (-not $bytes) { return "" }
    (($bytes | Where-Object { $_ -ne 0 } | ForEach-Object { [char]$_ }) -join "").Trim()
}
$monitorFound = $false
$monitorInstance = $null
try {
    $ids = Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorID -ErrorAction SilentlyContinue
    foreach ($m in $ids) {
        $name = Decode-Name $m.UserFriendlyName
        if ($name -like "*USBDisplay*") {
            $monitorFound = $true
            $monitorInstance = $m.InstanceName
            Write-Host ("  Monitor name  : {0}" -f $name)
            Write-Host ("  InstanceName  : {0}" -f $m.InstanceName)
            Write-Host ("  Mfg / Product : {0} / {1}" -f (Decode-Name $m.ManufacturerName), (Decode-Name $m.ProductCodeID))
            break
        }
    }
} catch { }

if ($monitorFound) {
    try {
        $edidObj = Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorDescriptorMethods -ErrorAction SilentlyContinue |
            Where-Object { $_.InstanceName -eq $monitorInstance } | Select-Object -First 1
        if ($edidObj) {
            $block = Invoke-CimMethod -InputObject $edidObj -MethodName WmiGetMonitorRawEEdidV1Block -Arguments @{ BlockId = [byte]0 } -ErrorAction SilentlyContinue
            if ($block -and $block.BlockContent) {
                $hex = ($block.BlockContent | ForEach-Object { $_.ToString("X2") }) -join " "
                Write-Host "  EDID (128 bytes):"
                for ($i = 0; $i -lt $block.BlockContent.Count; $i += 16) {
                    $slice = $block.BlockContent[$i..([math]::Min($i+15, $block.BlockContent.Count-1))]
                    Write-Host ("    {0:X3}: {1}" -f $i, (($slice | ForEach-Object { $_.ToString('X2') }) -join ' '))
                }
            }
        }
    } catch { Write-Host "  (EDID read failed: $($_.Exception.Message))" }
} else {
    Write-Host "  USBDisplay monitor not enumerated by Windows yet."
    Write-Host "  If the device is loaded, open Settings > System > Display or run: DisplaySwitch.exe /extend"
}
Add-Check "USBDisplay monitor exists" $monitorFound $monitorInstance

# ---------------------------------------------------------------------------
# 5. Recent events
# ---------------------------------------------------------------------------
function Show-Log {
    param([string]$LogName, [int]$Count)
    Section "Recent events: $LogName"
    try {
        $events = Get-WinEvent -LogName $LogName -MaxEvents 200 -ErrorAction Stop |
            Where-Object { $_.Message -match "USBDisplay|WUDF|Indirect|IddCx" } |
            Select-Object -First $Count
        if (-not $events) {
            $events = Get-WinEvent -LogName $LogName -MaxEvents $Count -ErrorAction Stop
        }
        if ($events) {
            $events | ForEach-Object {
                Write-Host ("  {0}  [{1}]  id={2}" -f $_.TimeCreated, $_.LevelDisplayName, $_.Id)
                Write-Host ("      {0}" -f (($_.Message -split "`r?`n")[0]))
            }
        } else {
            Write-Host "  (no matching events)"
        }
    } catch {
        Write-Host "  (log unavailable -- it may be disabled. Enable with:"
        Write-Host ("     wevtutil sl `"{0}`" /e:true )" -f $LogName)
    }
}
Show-Log "Microsoft-Windows-DriverFrameworks-UserMode/Operational" $EventCount
Show-Log "Microsoft-Windows-Kernel-PnP/Configuration" $EventCount
Show-Log "System" $EventCount

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
Section "PASS / FAIL summary"
$gateNames = @("Driver package installed", "ROOT device exists", "Driver loaded (problem 0)", "USBDisplay monitor exists")
$overall = $true
foreach ($c in $checks) {
    $isGate = $gateNames -contains $c.Name
    $mark = if ($c.Pass) { "PASS" } else { "FAIL" }
    $color = if ($c.Pass) { "Green" } else { if ($isGate) { "Red" } else { "Yellow" } }
    Write-Host ("  [{0}] {1}" -f $mark, $c.Name) -ForegroundColor $color
    if ($isGate -and -not $c.Pass) { $overall = $false }
}
Write-Host ""
if ($overall) {
    Write-Host "  OVERALL: PASS -- USBDisplay virtual monitor is enumerated and the driver is loaded." -ForegroundColor Green
    exit 0
} else {
    Write-Host "  OVERALL: FAIL -- see failed gates above and the recent events / DebugView traces." -ForegroundColor Red
    exit 1
}
