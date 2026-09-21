# USBDisplay

[![Rust](https://github.com/priyanshuthakare/OpenDisplay/actions/workflows/rust.yml/badge.svg)](https://github.com/priyanshuthakare/OpenDisplay/actions/workflows/rust.yml)
[![Android](https://github.com/priyanshuthakare/OpenDisplay/actions/workflows/android.yml/badge.svg)](https://github.com/priyanshuthakare/OpenDisplay/actions/workflows/android.yml)

USBDisplay is an open-source Windows-to-Android secondary display stack designed for **USB-first** operation, with **WiFi LAN (TLS 1.3 + PIN)** as an alternative transport.

The target experience is the same mental model as plugging in a physical HDMI monitor:

- Windows sees a real secondary monitor through an Indirect Display Driver (IDD).
- The host captures **only that virtual monitor** (never the whole desktop).
- Frames are hardware encoded (H.264 today, H.265 path exists) and streamed over USB.
- Android decodes with `MediaCodec` and presents through a low-latency surface.
- Touch, pen (basic), keyboard, and mouse input return to Windows as injected input (HID-class device is a later driver step).

No cloud. No telemetry. No account. Local link only.

```text
Windows Display Stack → IDD (USBDisplay monitor) → Capture (swap-chain readback)
  → Hardware Encoder (Media Foundation today) → Transport (USB ADB / WiFi TLS)
  → Android MediaCodec → SurfaceView
  ← Input return channel (Control packets → SendInput)
```

---

## Get started

> ### ⚠ Streaming does not work end-to-end yet
>
> The driver captures the virtual monitor into a CPU buffer, but **nothing hands
> those frames to the streamer**. The driver→host capture handoff is not
> implemented — `docs/PRD.md` FR-CAP-4 ("disk-free shared-memory handoff") is a
> future PR, and disk capture was deliberately removed, so no `capture_*.bmp`
> files are produced either.
>
> `stream-capture` therefore reports `no capture_*.bmp frames found` (or, with
> `--loop`, waits forever printing `waiting_for_capture_frames=true`) on **both**
> USB and Wi-Fi. Reinstalling the driver does not change this.
>
> **What does work:** everything downstream of capture, and it is tested. Point
> `--input-dir` at your own `.bmp` frames and the full encode → transport →
> Android decode path runs. Driver install and the virtual monitor, Wi-Fi
> pairing, the Control Center, and the encoder all work. See §3 for the roadmap.

**Just want to use it?** → **[QUICKSTART.md](QUICKSTART.md)** walks through the
driver and the tablet app. Prebuilt, no-compile-needed artifacts are on the
[Releases](https://github.com/priyanshuthakare/OpenDisplay/releases) page.

**Building from source?**

```powershell
git clone https://github.com/priyanshuthakare/OpenDisplay
cd OpenDisplay
.\scripts\setup.ps1 -Check   # report which toolchains are present
.\scripts\setup.ps1          # build + test everything available
```

**Releasing?** Push a tag and CI builds and publishes everything:

```powershell
.\scripts\check-versions.ps1 -Strict   # confirm components match VERSION
git tag v0.1.0 && git push origin v0.1.0
```

---

## 1. Repository Map

```text
driver/idd/             Windows UMDF Indirect Display Driver (IddCx, C++)
  Driver.cpp/.h         DllMain, DriverEntry, DeviceAdd, PnP/power, IddCx callbacks
  Device.cpp/.h         Adapter lifetime, monitor create/arrival/departure
  IndirectMonitor.cpp   Per-monitor swap-chain assignment
  SwapChainProcessor.*  D3D11 render device + frame-acquire worker thread
  FrameCapture.*        GPU→CPU readback, BGRA normalization (in-memory only)
  Edid.*                BuildEdid() 128-byte EDID + product-name descriptor
  Trace.*               TraceLogging provider + USBLOG_* macros
  *.ps1                 install / uninstall / verify / build / diagnose-bind /
                        capture-iddcx / capture-debug / capture-crash / sign-driver
  Driver.inf            PnP identity Root\USBDisplayIdd, DriverVer-keyed store

host/streamer/          Rust host service + CLI (usbdisplay-streamer)
  src/main.rs           CLI: devices, capabilities, probe-frame, transport-probe,
                        encode-capture, stream-capture
  src/stream_android.rs FrameSender, live vs replay loops, RateController,
                        USB + WiFi run paths, reverse input reader thread
  src/encode_capture.rs encode-capture command implementation
  src/adb.rs            adb list_devices parsing
  src/input_inject.rs   Win32 SendInput sink (mouse absolute + Unicode keys)
  src/wifi.rs           WiFi device parse, TLS 1.3 connect, pairing store
  src/pairing.rs        stable host_id (host-<COMPUTERNAME>), paired.json

host/encoder/           Rust encoder crate (usbdisplay-encoder)
  src/lib.rs, config.rs, frame.rs, bmp.rs, color.rs, nal.rs, validate.rs,
  probe.rs, backends/{mod,nvenc,qsv,amf,mediafoundation}.rs

protocol/               Shared binary frame protocol (usbdisplay-protocol)
  src/lib.rs            EncodedFrame, FrameHeader, Codec, FrameFlags, Fragment
  src/input.rs          InputEvent (16-byte pointer/key records)

transport/              Reliable packet layer (usbdisplay-transport)
  src/lib.rs            TransportPacket, Packetizer, ReassemblyBuffer,
                        RetransmitWindow, ReceiverAcks, HeartbeatMonitor

android/                Kotlin Android client (org.usbdisplay.client)
  MainActivity.kt       Fullscreen SurfaceView + touch/keyboard capture
  StreamSession.kt      USB listener 127.0.0.1:27183
  WifiListener.kt       WiFi TLS listener 0.0.0.0:27184
  StreamPipeline.kt     Shared framing → reassembly → MediaCodec → Surface
  FramePacer.kt         PTS → nanoTime deadline pacing
  transport/            TransportPacket.kt, InputEvent.kt
  pair/                 WifiPairActivity, ScanPcActivity, TabletIdentity,
                        LanAddress, PairPayload, PcPairPayload, Handshake,
                        PinVerifier, TrustedHosts, CertFingerprint, QrGenerator

control-app/            Windows Control Center GUI (.NET 8 + WPF, C#)
  src/USBDisplay.ControlApp/
    Mvvm/               ObservableObject, RelayCommand (no MVVM package)
    Models/             Records.cs (DeviceInfo, DriverInfo, DisplayInfo,
                        StreamTelemetry, AppSettings…), Enums.cs
    Parsing/            CliOutputParser (streamer key=value, adb, pnputil)
    Native/             SetupApi.cs, DisplayApi.cs (P/Invoke, no PS hosting)
    Services/           IUsbDisplayGateway + RealUsbDisplayGateway,
                        StreamerCli, AdbClient, DriverManager, DisplayManager,
                        ProcessManager, DiagnosticsService, LogService,
                        ConfigurationService, CaptureMonitor, HostIdentity,
                        QrCodeService, StreamTelemetryAccumulator
    ViewModels/         MainViewModel + Setup/Advanced containers + leaf VMs
    UI/                 MainWindow.xaml + views + SignalMonitor + dark theme
  tests/                MSTest: parsers, state machine, backoff, config,
                        diagnostics, pairing QR, capture monitor

docs/                   architecture.md, protocol.md, idd-driver.md, encoder.md,
                        wifi.md, control-app.md, development-plan.md, testing.md
scripts/                setup.ps1 (prereq check + guided build), check-versions.ps1
.github/workflows/      rust.yml, android.yml, dotnet.yml, release.yml (tagged releases)

QUICKSTART.md           End-user install + streaming walkthrough
CHANGELOG.md            Release history (Keep a Changelog)
LICENSE-APACHE, LICENSE-MIT   Dual license (Apache-2.0 OR MIT)
SECURITY.md             Security model + private vulnerability reporting
CONTRIBUTING.md         Build/test per component, conventions, PR flow
THIRD_PARTY_NOTICES.md  Bundled dependency licenses
CODE_OF_CONDUCT.md      Contributor Covenant 2.1
VERSION                 Single source of truth for the release version
```

---

## 2. What Is Implemented Today

Three slices are fully working, plus two transports and a GUI orchestration layer:

### 2.1 Shared USB stream protocol — DONE

- Fixed 50-byte binary frame header (`USBD`, version 1).
- Fields: sequence, timestamp_ns, codec (1=H.264, 2=H.265, 3=AV1), flags (keyframe/config/EOS), width, height, refresh_millihz, payload_len, payload_crc32, 8 reserved bytes.
- Fragmentation/reassembly keyed by frame sequence + fragment index.
- Rust unit + proptest round-trips, CRC rejection, fragmentation tests.

### 2.2 Reliable transport packet layer — DONE

- 40-byte header (`USBT`, version 1) wrapping every frame fragment.
- Kinds: `1=FrameFragment, 2=Ack, 3=Heartbeat, 4=KeyframeRequest, 5=Control, 6=Handshake`.
- ACK with selective-missing list, heartbeat monitor, retransmit-window bookkeeping, receiver ACK coalescing, reassembly buffer.
- Shared by ADB bridge, (future) native USB bulk, and WiFi TLS — same `u32LE(len) + USBT + USBD` framing on the wire.

### 2.3 Windows IDD virtual monitor — DONE

- Enumerates a virtual `USBDisplay` monitor with full 128-byte EDID from `BuildEdid()`.
- Loads cleanly (problem code 0), appears as additional display adapter.
- Extend and Duplicate work in Windows Display Settings.
- Frame loop `SwapChainProcessor::ProcessFrames`: `IddCxSwapChainSetDevice` → `ReleaseAndAcquireBuffer` loop (16 ms timeout on `E_PENDING`) → `FinishedProcessingFrame`.
- Capture is a readback of **our own swap-chain surface only** — cannot contain whole desktop. `FrameCapturer`: lazy staging texture, `CopyResource`, `Map`, BGRA normalize honoring `RowPitch`, handles `B8G8R8A8/R8G8B8A8/R10G10B10A2`.
- Hardening: try/catch around per-frame work (was killing WUDFHost), adapter-lifetime idempotency guard (`m_adapterInitStarted`), CRT-free early `OutputDebugStringW` signals, TraceLogging throughout, bounds-checked surface sizing, `verify.ps1` PASS/FAIL gates.
- The production loop keeps the normalized frame in a CPU buffer for an in-memory handoff and writes nothing to disk. **That handoff has no consumer yet** — see the banner above and FR-CAP-4.

### 2.4 Hardware encoder (host) — DONE (Media Foundation H.264)

- Backend chain `probe::select_encoder`: **NVENC → Quick Sync → AMF → Media Foundation**. Only MF implemented; vendor backends are honest stubs behind the same trait. Every backend prints its status so selection is always visible.
- Pipeline: `capture_*.bmp → BgraFrame → NV12 (CPU BT.601 limited-range) → MF hardware async MFT (METransformNeedInput/HaveOutput/DrainComplete, NOT sync ProcessInput which fails 0xC00D6D77) → Annex-B H.264 → EncodedFrame → Packetizer`.
- `encode-capture` validates deterministically: structural NAL check (SPS+PPS+≥1 IDR → `stream_playable=true`) + MF decoder MFT round-trip (decoded frame count must match; `1920x1088` coded size for 1080p is accepted macroblock rounding).
- Unit tests for BMP reader, color conversion, NAL parser need no hardware.

### 2.5 Encode → transport → decode path — DONE (given frames)

- `stream-capture` encodes frames found in `--input-dir` and streams over `adb forward tcp:27183 → 127.0.0.1:27183`. It does **not** capture anything itself: with no `.bmp` frames present it exits immediately, or waits forever printing `waiting_for_capture_frames=true` under `--loop`. See the banner at the top.
- **Live mode (default)**: always encodes newest capture, deletes stale backlog (bounds disk), wall-clock timestamps — tablet stays a real second monitor instead of falling behind. `--no-live` = ordered fixed-fps replay for demos/inspection.
- Android `StreamSession + StreamPipeline`: length-prefixed transport packets → frame reassembly → CRC check → `MediaCodec` decode → `SurfaceView`. Frame pacing via `FramePacer` (PTS → `nanoTime` deadline, timestamped `releaseOutputBuffer`), backpressure by draining/retrying instead of dropping when decoder is full.

### 2.6 Input return channel — DONE (software slice)

- Android captures touch (incl. batched historical MOVE samples) + keyboard, normalizes to `0..65535`, packs fixed 16-byte events, sends in transport `Control` packets on the same socket.
- Host reads on dedicated thread (`run_input_reader`), injects via Win32 `SendInput`: absolute mouse mapped into the virtual monitor's rectangle (`MOUSEEVENTF_ABSOLUTE|VIRTUALDESK`), Unicode text (`KEYEVENTF_UNICODE`), named keys via VK codes. `input_events_injected` counter proves delivery; `input_target_monitor=` on stderr records which monitor was resolved.
- Pinned by matching Rust + Kotlin reference-byte tests so wire formats never drift.
- Still open: stylus pressure/tilt/eraser, IME composition, relative mouse mode, dedicated HID device bound to virtual monitor (driver-side).

### 2.7 WiFi LAN transport (TLS 1.3 + PIN) — DONE

- Alternative to USB, same framing/pipeline/decoder — only the socket differs.
- Tablet `WifiListener` on `SSLServerSocket(0.0.0.0:27184, TLSv1.3 only)`; host dials with rustls, 5 s timeout, `TCP_NODELAY`.
- Pairing: tablet shows LAN IP + `SHA256:xxxx…` fingerprint + 6-digit PIN + QR `{"v":1,"ip":"…","port":27184,"fp":"SHA256:…"}`. Host sends Hello as Handshake kind-6 `{"v":1,"pin":"…","host_id":"host-<pc>","codecs":["h264","h265"]}`, expects Welcome `{"v":1,"accept":true,…}`. Constant-time PIN compare, 3 strikes → 30 s lockout, trusted `host_id` skips PIN next time (TLS fp still enforced). TOFU store `%AppData%\USBDisplay\paired.json`. Cert: ECDSA P-256 self-signed 10-yr `CN=USBDisplay-Tablet`, in-memory via BouncyCastle, PKCS8+DER Base64 in private prefs.
- Reverse scan-to-trust: PC shows QR `{"v":1,"host_id":"host-<pc>"}` (C# `HostIdentity` mirrors Rust `pairing::stable_host_id`), tablet `ScanPcActivity` (Camera2 + bundled ZXing-core) adds it to trusted set.
- Perf: WiFi defaults 12 Mbps/GOP 30 (USB 20 Mbps/GOP 60) unless user overrode flags; `RateController` ladder `20→12→8→4 Mbps` (p95 stall >50 ms over 60-frame window OR >2 KF/window steps down; 600 clean frames steps up; encoder re-created via `select_encoder`); `--stats-json` every 60 frames; contract 1080p60 `<80 ms p50 / <120 ms p95` on WiFi 5/6.
- No plaintext fallback (`--insecure-lan` removed). AP-isolated WLANs print `host unreachable … use USB` and never silently fall back.

### 2.8 Control Center GUI — DONE

- Native WPF (.NET 8, Option B — no WinUI/AppSDK runtime needed), MVVM without packages, dark theme, tray support, `--demo` mock-gateway mode, `--minimized`.
- Orchestrates driver scripts + streamer child process + adb + display APIs. Implements **no** driver/encoder/transport logic itself.
- Gated start state machine `Stopped → Starting(steps) → Active (verified: connection + increasing frames) → Stopping → Stopped`, `→ Error` on any failed gate. Bounded streamer auto-restart (3), first-run overlay, guided diagnostics, capture-dir age-gated purge, elevation only for driver/log/ACL ops (never self-elevates at launch).

---

## 3. Status / Roadmap

| Phase | Scope | Status |
|---|---|---|
| 1 Virtual Display Driver | IDD sample → USBDisplay EDID → dynamic create/remove → mode changes, sleep/resume/hotplug/rotation | **Done** for enumerate + Extend/Duplicate + swap-chain. Sleep/resume/hotplug/rotation tests still open. |
| 2 Virtual Monitor Capture | Enumerate virtual monitor only, no whole-desktop path, dirty rects | **Blocked** — readback into a CPU BGRA buffer works and is inherently monitor-only, but there is **no handoff to the host process**, so no frames ever reach the streamer. Handoff (FR-CAP-4) and dirty-rect tracking open. |
| 3 Hardware Encoding | Probe chain, MF impl, H.265 baseline, bitrate/GOP/scene controls | **Partial**: chain + MF H.264 + validation done. NVENC/QSV/AMF stubs, H.265 E2E validation, rate-control tuning open. |
| 4 USB Transport | ADB bridge → native bulk, fragmentation/CRC/double-buffer/reconnect, USB2/3 adaptation | **Partial**: ADB bridge + fragmentation/CRC + live mode done; a Windows mode switch is now absorbed live (the encoder is rebuilt and the sender retagged, `stream_resolution_change`) instead of aborting. Native bulk endpoints, disk-free SHM handoff, reconnect SM, USB2/3 adaptation open. |
| 5 Android Decode/Render | H.265 decode, SurfaceView pacing, adaptive buffering/backpressure, 60/120 fps validation | **Partial**: H.264/H.265 decode + pacing + backpressure done. Buffer tuning + on-device 60/120 fps latency validation need hardware. |
| 6 Input | Touch→HID, stylus pressure/tilt/eraser/Ink, keyboard/IME, mouse abs/rel/scroll | **Partial**: touch + keyboard software slice done, with coordinates mapped into the virtual monitor's rectangle so multi-monitor desktops are correct. Stylus/IME/relative/HID-device binding open. |
| 7 Diagnostics/Packaging | Dashboard, installer + signing, latency/bandwidth graphs, CI | **Partial**: Control Center (3-page shell: Home / Setup / Advanced) + `verify.ps1` + GitHub Actions (Rust ubuntu+windows, Android JDK17 test+APK, .NET test on windows-latest) done. Installer/signing flow + realtime graphs open. Lint (`fmt --check`, `clippy`) runs non-blocking — pre-existing debt, not a merge gate yet. |

**Release readiness:** the repository is *packaged* for an open-source release — dual `LICENSE-*`, `SECURITY.md`, `CONTRIBUTING.md`, `THIRD_PARTY_NOTICES.md`, no committed IDE/build junk, honest `capabilities`, aligned driver version, a tag-triggered release workflow, and CI covering all three buildable components. It is **not yet usable as a second display**: the driver→streamer capture handoff is missing (see the banner at the top), and that is the single blocking gap. The correctness findings from the release review are closed — multi-monitor input mapping (§4.5), live resolution changes (§4.2/§4.4), strict pairing-payload parsing — but they stay latent until capture is wired up. Remaining gaps are tracked in [`docs/review-findings.md`](docs/review-findings.md).

**Today you can:** build/test the Rust protocol, run CLI probes, sign+install the IDD and see the virtual monitor, `encode-capture` your own BMPs to validated H.264, and stream those frames through the full transport → Android decode path over USB or Wi-Fi.

**You cannot yet:** stream the virtual monitor at all — the driver→streamer capture handoff is not implemented (the blocking gap); use stylus pressure/tilt as Windows input; route input as a dedicated HID device bound to the virtual monitor; do native USB bulk (ADB bridge only); expect tuned 1440p120 / USB2-3 auto-adaptation.

---

## 4. Interface Details

This section is the contract reference. Field orders, byte orders, ports, paths, and `key=value` lines below are normative for this milestone.

### 4.1 Process & network topology

```text
Windows host                          Link                          Android tablet
────────────────                      ────                          ──────────────
IDD virtual monitor
  → BGRA frame in the driver process  ╳  no handoff implemented (blocking gap)
  → MF H.264 encode ─┐
                     ├─► FrameSender ─► USB: adb forward tcp:27183 ─► ServerSocket(127.0.0.1:27183)
  ◄─ input events ───┘   (u32LE+USBT)  WiFi: TLS 1.3 → ip:27184 ────► SSLServerSocket(0.0.0.0:27184)
       SendInput                                                              MediaCodec → Surface
```

`stream-capture` takes its frames from `--input-dir` (BMP files). Point it at your
own frames to exercise everything below the gap.

| Endpoint | Value | Notes |
|---|---|---|
| USB video + input | `127.0.0.1:27183` via `adb forward tcp:27183 tcp:27183` | Default `--transport usb`. ADB removed on drop (`AdbForwardGuard`). |
| WiFi video + input | `<tablet-LAN-IP>:27184`, TLS 1.3 only | `--transport wifi --device-ip … --pin …`. No plaintext. |
| Capture dir | `%ProgramData%\USBDisplay\capture\capture_*.bmp` | 32-bpp top-down `BI_RGB`. **Nothing writes this today** — the driver's disk capture was deliberately removed and the in-memory handoff is not implemented. `stream-capture` reads whatever frames you place here. |
| WiFi TOFU store | `%AppData%\USBDisplay\paired.json` → `{tablet_id, ip, cert_fingerprint, paired_at}` | Read-only in GUI; never edited by GUI. |
| GUI settings | `%AppData%\USBDisplay\control-app-settings.json` | Plain JSON, schema-tolerant load (see 4.8). |
| Driver PnP ID | `Root\USBDisplayIdd`, driver `0.2.0.1` | 4-part install version, independent of the product version in `VERSION` (PnP keys the store on it). `install.ps1 / uninstall.ps1 / verify.ps1`, `pnputil`, SetupAPI. |
| Driver logs | ETW `USBDisplay.IddDriver`, `DriverFrameworks-UserMode/Operational`, `Kernel-PnP/Configuration`, `System` | Read via `wevtutil`/EventLog (read-only). |

### 4.2 Host CLI (`usbdisplay-streamer`)

```powershell
cargo run -p usbdisplay-streamer -- --help
cargo run -p usbdisplay-streamer -- devices
cargo run -p usbdisplay-streamer -- capabilities
cargo run -p usbdisplay-streamer -- probe-frame --width 1920 --height 1080 --refresh-millihz 60000
cargo run -p usbdisplay-streamer -- transport-probe --max-packet-payload 65536
cargo run -p usbdisplay-streamer -- encode-capture --input-dir "$env:ProgramData\USBDisplay\capture" --out capture.h264 --codec h264 --fps 60 --bitrate 20000000 --gop 60 --verify-decode [--max-frames N]
cargo run -p usbdisplay-streamer -- stream-capture --input-dir "$env:ProgramData\USBDisplay\capture" --codec h264 --fps 60 --bitrate 20000000 --gop 60 --loop [--no-live] [--port 27183] [--serial <adb>] [--max-frames N] [--stats-json]
cargo run -p usbdisplay-streamer -- stream-capture --transport wifi --device-ip 192.168.1.42 --pin 123456 --input-dir "$env:ProgramData\USBDisplay\capture" --loop [--stats-json]
```

`--device-ip` accepts bare IP, `ip:port`, or full QR JSON `{"v":1,"ip":"…","port":27184,"fp":"SHA256:…"}`. `--live` defaults true (newest-frame-wins); `--no-live` = ordered fixed-fps replay.

**Stable `key=value` stdout contract** (the GUI parses only these, never free text):

| Command | Lines |
|---|---|
| `devices` | `serial=… state=Device|Unauthorized|Offline|Other model=… product=… transport_id=…` per device, or `no_android_devices=true` |
| `capabilities` | `codecs=h264,h265`, `transport=adb-compat,wifi-tls`, `capture=virtual-monitor-only`, `input=mouse,keyboard` |
| `probe-frame` | `probe_frame_bytes=N`, `payload_crc32=0x…` |
| `transport-probe` | `transport_packets=N`, `transport_bytes=N`, `max_packet_payload=N` |
| `encode-capture` | `encoder_backend=MediaFoundation`, `encoded_units=N`, `nal_total=N sps=… pps=… vps=… idr=N non_idr=N`, `stream_playable=true`, `transport_packets=N`, `decoded_frames=N decoded_size=WxH`, `decode_roundtrip=PASS`, `OVERALL: PASS …` |
| `stream-capture` (USB) | `stream_frame_size=WxH`, `backend …: SELECTED`, `stream_encoder_backend=…`, `stream_bitrate=… stream_gop=…`, `adb_serial=…`, `adb_forward=tcp:P->tcp:P`, `waiting_for_android_listener=true`, `android_connection=established`, `input_return_channel=enabled\|disabled …`, `input_target_monitor=…` (stderr, Windows input), `streamed_input_frames=… streamed_frames=… streamed_packets=…`, `input_events_injected=N`, `stream_resolution_change old=WxH new=WxH` (on a Windows mode switch), `stream_complete=true`, optional `{"streamed_frames":N,"streamed_packets":M,"write_stall_ms_max":X,"input_events_injected":K}` every 60 frames with `--stats-json`, `bitrate_step_down/up old_bitrate=… new_bitrate=…` |
| `stream-capture` (WiFi) | `wifi_device=ip:port`, `wifi_fingerprint=SHA256:…`, `wifi_pin=omitted …` (if skipped), `wifi_encryption=tls-1.3`, `wifi_defaults_applied bitrate=… gop=…` (if defaulted), then same streaming lines as USB |
| Failures | Exit ≠ 0 + stderr (e.g. missing `--device-ip`, TLS fp mismatch, no BMPs) |

### 4.3 Frame protocol (`protocol/`, `USBD`, v1, LE)

50-byte header + payload. Rust: `EncodedFrame::new/encode/decode/fragment/reassemble`. Max payload 128 MiB.

| Field | Size | Description |
|---|---|---|
| magic | 4 | ASCII `USBD` |
| version | 2 | `1`, little endian |
| header_len | 2 | `50` |
| sequence | 8 | Monotonic frame sequence |
| timestamp_ns | 8 | Host capture timestamp (live: wall clock; replay: synthetic fps cadence) |
| codec | 1 | `1=H.264, 2=H.265, 3=AV1` |
| flags | 1 | bit0 keyframe, bit1 config, bit2 EOS |
| width | 2 | Encoded width |
| height | 2 | Encoded height |
| refresh_millihz | 4 | e.g. `60000` |
| payload_len | 4 | Compressed bytes |
| payload_crc32 | 4 | CRC32 (IEEE) of payload |
| reserved | 8 | Zero, forward-compat |

Fragmentation: `EncodedFrame::fragment(max)` splits `encode()` bytes into `{frame_sequence, index, total, bytes}`; missing fragment → drop frame + request keyframe; CRC mismatch → drop + request keyframe; late frame → drop if over latency budget.

### 4.4 Transport packets (`transport/`, `USBT`, v1, LE)

On the wire every packet is `u32LE(total_packet_bytes) + TransportPacket::encode()`. 40-byte header + payload. `Packetizer::new(max_payload)` (default 64 KiB) assigns `packet_sequence` from 1.

| Field | Size | Description |
|---|---|---|
| magic | 4 | ASCII `USBT` |
| version | 2 | `1` |
| header_len | 2 | `40` |
| kind | 1 | `1=FrameFragment 2=Ack 3=Heartbeat 4=KeyframeRequest 5=Control 6=Handshake` (+3 reserved) |
| packet_sequence | 8 | Transport sequence (ACK/retransmit key) |
| frame_sequence | 8 | Parent frame (fragment/KF-request) else 0 |
| fragment_index | 2 | Fragment number |
| fragment_total | 2 | Fragment count |
| payload_len | 4 | Payload bytes |
| payload_crc32 | 4 | CRC32 of payload |

> **What is live today:** the packetizer only *frames* fragments. Both transports
> run over TCP, which already delivers reliably, so **nothing is retransmitted
> and no heartbeat drives a reconnect.** The primitives below are implemented and
> unit tested, but are not wired into the streaming paths — they exist for the
> planned native USB bulk endpoint (no TCP underneath) and to pin the wire
> contract for packet kinds video-only receivers must ignore. See PRD FR-TR-3/4.

- `Ack` payload: `through_packet_sequence:u64 + missing:u64[]`. `ReceiverAcks::observe` computes contiguous high-water + gaps.
- `RetransmitWindow` tracks only `FrameFragment`; `expired(now)` lists timed-out packets; `apply_ack` clears acked.
- `HeartbeatMonitor(timeout, last_seen)` → `is_disconnected` (intended to drive reconnect; unused today).
- `Control` payload = 1+ concatenated 16-byte `InputEvent`s (device→host). `Handshake` payload = opaque JSON (WiFi hello/welcome); video-only receivers ignore non-Fragment kinds.

### 4.5 Input return channel (device→host, 16 bytes LE, kind-tagged)

Shared definition: Rust `protocol/src/input.rs` ↔ Kotlin `transport/InputEvent.kt`, pinned by identical reference-byte tests.

Pointer (`kind=1`):

| Off | Size | Field |
|---|---|---|
| 0 | 1 | kind=`1` |
| 1 | 1 | action `1=down 2=move 3=up 4=scroll` |
| 2 | 1 | button `0=left 1=right 2=middle 255=none` |
| 3 | 1 | pointer_id (multi-touch slot, 0=primary) |
| 4 | 2 | x normalized `0..65535` across surface width |
| 6 | 2 | y normalized `0..65535` across surface height |
| 8 | 2 | scroll_x signed notches (scroll only) |
| 10 | 2 | scroll_y signed notches (scroll only) |
| 12 | 4 | reserved |

Key (`kind=2`):

| Off | Size | Field |
|---|---|---|
| 0 | 1 | kind=`2` |
| 1 | 1 | action `1=down 2=up` |
| 2 | 1 | named `0=char 1=enter 2=backspace 3=tab 4=escape 5=delete 6-9=arrows 10=home 11=end` |
| 3 | 1 | reserved (modifiers, future) |
| 4 | 2 | Unicode BMP code point (when named=`char`) |
| 6 | 10 | reserved |

Reference bytes: pointer `Down/Left/id2 (40000,12345)` → `01 01 00 02 40 9C 39 30 00 00 00 00 00 00 00 00`; key `'A'` down → `02 01 00 00 41 00 00…`. The host maps normalized coords into the **virtual monitor's rectangle** within the virtual desktop and injects with `MOUSEEVENTF_ABSOLUTE|VIRTUALDESK` (multi-monitor safe; falls back to the whole desktop when the monitor is absent); text via `KEYEVENTF_UNICODE`, named keys via VK.

### 4.6 Encoder interface (`host/encoder`)

- `EncoderConfig::new(w,h).with_fps().with_bitrate().with_gop().with_codec(H264|H265)` → `probe::select_encoder(&config) -> (Option<Box<dyn VideoEncoder>>, Vec<BackendStatus>)`.
- `VideoEncoder::encode(&BgraFrame, timestamp_ns) -> Vec<EncodedUnit{bytes, keyframe, timestamp_ns}>`, `drain()`, `backend_name()`.
- `bmp::read_bgra_bmp` (driver's 32-bpp top-down `BI_RGB`), `color::bgra_to_nv12` (BT.601 limited-range, CPU unit-tested), `nal::parse` (Annex-B split, SPS/PPS/IDR detect), `validate::roundtrip` (MF decoder MFT frame count).
- CLI surface is `encode-capture` / `stream-capture` flags `--codec h264|h265 --bitrate --fps --gop --max-frames --verify-decode`.

### 4.7 Driver interface (`driver/idd`)

- Build/install: `build.ps1`, `install.ps1 [-SkipBuild]`, `uninstall.ps1`, `verify.ps1`, `verify-signing.ps1`, `sign-driver.ps1`, `diagnose-bind.ps1`, `capture-*.ps1` (diagnostics). Reinstall rule: PnP keys store on INF `DriverVer` — bump it (or uninstall first) or `pnputil` keeps the old binary.
- `verify.ps1` gates: package installed, WUDFRd reflector, ROOT device present, driver loaded (problem 0), adapter count, display count, `USBDisplay` monitor + decoded EDID; decodes CM problems 28/31/37/39/41.
- Topology ops are stock Windows: `DisplaySwitch.exe`, `ms-settings:display`. GUI reads via `EnumDisplayDevices` / `Screen.AllScreens`, manages driver via elevated `powershell -File` + `pnputil /enum-drivers`, `/restart-device`, SetupAPI/CfgMgr P/Invoke.

### 4.8 Android client interface (`android/`)

- Manifest: `INTERNET`, `ACCESS_WIFI_STATE`, `ACCESS_NETWORK_STATE`, `CAMERA` (scan-to-trust only), `usb.host` feature. `allowBackup=false`. Activities: `.MainActivity` (launcher, `fullSensor`), `.pair.WifiPairActivity`, `.pair.ScanPcActivity`.
- `MainActivity`: fullscreen `SurfaceView` (holder callback owns pipeline), `FLAG_KEEP_SCREEN_ON`, immersive bars (`WindowInsetsController` R+, immersive-sticky pre-R), overlay `WiFi Pair` button (alpha 0.6, top-left). `onTouchEvent` maps Down/PointerDown→Down, Move→Move (+historical replay), Up/PointerUp/Cancel→Up; `InputEvent.normalize(px, extent)` → `0..65535`. `onKeyDown/Up` maps Enter/Del/ForwardDel/Tab/Escape/DPAD/Home/End → `NamedKey`, else Unicode char; unknown keys return false (system handles, e.g. Back).
- `StreamPipeline(surface)`: `handleClient(BufferedInputStream, OutputStream)` loop — `u32LE len → TransportPacket::decode → FrameFragment → FrameReassembler → CRC → MediaCodec (video/avc|hevc|av01, recreate on WxH/codec change) → queueInputBuffer(pts_us=timestamp_ns/1000, KEY_FRAME flag) → dequeueOutputBuffer → paced present`. `sendInput(event)` writes `u32LE + Control packet` on the same socket. Ignores Heartbeat/KeyframeRequest/other kinds with log.
- `StreamSession`: USB `ServerSocket(127.0.0.1:27183)` accept loop → `handleClient`. `WifiListener`: `SSLServerSocket(0.0.0.0:27184, TLSv1.3)` + PIN/lockout/trust + Welcome handshake → `handleClient` over TLS. Both share one `StreamPipeline` so framing is identical.
- `WifiPairActivity` (layout `activity_wifi_pair.xml`): `IP: <lan>:27184`, `SHA256:xxxx…` (short fp), `PIN: 123456` (48sp bold), 512px QR `PairPayload.encode(ip,port,fp)`, buttons `Rotate PIN | Forget hosts | Scan PC code`, `New certificate` (confirm dialog; rotation resets trust). IP from `LinkProperties` (no location perm). `ScanPcActivity` (Camera2 + ZXing-core, runtime CAMERA): scans PC QR `PcPairPayload{"v":1,"host_id":"host-<pc>"}` → adds to `TrustedHosts` (SharedPreferences set).
- Pairing JSON: QR `{"v":1,"ip":"…","port":27184,"fp":"SHA256:<64 lower hex>"}`, Hello (Handshake kind-6) `{"v":1,"pin":"…","host_id":"host-<pc>","codecs":["h264","h265"]}`, Welcome `{"v":1,"accept":true,…}`. `host_id` = `host-<COMPUTERNAME>` stable (not PID).

### 4.9 Control Center GUI (`control-app/`)

- Window: `MainWindow.xaml` 1080×720 (min 900×600), top bar (title, system badge, DEMO MODE flag, admin badge/detail, Refresh), **three-item left nav (Home, Setup, Advanced)**, page `ContentControl`, first-run overlay (3 steps + Run Diagnostics / Get Started).
- Pages (three-item shell; the leaf surfaces are unchanged, just grouped):
  - **Home** = the Dashboard: giant status text, START/STOP/RESTART, transport combo (usb/wifi) + CONNECT USB / CONNECT WI-FI, WiFi IP/QR-JSON + PIN fields (visible only for wifi), connection note, SHOW PAIRING CODE (PC QR `{"v":1,"host_id":…}` via `QrCodeService` + caption), SIGNAL (`SignalMonitor` 110px pipeline strip), STREAM telemetry line (Consolas; `Latency: N/A (not measured)` is honest), RECENT ACTIVITY list.
  - **Setup** = tabs: **Driver** (package/instance/problem-code status, Install/Uninstall/Restart/Enable-Disable, elevated + confirmed, `verify.ps1` output) and **Device** (adb devices table serial/state/model/product/transport_id, poll every 5 s, reconnect/restart-server maintenance, Wi-Fi trust/security panel). Wi-Fi pairing inputs live on Home only — not duplicated here.
  - **Advanced** = tabs: **Display** (`DisplayInfo` list, Open Display Settings / Extend), **Services** (`ServiceEntry` list — streamer process, WUDFRd reflector binding, ADB server; honest: child **processes**, not Windows services), **Logs** (ring-buffered stdout/stderr + ETW/event-log excerpts, level filter, export), **Diagnostics** (one-click full run, per-check Pass/Fail + remediation, `SYSTEM READY` vs `N check(s) failed`), **Settings** (streamer/adb/driver-script paths, codec/bitrate/fps/GOP, USB port, transport/device-ip/PIN, capture purge toggles, log level, tray/startup, danger-zone driver management).
  - `SetupViewModel` / `AdvancedViewModel` are thin containers that re-host the existing leaf view models as tabs; the tray "Diagnostics" action and first-run overlay jump straight to the Diagnostics tab via `AdvancedViewModel.SelectedIndex`.
- Gateway contract: `IUsbDisplayGateway` (State, Health, StatusMessage, StartupSteps, Telemetry, Driver, Displays, Devices, Processes, ServiceEntries, LastDiagnostics, Capabilities, Settings; `Start/Stop/Restart/RestartComponent/RunDiagnostics/Driver* /OpenDisplaySettings/ExtendDisplays/RunAdbMaintenance/SaveSettings/RefreshAll`). `RealUsbDisplayGateway.StartAsync` gates: driver package → driver loaded → virtual monitor enumerated → streamer binary → device (USB) or device-ip (WiFi) → purge stale captures → `StartStream(live:true, loop:true, --stats-json)` → wait ≤45 s for `connected + streamed_frames increasing` → ACTIVE. `MockUsbDisplayGateway` powers `--demo`.
- `AppSettings` JSON (`%AppData%\USBDisplay\control-app-settings.json`): `StreamerPath, AdbPath, DriverScriptsDir, Codec=h264, BitrateBps=20000000, Fps=60, Gop=60, UsbPort=27183, Transport=usb, DeviceIp=, Pin=, LaunchAtStartup, MinimizeToTray=true, ConfirmBeforeStop=true, ConfirmDestructive=true, LogLevel=Info, DemoMode, AutoPurgeCapture=true, CapturePurgeAgeSeconds=300, CaptureWarnMb=512, FirstRunDone, AutoStartStreaming, DevicePollSeconds=5`.
- `StreamTelemetry` record: `StreamedFrames, StreamedPackets, WriteStallMsMax, InputEventsInjected, Fps, Codec, Resolution, EncoderBackend, BitrateBps, DroppedFrames, CrcFailures, Reconnects, UpdatedAt`. `StreamStartOptions(Transport, Codec, BitrateBps, Fps, Gop, UsbPort, Serial?, DeviceIp?, Pin?, Live, Loop)` → CLI args (no shell strings; `ProcessStartInfo` array).
- Tests: MSTest over mock gateways — CLI/adb/pnputil parsers, telemetry accumulator, settings round-trip, log service, crash bounds, start/stop machine, pairing QR, capture monitor. No hardware/admin/driver needed.

---

## 5. Step-by-Step Guide

### 5.1 Prerequisites

Windows: Rust stable (`https://rustup.rs/`), Android Studio + SDK 35+, platform-tools (`adb`), WDK (driver work), .NET 8 SDK (Control Center), USB data cable.

Tablet: Developer Options → USB debugging ON → connect via USB → accept debugging prompt.

### 5.2 Build and test Rust workspace

```powershell
cargo test --workspace
```

Verifies protocol, transport, encoder unit/proptest suites, streamer helpers (`RateController`, WiFi tuning, `FrameSender`).

### 5.3 Run host probes (no hardware streaming yet)

```powershell
cargo run -p usbdisplay-streamer -- --help
cargo run -p usbdisplay-streamer -- devices
cargo run -p usbdisplay-streamer -- capabilities
cargo run -p usbdisplay-streamer -- probe-frame --width 1920 --height 1080 --refresh-millihz 60000
cargo run -p usbdisplay-streamer -- transport-probe --max-packet-payload 65536
```

Expect: `devices` lists adb tablets; `capabilities` prints codecs/transport/capture/input; `probe-frame` prints byte size + CRC; `transport-probe` prints packet count.

### 5.4 Encode captured frames (optional, needs BMPs)

```powershell
cargo run -p usbdisplay-streamer -- encode-capture `
    --input-dir "$env:ProgramData\USBDisplay\capture" `
    --out capture.h264 --codec h264 --fps 60 --bitrate 20000000 --gop 60 `
    --verify-decode
```

Expect `encoder_backend=MediaFoundation`, `stream_playable=true`, `decode_roundtrip=PASS`, `OVERALL: PASS`. See `docs/encoder.md`.

### 5.5 Verify Android USB connection

```powershell
adb devices
# List of devices attached
# <device-id>    device
```

`unauthorized` → unlock tablet, accept prompt.

### 5.6 Build and install Android app

```powershell
cd android
.\gradlew.bat assembleDebug
adb install -r app\build\outputs\apk\debug\app-debug.apk
```

Or open `android/` in Android Studio → Run. App opens fullscreen `SurfaceView` + decoder + both listeners.

### 5.7 Open Android screen

```powershell
adb shell monkey -p org.usbdisplay.client 1
```

Tablet switches to USBDisplay fullscreen surface.

### 5.8 Stream to Android over USB (ADB path)

> **Frames must come from `--input-dir`.** Virtual-monitor frames never reach the
> host process today (the capture handoff is unimplemented — see the banner at
> the top), so the default capture directory is empty and this command exits
> immediately. Point `--input-dir` at your own 32-bpp top-down BMPs instead.

1. Install/open app, confirm `adb devices` shows `device`.
2. Start host streaming:

```powershell
cargo run -p usbdisplay-streamer -- stream-capture `
    --input-dir "$env:ProgramData\USBDisplay\capture" `
    --codec h264 --fps 60 --bitrate 20000000 --gop 60 --loop
```

Live mode is default (newest-frame-wins, backlog purged, wall-clock PTS). Flags: `--serial`, `--port` (default 27183), `--max-frames N`, `--no-live` (ordered replay), `--stats-json`.

Expect `android_connection=established` and decoded frames on the tablet — **once frames are present in `--input-dir`**. Transport is USB (`adb` cable), not WiFi.

### 5.9 Stream over WiFi LAN (TLS + PIN)

> Same caveat as 5.8: supply frames via `--input-dir`; the virtual monitor does
> not produce them yet.

USB stays default. Same LAN, non-isolated SSID (see `docs/wifi.md`):

1. Tablet app → **WiFi Pair**. Note LAN IP, 6-digit PIN, QR.
2. Host:

```powershell
cargo run -p usbdisplay-streamer -- stream-capture `
    --transport wifi --device-ip 192.168.1.42 --pin 123456 `
    --input-dir "$env:ProgramData\USBDisplay\capture" --loop
```

`--device-ip` = bare IP, `ip:port`, or QR JSON. Port **27184**. TLS 1.3 only. Trusted `host_id` skips PIN next time; wrong PIN → 3 strikes + 30 s lockout; AP-isolated → `host unreachable … use USB`. WiFi defaults 12 Mbps/GOP 30 + adaptive `20→12→8→4 Mbps`; `--stats-json` prints JSON every 60 frames.

### 5.10 Control Center GUI (optional, recommended on Windows)

```powershell
cd control-app
dotnet run --project src/USBDisplay.ControlApp -- --demo   # simulated stack, no hardware
dotnet run --project src/USBDisplay.ControlApp             # real mode
dotnet test USBDisplay.sln                                 # MSTest suite
```

Dashboard → START USB DISPLAY runs the same gated orchestration as the CLI path with telemetry, logs, diagnostics. See `docs/control-app.md`, `control-app/README.md`.

### 5.11 Planned user flow (once all slices land)

1. Connect tablet via USB. 2. Start USBDisplay on Windows. 3. Start USBDisplay on Android. 4. Windows creates virtual monitor via IDD. 5. Display Settings shows tablet as external monitor. 6. Extend/Duplicate. 7. Host captures virtual monitor only → encode → stream. 8. Android decodes/renders. 9. Input returns to Windows.

---

## 6. Testing & CI

- `cargo test --workspace`: protocol round-trips/proptests, CRC rejection, fragmentation, transport packetize/reassemble, ACK gaps, retransmit expiry, heartbeat disconnect, input encode/decode + Kotlin reference bytes, BMP/color/NAL unit tests, `RateController` stall/KF step-down/step-up, WiFi tuning, `FrameSender` stats.
- `:app:testDebugUnitTest`: QR parse, PIN verify + lockout, fp format, handshake encode/decode, `RateController` equivalent, pairing store, `FrameReassembler`, `FramePacer`, transport/input packet tests.
- `dotnet test USBDisplay.sln`: CLI/adb/pnputil parsing, telemetry, settings round-trip, log service, crash bounds, start/stop machine, pairing QR, capture monitor.
- Integration/system (need hardware): ADB loop, decoder recovery after drops, unplug reconnect, suspend/resume, 1080p60 USB2, 1440p120 USB3, multi-monitor/tablet, rotation/resolution switch, Extend/Duplicate.
- Perf gates: USB2 1080p60 <35 ms glass-to-glass, USB3 1440p120 <20 ms, host CPU <10%, GPU <15%, RAM <250 MB; WiFi 1080p60 <80 ms p50 / <120 ms p95, 5-min soak no disconnect, adaptation lines under loss, input round-trip via `input_events_injected`, pairing gates (wrong PIN rejected, rotated cert rejected with fp guidance, trusted reconnect skips PIN, AP-isolated prints guidance).
- CI (`.github/workflows/`): **Rust** on `ubuntu-latest + windows-latest` (Linux = portable fallbacks, Windows = real MF encoder + `SendInput` path) + non-blocking `fmt --check`/`clippy` lint; **Android** on JDK 17 (`testDebugUnitTest` + `assembleDebug` + APK artifact); **Control Center** on `windows-latest` (`dotnet build` + `dotnet test`). A `v*` tag additionally builds the release assets — see §8.

---

## 7. Troubleshooting

| Symptom | Fix |
|---|---|
| `pnputil` says up-to-date but driver unchanged | Bump INF `DriverVer` or run `uninstall.ps1` first — PnP keys store on version. |
| UMDF host crashes in System log | Check install date: pre-fix crashes are stale. Current loop has try/catch + adapter guard; collect `capture-crash.ps1` + DebugView TraceLogging. |
| `no capture_*.bmp frames found` / `waiting_for_capture_frames=true` | **Expected today.** The driver writes no capture files and the in-memory handoff is not implemented — see the banner at the top. Nothing to fix on your side; supply your own frames via `--input-dir` to exercise the pipeline. |
| `capture size changed …` no longer occurs | A Windows mode switch now rebuilds the encoder live (`stream_resolution_change old=… new=…`). If the tablet stays black after one, restart `stream-capture`. |
| `adb device unauthorized/offline` | Unlock tablet, accept prompt, `adb reconnect` / restart server from Device page. |
| `No frames received within 45 s` | Almost always the missing capture handoff (see the banner at the top). Otherwise check `stream_encoder_backend=` in Logs, the tablet screen, and the firewall for Wi-Fi. |
| WiFi `host unreachable … use USB` | AP-isolated/guest WLAN — use non-isolated SSID, hotspot, or USB. Never silently falls back. |
| Wrong PIN / cert mismatch | Re-read current PIN/QR on tablet; rotated cert requires re-scan + PIN; TOFU is `%AppData%\USBDisplay\paired.json`. |
| Streamer crash loop | Bounded to 3 auto-restarts then Error — see Logs → Diagnostics. Stop never uninstalls driver. |

---

## 8. Continuous Integration & Releases

**On every push/PR** (`.github/workflows/`): Rust workspace build+test on Ubuntu
+ Windows (`rust.yml`), Android `testDebugUnitTest` + `assembleDebug` (`android.yml`),
and the Control Center MSTest suite on `windows-latest` (`dotnet.yml`). The Rust
lint job is informational until formatting/clippy debt is cleared.

**On a `v*` tag** (`release.yml`): builds and attaches these to a GitHub Release —

| Asset | How it is produced | Runs with |
|---|---|---|
| `usbdisplay-control-center-<ver>-win-x64.zip` | `dotnet publish` self-contained, single-file, win-x64 | No .NET install needed |
| `usbdisplay-streamer-<ver>-win-x64.zip` | `cargo build --release -p usbdisplay-streamer` | Standalone `.exe` |
| `usbdisplay-android-<ver>.apk` | `gradlew assembleDebug` (debug-signed, directly installable) | Android 8.0+ |
| `usbdisplay-driver-scripts.zip` | Sources + INF + `*.ps1` for the IDD | Needs WDK; test-signed locally |

The IDD itself is **not** built in CI — it needs the Windows Driver Kit and, for
anything beyond a test-signed machine, a real signing certificate. Releasing a
WHQL-signed package is tracked in `docs/review-findings.md` (6.2). A Play-ready
APK likewise needs a keystore supplied via release secrets; the shipped APK is
debug-signed so it installs without ceremony.

Version drift across Cargo/MSBuild/Gradle/INF is reported by
`scripts/check-versions.ps1` against the root `VERSION` file.

## 9. Non-Goals

- No cloud dependency
- No telemetry
- No whole-desktop capture
- No software decoding on Android

## 10. License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT)
at your option. Bundled third-party components are summarized in
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md). See [`SECURITY.md`](SECURITY.md)
for the security model and reporting, and [`CONTRIBUTING.md`](CONTRIBUTING.md) to
get started. The IDD driver builds against the Microsoft WDK under its own terms.
