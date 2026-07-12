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

The first implemented slice is the shared USB stream protocol:

- Fixed binary frame header
- Timestamp, codec, flags, payload length, sequence number, and CRC32
- Fragmentation and reassembly helpers
- Reliable transport packet layer with ACK, heartbeat, CRC, retransmit-window bookkeeping, and reassembly
- Rust tests for round trips, CRC rejection, and fragmentation

The Windows IDD, GPU capture, encoder, Android decoder, and HID paths are documented with contracts and milestones so each subsystem can be implemented without changing the protocol shape.

## Status

USBDisplay is not yet a usable second-monitor application. The repository currently contains the project structure, protocol implementation, host CLI skeleton, Android client skeleton, and documentation needed to build the full system.

The Android screen will not show the Windows desktop until these pieces are implemented:

- Windows Indirect Display Driver
- Virtual monitor capture
- Hardware encoder
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

The current Android app opens a fullscreen `SurfaceView`. It is ready for the future decoder/rendering pipeline, but it does not receive video frames yet.

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

### 8. What You Can Do Today

Today, you can:

- Build and test the Rust protocol.
- Run the host CLI probe.
- Build and install the Android fullscreen client shell.
- Compile the Android transport decoder used by the future USB receive path.
- Verify ADB sees the tablet over USB.
- Use the docs in `docs/` to continue implementing the driver, capture, encoder, transport, decoder, and input layers.

You cannot yet:

- Extend the Windows desktop to Android.
- Duplicate the Windows desktop to Android.
- Change tablet display resolution from Windows.
- Use touch or stylus as Windows input.
- Stream real frames from Windows to Android.

## Non-Goals

- No cloud dependency
- No telemetry
- No Wi-Fi transport
- No whole-desktop capture
- No software decoding on Android
