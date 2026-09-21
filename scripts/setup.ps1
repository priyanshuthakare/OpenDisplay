<#
.SYNOPSIS
    USBDisplay local setup: check prerequisites, then build what is available.

.DESCRIPTION
    Detects each toolchain and reports what is missing. Nothing is installed
    automatically and nothing destructive is done — this only builds and tests
    the components whose prerequisites are present.

.PARAMETER Check
    Report prerequisites and exit without building.

.PARAMETER SkipTests
    Build only; do not run the test suites.

.EXAMPLE
    .\scripts\setup.ps1
    .\scripts\setup.ps1 -Check
#>
[CmdletBinding()]
param(
    [switch]$Check,
    [switch]$SkipTests
)

$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Test-Tool {
    param([string]$Name, [string]$Command)
    $cmd = Get-Command $Command -ErrorAction SilentlyContinue
    if ($cmd) {
        return @{ Name = $Name; Ok = $true; Detail = $cmd.Source }
    }
    return @{ Name = $Name; Ok = $false; Detail = 'not found on PATH' }
}

function Get-WdkDevgen {
    $tool = Get-ChildItem 'C:\Program Files (x86)\Windows Kits' -Recurse -Filter 'devgen.exe' -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\devgen\.exe$' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if ($tool) { return $tool.FullName }
    return $null
}

Write-Host ''
Write-Host 'USBDisplay - prerequisite check' -ForegroundColor Cyan
Write-Host ('=' * 60)

$isWindows = $true
if ($PSVersionTable.PSVersion.Major -ge 6) { $isWindows = $IsWindows }

$tools = @(
    (Test-Tool 'git'    'git'),
    (Test-Tool 'Rust'   'cargo'),
    (Test-Tool 'dotnet' 'dotnet'),
    (Test-Tool 'Java'   'java'),
    (Test-Tool 'adb'    'adb')
)

$missingCore = @()
foreach ($t in $tools) {
    if ($t.Ok) {
        Write-Host ('  [ok]   {0,-8} {1}' -f $t.Name, $t.Detail) -ForegroundColor Green
    } else {
        Write-Host ('  [MISS] {0,-8} {1}' -f $t.Name, $t.Detail) -ForegroundColor Yellow
    }
}

# The driver toolchain is Windows-only and optional for most contributors.
if ($isWindows) {
    $devgen = Get-WdkDevgen
    if ($devgen) {
        Write-Host ('  [ok]   WDK      {0}' -f $devgen) -ForegroundColor Green
    } else {
        Write-Host '  [MISS] WDK      devgen.exe not found (needed only to build the IDD)' -ForegroundColor Yellow
    }
}

Write-Host ''
Write-Host 'Prerequisite hints:' -ForegroundColor Cyan
Write-Host '  Rust    https://rustup.rs/'
Write-Host '  .NET 8  https://dotnet.microsoft.com/download/dotnet/8.0'
Write-Host '  JDK 17  https://adoptium.net/'
Write-Host '  adb     https://developer.android.com/tools/releases/platform-tools'
Write-Host '  WDK     https://learn.microsoft.com/windows-hardware/drivers/download-the-wdk'
Write-Host '  Android SDK + JDK 17 are required only for the tablet app.'
Write-Host ''

if ($Check) {
    Write-Host 'Check complete (no build performed).' -ForegroundColor Cyan
    exit 0
}

$failures = @()

# --- Rust workspace -------------------------------------------------------
if (Get-Command cargo -ErrorAction SilentlyContinue) {
    Write-Host '=== Rust workspace: build ===' -ForegroundColor Cyan
    cargo build --workspace --all-targets
    if ($LASTEXITCODE -ne 0) { $failures += 'cargo build' }

    if (-not $SkipTests) {
        Write-Host '=== Rust workspace: test ===' -ForegroundColor Cyan
        cargo test --workspace
        if ($LASTEXITCODE -ne 0) { $failures += 'cargo test' }
    }
} else {
    Write-Host 'Skipping Rust (cargo not found).' -ForegroundColor Yellow
}

# --- Control Center -------------------------------------------------------
if (Get-Command dotnet -ErrorAction SilentlyContinue) {
    $sln = Join-Path $root 'control-app\USBDisplay.sln'
    if (Test-Path $sln) {
        Write-Host '=== Control Center: build ===' -ForegroundColor Cyan
        dotnet build $sln -c Debug --nologo
        if ($LASTEXITCODE -ne 0) { $failures += 'dotnet build' }

        if (-not $SkipTests) {
            Write-Host '=== Control Center: test ===' -ForegroundColor Cyan
            dotnet test $sln --no-build --nologo
            if ($LASTEXITCODE -ne 0) { $failures += 'dotnet test' }
        }
    }
} else {
    Write-Host 'Skipping Control Center (dotnet not found).' -ForegroundColor Yellow
}

# --- Android --------------------------------------------------------------
if ((Get-Command java -ErrorAction SilentlyContinue) -and (Test-Path (Join-Path $root 'android\gradlew.bat'))) {
    Write-Host '=== Android: unit tests + debug APK ===' -ForegroundColor Cyan
    Push-Location (Join-Path $root 'android')
    & .\gradlew.bat testDebugUnitTest assembleDebug --console=plain
    if ($LASTEXITCODE -ne 0) { $failures += 'gradlew' }
    Pop-Location
} else {
    Write-Host 'Skipping Android (java or android/gradlew.bat not found).' -ForegroundColor Yellow
}

# --- Driver ---------------------------------------------------------------
if ($isWindows) {
    if (Get-WdkDevgen) {
        Write-Host 'Driver toolchain detected. To build + install (elevated, once):' -ForegroundColor Cyan
        Write-Host '  cd driver\idd; .\install.ps1'
    } else {
        Write-Host 'Skipping driver (WDK not found).' -ForegroundColor Yellow
    }
}

Write-Host ''
if ($failures.Count -gt 0) {
    Write-Host ('FAILED: {0}' -f ($failures -join ', ')) -ForegroundColor Red
    exit 1
}
Write-Host 'All requested builds and tests completed successfully.' -ForegroundColor Green
