param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",
    [ValidateSet("x64")]
    [string]$Platform = "x64",
    [string]$CertificateSubject = "USBDisplay Development Driver Signing",
    # Online timestamping is OFF by default for development builds. Pass -Timestamp
    # to explicitly opt in (requires network access to $TimestampUrl).
    [switch]$Timestamp,
    [string]$TimestampUrl = "http://timestamp.digicert.com"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$packageDir = Join-Path $root "package\$Platform\$Configuration"
$catalog = Join-Path $packageDir "usbdisplayidd.cat"
$dll = Join-Path $packageDir "USBDisplayIdd.dll"

function Test-IsAdministrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]$identity
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-SignTool {
    $candidates = Get-ChildItem "C:\Program Files (x86)\Windows Kits" -Recurse -Filter "signtool.exe" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "\\x64\\signtool.exe$" } |
        Sort-Object FullName -Descending

    $tool = $candidates | Select-Object -First 1
    if (-not $tool) {
        throw "signtool.exe was not found. Install the Windows SDK signing tools."
    }
    return $tool.FullName
}

function Get-DevelopmentCertificate {
    param(
        [string]$Subject,
        [string]$StoreScope
    )

    Get-ChildItem "Cert:\$StoreScope\My" -CodeSigningCert -ErrorAction SilentlyContinue |
        Where-Object { $_.Subject -eq "CN=$Subject" -and $_.HasPrivateKey } |
        Sort-Object NotAfter -Descending |
        Select-Object -First 1
}

function Ensure-DevelopmentCertificate {
    param(
        [string]$Subject,
        [string]$StoreScope
    )

    $cert = Get-DevelopmentCertificate -Subject $Subject -StoreScope $StoreScope
    if ($cert) {
        return $cert
    }

    $cert = New-SelfSignedCertificate `
        -Type CodeSigningCert `
        -Subject "CN=$Subject" `
        -CertStoreLocation "Cert:\$StoreScope\My" `
        -KeyAlgorithm RSA `
        -KeyLength 2048 `
        -HashAlgorithm SHA256 `
        -KeyExportPolicy Exportable `
        -NotAfter (Get-Date).AddYears(5)

    if (-not $cert) {
        throw "Failed to create development code-signing certificate."
    }

    return $cert
}

# Runs an external tool, printing the exact command line and capturing/echoing
# stdout, stderr and the exit code. Returns the process exit code as an [int].
function Invoke-CapturedTool {
    param(
        [string]$FilePath,
        [string[]]$Arguments,
        [string]$Label
    )

    # Quote any argument containing whitespace/quotes so the printed line matches
    # exactly what is executed.
    $argLine = ($Arguments | ForEach-Object {
        if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ }
    }) -join ' '

    Write-Host ""
    Write-Host "[$Label] command line:"
    Write-Host "  `"$FilePath`" $argLine"

    $outFile = [System.IO.Path]::GetTempFileName()
    $errFile = [System.IO.Path]::GetTempFileName()
    try {
        $proc = Start-Process -FilePath $FilePath -ArgumentList $argLine -NoNewWindow -Wait -PassThru `
            -RedirectStandardOutput $outFile -RedirectStandardError $errFile
        $exitCode = $proc.ExitCode
        $stdout = (Get-Content -LiteralPath $outFile -Raw -ErrorAction SilentlyContinue)
        $stderr = (Get-Content -LiteralPath $errFile -Raw -ErrorAction SilentlyContinue)
    }
    finally {
        Remove-Item -LiteralPath $outFile, $errFile -Force -ErrorAction SilentlyContinue
    }

    Write-Host "[$Label] exit code: $exitCode"
    Write-Host "[$Label] stdout:"
    if ($stdout) { Write-Host ($stdout.TrimEnd()) } else { Write-Host "  <empty>" }
    Write-Host "[$Label] stderr:"
    if ($stderr) { Write-Host ($stderr.TrimEnd()) } else { Write-Host "  <empty>" }

    return [int]$exitCode
}

