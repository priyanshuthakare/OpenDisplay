#requires -Version 5.1
<#
    Captures the IddCx class-extension's own ETW provider (plus the UMDF platform
    provider) across a fresh driver load, to reveal exactly why IddCx rejects the
    bind (host error 0xD000000D = STATUS_INVALID_PARAMETER). IddCx's provider is a
    manifest/TraceLogging provider, so tracerpt decodes the messages.

    Run elevated.
#>
param([int]$WaitSeconds = 10)
$ErrorActionPreference = "Continue"
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Run capture-iddcx.ps1 elevated."
}
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$hardwareId = "Root\USBDisplayIdd"
$session = "UsbIddCap"
$etl = Join-Path $env:TEMP "usb_iddcx.etl"
$xml = Join-Path $env:TEMP "usb_iddcx.xml"

# IddCx class-extension provider (from RdpIdd.inf IddCx_Provider_Install), plus the
# two UMDF platform providers that log host driver/extension load decisions.
$providers = @(
    "{D92BCB52-FA78-406F-A9A5-2037509FADEA}",   # IddCx
    "{485E7DE9-0A80-11D8-AD15-505054503030}",   # WUDF (WUDFPlatform)
    "{30E7E769-8A2A-4A3C-BE0E-7C6C4C0C4F9A}"     # UMDF host (best-effort)
)

logman stop $session -ets 2>$null | Out-Null
Remove-Item $etl,$xml -Force -ErrorAction SilentlyContinue

Write-Host "Starting IddCx/UMDF trace ..."
# Create with first provider, then update to add the rest (logman update per provider).
logman create trace $session -p $providers[0] 0xFFFFFFFFFFFFFFFF 0xFF -o $etl -ets | Out-Null
foreach ($p in $providers[1..($providers.Count-1)]) {
    logman update trace $session -p $p 0xFFFFFFFFFFFFFFFF 0xFF -ets 2>$null | Out-Null
}

# Force a fresh load.
$dev = Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
    ((Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName 'DEVPKEY_Device_HardwareIds' -ErrorAction SilentlyContinue).Data -join ';') -match 'USBDisplayIdd'
} | Select-Object -First 1
if ($dev) { & pnputil /remove-device $dev.InstanceId 2>&1 | Out-Null; Start-Sleep 1 }
$devgen = Get-ChildItem "C:\Program Files (x86)\Windows Kits" -Recurse -Filter "devgen.exe" -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match "\\x64\\devgen.exe$" } | Select-Object -First 1 -ExpandProperty FullName
& $devgen /add /bus ROOT /hardwareid $hardwareId 2>&1 | Out-Null
Start-Sleep 2
& pnputil /add-driver (Join-Path $root "package\x64\Release\Driver.inf") /install 2>&1 | Out-Null
Write-Host "Capturing $WaitSeconds s ..."
Start-Sleep -Seconds $WaitSeconds

logman stop $session -ets | Out-Null
tracerpt $etl -o $xml -of XML -y 2>&1 | Out-Null

Write-Host ""
Write-Host "==================== IddCx / UMDF trace (errors + IddCx events) ====================" -ForegroundColor Cyan
if (Test-Path $xml) {
    [xml]$doc = Get-Content -Raw $xml
    $n = 0
    foreach ($e in $doc.Events.Event) {
        $lvl = $e.System.Level
        $prov = $e.System.Provider.Name
        # Collect all rendered data fields.
        $data = @()
        if ($e.EventData -and $e.EventData.Data) {
            foreach ($d in $e.EventData.Data) { $data += ("{0}={1}" -f $d.Name, $d.'#text') }
        }
        $txt = ($data -join "  ")
        # Show IddCx events and anything that looks like an error/status.
        if ($prov -match 'Idd' -or $txt -match 'IddCx|Idd|Status|Error|0x[0-9A-Fa-f]|Bind|Version') {
            Write-Host ("  [{0}] {1}: {2}" -f $lvl, $prov, $txt)
            $n++
            if ($n -ge 60) { break }
        }
    }
    if ($n -eq 0) { Write-Host "  (no decodable IddCx events; provider may not be manifest-registered)" }
} else { Write-Host "  (no XML produced)" }

Write-Host ""
Write-Host "ETL: $etl   XML: $xml"
