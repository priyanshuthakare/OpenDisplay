# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Release-readiness pass: licensing, honest capability reporting, a simplified
Control Center, one-command packaging, and repository hygiene. No wire-protocol
or security-behavior changes.

### Added

- **Dual licensing** — `LICENSE-APACHE` and `LICENSE-MIT` (Apache-2.0 OR MIT),
  matching the declaration in `Cargo.toml`.
- `SECURITY.md` — security model, supported versions, and private vulnerability
  reporting.
- `CONTRIBUTING.md` — per-component build/test instructions and conventions.
- `THIRD_PARTY_NOTICES.md` — licenses of bundled dependencies.
- `QUICKSTART.md` — end-user install path (driver, APK, streaming, Wi-Fi pairing).
- `.github/workflows/release.yml` — tag-triggered (`v*`) release that publishes
  self-contained Control Center (win-x64), the streamer CLI, the Android APK,
  and the driver build/install scripts as GitHub Release assets.
- `.github/workflows/dotnet.yml` — CI job that builds and tests the Control
  Center suite on `windows-latest`.
- `.github/dependabot.yml`, pull-request and issue templates.
- `scripts/setup.ps1` — prerequisite check and guided build.
- `scripts/check-versions.ps1` + `VERSION` — component version drift report.

### Fixed

- **Multi-monitor input landed on the wrong screen** (review 4.1). Tablet
  coordinates were mapped across the *entire virtual desktop* instead of the
  virtual monitor, so touch was offset whenever more than one display was
  active. The host now resolves the `USBDisplay` monitor via
  `EnumDisplayMonitors`/`GetMonitorInfoW` — matched on the driver's EDID
  friendly name — and maps into its rectangle, refreshing once a second so a
  mode switch is absorbed. Falls back to the previous whole-desktop mapping if
  the monitor is absent.
- **A Windows mode switch killed the stream** (review 2.4).
  `stream-capture` aborted with "capture size changed … restart stream" on any
  resolution change. The encoder is now rebuilt at the new size and the sender
  retagged, emitting `stream_resolution_change old=WxH new=WxH`. Applied to
  both live and replay paths, on USB and Wi-Fi.
- **Pairing payloads accepted the wrong version** (review 5.3).
  `PcPairPayload`/`PairPayload` checked the version with
  `String.contains("\"v\":1")`, which also matches `"v":10`, and their
  `indexOf` helpers could latch onto a key name inside another value. Both now
  parse with `org.json` and compare the version exactly.
  `WifiPairPayload.Parse` (C#) ignored the `v` field entirely and skipped the
  port range check on its JSON branch; both are now enforced. Regression tests
  added on all three sides.

### Changed

- **Control Center shell simplified from eight pages to three** — *Home*
  (status, start/stop, transport, pairing, pipeline, telemetry), *Setup*
  (Driver + Device tabs), and *Advanced* (Display, Services, Logs, Diagnostics,
  Settings tabs). Leaf view models and views are unchanged; the new
  `SetupViewModel` / `AdvancedViewModel` only re-host them.
- Wi-Fi pairing inputs are no longer duplicated on the Device page.
- `capabilities` now reports only what is implemented: `codecs=h264,h265`,
  `transport=adb-compat,wifi-tls`, `input=mouse,keyboard`. It previously
  advertised `av1`, `native-usb-bulk`, `hid-touch`, and `hid-pen`, none of which
  were implemented. The same strings in the README and mock gateway were fixed.
- Driver resource version aligned to the INF (`0.2.0.1`, was `0.1.0.0`).
- Control Center version aligned to the product version in `VERSION` (`0.1.0`,
  was `0.3.0`).
- Workspace `Cargo.toml` repository URL corrected; duplicate
  `usbdisplay-encoder` dev-dependency removed.

### Removed

- Committed IDE/build artifacts: `control-app/.vs/*`, `control-app/Backup/`,
  `control-app/UpgradeLog.htm`. `.gitignore` now covers `**/.vs/`, `**/.suo`,
  `**/*.user`, `**/*.vsidx`, and `UpgradeLog*.htm`.
- Empty placeholder directories (`common/`, `benchmarks/`, `tests/`, `tools/`).

### Known limitations

Unchanged in this release and tracked in [`docs/review-findings.md`](docs/review-findings.md):
test-signed driver (no WHQL), ADB-based USB transport (no native bulk), Wi-Fi
hardening items (PIN pairing window, per-host input consent, key/PIN storage),
vendor encoder backends as stubs, and no stylus/pen input.

[Unreleased]: https://github.com/priyanshuthakare/OpenDisplay/commits/master
