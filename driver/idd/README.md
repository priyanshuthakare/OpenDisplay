# Windows Indirect Display Driver

USBDisplay requires a Windows Indirect Display Driver (IDD). The driver is the component that makes Windows expose the Android tablet as a real monitor in Display Settings.

## Implemented Driver Responsibilities

- Register an indirect display adapter.
- Create a virtual USBDisplay monitor when the adapter reaches D0.
- Advertise EDID modes:
  - 1920x1080
  - 2560x1440
  - 3840x2160
- Advertise refresh rates:
  - 60 Hz
  - 120 Hz for 2560x1440
- Handle hot plug, unplug, sleep, resume, rotation, and mode changes.
- Consume IddCx swap-chain frames and report frame completion to Windows.

## Implementation Base

The driver follows Microsoft's Indirect Display Driver sample callback model and replaces the sample monitor data with USBDisplay EDID, naming, modes, and install identity.

## Driver Boundary

The driver should not encode video, own Android device state, or run transport logic. Those live in `host/streamer`.

## Build and Install

See `BUILDING.md`.
