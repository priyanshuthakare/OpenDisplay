param(
    [switch]$KeepDriverStore
)

$ErrorActionPreference = "Continue"

if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Run this uninstaller from an elevated PowerShell session."
}

function Get-DevGen {
    Get-ChildItem "C:\Program Files (x86)\Windows Kits" -Recurse -Filter "devgen.exe" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "\\x64\\devgen.exe$" } |
        Sort-Object FullName -Descending |
        Select-Object -First 1 |
        Select-Object -ExpandProperty FullName
}

# 1. Remove the device node(s) created by devgen.
$devices = Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
    $hw = (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName 'DEVPKEY_Device_HardwareIds' -ErrorAction SilentlyContinue).Data
    (($hw -join ';') -match 'USBDisplayIdd') -or
    ($_.Class -eq "Display" -and $_.FriendlyName -like "*USBDisplay*")
}

$devgen = Get-DevGen
foreach ($d in $devices) {
    Write-Host "Removing device node: $($d.InstanceId)"
    if ($devgen) {
        & $devgen /remove $d.InstanceId /subtree 2>&1 | ForEach-Object { Write-Host "  $_" }
    }
    if ($LASTEXITCODE -ne 0 -or -not $devgen) {
        # Fall back to pnputil for nodes devgen won't remove.
        & pnputil /remove-device $d.InstanceId 2>&1 | ForEach-Object { Write-Host "  $_" }
    }
}
if (-not $devices) {
    Write-Host "No USBDisplay device node present."
}

# 2. Optionally remove the driver package from the driver store.
if (-not $KeepDriverStore) {
    $enum = & pnputil /enum-drivers 2>&1
    $block = @()
    $published = @()
    foreach ($line in $enum) {
        if ([string]::IsNullOrWhiteSpace($line)) {
            if (($block -join "`n") -match "USBDisplay") {
                $pub = ($block | Where-Object { $_ -match "Published Name" }) -replace '.*:\s*', ''
                if ($pub) { $published += $pub.Trim() }
            }
            $block = @()
        } else { $block += $line }
    }
    foreach ($p in $published) {
        Write-Host "Deleting driver package: $p"
        & pnputil /delete-driver $p /uninstall /force 2>&1 | ForEach-Object { Write-Host "  $_" }
    }
    if (-not $published) {
        Write-Host "No USBDisplay driver package in the driver store."
    }
}

Write-Host ""
Write-Host "Uninstall complete. Run a hardware rescan or reboot if a stale node remains."
