param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Run this installer from an elevated PowerShell session."
}

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$hardwareId = "Root\USBDisplayIdd"

# NOTE: This installer assumes Windows test signing is already ON for the self-signed
# development certificate (bcdedit /set testsigning on + reboot). It intentionally does
# NOT change boot configuration. Verify with: bcdedit /enum {current} | findstr testsigning

# Enable the UMDF operational event log so verify.ps1 and Event Viewer show rich
# reflector/driver events during bring-up (disabled by default on most machines).
try {
    & wevtutil sl "Microsoft-Windows-DriverFrameworks-UserMode/Operational" /e:true 2>&1 | Out-Null
    Write-Host "Enabled DriverFrameworks-UserMode/Operational event log."
} catch {
    Write-Host "Could not enable DriverFrameworks-UserMode/Operational log: $($_.Exception.Message)"
}

# Create the capture output directory with a permissive ACL so the LocalService
# WUDFHost process can write BMPs there. The driver reads back the virtual
# monitor surface and writes captured frames to %ProgramData%\USBDisplay\capture
# as deterministic proof of real capture.
try {
    $captureDir = Join-Path $env:ProgramData "USBDisplay\capture"
    New-Item -ItemType Directory -Force -Path $captureDir | Out-Null
    $acl = Get-Acl $captureDir
    $rule = New-Object System.Security.AccessControl.FileSystemAccessRule(
        "Everyone", "Modify", "ContainerInherit,ObjectInherit", "None", "Allow")
    $acl.AddAccessRule($rule)
    Set-Acl -Path $captureDir -AclObject $acl
    Write-Host "Captured frames will be written to: $captureDir"
} catch {
    Write-Host "Could not prepare capture directory: $($_.Exception.Message)"
}

function Get-DevGen {
    $tool = Get-ChildItem "C:\Program Files (x86)\Windows Kits" -Recurse -Filter "devgen.exe" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "\\x64\\devgen.exe$" } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if (-not $tool) {
        throw "devgen.exe was not found. Install the Windows Driver Kit (WDK) Tools."
    }
    return $tool.FullName
}

# 1. Build + sign the driver package (unless skipped).
if (-not $SkipBuild) {
    & "$root\build.ps1" -Configuration $Configuration
}

$packageRoot = Join-Path $root "package\x64\$Configuration"
$packageInf = Join-Path $packageRoot "Driver.inf"
if (-not (Test-Path $packageInf)) {
    throw "Packaged driver INF was not found at $packageInf. Run build.ps1 first."
}

# 2. Confirm signing is valid before asking PnP to trust the package.
& "$root\verify-signing.ps1" -Configuration $Configuration | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "USBDisplay driver package failed signing verification; aborting install."
}

# 2. Confirm signing is valid before asking PnP to trust the package.
& "$root\verify-signing.ps1" -Configuration $Configuration | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "USBDisplay driver package failed signing verification; aborting install."
}

# 3. Create (or reuse) the persistent ROOT device node FIRST, so that the driver
#    package install in the next step has a present device to bind to. (pnputil
#    /install only binds to devices present at install time.)
$existing = Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object {
    ((Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName 'DEVPKEY_Device_HardwareIds' -ErrorAction SilentlyContinue).Data -join ';') -match 'USBDisplayIdd'
}

if ($existing) {
    Write-Host "USBDisplay device node already present:"
    $existing | ForEach-Object { Write-Host ("  {0}  [{1}]" -f $_.InstanceId, $_.Status) }
} else {
    $devgen = Get-DevGen
    Write-Host "Creating persistent ROOT device node via devgen: $hardwareId"
    $devgenOutput = & $devgen /add /bus ROOT /hardwareid $hardwareId 2>&1
    $devgenOutput | ForEach-Object { Write-Host "  $_" }
    if ($LASTEXITCODE -ne 0) {
        throw "devgen failed to create the ROOT\USBDisplayIdd device node (exit $LASTEXITCODE)."
    }
    Start-Sleep -Seconds 2
}

# 4. Add the driver package to the driver store AND install it onto the now-present
#    device. /install binds the package to any matching present device.
Write-Host "Adding driver package to the driver store and installing onto the device..."
& pnputil /add-driver $packageInf /install
if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne 259) {
    throw "pnputil failed to add the USBDisplay driver package (exit $LASTEXITCODE)."
}

# 5. Nudge PnP to (re)evaluate drivers for the device in case it was created
#    before the package was in the store.
& pnputil /scan-devices 2>&1 | Out-Null

# 5. Report status.
Write-Host ""
Write-Host "Running verification..."
& "$root\verify.ps1"
