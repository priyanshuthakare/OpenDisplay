<#
.SYNOPSIS
    Report each component's version and flag drift from the root VERSION file.

.DESCRIPTION
    Component versions live in several build systems (Cargo, MSBuild, Gradle,
    INF/RC) and cannot be single-sourced without generated files. This script
    makes drift visible.

    Two groups are reported:
      * Product components - expected to match .\VERSION exactly.
      * Driver install version - Windows driver versions are a 4-part namespace
        (a.b.c.d) keyed by PnP, independent of the product SemVer. Reported for
        visibility but NOT counted as drift.

    Exit code is non-zero only with -Strict and a product mismatch.

.EXAMPLE
    .\scripts\check-versions.ps1
    .\scripts\check-versions.ps1 -Strict
#>
[CmdletBinding()]
param(
    [switch]$Strict
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Read-FirstMatch {
    param([string]$Path, [string]$Pattern)
    if (-not (Test-Path $Path)) { return $null }
    $text = Get-Content -Raw -Path $Path
    # Multiline so '^' anchors at line starts, not just the start of the file.
    $m = [regex]::Match($text, $Pattern, [System.Text.RegularExpressions.RegexOptions]::Multiline)
    if ($m.Success) { return $m.Groups[1].Value.Trim() }
    return $null
}

$releaseVersion = '0.0.0'
if (Test-Path 'VERSION') {
    $releaseVersion = (Get-Content -Raw 'VERSION').Trim()
}

# Expected to equal VERSION.
$components = @(
    @{ Name = 'protocol (crate)';   Version = Read-FirstMatch 'protocol/Cargo.toml' '^\s*version\s*=\s*"([^"]+)"' }
    @{ Name = 'transport (crate)';  Version = Read-FirstMatch 'transport/Cargo.toml' '^\s*version\s*=\s*"([^"]+)"' }
    @{ Name = 'encoder (crate)';    Version = Read-FirstMatch 'host/encoder/Cargo.toml' '^\s*version\s*=\s*"([^"]+)"' }
    @{ Name = 'streamer (crate)';   Version = Read-FirstMatch 'host/streamer/Cargo.toml' '^\s*version\s*=\s*"([^"]+)"' }
    @{ Name = 'control-app (.NET)'; Version = Read-FirstMatch 'control-app/src/USBDisplay.ControlApp/USBDisplay.ControlApp.csproj' '<Version>([^<]+)</Version>' }
    @{ Name = 'android (Gradle)';   Version = Read-FirstMatch 'android/app/build.gradle.kts' 'versionName\s*=\s*"([^"]+)"' }
)

# Windows driver install version - separate 4-part namespace, reported only.
$independent = @(
    @{ Name = 'driver (INF)'; Version = Read-FirstMatch 'driver/idd/Driver.inf' 'DriverVer\s*=\s*[0-9/]+,\s*([0-9.]+)' }
    @{ Name = 'driver (.rc)'; Version = Read-FirstMatch 'driver/idd/Driver.rc' 'FILEVERSION\s+([0-9,]+)' }
)

Write-Host ''
Write-Host ("Release version (VERSION): {0}" -f $releaseVersion) -ForegroundColor Cyan
Write-Host ('=' * 62)

$drift = @()
foreach ($c in $components) {
    if (-not $c.Version) {
        Write-Host ('  {0,-20} {1}' -f $c.Name, '(not found)') -ForegroundColor DarkGray
        continue
    }
    if ($c.Version -eq $releaseVersion) {
        Write-Host ('  {0,-20} {1}' -f $c.Name, $c.Version) -ForegroundColor Green
    } else {
        Write-Host ('  {0,-20} {1}   (VERSION says {2})' -f $c.Name, $c.Version, $releaseVersion) -ForegroundColor Yellow
        $drift += $c.Name
    }
}

Write-Host ''
Write-Host 'Driver install version (independent 4-part PnP namespace):' -ForegroundColor Cyan
foreach ($c in $independent) {
    $v = $c.Version
    if (-not $v) {
        Write-Host ('  {0,-20} {1}' -f $c.Name, '(not found)') -ForegroundColor DarkGray
    } else {
        $shown = ($v -replace ',', '.')
        Write-Host ('  {0,-20} {1}' -f $c.Name, $shown) -ForegroundColor DarkGray
    }
}

Write-Host ''
if ($drift.Count -eq 0) {
    Write-Host 'All product components match VERSION.' -ForegroundColor Green
    exit 0
}

Write-Host ('Product components differing from VERSION: {0}' -f ($drift -join ', ')) -ForegroundColor Yellow
Write-Host 'This is informational unless -Strict is passed.' -ForegroundColor DarkGray
if ($Strict) { exit 1 }
exit 0
