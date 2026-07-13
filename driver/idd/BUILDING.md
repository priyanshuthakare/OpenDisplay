# USBDisplay Indirect Display Driver

This directory contains the Windows UMDF Indirect Display Driver for USBDisplay.

## Build Requirements

- Visual Studio with C++ desktop build tools
- Windows Driver Kit with IddCx headers and libraries
- Windows SDK 10.0.26100.0 or newer

The driver cannot build with the Windows SDK alone. The WDK installation must include `iddcx.h` and `IddCxStub.lib`.

## Build

From an elevated PowerShell session:

```powershell
cd driver\idd
.\build.ps1 -Configuration Release
```

The build script:

1. Builds `USBDisplayIdd.dll`.
2. Creates `package\x64\Release`.
3. Copies `USBDisplayIdd.dll` and `Driver.inf` into the package.
4. Runs `Inf2Cat` to create `usbdisplayidd.cat`.
5. Detects `signtool.exe`.
6. Creates a local development code-signing certificate if needed.
7. Installs that certificate into:
   - `LocalMachine\Root`
   - `LocalMachine\TrustedPublisher`
8. Signs `usbdisplayidd.cat`.
9. Signs `USBDisplayIdd.dll`.
10. Verifies the package is ready for `pnputil`.

The signing certificate subject is:

```text
CN=USBDisplay Development Driver Signing
```

The first signed build must run elevated because creating and trusting a LocalMachine certificate requires administrator rights. Later builds can reuse the existing certificate, but elevated PowerShell is still recommended for local driver development.

To compile and package without signing:

```powershell
.\build.ps1 -Configuration Release -SkipSigning
```

## Verify Signing

```powershell
cd driver\idd
.\verify-signing.ps1 -Configuration Release
```

The verifier reports:

- certificate found
- certificate trusted in root
- certificate trusted as publisher
- signtool found
- catalog signed
- DLL signed
- timestamp presence
- ready for `pnputil`

Timestamping uses `http://timestamp.digicert.com` by default. If timestamping is unavailable during offline development, the package is still signed and can be installed on a test-signing-enabled machine as long as the certificate is trusted locally.

## Install For Local Testing

From an elevated PowerShell session:

```powershell
cd driver\idd
.\install.ps1 -Configuration Release -EnableTestSigning
```

If test signing is newly enabled, reboot before installing the driver package again.

## Verify

```powershell
cd driver\idd
.\verify.ps1
```

Expected result:

- Device Manager contains `USBDisplay Indirect Display Adapter`.
- Windows monitor enumeration exposes a monitor named `USBDisplay`.
- Windows Display Settings can use the monitor for Extend or Duplicate mode.

`verify.ps1` prints a PASS/FAIL summary across the milestone gates (package
installed, WUDFRd reflector, ROOT device, driver loaded with problem code 0,
adapter count, display count, and the `USBDisplay` monitor with decoded EDID),
and decodes common CM problem codes (28/31/37/39/41) when a gate fails.

## Verify Capture

Once the monitor is enumerated, the swap-chain worker reads back the acquired
monitor surface and dumps a captured frame every 120 frames:

```powershell
Get-ChildItem "$env:ProgramData\USBDisplay\capture" | Select Name,Length,LastWriteTime
```

Expect `capture_000000.bmp`, `capture_000120.bmp`, … growing over time. Open the
newest in an image viewer to see the actual contents of the USBDisplay virtual
monitor (extend a window onto it first, or it may show blank wallpaper).
Consecutive dumps differ, which proves live capture. This is the deterministic
validation of real monitor capture before an encoder is added. See
[../../docs/idd-driver.md](../../docs/idd-driver.md) for the full architecture.

> When validating a rebuilt driver, bump `DriverVer` in `Driver.inf` (or run
> `uninstall.ps1` first). PnP keys the driver store on `DriverVer`; without a
> bump, `pnputil` keeps the previously installed binary and reports it
> "up-to-date."

## Uninstall

From an elevated PowerShell session:

```powershell
cd driver\idd
.\uninstall.ps1
```

This removes the `USBDisplay` device node and deletes the driver package from
the driver store. Run a hardware rescan or reboot if a stale node remains.
