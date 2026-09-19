# USBDisplay Control Center (`control-app/`)

Native Windows desktop application (C# + .NET 8 + WPF) that orchestrates,
monitors, configures, and diagnoses the USBDisplay stack. It implements no
driver/encoder/transport/protocol logic — it drives the existing binaries,
scripts, and Windows APIs. See `../docs/control-app.md` for the full
integration map (component inventory, CLI contract, state machine,
privileges, testing).

## Prerequisites

- Windows 10/11 x64.
- .NET 8 SDK (to build). The app itself needs the .NET 8 Desktop Runtime,
  unless published self-contained (see below).
- For driver install/uninstall: WDK `devgen.exe` + test-signing on + reboot
  (see `../driver/idd/BUILDING.md`); those steps run elevated from the GUI.
- Optional: `adb` (platform-tools) for the Device page and USB transport.

## Build

```powershell
dotnet build USBDisplay.sln -c Release
```

## Test

```powershell
dotnet test USBDisplay.sln
```

Unit tests (MSTest) cover CLI/adb/pnputil parsing, stream telemetry,
settings round-trip, log service, process crash bounds, and the
start/stop state machine — all against fakes, no hardware/admin needed.

## Run

```powershell
# Normal mode (drives the real repo components it can find)
dotnet run --project src/USBDisplay.ControlApp -- --demo   # demo mode: simulated stack
dotnet run --project src/USBDisplay.ControlApp             # real mode
```

Flags: `--demo` forces the mock gateway (UI shows DEMO MODE);
`--minimized` starts hidden in the tray (with Minimize-to-tray enabled).

On first run the app auto-detects: `usbdisplay-streamer` (app dir, then
`target/{debug,release}/`), `adb` (settings, then `C:\platform-tools`, then
PATH), driver scripts (`driver/idd`). Overrides live in
Settings → Advanced (stored in `%AppData%\USBDisplay\control-app-settings.json`).

## Publish / packaging

Framework-dependent single file (~0.5 MB, needs .NET 8 Desktop Runtime):

```powershell
dotnet publish src/USBDisplay.ControlApp -c Release -r win-x64 `
  --self-contained false -p:PublishSingleFile=true -o publish/win-x64-fx
```

Self-contained single file (~150 MB, runs anywhere):

```powershell
dotnet publish src/USBDisplay.ControlApp -c Release -r win-x64 `
  --self-contained true -p:PublishSingleFile=true `
  -p:IncludeNativeLibrariesForSelfExtract=true -o publish/win-x64-sc
```

To ship: copy the publish folder plus `driver/idd/install.ps1`,
`uninstall.ps1`, `verify.ps1`, and the signed driver package. Driver
install stays script-based and elevated — there is no auto-updater.

## Layout

```text
control-app/
  USBDisplay.sln
  src/USBDisplay.ControlApp/   WPF app (Option B per spec — no WinUI/AppSDK dependency)
    Mvvm/        ObservableObject, RelayCommand (no MVVM package)
    Models/      enums + records (state, telemetry, settings…)
    Parsing/     pure parsers: streamer key=value, adb devices -l, pnputil
    Native/      SetupAPI/CfgMgr + display P/Invoke (no PowerShell hosting)
    Services/    gateway, orchestrator, CLI/adb/driver/display/process/log/config/diagnostics
    ViewModels/  one per page + MainViewModel
    UI/          MainWindow, 8 views, SignalMonitor, dark theme
  tests/USBDisplay.ControlApp.Tests/   MSTest suite
```
