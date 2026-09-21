# Control App Integration Map

The Windows control app (`control-app/`) is the **orchestration, monitoring,
configuration, and diagnostics layer** around the existing USBDisplay stack.
It implements no driver, encoder, transport, or protocol logic itself.

Framework choice: **C# + .NET 8 + WPF (Option B)**. WPF was chosen over
WinUI 3 because it builds with the plain .NET SDK + MSBuild already present
in this repo's toolchain, needs no Windows App SDK runtime on the target
machine, and gives direct access to services, SetupAPI, display config,
tray icons, and UAC elevation. UI framework is an implementation detail;
all USBDisplay knowledge lives behind service interfaces.

## 1. Component inventory (existing repo)

| Component | Location | Interface the GUI uses |
|---|---|---|
| IDD driver 0.2.0.1 (`Root\USBDisplayIdd`) | `driver/idd/` | `install.ps1` / `uninstall.ps1` / `verify.ps1` (elevated); `pnputil`; SetupAPI PnP state |
| Streamer CLI (`usbdisplay-streamer`) | `host/streamer/` via `cargo build` | Subprocesses: `devices`, `capabilities`, `probe-frame`, `transport-probe`, `encode-capture`, `stream-capture`; stdout `key=value` lines |
| Encoder (Media Foundation) | `host/encoder/` | Via streamer stdout (`stream_encoder_backend=`, `backend …: SELECTED`) |
| Transport/protocol | `transport/`, `protocol/` | Via streamer stdout (`streamed_packets=`, `bitrate_step_*`); no direct calls |
| Android client | `android/` | Installed APK; observed via `adb` + streamer output |
| ADB | external (`adb` on PATH or `C:\platform-tools`) | `adb devices -l`, `adb install -r`, `adb shell monkey` |
| Capture frames | `%ProgramData%\USBDisplay\capture\capture_*.bmp` | Directory watch (file count / newest timestamp). **Nothing writes these today** — the driver's disk capture was removed and the in-memory handoff is unimplemented, so the Control Center's "no frames" gate is expected to fail until capture is wired up. |
| WiFi pairing store | `%AppData%\USBDisplay\paired.json` | Read-only display; never edited by the GUI |
| Driver logs | ETW `USBDisplay.IddDriver`, event logs `DriverFrameworks-UserMode/Operational`, `Kernel-PnP/Configuration`, `System` | `Get-WinEvent` via `wevtutil` / EventLog API (read-only) |

There are **no Windows services** in this repo (verified: no service
registration anywhere). The streamer and encoder run as child processes of
 whoever starts them. The GUI therefore manages **processes**, not services —
the Services page shows the WUDFRd reflector binding plus the GUI-tracked
component processes, and is honest that these are processes.

## 2. CLI contract (what the GUI parses)

`usbdisplay-streamer` already prints parseable `key=value` lines; the GUI
treats these as the stable contract and never scrapes free text:

- `devices`: `serial=… state=… model=… product=… transport_id=…`, or `no_android_devices=true`
- `capabilities`: `codecs=…`, `transport=…`, `capture=…`, `input=…`
- `stream-capture`: `wifi_device=`, `wifi_fingerprint=`, `wifi_encryption=`,
  `stream_frame_size=WxH`, `backend …: SELECTED`, `stream_encoder_backend=`,
  `stream_bitrate=`, `stream_gop=`, `adb_serial=`, `adb_forward=`,
  `android_connection=established`, `input_return_channel=`,
  `{"streamed_frames":N,"streamed_packets":M,"write_stall_ms_max":X,"input_events_injected":K}`,
  `bitrate_step_down/up old_bitrate=… new_bitrate=…`,
  `streamed_input_frames=… streamed_frames=… streamed_packets=…`,
  `input_events_injected=…`, `stream_complete=true`
- Exit code ≠ 0 + stderr = failure (e.g. missing `--device-ip`, TLS mismatch).

## 3. CONTROL APP INTEGRATION MAP

| Component | Existing interface | How GUI controls it | Privileges | Health check | Logs | Restart strategy |
|---|---|---|---|---|---|---|
| IDD driver | `install.ps1 -SkipBuild`, `uninstall.ps1`, `verify.ps1`, `pnputil` | Elevated `powershell -File` with `Verb=runas`; never silent | Admin (UAC prompt with reason) | `pnputil /enum-drivers` ∋ USBDisplay; PnP HWID `Root\USBDisplayIdd` present; service `WUDFRd`; problem code 0 | `verify.ps1` output; event logs | `Restart Driver` = disable+enable via `pnputil /restart-device` |
| Virtual monitor | Windows display stack | Read-only; `DisplaySwitch.exe`, `ms-settings:display` for user action | None for read | `EnumDisplayDevices` friendly name ∋ USBDisplay; `Screen.AllScreens` | N/A | Recreated by driver restart |
| Streamer process | `stream-capture …` child process | `ProcessStartInfo` with argument array (no shell strings); graceful stop = close stdin/clear flag then wait, kill after timeout | None (USB); capture dir may need admin to read | Process alive **and** `android_connection=established` **and** `streamed_frames` increasing | Captured stdout ring buffer + export | Bounded retries (3), then paused + error |
| Encoder | Inside streamer process | Flags `--codec/--bitrate/--fps/--gop` | None | `stream_encoder_backend=` line | Same as streamer | Encoder re-init is streamer's own `RateController` job |
| ADB/devices | `adb devices -l` | Poll every 5 s (no spam); `adb` path configurable | None | Parse state (`device` vs `unauthorized`/`offline`) | `adb` stderr surfaced | `adb reconnect` / server restart action |
| WiFi transport | Streamer `--transport wifi --device-ip --pin` | Same child-process management; QR values typed by user | None | Same as streamer + `wifi_encryption=tls-1.3` | Same as streamer | Same as streamer |
| App settings | `%AppData%\USBDisplay\control-app-settings.json` | Plain JSON read/write | None | Schema-tolerant load | N/A | N/A |

## 4. State machine

`Stopped → Starting(steps 1..N) → Active`, `→ Error` on any failed gate.
`Active → Stopping → Stopped`. ACTIVE requires **verified** health
(connection established + frames increasing), never mere process liveness.
Stop never uninstalls; Disable/Uninstall are separate confirmed danger-zone
actions under Settings → Advanced → Driver Management.

## 5. Privileges

User mode: status, logs, device info, non-invasive diagnostics.
Elevation (explicit UAC with reason string): driver install/remove/enable/
disable, `wevtutil` log enabling, capture-dir ACL repair. GUI never
self-elevates at launch.

## 6. Testing

`control-app/tests/` (MSTest): parsers (CLI `key=value`, `adb devices -l`,
`pnputil` blocks), startup/shutdown state machine, recovery backoff, config
round-trip, diagnostics aggregation — all against mock gateways, no hardware/
admin/driver required. `MockUsbDisplayGateway` also powers in-app demo mode.

## 7. Build & packaging

`dotnet build control-app/USBDisplay.sln -c Release`; tests via
`dotnet test`. Publish single-file:
`dotnet publish control-app/src/USBDisplay.ControlApp -c Release -r win-x64
--self-contained false -p:PublishSingleFile=true`. Installer: copy the
publish folder + repo `driver/idd` scripts; driver install stays script-based
(elevated) — no auto-updater.
