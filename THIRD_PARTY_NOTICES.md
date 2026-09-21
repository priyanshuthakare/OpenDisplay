# Third-Party Notices

USBDisplay bundles or depends on the third-party components listed below. Each
remains under its own license; this file is an informational summary. Refer to
each project for authoritative license text.

## Rust host (`host/`, `protocol/`, `transport/`)

| Component | License |
|---|---|
| `anyhow`, `thiserror` | MIT OR Apache-2.0 |
| `crc32fast` | MIT OR Apache-2.0 |
| `base64`, `hex`, `sha2` | MIT OR Apache-2.0 |
| `chrono` | MIT OR Apache-2.0 |
| `clap` | MIT OR Apache-2.0 |
| `serde`, `serde_json` | MIT OR Apache-2.0 |
| `rcgen` | MIT OR Apache-2.0 |
| `rustls`, `rustls-pki-types`, `rustls-native-certs` | Apache-2.0 OR ISC OR MIT |
| `windows` (windows-rs) | MIT OR Apache-2.0 |
| `proptest`, `tempfile` (dev) | MIT OR Apache-2.0 |

## Android client (`android/`)

| Component | License |
|---|---|
| ZXing `core` | Apache-2.0 |
| Bouncy Castle `bcpkix-jdk18on` | Bouncy Castle License (MIT-style) |
| JUnit 4 (test) | Eclipse Public License 1.0 |

## Control Center (`control-app/`)

| Component | License |
|---|---|
| QRCoder | MIT |
| MSTest (TestFramework / TestAdapter), Microsoft.NET.Test.Sdk (test) | MIT |
| coverlet.collector (test) | MIT |

## Platform SDKs

The Windows Indirect Display Driver builds against the Microsoft Windows
Driver Kit (WDK) and IddCx headers, used under the Microsoft Software License
Terms for the WDK. These are not redistributed by this repository.
