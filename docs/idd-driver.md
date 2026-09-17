# Indirect Display Driver (IDD)

The `driver/idd` component is a Windows UMDF Indirect Display Driver built on
IddCx. It presents a virtual `USBDisplay` monitor to the Windows display stack.
As of this milestone the driver **enumerates a real monitor, loads cleanly
(problem code 0), and drives an OS-assigned swap chain**.

## What runs today

- `USBDisplay` monitor is enumerated by Windows with a full 128-byte EDID
  produced by `BuildEdid()`, including the product-name descriptor.
- The device is `Status: OK`, `Problem: 0`, and appears as an additional
  display adapter (`USBDisplay [OK]`).
- Windows Display Settings can Extend or Duplicate onto the virtual monitor.

## Component map

| File | Responsibility |
| --- | --- |
| `Driver.cpp` | `DllMain`, `DriverEntry`, `DeviceAdd`, PnP/power and IddCx event callbacks |
| `Device.cpp/.h` | Adapter lifetime, monitor creation, arrival/departure |
| `IndirectMonitor.cpp` | Per-monitor swap-chain assignment |
| `SwapChainProcessor.cpp/.h` | D3D11 render device + the frame-acquire worker thread |
| `FrameCapture.cpp/.h` | `FrameCapturer` — GPU→CPU readback of the acquired surface, BGRA normalization (in-memory only) |
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
3. `IddCxSwapChainFinishedProcessingFrame` and repeat.

### Capture: reading back our own monitor

The surface returned by `IddCxSwapChainReleaseAndAcquireBuffer` is the OS-composed
image for **our** virtual monitor, allocated on the D3D device we passed to
`IddCxSwapChainSetDevice`. It can never contain the whole desktop or any other
display — so "capturing the USBDisplay monitor" is simply reading that surface
back, with no Desktop Duplication or whole-desktop API involved. `FrameCapturer`
does the GPU→CPU readback:

1. Lazily create a CPU-readable `D3D11_USAGE_STAGING` texture matching the
   surface's dimensions and format.
2. `CopyResource(staging, acquiredSurface)` on the render context.
3. `Map` the staging texture (blocking, so the copy is complete) and normalize
   the pixels to 32-bpp BGRA, honoring the mapped `RowPitch` (which is `>=`
   width×4 and often padded). `B8G8R8A8`, `R8G8B8A8`, and `R10G10B10A2` layouts
   are handled.

When enabled for diagnostics, the normalized frame can live in a CPU buffer
(`FrameCapturer::Pixels()`) as the hand-off point for the future encoder path.
The production frame loop does not persist frame data to disk.

## Debugging journey (why the code looks the way it does)

Getting from "installs but no monitor" to "enumerates, drives a swap chain, and
captures" surfaced several failure modes. The fixes are load-bearing, so they are
documented here.

- **Exception guard around the frame work.** A transient failure in the per-frame
  path was throwing out of the swap-chain worker thread and terminating the UMDF
  host, which showed up as periodic crashes in the System log. The frame loop
  now wraps all per-frame work in `try { ... } catch (...)`, and on any throw it
  logs a warning and disables the offending stage rather than taking down the
  host. This was the fix that turned intermittent crashes into a stable device.
  (Crash entries
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
- **Bounds-checked surface sizing.** The capturer only creates its staging
  texture once the acquired surface reports sane dimensions (`0 < w,h ≤ 8192`)
  and rejects multisampled surfaces (which cannot be copied to a plain staging
  texture).

## Verifying

`verify.ps1` reports a PASS/FAIL summary across the milestone gates: driver
package installed, UMDF reflector (WUDFRd), ROOT device present, driver loaded
(problem 0), adapter count, display count, and the `USBDisplay` monitor with its
decoded EDID. It also decodes common CM problem codes (28/31/37/39/41) to make
install failures self-explaining.

> Reinstalling a changed driver: PnP keys the driver store on the INF
> `DriverVer`. If you rebuild the DLL without bumping `DriverVer`, `pnputil`
> reports "up-to-date" and keeps running the **old** binary. Bump `DriverVer`
> (and/or run `uninstall.ps1` first) when validating driver changes.

## Not yet implemented

- Hardware encode of captured frames.
- USB transport of encoded frames to the Android client.
- Input return path (HID injection).
