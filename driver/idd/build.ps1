param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",
    [ValidateSet("x64")]
    [string]$Platform = "x64"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$solution = Join-Path $root "USBDisplayIdd.sln"

$iddHeader = Get-ChildItem "C:\Program Files (x86)\Windows Kits" -Recurse -Filter "iddcx.h" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $iddHeader) {
    throw "IddCx WDK headers were not found. Install the Windows Driver Kit component that includes iddcx.h and IddCxStub.lib."
}

$msbuildCandidates = @(
    "C:\Program Files\Microsoft Visual Studio\18\Community\MSBuild\Current\Bin\amd64\MSBuild.exe",
    "C:\Program Files\Microsoft Visual Studio\18\Community\MSBuild\Current\Bin\MSBuild.exe",
    "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\MSBuild\Current\Bin\amd64\MSBuild.exe",
    "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\MSBuild\Current\Bin\MSBuild.exe"
)

$msbuild = $msbuildCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $msbuild) {
    throw "MSBuild was not found. Install Visual Studio Build Tools with C++ and WDK support."
}

$outputDir = Join-Path $root "bin\$Platform\$Configuration"
$intermediateDir = Join-Path $root "bin\obj\$Platform\$Configuration"
New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $intermediateDir "USBDisplayIdd.tlog") | Out-Null

& $msbuild $solution /m /p:Configuration=$Configuration /p:Platform=$Platform /v:minimal
if ($LASTEXITCODE -ne 0) {
    throw "USBDisplay IDD build failed with exit code $LASTEXITCODE."
}

$builtDll = Join-Path $outputDir "USBDisplayIdd.dll"
if (-not (Test-Path $builtDll)) {
    throw "USBDisplayIdd.dll was not produced at $builtDll."
}

$packageDir = Join-Path $root "package\$Platform\$Configuration"
New-Item -ItemType Directory -Force -Path $packageDir | Out-Null
Copy-Item -LiteralPath $builtDll -Destination (Join-Path $packageDir "USBDisplayIdd.dll") -Force
Copy-Item -LiteralPath (Join-Path $root "Driver.inf") -Destination (Join-Path $packageDir "Driver.inf") -Force

$inf2Cat = Get-ChildItem "C:\Program Files (x86)\Windows Kits" -Recurse -Filter "Inf2Cat.exe" -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match "\\x86\\Inf2Cat.exe$" } |
    Select-Object -First 1

if ($inf2Cat) {
    & $inf2Cat.FullName /driver:$packageDir /os:10_X64
    if ($LASTEXITCODE -ne 0) {
        throw "Inf2Cat failed with exit code $LASTEXITCODE."
    }
}

Write-Host "USBDisplay IDD build output:"
Write-Host "  $builtDll"
Write-Host "USBDisplay IDD driver package:"
Write-Host "  $packageDir"
