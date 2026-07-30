# USBDisplay

USBDisplay is an open-source Windows-to-Android secondary display stack designed for USB-only operation.

The target experience is the same mental model as plugging in a physical HDMI monitor:

- Windows sees a real secondary monitor through an Indirect Display Driver (IDD).
- The host captures only that virtual monitor.
- Frames are hardware encoded and streamed over USB.
- Android decodes with `MediaCodec` and presents through a low-latency surface.
- Touch, pen, keyboard, and mouse input return to Windows as HID-class input.

This repository is organized as a production system with independently buildable components.

```text
driver/
  idd/                 Windows Indirect Display Driver integration plan and driver source area
host/
  streamer/            Rust host service and diagnostics entry point
protocol/              Shared binary frame protocol
android/               Kotlin Android client
common/                Cross-component contracts and shared notes
tools/                 Developer tools
docs/                  Architecture and design documentation
tests/                 System and integration tests
benchmarks/            Latency and throughput benchmarks
scripts/               Build and packaging scripts
```

## Current Implementation Slice

Three slices are implemented today.

**Shared USB stream protocol:**

- Fixed binary frame header
- Timestamp, codec, flags, payload length, sequence number, and CRC32
- Fragmentation and reassembly helpers
- Reliable transport packet layer with ACK, heartbeat, CRC, retransmit-window bookkeeping, and reassembly
- Rust tests for round trips, CRC rejection, and fragmentation

**Windows Indirect Display Driver (IDD):**

- Enumerates a virtual `USBDisplay` monitor with a full 128-byte EDID
- Loads cleanly (problem code 0) and appears as an additional display adapter
- Drives an OS-assigned swap chain and captures the actual composed contents of
  the virtual monitor (GPU→CPU surface readback), dumping frames to disk as
  deterministic proof
- Extend and Duplicate work in Windows Display Settings

See [docs/idd-driver.md](docs/idd-driver.md) for the driver architecture, the
frame loop, the capture readback, and the debugging journey behind the current
build.

**Hardware encoder (host):**

- Reads the driver's captured `capture_*.bmp` frames and encodes them to an
  Annex-B H.264 elementary stream with a hardware Media Foundation encoder
- Backend selection chain (NVENC → Quick Sync → AMF → Media Foundation); Media
  Foundation is implemented, the vendor backends are honest stubs behind the
  same trait
- Each coded picture flows through the protocol `EncodedFrame` and transport
  `Packetizer`, proving the encode → frame → transport path
- Deterministic on-device validation: structural NAL check (SPS/PPS/IDR present)
  and a decode round-trip through the Media Foundation decoder MFT

See [docs/encoder.md](docs/encoder.md) for the backend chain, the pipeline, the
`encode-capture` command, and the validation strategy.

The USB transport, Android decoder, and HID paths are documented with contracts
and milestones so each subsystem can be implemented without changing the
protocol shape.

## Status

USBDisplay is not yet a usable second-monitor application. The repository
currently contains the project structure, protocol implementation, a working
Windows Indirect Display Driver that enumerates a virtual monitor, host CLI
skeleton, Android client skeleton, and documentation needed to build the full
system.

The Windows Indirect Display Driver now enumerates a virtual monitor and
captures its actual composed contents, and the host encodes those captured
frames to a validated H.264 stream. The Android screen will not show the
Windows desktop until these remaining pieces are implemented:

- Live shared-memory frame boundary from the driver to the encoder (the encoder
  currently reads the on-disk capture frames)
- USB or ADB transport session
- Android `MediaCodec` decoder

## Step-by-Step Guide

### 1. Install Prerequisites

On Windows, install:

- Rust stable: <https://rustup.rs/>
- Android Studio with Android SDK 35 or newer
- Android platform tools, including `adb`
- Windows Driver Kit for future IDD driver development
- A USB cable that supports data transfer

On the Android tablet:

- Enable Developer Options.
- Enable USB debugging.
- Connect the tablet to the Windows PC over USB.
- Accept the USB debugging prompt on the tablet.

### 2. Build and Test the Rust Workspace

```powershell
cargo test --workspace
```

This verifies the shared frame protocol used between the Windows streamer and Android client.

### 3. Run the Host Streamer Probe

```powershell
cargo run -p usbdisplay-streamer -- --help
cargo run -p usbdisplay-streamer -- devices
cargo run -p usbdisplay-streamer -- capabilities
cargo run -p usbdisplay-streamer -- probe-frame --width 1920 --height 1080 --refresh-millihz 60000
cargo run -p usbdisplay-streamer -- transport-probe --max-packet-payload 65536
```

Expected result:

