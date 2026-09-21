# Quickstart

Turn an Android tablet into a real secondary Windows monitor over USB (or Wi-Fi).

This page is the short path. For the full reference see [`README.md`](README.md);
for architecture and protocol details see [`docs/`](docs/).

---

## What it does

Windows gets a **real virtual monitor** (an Indirect Display Driver), captures
**only that monitor** — never your whole desktop — hardware-encodes it, and
streams it to the tablet, which decodes it fullscreen. Touch and keyboard come
back to Windows as real input.

No cloud. No account. No telemetry.

---

## What you need

| | Requirement |
|---|---|
| **PC** | Windows 10/11 x64, a spare USB port |
| **Tablet** | Android 8.0+ (API 26), USB data cable (not charge-only) |
| **PC (driver step only)** | [Windows Driver Kit (WDK)](https://learn.microsoft.com/windows-hardware/drivers/download-the-wdk) |
| **PC (from-source only)** | [Rust](https://rustup.rs/), [.NET 8 SDK](https://dotnet.microsoft.com/download/dotnet/8.0) |

> **Why the WDK?** The virtual monitor is a kernel-visible display device. It has
> to be built and installed like any other Windows driver. There is no way around
> this step — but you only do it once.

---

## 1. Download a release

Grab the latest from **[Releases](https://github.com/priyanshuthakare/OpenDisplay/releases)**:

| File | What it is |
|---|---|
| `usbdisplay-control-center-<ver>-win-x64.zip` | The GUI (self-contained — no .NET install needed) |
| `usbdisplay-streamer-<ver>-win-x64.zip` | The CLI/engine (optional if you use the GUI) |
| `usbdisplay-android-<ver>.apk` | The tablet app |
| `usbdisplay-driver-scripts.zip` | Everything needed to build + install the virtual display driver |

Unzip the Control Center somewhere permanent (e.g. `C:\Program Files\USBDisplay`).

---

## 2. Install the virtual display driver (once)

This is the only step that needs administrator rights and the WDK.

```powershell
# Unzip usbdisplay-driver-scripts.zip, then from inside that folder:
bcdedit /set testsigning on      # then REBOOT once
.\install.ps1                    # run from an ELEVATED PowerShell
```

`install.ps1` builds, signs (test certificate), creates the `Root\USBDisplayIdd`
device node, installs the package, and runs `verify.ps1` as a PASS/FAIL gate.

> **Test signing.** The driver is self-signed for development, so Windows needs
> test-signing mode for it to load. You will see a "Test Mode" watermark on the
> desktop — that is expected and means it worked. Uninstall with
> `driver\idd\uninstall.ps1` and `bcdedit /set testsigning off` to remove it.

When it succeeds, **Windows Display Settings will list a second monitor**.

---

## 3. Install the tablet app

```powershell
adb install -r usbdisplay-android-<ver>.apk
```

Or copy the APK to the tablet and tap it (allow "install from unknown sources").

Then on the tablet: **Settings → Developer options → USB debugging → ON**, and
plug in the USB cable. Accept the "Allow USB debugging?" prompt.

---

## 4. Run it

Open **USBDisplay Control Center** (the app from step 1) and press **START USB DISPLAY**.

The Home page walks you through it:

```
┌──────────────────────────────────┐
│ USBDisplay          [Refresh]     │
├────────┬─────────────────────────┤
│ Home   │  ● DISPLAY ACTIVE       │
│ Setup  │  [STOP]   [RESTART]     │
│ Adv    │  Transport: USB ▾       │
│        │  WIN→CAP→ENC→USB→ANDROID│
│        │  1080p60 · h264         │
└────────┴─────────────────────────┘
```

If anything is red, open **Advanced → Diagnostics** and press **RUN FULL
DIAGNOSTIC** — it reports exactly which step failed and how to fix it.

Finally, in Windows **Display Settings**, set the new monitor to **Extend**.
Your tablet is now a second screen.

---

## Prefer the command line?

```powershell
# Verify the pieces are present
cargo run -p usbdisplay-streamer -- devices
cargo run -p usbdisplay-streamer -- capabilities

# Stream over USB
cargo run -p usbdisplay-streamer -- stream-capture `
    --input-dir "$env:ProgramData\USBDisplay\capture" --loop
```

---

## Wi-Fi instead of USB

Same result, no cable — but TLS-only, so it needs pairing once:

1. On the tablet, open **WiFi Pair**. Note the LAN IP, the 6-digit PIN, and the QR.
2. In Control Center, Home page → Transport **Wi-Fi** → enter the IP and PIN →
   **CONNECT WI-FI**.

Wrong PIN three times locks the tablet for 30 s. A rotated certificate must be
re-paired — it is never silently accepted. Guest/isolated Wi-Fi networks block
device-to-PC connections; use a normal home network, a hotspot, or USB.

---

## Troubleshooting

| Symptom | Fix |
|---|---|
| Driver won't load / yellow bang in Device Manager | Test signing not on — `bcdedit /set testsigning on`, reboot. |
| `pnputil` says "up to date" but nothing changed | Bump `DriverVer` in `driver/idd/Driver.inf` or run `uninstall.ps1` first. |
| Tablet shows `unauthorized` in `adb devices` | Unlock the tablet and accept the USB debugging prompt. |
| "No frames received within 45 s" | Check Advanced → Logs for `stream_encoder_backend=`, and that the tablet app is open. |
| Changed resolution briefly froze the tablet | Expected — the host rebuilds the encoder at the new size (`stream_resolution_change`). If it stays black, press **RESTART**. |
| Wi-Fi says "host unreachable … use USB" | AP-isolated/guest network. Use a normal SSID, hotspot, or USB. |

---

## Building from source

```powershell
git clone https://github.com/priyanshuthakare/OpenDisplay
cd USBdisplay
.\scripts\setup.ps1          # checks prerequisites, then builds what it can
```

`scripts/setup.ps1 -Check` only reports what is installed without building anything.

---

## Not there yet

Be aware of these before you file an issue — they are known and tracked in
[`docs/review-findings.md`](docs/review-findings.md):

- The driver is **test-signed**, not WHQL-signed.
- **USB goes through ADB**; native USB bulk transfer is not implemented.
- **Wi-Fi** is for trusted home/lab networks, not hostile ones.
- Encoders: **Media Foundation H.264/H.265** only (NVENC/QSV/AMF are stubs).
- Input is **mouse + keyboard** (no stylus pressure/tilt).
- Resolution changes restart the stream.
