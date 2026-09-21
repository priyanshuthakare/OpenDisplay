# Hardware Encoding

The host encodes the captured USBDisplay monitor frames to a compressed video
stream. Encoding runs in the **host** (`usbdisplay-encoder`, consumed by
`usbdisplay-streamer`), not in the sandboxed UMDF driver, so heavy vendor SDKs
stay out of the restricted driver host process.

As of this milestone the host encodes real captured frames to H.264 with a
hardware Media Foundation encoder, validates the stream structurally and by a
decode round-trip, and feeds each coded picture through the protocol + transport
layers. The driver is unchanged.

## Backend selection

Preference order (`probe::select_encoder`): **NVENC -> Quick Sync -> AMF ->
Media Foundation**. Only Media Foundation is implemented; NVENC/QSV/AMF are
honest stubs that report themselves unavailable, so the chain runs end to end
today and each vendor backend is a drop-in later. Every backend's status is
printed, so the selected one and the reasons the others were skipped are always
visible.

| Backend | State | Needs |
| --- | --- | --- |
| NVENC | stub | NVIDIA Video Codec SDK (`nvEncodeAPI`) FFI + D3D11 interop |
| Quick Sync | stub | Intel oneVPL |
| AMF | stub | AMD AMF SDK |
| Media Foundation | **implemented** | `windows` crate (ships with Windows) |

## Pipeline

```
capture_*.bmp  ->  BgraFrame  ->  NV12 (CPU)  ->  MF hardware encoder  ->  Annex-B H.264
                                                                              |
                                            EncodedFrame (protocol) <---------+
                                                     |
                                          Packetizer (transport)
```

* **`bmp`** reads the driver's exact dump format (32-bpp, top-down, `BI_RGB`).
* **`color`** converts BGRA -> NV12 on the CPU with BT.601 limited-range
  coefficients (unit tested against known Y/U/V values; no GPU needed).
* **`backends::mediafoundation`** drives a hardware `IMFTransform`. Hardware
  encoder MFTs are **asynchronous**: the code unlocks async mode
  (`MF_TRANSFORM_ASYNC_UNLOCK`) and drives the transform with the event model
  (`METransformNeedInput` / `METransformHaveOutput` / `METransformDrainComplete`
  via `IMFMediaEventGenerator`), rather than the synchronous
  `ProcessInput`/`ProcessOutput` loop (which fails on hardware MFTs with
  `0xC00D6D77`).
* Each coded picture becomes a protocol `EncodedFrame` and is run through the
  `Packetizer`, proving the encode -> frame -> transport path.

## The `encode-capture` command

```powershell
cargo run -p usbdisplay-streamer -- encode-capture `
    --input-dir "$env:ProgramData\USBDisplay\capture" `
    --out capture.h264 --codec h264 --fps 60 --bitrate 20000000 --gop 60 `
    --verify-decode
```

Reads `capture_*.bmp` in order, selects the best backend, encodes to an Annex-B
elementary stream, and prints a report. `--max-frames N` caps the run;
`--verify-decode` adds the decode round-trip; `--codec h265` targets HEVC.

## Deterministic validation

Two independent checks, both on-device and repeatable:

1. **Structural (`nal`)** -- split the Annex-B stream on start codes and confirm
   SPS + PPS are present and at least one IDR (keyframe) exists
   (`stream_playable=true`).
2. **Decode round-trip (`validate`)** -- feed the exact encoded bytes into the
   Media Foundation H.264 **decoder MFT** and count the frames it emits. A
   working run decodes the same number of frames it encoded. The decoder reports
   the coded size (e.g. `1920x1088` -- 1080 rounded up to a 16px macroblock
   boundary), which is expected and accepted.

Plus `cargo test --workspace` unit tests for the CPU color conversion, BMP
reader, and NAL parser, which need no hardware.

Example output (60 frames):

```
encoder_backend=MediaFoundation
encoded_units=60
nal_total=241 sps=true pps=true vps=false idr=1 non_idr=59
stream_playable=true
transport_packets=96
decoded_frames=60 decoded_size=1920x1088
decode_roundtrip=PASS
OVERALL: PASS -- captured frames encoded and validated
```

## Not yet implemented

- NVENC / Quick Sync / AMF real backends (behind the same trait + probe chain).
- Live shared-memory frame boundary from the driver. On-disk capture was removed
  from the driver (`360fb90`) and the in-memory handoff is not built, so the
  encoder currently has **no** feed from the virtual monitor — supply frames with
  `--input-dir`.
- H.265 validated end to end (the path exists; H.264 is the validated default).
- Bitrate / GOP / scene-change tuning and rate-control modes.