function Ensure-CertificateTrusted {
    param(
        [System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate,
        [string]$StoreScope
    )

    $tempCert = Join-Path $env:TEMP "USBDisplayDevelopmentDriverSigning.cer"
    Export-Certificate -Cert $Certificate -FilePath $tempCert -Force | Out-Null

    # Import-Certificate into the Root store triggers an interactive consent dialog
    # and fails with "UI is not allowed in this operation" in non-interactive
    # sessions, aborting the script before any signing happens. certutil -addstore
    # performs the same import non-interactively.
    $scopeFlag = if ($StoreScope -eq "CurrentUser") { @("-user") } else { @() }
    try {
        foreach ($store in @("Root", "TrustedPublisher")) {
            Write-Host "Trusting certificate $($Certificate.Thumbprint) in Cert:\$StoreScope\$store"
            $args = @("-addstore") + $scopeFlag + @("-f", $store, $tempCert)
            $exit = Invoke-CapturedTool -FilePath "certutil.exe" -Arguments $args -Label "certutil -addstore $store"
            if ($exit -ne 0) {
                throw "certutil failed (exit $exit) to add certificate to Cert:\$StoreScope\$store."
            }
        }
    }
    finally {
        Remove-Item -LiteralPath $tempCert -Force -ErrorAction SilentlyContinue
    }
}

function Invoke-SignTool {
    param(
        [string]$SignTool,
        [System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate,
        [string]$Path,
        [string]$StoreScope,
        [bool]$UseTimestamp,
        [string]$TimestampUrl
    )

    Write-Host ""
    Write-Host "============================================================"
    Write-Host "File being signed      : $Path"
    Write-Host "Certificate subject    : $($Certificate.Subject)"
    Write-Host "Certificate thumbprint : $($Certificate.Thumbprint)"
    Write-Host "Certificate store      : Cert:\$StoreScope\My"
    if ($UseTimestamp) {
        Write-Host "Timestamping           : ENABLED ($TimestampUrl)"
    } else {
        Write-Host "Timestamping           : DISABLED (development build; pass -Timestamp to enable)"
    }

    $signArgs = @("sign", "/v", "/fd", "SHA256", "/sha1", $Certificate.Thumbprint)
    if ($StoreScope -eq "LocalMachine") { $signArgs += "/sm" }
    if ($UseTimestamp) { $signArgs += @("/tr", $TimestampUrl, "/td", "SHA256") }
    $signArgs += $Path

    # Sign. Stop immediately on any non-zero exit; do not silently continue.
    $signExit = Invoke-CapturedTool -FilePath $SignTool -Arguments $signArgs -Label "signtool sign"
    if ($signExit -ne 0) {
        throw "signtool sign returned exit code $signExit for $Path. Aborting."
    }

    # Verify the signature immediately after signing and fail if it is missing.
    $verifyArgs = @("verify", "/v", "/pa", $Path)
    $verifyExit = Invoke-CapturedTool -FilePath $SignTool -Arguments $verifyArgs -Label "signtool verify"
    if ($verifyExit -ne 0) {
        throw "signtool verify returned exit code $verifyExit for $Path. Signature missing or untrusted."
    }

    Write-Host "OK: $Path signed and verified."
}

if (-not (Test-Path $catalog)) {
    throw "Catalog was not found at $catalog. Run build.ps1 first."
}
if (-not (Test-Path $dll)) {
    throw "Driver DLL was not found at $dll. Run build.ps1 first."
}

$signTool = Get-SignTool
Write-Host "signtool.exe: $signTool"

$storeScope = if (Test-IsAdministrator) { "LocalMachine" } else { "CurrentUser" }
Write-Host "USBDisplay signing certificate store scope: $storeScope"
Write-Host "Searching for certificate in store: Cert:\$storeScope\My"

if ($storeScope -eq "CurrentUser") {
    Write-Warning ("Running non-elevated: certificate trust will be placed in the CurrentUser stores only. " +
        "'pnputil /add-driver /install' requires elevation and validates trust against the LocalMachine stores, " +
        "so it will report 'root certificate which is not trusted' against this signing. " +
        "Re-run this script from an elevated (Administrator) prompt before installing the driver.")
}

$cert = Ensure-DevelopmentCertificate -Subject $CertificateSubject -StoreScope $storeScope
Ensure-CertificateTrusted -Certificate $cert -StoreScope $storeScope

$useTimestamp = [bool]$Timestamp
Invoke-SignTool -SignTool $signTool -Certificate $cert -Path $catalog -StoreScope $storeScope -UseTimestamp $useTimestamp -TimestampUrl $TimestampUrl
Invoke-SignTool -SignTool $signTool -Certificate $cert -Path $dll -StoreScope $storeScope -UseTimestamp $useTimestamp -TimestampUrl $TimestampUrl

& "$root\verify-signing.ps1" -Configuration $Configuration -Platform $Platform -CertificateSubject $CertificateSubject
