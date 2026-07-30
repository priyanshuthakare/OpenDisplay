# Development Plan

## Phase 1: Virtual Display Driver

Milestones:

- Build Microsoft IDD sample unchanged.
- Replace static sample monitor with USBDisplay EDID model.
- Add dynamic monitor create/remove path.
- Validate Windows Display Settings mode changes.
- Add sleep, resume, hot plug, and rotation tests.

## Phase 2: Virtual Monitor Capture

Milestones:

- Enumerate the USBDisplay virtual monitor.
- Capture only that output.
- Verify no whole-desktop frame path exists.
- Add dirty rectangle tracking when available.

## Phase 3: Hardware Encoding

Milestones:

- Implement encoder capability probing.
- Prefer NVENC, QuickSync, AMF, then Media Foundation.
- Add H.265 baseline stream.
- Add bitrate, GOP, resolution, and scene-change controls.

Status: the backend selection chain (NVENC -> QuickSync -> AMF -> Media
Foundation) and the Media Foundation H.264 encoder are implemented. Captured
frames encode to H.264 and pass structural + decode-round-trip validation. The
vendor backends are stubs, and H.265 / rate-control tuning are still open. See
[encoder.md](encoder.md).

## Phase 4: USB Transport

Milestones:

- Use ADB forwarding for early compatibility.
- Implement native USB bulk endpoints.
- Add fragmentation, CRC, double buffering, and reconnect state machines.
- Detect USB 2 versus USB 3 throughput and adjust bitrate.

## Phase 5: Android Decode and Render

Milestones:

- Implement MediaCodec H.265 decode.
- Render to SurfaceView with frame pacing.
- Add adaptive buffering and decoder backpressure.
- Add 60 fps and 120 fps validation.

Status: the client decodes H.264/H.265 with `MediaCodec` to a `SurfaceView`.
Frames are now paced: a pure `FramePacer` maps each frame's presentation
timestamp to a `System.nanoTime` deadline anchored on the first frame, and the
decoder presents via the timestamped `releaseOutputBuffer(index, presentNs)` so
bursty USB arrival is smoothed to the encoded cadence. Backpressure is applied
by draining output and retrying instead of dropping payloads when the decoder is
full. Still open: adaptive buffering tuning and on-device 60/120 fps latency
validation (needs hardware).

## Phase 6: Input

Milestones:

- Map Android touch to Windows HID touch.
- Map stylus pressure, tilt, eraser, and buttons to Windows Ink.
- Add keyboard, IME, mouse absolute mode, mouse relative mode, and scroll wheel.

Status: the input return channel is implemented as a software slice. Android
captures touch (including batched historical move samples) and keyboard input,
normalizes pointer coordinates to `0..65535`, and sends fixed 16-byte tagged
input events back to the host inside transport `Control` packets. The host reads
them on a dedicated thread and injects absolute mouse move/click/scroll plus
keyboard (Unicode text via `KEYEVENTF_UNICODE` and named editing keys via
virtual-key codes) with Win32 `SendInput`. Still open: stylus pressure/tilt/
eraser, IME composition, relative mouse mode, and routing input as a dedicated
HID device bound to the virtual monitor (driver-side). See
[protocol.md](protocol.md#input-return-channel).

## Phase 7: Diagnostics and Packaging

Milestones:

- Tauri or Qt dashboard.
- Installer and driver signing flow.
- Real-time graphs for latency, USB bandwidth, drops, CPU, GPU, memory, and thermals.
- GitHub Actions CI.

