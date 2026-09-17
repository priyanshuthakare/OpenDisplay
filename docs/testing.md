# Testing Strategy

## Unit Tests

- Protocol encode/decode round trips
- CRC rejection
- Fragment reassembly
- Capability negotiation
- Input event mapping

## Integration Tests

- ADB compatibility stream loop
- Native USB bulk stream loop
- Decoder recovery after dropped frames
- Auto reconnect after unplug
- Suspend and resume

## System Tests

- 1080p60 on USB 2
- 1440p120 on USB 3
- Multi-monitor topology
- Multi-tablet topology
- Rotation and resolution switching
- Windows duplicate and extend modes

## Performance Gates

- USB 2 target: 1080p60 under 35 ms glass-to-glass latency
- USB 3 target: 1440p120 under 20 ms glass-to-glass latency
- Host CPU under 10 percent during steady state
- Host GPU under 15 percent during steady state
- Host memory under 250 MB

## WiFi Gates (PR-4)

- 1080p60 glass-to-glass **<80 ms p50, <120 ms p95** on WiFi 5/6 (see `docs/wifi.md`).
- Soak 5 min on WiFi: no disconnect, `bitrate_step_down`/`bitrate_step_up`
  lines show adaptation under artificial loss and recovery when clean.
- Input round-trip: touch drag + key type on tablet arrive via `Control`
  packets (`input_events_injected` increments; `--stats-json` includes it).
- Pairing: wrong PIN rejected; regenerated cert rejected with fingerprint
  guidance; second connect from same host skips PIN; AP-isolated SSID prints
  `host unreachable … use USB` and never falls back silently.
- Suites: `cargo test --workspace` green; `:app:testDebugUnitTest` green
  (covers QR parse, PIN verify + lockout, fp format, handshake encode/decode,
  `RateController`, pairing store).

