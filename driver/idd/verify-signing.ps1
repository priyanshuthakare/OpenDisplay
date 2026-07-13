param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",
    [ValidateSet("x64")]
    [string]$Platform = "x64",
    [string]$CertificateSubject = "USBDisplay Development Driver Signing"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$packageDir = Join-Path $root "package\$Platform\$Configuration"
$catalog = Join-Path $packageDir "usbdisplayidd.cat"
$dll = Join-Path $packageDir "USBDisplayIdd.dll"

function Get-SignTool {
    Get-ChildItem "C:\Program Files (x86)\Windows Kits" -Recurse -Filter "signtool.exe" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "\\x64\\signtool.exe$" } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
}

function Get-DevelopmentCertificate {
    foreach ($scope in @("LocalMachine", "CurrentUser")) {
        $cert = Get-ChildItem "Cert:\$scope\My" -CodeSigningCert -ErrorAction SilentlyContinue |
            Where-Object { $_.Subject -eq "CN=$CertificateSubject" -and $_.HasPrivateKey } |
            Sort-Object NotAfter -Descending |
            Select-Object -First 1
        if ($cert) {
            return [pscustomobject]@{
                Certificate = $cert
                StoreScope = $scope
            }
        }
    }
    return $null
}

function Test-CertificateInStore {
    param(
        [string]$Thumbprint,
        [string]$StorePath
    )

    if (-not $Thumbprint) {
        return $false
    }

    [bool](Get-ChildItem $StorePath -ErrorAction SilentlyContinue | Where-Object { $_.Thumbprint -eq $Thumbprint } | Select-Object -First 1)
}

function Test-AuthenticodeTimestamp {
    param([string]$Path)

    if (-not (Test-Path $Path)) {
        return $false
    }

    $signature = Get-AuthenticodeSignature -FilePath $Path
    return [bool]$signature.TimeStamperCertificate
}

function Invoke-SignToolVerify {
    param(
        [string]$SignToolPath,
        [string]$Path
    )

    if (-not (Test-Path $Path)) {
        return $false
    }

    & $SignToolPath verify /v /pa $Path *> $null
    return $LASTEXITCODE -eq 0
}

$signTool = Get-SignTool
$certInfo = Get-DevelopmentCertificate
$cert = if ($certInfo) { $certInfo.Certificate } else { $null }
$certScope = if ($certInfo) { $certInfo.StoreScope } else { "" }
$certTrustedRoot = $false
$certTrustedPublisher = $false

if ($cert) {
    $certTrustedRoot = Test-CertificateInStore -Thumbprint $cert.Thumbprint -StorePath "Cert:\$certScope\Root"
    $certTrustedPublisher = Test-CertificateInStore -Thumbprint $cert.Thumbprint -StorePath "Cert:\$certScope\TrustedPublisher"
}

$catalogSigned = $false
$dllSigned = $false
if ($signTool) {
    $catalogSigned = Invoke-SignToolVerify -SignToolPath $signTool.FullName -Path $catalog
    $dllSigned = Invoke-SignToolVerify -SignToolPath $signTool.FullName -Path $dll
}

$catalogTimestamp = Test-AuthenticodeTimestamp -Path $catalog
$dllTimestamp = Test-AuthenticodeTimestamp -Path $dll
$ready = [bool]($signTool -and $cert -and $certTrustedRoot -and $certTrustedPublisher -and $catalogSigned -and $dllSigned)

$report = [ordered]@{
    certificate_found = [bool]$cert
    certificate_thumbprint = if ($cert) { $cert.Thumbprint } else { "" }
    certificate_store_scope = $certScope
    certificate_trusted_root = $certTrustedRoot
    certificate_trusted_publisher = $certTrustedPublisher
    signtool_found = [bool]$signTool
    signtool_path = if ($signTool) { $signTool.FullName } else { "" }
    catalog_path = $catalog
    catalog_signed = $catalogSigned
    dll_path = $dll
    dll_signed = $dllSigned
    timestamp_present = [bool]($catalogTimestamp -and $dllTimestamp)
    catalog_timestamp_present = $catalogTimestamp
    dll_timestamp_present = $dllTimestamp
    ready_for_pnputil = $ready
}

foreach ($entry in $report.GetEnumerator()) {
    Write-Host ("{0}={1}" -f $entry.Key, $entry.Value)
}

if (-not $ready) {
    exit 1
}
