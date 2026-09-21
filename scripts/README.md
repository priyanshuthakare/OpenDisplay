# Scripts

Developer and contributor helper scripts.

| Script | Purpose |
|---|---|
| `setup.ps1` | Check prerequisites (Rust, .NET 8, JDK 17, adb, WDK) and build/test every component whose toolchain is present. `-Check` reports only; `-SkipTests` builds only. |
| `check-versions.ps1` | Print each component's version and flag drift from the root `VERSION` file. `-Strict` exits non-zero on mismatch (for release checks). |

Driver-specific scripts (build, sign, install, uninstall, verify, diagnostics)
live with the driver in [`driver/idd/`](../driver/idd/).
