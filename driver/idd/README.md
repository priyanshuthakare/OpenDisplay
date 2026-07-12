# Windows Indirect Display Driver

USBDisplay requires a Windows Indirect Display Driver (IDD). The driver is the component that makes Windows expose the Android tablet as a real monitor in Display Settings.

## Driver Responsibilities

- Register an indirect display adapter.
- Create one virtual monitor per connected Android tablet.
- Advertise EDID modes:
  - 1920x1080
  - 2560x1600
  - 2880x1800
  - 3200x2000
  - 3840x2160
- Advertise refresh rates:
  - 60 Hz
  - 90 Hz
  - 120 Hz when the display path can sustain it
- Handle hot plug, unplug, sleep, resume, rotation, and mode changes.
- Expose frame acquisition to the user-mode streaming service without whole-desktop capture.

## Implementation Base

The driver should be implemented from Microsoft's Indirect Display Driver sample and built with the Windows Driver Kit. The sample already contains the correct UMDF/IDD control flow; USBDisplay should replace the sample monitor model with dynamic tablet-backed monitor instances.

## Driver Boundary

The driver should not encode video, own Android device state, or run transport logic. Those live in `host/streamer`.

