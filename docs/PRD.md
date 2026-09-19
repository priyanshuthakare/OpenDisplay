# USBDisplay — Product Requirements Document (PRD)

**Version:** 1.0 (new baseline, Sep 2026)
**Status:** Draft for review — reflects implemented slices + open milestones
**Components:** Windows IDD (`driver/idd`), Host streamer/encoder (Rust), Protocol/Transport (Rust), Android client (Kotlin), Control Center (C#/.NET 8 WPF)
**Related:** `docs/architecture.md`, `docs/development-plan.md`, `docs/protocol.md`, `docs/idd-driver.md`, `docs/encoder.md`, `docs/wifi.md`, `docs/control-app.md`, `docs/testing.md`, `README.md §4`

---

## 1. Purpose

### 1.1 Problem
Users with a Windows PC and an Android tablet want the tablet to behave like a real second monitor (extend/duplicate, drag windows, rotate) over a local USB cable — without cloud accounts, telemetry, Wi-Fi dependence, whole-desktop capture, or laggy software encoding.

Existing solutions are typically closed-source, Wi-Fi-only, whole-desktop screen-mirroring, or require sign-in / internet. They do not present a true Windows display topology object.

### 1.2 Product vision
Plug the tablet in with USB → Windows sees a real `USBDisplay` monitor → tablet shows only that monitor's content with glass-to-glass latency suitable for interactive use → touch/keyboard on the tablet drives Windows.

Mental model: **same as plugging in an HDMI monitor**, except the cable is USB and the panel is an Android tablet.

### 1.3 Scope of this PRD
Defines v1 requirements for:

1. Virtual display + capture (Windows driver)
2. Hardware encode (host)
3. USB transport (ADB bridge now, native bulk later) + Wi-Fi LAN alternative
4. Android decode/render + input capture
5. Input return to Windows
6. Control Center orchestration/diagnostics
7. Install, signing, testing, and performance gates

Out of scope is explicitly listed in §3 and §11.

---

## 2. Goals and non-goals

### 2.1 Goals (G-xxx)

| ID | Goal | Success signal |
|----|------|----------------|
| G-1 | True Windows second monitor over USB | `USBDisplay` appears in Display Settings; Extend/Duplicate work |
| G-2 | Virtual-monitor-only capture | No whole-desktop frame path exists by construction (swap-chain readback) |
| G-3 | Hardware-encoded, low-latency video | 1080p60 USB2 <35 ms, 1440p120 USB3 <20 ms; Wi-Fi 1080p60 <80 ms p50 / <120 ms p95 |
| G-4 | Reliable local transport | CRC + fragmentation + ACK/heartbeat + keyframe recovery; no silent fallback |
| G-5 | Usable tablet input | Touch drag + typing arrive on host (`input_events_injected` increments) |
| G-6 | One-click Windows orchestration | Control Center gated Start → verified Active; guided diagnostics |
| G-7 | Private by design | No cloud, no telemetry, no account; LAN/USB only |

### 2.2 Non-goals (NG-xxx)

- NG-1: No cloud relay, analytics, or account system.
- NG-2: No whole-desktop capture or software decoding on Android.
- NG-3: No iOS client, no macOS/Linux host in v1.
- NG-4: No multi-tablet striping or GPU-shared-memory zero-copy in v1 (single tablet, BMP/SHM staging path is acceptable bridge).
- NG-5: No auto-updater in v1 (script-based driver install stays explicit + elevated).

---

## 3. Users, personas, use cases

### 3.1 Personas
- **P-1 Developer/office worker:** wants tablet as code/chat/docs side monitor over office USB.
- **P-2 Student:** laptop + tablet dorm setup, duplicate for presentation or extend for study.
- **P-3 IT/tester:** needs deterministic diagnostics, logs, and driver verify gates.

### 3.2 Primary use cases (UC-xxx)
- UC-1 Extend: drag IDE/docs onto tablet, interact via touch/keyboard.
- UC-2 Duplicate: present laptop screen on tablet over USB in a meeting.
- UC-3 Wi-Fi fallback: same experience on non-isolated LAN when USB is unavailable.
- UC-4 Diagnose: one-click check + log export when the monitor/stream fails.

### 3.3 User stories (US-xxx)
- US-1 As P-1, I plug in USB so Windows shows a second monitor without reboot.
- US-2 As P-1, I choose Extend so I can drag a window to the tablet.
- US-3 As P-1, I touch/type on the tablet so the Windows cursor/text updates.
- US-4 As P-2, I tap WiFi Pair + scan/enter PIN so video starts on LAN.
- US-5 As P-3, I run Diagnostics so I get Pass/Fail + remediation per check.

---

## 4. Product experience

### 4.1 End-to-end flow (happy path, USB)
1. Tablet connected via USB, USB debugging accepted.
2. User installs IDD (`install.ps1`, elevated) → `verify.ps1` PASS, problem code 0.
3. User opens Control Center → Dashboard → START USB DISPLAY (or CLI `stream-capture --loop`).
4. Gateway gates: driver package → driver loaded → virtual monitor enumerated → streamer binary → adb device → purge stale captures → spawn `stream-capture (live, --stats-json)` → wait ≤45 s for `android_connection=established` + increasing `streamed_frames` → ACTIVE.
5. Android app open (`monkey -p org.usbdisplay.client`) shows decoded frames on fullscreen `SurfaceView`.
6. Touch/keyboard on tablet → host cursor/text updates; `input_events_injected` increments.
7. STOP ends session only; driver stays installed. Disable/Uninstall are separate confirmed danger-zone actions.

### 4.2 End-to-end flow (Wi-Fi)
1. Tablet → WiFi Pair screen shows `IP:port`, `SHA256:xxxx…`, 6-digit PIN, QR `{"v":1,"ip","port":27184,"fp"}`.
2. Host `stream-capture --transport wifi --device-ip <ip|ip:port|QR-JSON> --pin <pin> --loop`.
3. TLS 1.3 handshake + kind-6 Hello/Welcome; wrong PIN → reject + lockout note; trusted `host_id` skips PIN (fp still enforced).
4. Same video/input path as USB over TLS socket. Defaults 12 Mbps/GOP 30 + adaptive ladder unless overridden.
5. AP-isolated WLAN → `host unreachable … use USB`, never silent fallback.

### 4.3 UX surfaces
- **Android:** fullscreen surface + low-alpha `WiFi Pair` button; `WifiPairActivity` (IP/fp/PIN/QR/Rotate/Forget/Scan/New-cert); `ScanPcActivity` for PC-QR trust.
- **Windows GUI:** 1080×720 dark WPF, top bar (badge/admin/Refresh), left nav (Dashboard, Driver, Display, Device, Services, Logs, Diagnostics, Settings), first-run overlay, Dashboard pairing QR (`SHOW PAIRING CODE`), `SignalMonitor` strip, honest `Latency: N/A` telemetry line until measured.

---

## 5. Functional requirements

Notation: **M** = must (v1 gate), **S** = should, **F** = future (tracked, not gate).

### 5.1 Virtual display driver (FR-DRV)

| ID | Pri | Requirement | Acceptance |
|----|-----|-------------|------------|
| FR-DRV-1 | M | Enumerate `USBDisplay` monitor with 128-byte EDID (`BuildEdid` + product name) via IddCx | `verify.ps1`: adapter + display count, monitor present, decoded EDID |
| FR-DRV-2 | M | Load cleanly, problem code 0, `Root\USBDisplayIdd` present, WUDFRd bound | `pnputil /enum-drivers` ∋ USBDisplay; SetupAPI instance present |
| FR-DRV-3 | M | Support Extend + Duplicate via stock Display Settings | Manual topology test passes |
| FR-DRV-4 | M | Drive OS-assigned swap chain (`SetDevice` → acquire loop 16 ms `E_PENDING` → `FinishedProcessingFrame`) without persisting frames in production | Code path + DebugView TraceLogging sequence `DeviceAdd→D0Entry→…→AssignSwapChain` |
| FR-DRV-5 | M | Never throw out of frame worker (try/catch + disable stage); idempotent adapter init across `D0Exit/D0Entry` | No WUDFHost crash after clean install |
| FR-DRV-6 | S | `install/uninstall/verify` scripts + CM problem decode (28/31/37/39/41); `DriverVer` bump rule documented | Scripts green on fresh VM |
| FR-DRV-7 | F | Sleep/resume, hot-plug, rotation tests; HID touch/pen device bound to virtual monitor | Follow-up milestone |

### 5.2 Capture (FR-CAP)

| ID | Pri | Requirement | Acceptance |
|----|-----|-------------|------------|
| FR-CAP-1 | M | Capture only the virtual monitor via own swap-chain surface readback (no Desktop Duplication) | Architecture review: no whole-desktop API in driver path |
| FR-CAP-2 | M | GPU→CPU staging + BGRA normalize honoring `RowPitch`; handle `B8G8R8A8/R8G8B8A8/R10G10B10A2`; reject insane/multisampled surfaces | Unit + on-device capture test |
| FR-CAP-3 | S | Disk BMP bridge (`%ProgramData%\USBDisplay\capture\capture_*.bmp`, 32-bpp top-down `BI_RGB`) with live-mode purge of stale backlog | `stream-capture` live bounds disk |
| FR-CAP-4 | F | Disk-free shared-memory handoff; dirty-rect tracking | Future PR |

### 5.3 Hardware encoder (FR-ENC)

| ID | Pri | Requirement | Acceptance |
|----|-----|-------------|------------|
| FR-ENC-1 | M | Probe chain NVENC→QSV→AMF→Media Foundation behind one trait; print every backend status | `encode-capture`/`stream-capture` log `backend …: SELECTED` |
| FR-ENC-2 | M | MF H.264 Annex-B encode via async MFT event model (`MF_TRANSFORM_ASYNC_UNLOCK`, `NeedInput/HaveOutput/DrainComplete`) | `encode-capture` produces playable stream |
| FR-ENC-3 | M | Each coded picture → `EncodedFrame` → `Packetizer` (prove encode→frame→transport) | `transport_packets>0` in report |
| FR-ENC-4 | M | Deterministic validation: NAL SPS+PPS+≥1 IDR (`stream_playable=true`) + MF decoder round-trip count match | `OVERALL: PASS`, `decode_roundtrip=PASS` |
| FR-ENC-5 | S | Flags `--codec h264|h265 --bitrate --fps --gop --max-frames --verify-decode`; fixed-size encoders bail cleanly on resolution change with `restart stream` | CLI help + error test |
| FR-ENC-6 | F | Real NVENC/QSV/AMF, H.265 E2E validation, rate-control/scene-change tuning | Stubs today |

### 5.4 Transports — framing shared (FR-TR)

| ID | Pri | Requirement | Acceptance |
|----|-----|-------------|------------|
| FR-TR-1 | M | `USBD` v1 50B header + `USBT` v1 40B header, all LE, `u32LE(len)+packet` framing identical on USB/Wi-Fi | Rust proptest + Kotlin interop tests green |
| FR-TR-2 | M | Fragmentation/reassembly by `(frame_sequence, index/total)`; CRC32 on frame + packet payloads | Corruption tests → `CrcMismatch` |
| FR-TR-3 | M | Kinds `1 Fragment / 2 Ack / 3 Heartbeat / 4 KeyframeRequest / 5 Control / 6 Handshake`; non-Fragment ignored by video-only receivers | `handshake_packet_round_trips` + Android ignore-log |
| FR-TR-4 | M | Recovery: CRC/missing-fragment → drop + request keyframe; late → drop over budget; heartbeat timeout → disconnected/reconnect; ACK gap → retransmit | `RetransmitWindow/ReceiverAcks/HeartbeatMonitor` unit tests |
| FR-TR-5 | M | Never silently downgrade transport (USB↔Wi-Fi, plaintext↔TLS) | Error strings assert in tests |

### 5.5 USB path (FR-USB)

| ID | Pri | Requirement | Acceptance |
|----|-----|-------------|------------|
| FR-USB-1 | M | `adb forward tcp:27183→tcp:27183` managed with guard (remove on drop); `TCP_NODELAY`; retry connect (~100×200 ms) | `adb_serial`, `adb_forward`, `android_connection=established` |
| FR-USB-2 | M | Live default: newest capture only, wall-clock PTS, skip identical, tolerate half-written BMP | Latency stays flat in soak; `--no-live` ordered replay preserved |
| FR-USB-3 | S | Reverse input thread on cloned socket with 200 ms read-timeout poll, malformed-packet tolerant, `KeyframeRequest` counted for rate control | `input_return_channel=enabled`, `input_events_injected` |
| FR-USB-4 | F | Native USB bulk endpoints, double buffering, USB2/3 detect + adaptation | ADB bridge is v1 |

### 5.6 Wi-Fi path (FR-WIFI)

| ID | Pri | Requirement | Acceptance |
|----|-----|-------------|------------|
| FR-WIFI-1 | M | TLS 1.3 only to `<ip>:27184` (5 s dial timeout); plaintext always refused | No `--insecure-lan`; refusal test |
| FR-WIFI-2 | M | QR `{"v":1,"ip","port":27184,"fp":"SHA256:<64hex>"}`; `--device-ip` accepts bare IP / `ip:port` / QR JSON | Parse tests |
| FR-WIFI-3 | M | Hello as kind-6 Handshake `{"v":1,"pin","host_id":"host-<COMPUTERNAME>","codecs"}` → framed Welcome `{"v":1,"accept":true,…}` | Handshake tests |
| FR-WIFI-4 | M | 6-digit `SecureRandom` PIN, constant-time compare, 3 strikes → 30 s lockout; TOFU `%AppData%\USBDisplay\paired.json`; fp mismatch → expected-fp guidance; regen cert resets trust | `PinVerifier/TrustedHosts/CertFingerprint` tests |
| FR-WIFI-5 | M | Scan-to-trust: PC QR `{"v":1,"host_id"}` (C# `HostIdentity` == Rust `stable_host_id`); tablet Camera2+ZXing scan adds trust; empty-PIN connect allowed only for trusted id (fp still enforced) | `PcPairPayload` tests both sides |
| FR-WIFI-6 | M | Cert ECDSA P-256 self-signed 10-yr `CN=USBDisplay-Tablet`, BouncyCastle in-memory, PKCS8+DER Base64 private prefs, no backup; perms only `ACCESS_WIFI_STATE`+`ACCESS_NETWORK_STATE` (+`CAMERA` for scan, `INTERNET`) | Manifest + identity tests |
| FR-WIFI-7 | M | Defaults 12 Mbps/GOP 30 unless overridden; ladder `20→12→8→4 Mbps`; step down on p95 stall>50 ms/60f OR >2 KF/window, up after 600 clean; re-create via `select_encoder`; log `bitrate_step_down/up`; `--stats-json` every 60f | `RateController` tests + soak shows adaptation |
| FR-WIFI-8 | M | AP-isolation prints `host unreachable … use USB` | Negative-path test |

### 5.7 Android decode/render (FR-AND)

| ID | Pri | Requirement | Acceptance |
|----|-----|-------------|------------|
| FR-AND-1 | M | One shared `StreamPipeline(surface)` for USB+Wi-Fi: len→decode→reassemble→CRC→`MediaCodec`→surface; recreate codec on WxH/codec change (`video/avc|hevc|av01`) | `StreamPipeline/FrameReassembler` tests |
| FR-AND-2 | M | Pacing via `FramePacer` (PTS→`nanoTime` deadline, timestamped `releaseOutputBuffer`); backpressure drain+retry, never block UI thread | `FramePacerTest`, jank-free manual pass |
| FR-AND-3 | M | Listeners: USB `ServerSocket(127.0.0.1:27183)`, Wi-Fi `SSLServerSocket(0.0.0.0:27184, TLSv1.3)`; both can run side-by-side in dev | Dual-listen manual test |
| FR-AND-4 | M | Fullscreen `SurfaceView`, `KEEP_SCREEN_ON`, immersive bars, `fullSensor`, `allowBackup=false` | Manifest + activity test |
| FR-AND-5 | S | Pair screens per §4.8 (IP/fp/PIN/QR/Rotate/Forget/Scan/New-cert with confirm; QR failure never crashes pairing) | UI manual + unit tests |

### 5.8 Input return (FR-INP)

| ID | Pri | Requirement | Acceptance |
|----|-----|-------------|------------|
| FR-INP-1 | M | 16B LE kind-tagged events; pointer `(down/move/up/scroll, button, id, x/y 0..65535, scroll notches)`; key `(down/up, named 0..11, unicode BMP)`; Rust↔Kotlin byte-identical (reference vectors) | `InputEventTest` both sides green |
| FR-INP-2 | M | Android: touch→pointer (historical MOVE replay), keys→named-or-unicode; `normalize(px,extent)`; unknown keys return false | Touch drag + typing manual test |
| FR-INP-3 | M | Host: dedicated reader → `SendInput` absolute mouse (`ABSOLUTE\|VIRTUALDESK`) + `KEYEVENTF_UNICODE` text + VK named keys; `Control` payload may batch events | `input_events_injected` increments |
| FR-INP-4 | F | Pressure/tilt/eraser/Ink, IME composition, relative mouse, dedicated HID device bound to virtual monitor | Tracked, not v1 gate |

### 5.9 Control Center (FR-GUI)

| ID | Pri | Requirement | Acceptance |
|----|-----|-------------|------------|
| FR-GUI-1 | M | .NET 8 WPF, no AppSDK dep; 8 pages (Dashboard, Driver, Display, Device, Services, Logs, Diagnostics, Settings); dark theme; `--demo` mock; `--minimized` tray | `dotnet build/test` green |
| FR-GUI-2 | M | Drive only existing binaries/scripts/APIs; parse stable `key=value` only (never free text); `ProcessStartInfo` arg arrays (no shell strings) | `CliOutputParser` tests |
| FR-GUI-3 | M | Gated start (§4.1 step 4); ACTIVE requires verified connection + increasing frames; ≤45 s first-frame timeout; Stop never uninstalls; danger-zone confirms | `OrchestratorTests` |
| FR-GUI-4 | M | Bounded streamer restart (3) then paused+Error; 5 s adb poll; 1 s telemetry; capture age-gated purge; elevation only for driver/logs/ACL with reason; never self-elevate | Crash-bound + purge tests |
| FR-GUI-5 | M | Settings JSON schema-tolerant (`control-app-settings.json`); auto-detect streamer/adb/scripts with Advanced overrides | Round-trip tests |
| FR-GUI-6 | S | PC pairing QR (`HostIdentity` == Rust `stable_host_id`) via `QrCodeService`; `SHOW PAIRING CODE` caption | `PairingQrTests` |

### 5.10 Install / diagnostics / packaging (FR-OPS)

| ID | Pri | Requirement | Acceptance |
|----|-----|-------------|------------|
| FR-OPS-1 | M | Driver flows stay script-based + elevated; single-file publish (`-p:PublishSingleFile=true`, fx ~0.5 MB / sc ~150 MB); ship = publish folder + `install/uninstall/verify.ps1` + signed package | `control-app/README` steps work |
| FR-OPS-2 | M | One-click `RunFullDiagnosticAsync` with per-check Pass/Fail + remediation; log ring + export; `SYSTEM READY` vs `N failed` | `DiagnosticsService` tests |
| FR-OPS-3 | M | CI: Rust (`ubuntu+windows`) + Android (JDK17 tests + APK artifact) on push/PR; lint informational | Workflows green |

---

## 6. Non-functional requirements

| ID | Requirement | Target |
|----|-------------|--------|
| NFR-P1 | USB2 1080p60 glass-to-glass | <35 ms |
| NFR-P2 | USB3 1440p120 glass-to-glass | <20 ms |
| NFR-P3 | Wi-Fi 1080p60 glass-to-glass | <80 ms p50, <120 ms p95 (Wi-Fi 5/6) |
| NFR-P4 | Host steady-state CPU / GPU / RAM | <10% / <15% / <250 MB |
| NFR-P5 | Wi-Fi 5-min soak | No disconnect; adaptation lines under loss, recovery when clean |
| NFR-R1 | Decoder loss recovery | Drop + keyframe request recovers without restart |
| NFR-R2 | Unplug/reconnect | Heartbeat timeout → disconnected → guided reconnect (native bulk SM is F) |
| NFR-S1 | No plaintext on Wi-Fi; TOFU fp enforced even for trusted hosts | Security tests |
| NFR-S2 | PIN brute-force resistance | 3 strikes + 30 s lockout, constant-time compare |
| NFR-U1 | First-run to ACTIVE with defaults | ≤5 guided steps (overlay + diagnostics + START) |
| NFR-C1 | Protocol forward-compat | Reserved bytes + version checks; unknown kinds ignored |

---

## 7. Milestones and acceptance (maps to `development-plan.md` Phases 1–7)

| Milestone | Contains | Exit gate |
|-----------|----------|-----------|
| M1 Driver | FR-DRV-1..6 | `verify.ps1` PASS; Extend/Duplicate manual PASS |
| M2 Capture | FR-CAP-1..3 | Monitor-only review PASS; live purge bounds disk |
| M3 Encode | FR-ENC-1..5 | `encode-capture --verify-decode` → `OVERALL: PASS` |
| M4 USB transport | FR-TR + FR-USB-1..3 | `stream-capture --loop` → `android_connection=established`, frames on tablet |
| M5 Android | FR-AND | Paced, backpressured decode; 60/120 fps validation logged (HW-dependent) |
| M6 Input | FR-INP-1..3 | Drag + type round-trip; `input_events_injected>0` |
| M7 Wi-Fi | FR-WIFI | TLS+PIN soak + adaptation + pairing gates per `testing.md` Wi-Fi Gates |
| M8 GUI/OPS | FR-GUI + FR-OPS | `dotnet test` green; guided Start→Active on clean machine; CI green |

Current baseline already satisfies M1–M8 happy paths; §5 `F` items and NFR-P2/USB3 tuning remain open.

---

## 8. Test plan (summary; details in `docs/testing.md`)

- **Unit:** protocol round-trip/proptest, CRC reject, fragment reassembly, transport ACK/retransmit/heartbeat, input vectors (Rust+Kotlin identical), BMP/color/NAL, `RateController`, Wi-Fi tuning, `FrameSender` stats, QR/PIN/fp/handshake/store, `FrameReassembler/Pacer`, GUI parsers/telemetry/config/logs/crash-bounds/QR/capture.
- **Integration:** ADB loop, decoder recovery after drops, unplug reconnect, suspend/resume.
- **System:** 1080p60 USB2, 1440p120 USB3, multi-monitor/tablet, rotation/resolution switch, Extend/Duplicate.
- **Gates:** §6 NFR table + Wi-Fi Gates (soak, adaptation, input round-trip, pairing negatives, both suites green).

---

## 9. Risks, dependencies, open questions

| # | Risk/dependency | Mitigation |
|---|-----------------|------------|
| R-1 | Driver signing / attestation for broad install | Keep script-based elevated flow; document `DriverVer` + test-signing prereqs |
| R-2 | Vendor encoder SDKs (NVENC/QSV/AMF) licensing/FFI | Trait + stubs; MF default keeps v1 shippable |
| R-3 | AP-isolated / guest WLANs block Wi-Fi | Detect + explicit `use USB` guidance; never silent fallback |
| R-4 | Half-written BMP / mode-switch mid-stream | Skip-and-retry; bail with `restart stream` on size change; SHM is future fix |
| R-5 | Decoder heterogeneity across tablets | Recreate on WxH/codec; backpressure; on-device validation matrix |
| R-6 | `SendInput` vs true HID semantics | Document software-slice limits; HID device is F |

Open questions for review: v1 resolution/fps ceiling to promise? Which tablets form the certification matrix? Installer (MSIX/Inno) choice for M8 remainder? Realtime latency graph source (measured vs estimated)?

---

## 10. Appendix

### A. Normative ports/paths/IDs
- USB `127.0.0.1:27183` (`adb forward tcp:27183`); Wi-Fi `<lan>:27184` TLS 1.3-only.
- Capture `%ProgramData%\USBDisplay\capture\capture_*.bmp`; TOFU `%AppData%\USBDisplay\paired.json`; GUI `%AppData%\USBDisplay\control-app-settings.json`.
- PnP `Root\USBDisplayIdd`; ETW `USBDisplay.IddDriver`; `host_id` = `host-<COMPUTERNAME>`.

### B. Glossary
IDD (Indirect Display Driver, IddCx/UMDF), SHM (shared memory), MF (Media Foundation MFT), PTS (presentation timestamp), TOFU (trust-on-first-use), fp (certificate SHA-256 fingerprint), GOP (group of pictures), NAL (H.264 NAL units: SPS/PPS/IDR).

### C. Change log
- v1.0 (this file): new unified PRD consolidating implemented slices + open `F` items; supersedes scattered status notes as the planning baseline.
