# Quickstart

Turn an Android tablet into a real secondary Windows monitor over USB (or Wi-Fi).

This page is the short path. For the full reference see [`README.md`](README.md);
for architecture and protocol details see [`docs/`](docs/).

---

> ## ⚠ Read this first — the screen is not live yet
>
> The pieces each work, but **they are not joined up end-to-end**. The driver
> captures your virtual monitor into memory, and then nothing hands those frames
> to the streaming host. That handoff is not implemented (tracked as FR-CAP-4 in
> [`docs/PRD.md`](docs/PRD.md)), and on-disk capture was deliberately removed.
>
> **What this means in practice:** you can install the driver and get a real
> second monitor in Windows Display Settings, install the tablet app, and pair
> over Wi-Fi — but pressing START will not put your desktop on the tablet. The
> host reports `no capture_*.bmp frames found` (or waits on
> `waiting_for_capture_frames=true`), on both USB and Wi-Fi.
>
> **What you can still try today:** feed the host your own `.bmp` frames and the
> whole encode → USB/Wi-Fi → Android decode path runs end-to-end (see
> [Prefer the command line?](#prefer-the-command-line)). That exercises
> everything except capture itself.
>
> Everything below is accurate about *how* each piece works. It is not a
> statement that the full pipeline runs today.

---

## What it does

Windows gets a **real virtual monitor** (an Indirect Display Driver), captures
**only that monitor** — never your whole desktop — hardware-encodes it, and
streams it to the tablet, which decodes it fullscreen. Touch and keyboard come
back to Windows as real input.

No cloud. No account. No telemetry.

> The capture → handoff step is the part that is not finished; see the note above.

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

> **This will not show your desktop on the tablet yet.** The capture handoff is
> unimplemented (see the note at the top), so the host finds no frames and the
> run ends in an error like `No frames received within 45 s`. The driver and
> virtual monitor setup below is still real and worth checking.

The Home page looks like this:

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
DIAGNOSTIC** — it reports which step failed and how to fix it.

At this point Windows **Display Settings** should list the new monitor; set it to
**Extend**. That part genuinely works. Getting pixels onto the tablet needs the
missing handoff.

---

## Prefer the command line?

```powershell
# Verify the pieces are present
cargo run -p usbdisplay-streamer -- devices
cargo run -p usbdisplay-streamer -- capabilities

# Stream your OWN frames (the virtual monitor produces none yet).
# --input-dir must contain 32-bpp top-down BMPs.
cargo run -p usbdisplay-streamer -- stream-capture `
    --input-dir "C:\path\to\your\bmp\frames" --loop
```

The second command is the one that works today: with real BMPs in `--input-dir`,
frames encode, travel over USB or Wi-Fi, and decode on the tablet. It is the
capture step — not the transport — that is missing.

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
| "No frames received within 45 s" | Expected today — the capture handoff is unimplemented, so the host never gets frames (see the note at the top). Otherwise check Advanced → Logs for `stream_encoder_backend=`, and that the tablet app is open. |
| Changed resolution briefly froze the tablet | Expected — the host rebuilds the encoder at the new size (`stream_resolution_change`). If it stays black, press **RESTART**. |
| Wi-Fi says "host unreachable … use USB" | AP-isolated/guest network. Use a normal SSID, hotspot, or USB. |

---

## Building from source

```powershell
git clone https://github.com/priyanshuthakare/OpenDisplay
cd OpenDisplay
.\scripts\setup.ps1          # checks prerequisites, then builds what it can
```

`scripts/setup.ps1 -Check` only reports what is installed without building anything.

---

## Not there yet

Be aware of these before you file an issue — they are known and tracked in
[`docs/review-findings.md`](docs/review-findings.md):

- **Streaming does not work end-to-end.** The driver→host capture handoff is
  unimplemented, so no frames reach the streamer. This is the blocking gap.
- **Disk frame capture is intentionally disabled.** Frames are meant to move
  through memory, and that in-memory path is the one still to be built — please
  do not re-enable BMP capture as a workaround.
- The driver is **test-signed**, not WHQL-signed.
- **USB goes through ADB**; native USB bulk transfer is not implemented.
- **Wi-Fi** is for trusted home/lab networks, not hostile ones.
- Encoders: **Media Foundation H.264/H.265** only (NVENC/QSV/AMF are stubs).
- Input is **mouse + keyboard** (no stylus pressure/tilt).
