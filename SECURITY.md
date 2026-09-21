# Security Policy

USBDisplay is a **local-link-only** project: no cloud services, no telemetry, no
accounts. The trust boundary is your own machine and the local USB cable or
LAN segment between the Windows host and the Android tablet.

## Reporting a vulnerability

Please report suspected security issues **privately** — do not open a public
issue for anything exploitable.

- Use GitHub's private vulnerability reporting:
  <https://github.com/priyanshuthakare/OpenDisplay/security/advisories/new>
- Include a description, affected component (driver / host / android /
  control-app), reproduction steps, and impact.

You can expect an acknowledgement within a reasonable time. Because this is a
community project there is no formal SLA, but credible reports are taken
seriously and fixes are prioritized.

## Supported versions

This project is pre-1.0. Security fixes land on `master` and are not
back-ported to older tags. Always build from the latest `master`.

## Security model

| Surface | Protection |
|---|---|
| USB transport | Loopback socket `127.0.0.1:27183` reached only through `adb forward` over a physically connected, USB-debugging-authorized device. |
| Wi-Fi transport | TLS 1.3 **only** (`0.0.0.0:27184`), 6-digit PIN pairing, trust-on-first-use certificate fingerprint pinning, PIN lockout after repeated failures. No plaintext fallback. |
| Capture | The host captures **only** the virtual IDD monitor's swap chain — never the whole desktop. |
| Input return | Device→host input is injected via Win32 `SendInput` (absolute mouse + keyboard). |
| Data at rest | Pairing state is stored locally (`%AppData%\USBDisplay\paired.json`, tablet SharedPreferences). No secrets leave the machine. |

## Known limitations

This is an in-progress project. Security-relevant items that are **not yet
hardened** are tracked openly in [`docs/review-findings.md`](docs/review-findings.md),
including: PIN pairing-window / single-use semantics, per-host input consent,
private-key and PIN storage hardening, and Wi-Fi listener exposure gating.
Treat Wi-Fi mode as suitable for trusted home/lab LANs, not hostile networks.
When in doubt, use the USB transport.
