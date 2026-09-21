# WiFi Transport (LAN, TLS 1.3 + PIN)

WiFi is an **alternative** transport. USB (`adb forward` + `127.0.0.1:27183`)
stays the default and is unchanged. WiFi uses the **same** `u32LE(len) + USBT +
USBD` framing, the same `FrameSender`, and the same Android `StreamPipeline`
decode path — only the socket differs.

## Topology

```text
Windows host                          LAN (WiFi 5/6)                Android tablet
─────────────                         ────────────────              ──────────────
IDD virtual monitor
  → %ProgramData%\USBDisplay\capture
  → MF H.264 encode ─┐
                     ├─► FrameSender ─► TLS 1.3 ─► 192.168.x.x:27184 ─► SSLServerSocket
  ◄─ input events ───┘   (u32LE+USBT)   TCP+TLS      (all interfaces)      MediaCodec → Surface
       SendInput                                                              QR pair screen
```

Ports:

- **27183** — USB via `adb forward tcp:27183` → Android `ServerSocket(127.0.0.1:27183)`.
- **27184** — WiFi LAN → Android `SSLServerSocket(0.0.0.0:27184, TLSv1.3 only)`.

Both listeners can run side by side during development.

## QR pairing flow

1. Tablet: open **WiFi Pair** from `MainActivity`. The screen shows:
   - LAN IP (from `LinkProperties`, no location permission),
   - port `27184`,
   - `SHA256:xxxx…` fingerprint (first 16 hex) of the ECDSA P-256 self-signed cert,
   - 6-digit PIN (big),
   - QR `{"v":1,"ip":"…","port":27184,"fp":"SHA256:…"}` (ZXing).
2. Host: scan/copy the QR or type the IP:
   ```powershell
   cargo run -p usbdisplay-streamer -- stream-capture `
       --transport wifi --device-ip 192.168.1.42 --pin 123456 `
       --input-dir "$env:ProgramData\USBDisplay\capture" --loop
   ```
   `--device-ip` also accepts `ip:port` or the full QR JSON.
3. Host dials `ip:port` (5 s timeout, `TCP_NODELAY`), completes TLS 1.3,
   sends Hello as a **Handshake (kind 6)** packet:
   `{"v":1,"pin":"…","host_id":"host-<pc>","codecs":["h264","h265"]}`,
   reads one framed Welcome, expects `{"v":1,"accept":true,…}`.
4. Tablet verifies PIN **constant-time** (`MessageDigest.isEqual`), 3 strikes →
   30 s lockout, replies Welcome Handshake, stores `host_id` in
   `SharedPreferences` trusted set. Reconnect from a known `host_id` skips PIN
   (TLS fingerprint still enforced).
5. Video flows over the same TLS session via `FrameSender` + `write_framed_packet`;
   input returns as 16 B events in `Control` packets via `run_input_reader`
   (shared `SharedTlsStream` behind a mutex — `StreamOwned` cannot `try_clone`).

`host_id` is `host-<COMPUTERNAME>` (stable, not PID) so second connects skip PIN.
`Rotate PIN` regenerates; `Forget hosts` clears trusted set.

## Scan-to-trust (PC shows QR, tablet scans)

Reverse-direction pairing for users who prefer it (also what the Windows
control app offers via Dashboard → Show pairing code):

1. PC: show pairing code — a QR encoding `{"v":1,"host_id":"host-<pc>"}`.
   The C# `HostIdentity` scheme mirrors Rust `pairing::stable_host_id`
   exactly; keep them in lockstep or trust silently stops matching.
2. Tablet: WiFi Pair → **Scan PC code** (Camera2 + bundled ZXing-core
   decoder, runtime CAMERA permission). A valid scan adds the `host_id` to
   the trusted set — physical proximity is the authorization.
3. PC connects with an empty PIN; the tablet skips the PIN check for the
   trusted id (TLS fingerprint still enforced). No protocol change was
   needed: trust is the same `host_id` set the PIN flow writes to.

## TLS / PIN design

- **Cert**: ECDSA P-256, self-signed, 10-year validity, `CN=USBDisplay-Tablet`.
  Choice: `AndroidKeyStore` cannot mint TLS server certs directly, so we generate
  in-memory via BouncyCastle (`bcpkix`) and persist PKCS8 + DER Base64 in private
  `SharedPreferences` (no backup). Documented here per spec fallback allowance.
- **Fingerprint**: `SHA256:<64 lowercase hex>` of DER cert. QR embeds full fp;
  screen shows short `SHA256:xxxx…`. Host TOFU store:
  `%AppData%\USBDisplay\paired.json` (`{tablet_id, ip, cert_fingerprint, paired_at}`).
  Verifier accepts only if fp matches stored pairing **or** QR `fp`; otherwise
  bails with expected-fp guidance. Regenerating the cert breaks pairing until
  re-scan (cert-mismatch path).
- **PIN**: 6-digit `SecureRandom`, constant-time compare on tablet, 3-strike
  30 s lockout. Wrong PIN → host bails suggesting current PIN + lockout note.
- **Protocols**: TLS 1.3 only (`SSLServerSocket.enabledProtocols = ["TLSv1.3"]`,
  rustls client). Plaintext is **always refused** (`--insecure-lan` was removed
  at the end of PR-3; PR-2's plaintext gate is gone).
- **Permissions**: `ACCESS_WIFI_STATE`, `ACCESS_NETWORK_STATE` only. No location,
  no microphone. No telemetry. No cloud.

## AP-isolation limitation

Guest/AP-isolated WLANs block host↔tablet TCP even on the same SSID. Symptom:
`host unreachable at <ip:port> (AP isolation? …) — use USB (--transport usb)`.
Fix: use a non-isolated SSID, phone hotspot, or USB (`--transport usb`).

## Perf tuning (PR-4)

- WiFi defaults (unless user overrode flags): **12 Mbps, GOP 30**
  (USB: 20 Mbps, GOP 60). Override detection compares to USB defaults.
- `RateController`: per-send write-stall + `KeyframeRequest` rate (input thread
  counts via `AtomicU64`). Ladder `20→12→8→4 Mbps`; step down when p95 stall
  >50 ms over a 60-frame window **or** >2 KF in window (≈>2/s at 60 fps);
  step back up after 600 clean frames. Encoder re-created via `select_encoder`
  (same codec/resolution); a resolution change rebuilds the encoder too and
  logs `stream_resolution_change old=WxH new=WxH` instead of stopping.
  Logs `bitrate_step_down old_bitrate=… new_bitrate=…` /
  `bitrate_step_up …` (parseable `key=value`).
- `--stats-json`: every 60 frames
  `{"streamed_frames":N,"streamed_packets":M,"write_stall_ms_max":X,"input_events_injected":K}`.
- Contract: 1080p60 glass-to-glass **<80 ms p50 / <120 ms p95** on WiFi 5/6.
  See `docs/testing.md` for gates.
