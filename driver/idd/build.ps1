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

& $msbuild $solution /m /p:Configuration=$Configuration /p:Platform=$Platform /v:minimal
if ($LASTEXITCODE -ne 0) {
    throw "USBDisplay IDD build failed with exit code $LASTEXITCODE."
}

