# USBDisplay Indirect Display Driver

This directory contains the Windows UMDF Indirect Display Driver for USBDisplay.

## Build Requirements

- Visual Studio with C++ desktop build tools
- Windows Driver Kit with IddCx headers and libraries
- Windows SDK 10.0.26100.0 or newer

The driver cannot build with the Windows SDK alone. The WDK installation must include `iddcx.h` and `IddCxStub.lib`.

## Build

From an elevated or normal PowerShell session:

```powershell
cd driver\idd
.\build.ps1 -Configuration Release
```

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

