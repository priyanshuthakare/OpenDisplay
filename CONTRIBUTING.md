# Contributing to USBDisplay

Thanks for your interest! USBDisplay is a Windows-to-Android secondary-display
stack spanning four components. This guide covers how to build, test, and submit
changes.

## Components & prerequisites

| Component | Path | Toolchain |
|---|---|---|
| Shared protocol / transport / host | `protocol/`, `transport/`, `host/` | Rust stable (<https://rustup.rs/>) |
| Indirect Display Driver | `driver/idd/` | Visual Studio + WDK (Windows only) |
| Android client | `android/` | Android Studio + SDK 35, JDK 17 |
| Control Center GUI | `control-app/` | .NET 8 SDK (Windows) |

You do **not** need all four to contribute — most crates and the GUI build and
test without hardware, a WDK, or a tablet.

## Build & test

```powershell
# Rust workspace (portable — runs on Linux/Windows CI)
cargo test --workspace

# Control Center (Windows)
dotnet test control-app/USBDisplay.sln

# Android unit tests
cd android; ./gradlew testDebugUnitTest
```

CI runs the Rust workspace on Ubuntu + Windows, the Android unit tests, and the
.NET test suite on every push and PR. Please make sure the relevant suite is
green before opening a PR.

## Coding conventions

- **Rust:** run `cargo fmt --all` and address `cargo clippy` warnings on code you
  touch. Keep Windows-only code behind `#[cfg(windows)]` with a portable
  `#[cfg(not(windows))]` fallback so the Ubuntu CI leg keeps building.
- **Wire formats** (`protocol/`, `transport/`, input events) are pinned by
  reference-byte tests shared across Rust and Kotlin. If you change a byte
  layout, update **both** sides and the tests.
- **CLI ↔ GUI contract:** the streamer emits stable `key=value` stdout lines; the
  GUI parses only those, never free text. Don't break existing keys without
  updating `control-app` parsers and the README's interface tables.
- **Honesty:** advertise only capabilities that are actually implemented (see
  `docs/review-findings.md`). Stubs must clearly say "not yet implemented."

## Pull requests

1. Branch from `master` (`feature/...` or `fix/...`).
2. Keep PRs focused; describe what changed and how you tested it.
3. Update docs (`README.md`, `docs/`) when you change an interface or behavior.

## Licensing

This project is dual-licensed under **Apache-2.0 OR MIT** (see
[`LICENSE-APACHE`](LICENSE-APACHE) and [`LICENSE-MIT`](LICENSE-MIT)). By
contributing, you agree that your contributions are licensed under the same
terms.
