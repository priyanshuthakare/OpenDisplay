---
name: Bug report
about: Something is broken or behaves unexpectedly
labels: bug
---

<!--
Before filing: check QUICKSTART.md troubleshooting and docs/review-findings.md —
the issue may already be a known, documented limitation (test-signed driver,
ADB-based USB, Wi-Fi hardening, vendor encoder stubs, no pen input).
-->

## What happened

<!-- A clear, short description of the bug. -->

## What you expected

## Which component

- [ ] Driver (virtual monitor not appearing / yellow bang / crash)
- [ ] Android app
- [ ] Control Center GUI
- [ ] Streamer CLI
- [ ] Build / packaging / CI

## Steps to reproduce

1.
2.
3.

## Environment

| | |
|---|---|
| Windows version | <!-- run: winver --> |
| Android version / device | |
| Connection | USB (`adb`) / Wi-Fi |
| Control Center or CLI version | <!-- from the release zip name, or `git rev-parse --short HEAD` --> |
| Driver install method | `driver/idd/install.ps1` / prebuilt |

## Logs

<!--
Control Center: Advanced -> Logs -> Copy, then paste. Diagnostics output is
especially useful (Advanced -> Diagnostics -> RUN FULL DIAGNOSTIC).
CLI: rerun with --stats-json and paste the lines.
Driver: driver/idd/verify.ps1 output, and Event Viewer ->
Microsoft-Windows-DriverFrameworks-UserMode/Operational.
-->

```
paste logs here
```

## Anything else