- `devices` lists Android devices visible to `adb` over USB.
- `capabilities` prints the planned codecs, transport modes, capture mode, and input modes.
- `probe-frame` creates a synthetic encoded protocol frame and prints its byte size and CRC.
- `transport-probe` packetizes a synthetic frame using the reliable transport layer.

This does not stream the desktop yet. It only proves the host CLI and protocol layer are working.

### 3a. Encode Captured Frames (optional)

If the IDD driver has written frames to `%ProgramData%\USBDisplay\capture`, you
can encode them to a validated H.264 stream on the host:

```powershell
cargo run -p usbdisplay-streamer -- encode-capture `
    --input-dir "$env:ProgramData\USBDisplay\capture" `
    --out capture.h264 --codec h264 --fps 60 --bitrate 20000000 --gop 60 `
    --verify-decode
```

Expected result:

- The best available encoder backend is selected (Media Foundation today).
- The captured BMP frames encode to an Annex-B H.264 elementary stream.
- The stream passes the structural NAL check (`stream_playable=true`) and the
  decode round-trip (`decode_roundtrip=PASS`).

This proves the encode → protocol → transport path on real captured frames. It
does not yet send them to Android. See [docs/encoder.md](docs/encoder.md).

### 4. Verify the Android Device Is Connected

From a Windows terminal:

```powershell
adb devices
```

Expected result:

```text
List of devices attached
<device-id>    device
```

If the device says `unauthorized`, unlock the tablet and accept the USB debugging prompt.

### 5. Build and Install the Android App

From the Android project directory:

```powershell
cd android
.\gradlew.bat assembleDebug
adb install -r app\build\outputs\apk\debug\app-debug.apk
```

You can also open `android/` in Android Studio and press Run.

The current Android app opens a fullscreen `SurfaceView` and decodes
ADB-forwarded USBDisplay stream packets through `MediaCodec`.

The app now includes a local stream listener (`127.0.0.1:27183`) and a
`MediaCodec` decode/render path for the USBDisplay transport stream.

### 6. Open the Android Screen

After installing the app:

```powershell
adb shell monkey -p org.usbdisplay.client 1
```

The tablet should switch to the USBDisplay fullscreen surface.

### 7. Planned Connection Flow

When the driver, streamer, and decoder are implemented, the user flow will be:

1. Connect the Android tablet to the Windows PC with USB.
2. Start USBDisplay on Windows.
3. Start USBDisplay on Android.
4. Windows creates a virtual monitor through the IDD.
5. Windows Display Settings shows the tablet as an external monitor.
6. Choose Extend or Duplicate in Windows Display Settings.
7. The host captures only the virtual monitor, encodes it, and streams it over USB.
8. Android decodes and renders the stream.
9. Touch, pen, keyboard, and mouse input travel back to Windows.

### 7a. Stream Captured Frames to Android (ADB USB path)

This repository now provides a concrete USB debug streaming path from host to
tablet using `adb forward` and the shared transport protocol.

1. Build/install and open the Android app on the tablet.
2. Ensure `adb devices` shows the tablet as `device`.
3. Start host streaming:

```powershell
cargo run -p usbdisplay-streamer -- stream-capture `
    --input-dir "$env:ProgramData\USBDisplay\capture" `
    --codec h264 --fps 60 --bitrate 20000000 --gop 60
```

Optional flags:

- `--serial <adb-serial>` to target a specific tablet
- `--port <tcp-port>` to override `27183`
- `--max-frames <n>` for quick verification runs

Expected result:

- Host prints `android_connection=established`.
- The tablet displays decoded frames from the captured virtual-monitor stream.
- Transport uses USB (`adb` over cable), not Wi‑Fi.

### 8. What You Can Do Today

Today, you can:

- Build and test the Rust protocol.
- Run the host CLI probe.
- Build, sign, and install the Windows IDD, and see a virtual `USBDisplay`
  monitor enumerate in Windows Display Settings (Extend or Duplicate).
- Verify the driver captures the virtual monitor via the frames it writes to
  `%ProgramData%\USBDisplay\capture`.
- Encode those captured frames to a validated H.264 stream on the host
  (`encode-capture`), with structural and decode-round-trip checks.
- Build and install the Android fullscreen client shell.
- Run the Android local stream receiver + decoder with ADB-forwarded transport packets.
- Verify ADB sees the tablet over USB.
- Use the docs in `docs/` to continue implementing capture, encoder, transport, decoder, and input layers.

You cannot yet:

- Extend the Windows desktop to Android.
- Duplicate the Windows desktop to Android.
- Use touch or stylus as Windows input.
- Stream real frames from Windows to Android.

## Non-Goals

- No cloud dependency
- No telemetry
- No Wi-Fi transport
- No whole-desktop capture
- No software decoding on Android
