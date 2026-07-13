# Indirect Display Driver (IDD)

The `driver/idd` component is a Windows UMDF Indirect Display Driver built on
IddCx. It presents a virtual `USBDisplay` monitor to the Windows display stack.
As of this milestone the driver **enumerates a real monitor, loads cleanly
(problem code 0), and drives an OS-assigned swap chain**, rendering a
deterministic animated test pattern to prove the presentation path end to end.

This is the first subsystem to move from "documented contract" to "running on
hardware." Capture, encode, and USB transport are still ahead; the driver
currently *consumes* the OS-composed surface (as any IDD does) and writes a
local test pattern instead of encoding and sending frames.

## What runs today

- `USBDisplay` monitor is enumerated by Windows with a full 128-byte EDID
  produced by `BuildEdid()`, including the product-name descriptor.
- The device is `Status: OK`, `Problem: 0`, and appears as an additional
  display adapter (`USBDisplay [OK]`).
- The swap-chain worker composes an animated test pattern
  (SMPTE-style colour bars, moving gradient, bouncing square, and an FPS /
  frame-counter readout) and periodically writes it to disk as BMP.
- Windows Display Settings can Extend or Duplicate onto the virtual monitor.

## Component map

| File | Responsibility |
| --- | --- |
| `Driver.cpp` | `DllMain`, `DriverEntry`, `DeviceAdd`, PnP/power and IddCx event callbacks |
| `Device.cpp/.h` | Adapter lifetime, monitor creation, arrival/departure |
| `IndirectMonitor.cpp` | Per-monitor swap-chain assignment |
| `SwapChainProcessor.cpp/.h` | D3D11 render device + the frame-acquire worker thread |
| `RenderTest.cpp/.h` | `TestPatternRenderer` — CPU-composed animated pattern, BMP dump |
| `Edid.*` | `BuildEdid()` 128-byte EDID with product-name descriptor |
| `Trace.cpp/.h` | TraceLogging provider + `USBLOG_*` macros |

## Frame loop

`SwapChainProcessor::ProcessFrames` (`SwapChainProcessor.cpp`) is the heart of
the running driver:

1. Query `IDXGIDevice` from the render device and hand it to IddCx with
   `IddCxSwapChainSetDevice`. `DXGI_ERROR_ACCESS_LOST` here is expected when a
   fresh assign is immediately superseded — it returns quietly and IddCx
   reassigns.
2. Loop on `IddCxSwapChainReleaseAndAcquireBuffer`, waiting on the new-frame and
   stop events with a 16 ms timeout on `E_PENDING`.
3. Compose the test pattern into an offscreen buffer via `TestPatternRenderer`
   and dump a BMP every 120 frames.
4. `IddCxSwapChainFinishedProcessingFrame` and repeat.

The renderer writes to `%ProgramData%\USBDisplay\frames` (falling back to the
temp directory), creating each path component with `kernel32` only so nothing
extra is loaded into the sandboxed UMDF host.

## Debugging journey (why the code looks the way it does)

Getting from "installs but no monitor" to "enumerates and renders" surfaced
several failure modes. The fixes are load-bearing, so they are documented here.

- **Exception guard around rendering.** A transient failure in the render path
  was throwing out of the swap-chain worker thread and terminating the UMDF
  host, which showed up as periodic crashes in the System log. The frame loop
  now wraps all rendering in `try { ... } catch (...)`, and on any throw it logs
  a warning and disables the renderer rather than taking down the host. This was
  the fix that turned intermittent crashes into a stable device. (Crash entries
  in the System log dated before the current install are stale — the clean start
  has no crash after it.)
- **Adapter lifetime guard.** `D0Entry` can fire again after a `D0Exit` (power
  transitions). Creating a second adapter would leak the first and confuse
  IddCx, so `Device::InitializeAdapter` is guarded by `m_adapterInitStarted` and
  is idempotent.
- **Early, CRT-free signals.** `DllMain` emits `OutputDebugStringW` on
  process attach/detach and `DriverEntry` emits a raw string before any provider
  setup. These prove WDF version-bind succeeded and the module actually loaded
  into `WUDFHost`, independent of ETW/CRT state — essential when the driver
  failed *before* reaching normal logging.
- **TraceLogging throughout.** Every callback logs enter/exit and failure
  HRESULTs via `USBLOG_*`, so the full PnP/power/IddCx sequence
  (`DeviceAdd → D0Entry → InitializeAdapter → AdapterInitFinished →
  CreateMonitor → MonitorArrival → AssignSwapChain`) is observable in DebugView.
- **Bounds-checked surface sizing.** The renderer is only created once the
  acquired surface reports sane dimensions (`0 < w,h ≤ 8192`).

## Verifying

`verify.ps1` reports a PASS/FAIL summary across the milestone gates: driver
package installed, UMDF reflector (WUDFRd), ROOT device present, driver loaded
(problem 0), adapter count, display count, and the `USBDisplay` monitor with its
decoded EDID. It also decodes common CM problem codes (28/31/37/39/41) to make
install failures self-explaining.

To confirm the rendered pattern is live:

```powershell
Get-ChildItem "$env:ProgramData\USBDisplay\frames" | Select Name,Length,LastWriteTime
```

Expect `frame_000000.bmp`, `frame_000120.bmp`, … growing over time. Open the
newest in an image viewer to see the animation.

## Not yet implemented

- Capturing the OS-composed surface instead of drawing a local pattern.
- Hardware encode of captured frames.
- USB transport of encoded frames to the Android client.
- Input return path (HID injection).
