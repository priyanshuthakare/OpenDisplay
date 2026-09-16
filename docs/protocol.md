# USBDisplay Stream Protocol

All video frame transports carry the same binary protocol. ADB forwarding is an initial compatibility bridge; the native transport is USB bulk transfer.

## Frame Header

| Field | Size | Description |
| --- | ---: | --- |
| magic | 4 | ASCII `USBD` |
| version | 2 | Protocol version, little endian |
| header_len | 2 | Header size in bytes |
| sequence | 8 | Monotonic frame sequence |
| timestamp_ns | 8 | Host capture timestamp |
| codec | 1 | 1 = H.264, 2 = H.265, 3 = AV1 |
| flags | 1 | bit 0 keyframe, bit 1 config, bit 2 EOS |
| width | 2 | Encoded frame width |
| height | 2 | Encoded frame height |
| refresh_millihz | 4 | Refresh rate in mHz |
| payload_len | 4 | Compressed frame payload length |
| payload_crc32 | 4 | CRC32 of compressed payload |
| reserved | 8 | Reserved for forward-compatible metadata |

The current Rust implementation lives in `protocol/`.

## Fragmentation

Native USB bulk transport fragments encoded frames into bounded packet payloads. Reassembly is keyed by frame sequence and fragment index. Missing fragments cause the receiver to drop the frame and request a keyframe.

## Transport Packets

The stream frame protocol is wrapped by the `usbdisplay-transport` crate. Transport packets add:

- Packet sequence numbers
- Packet type
- Frame sequence
- Fragment index and total fragment count
- Payload CRC32
- ACK packets
- Heartbeat packets
- Keyframe request packets
- Handshake packets (kind 6, WiFi pairing / capability negotiation; ignored by
  receivers that only expect video — same rule as other non-Fragment kinds)

This keeps the frame format stable while allowing the ADB compatibility bridge, native USB bulk backend, and WiFi TLS transport to share reliability behavior.

## Input Return Channel

Video frames flow host → device. Input flows the other direction (device → host)
over the same connection, carried in transport `Control` packets. The Android
client writes length-prefixed transport packets back on the stream socket; the
host reads them on a dedicated thread and injects them with Win32 `SendInput`.

A `Control` payload is one or more fixed 16-byte input events. Byte 0 is a kind
tag (`1` = pointer, `2` = key); the rest is variant specific and zero padded.

Pointer event (`kind = 1`):

| Offset | Size | Description |
| ---: | ---: | --- |
| 0 | 1 | kind = 1 |
| 1 | 1 | action: 1 = down, 2 = move, 3 = up, 4 = scroll |
| 2 | 1 | button: 0 = left, 1 = right, 2 = middle, 255 = none |
| 3 | 1 | pointer_id (multi-touch slot, 0 = primary) |
| 4 | 2 | x, normalized 0..65535 across the surface width |
| 6 | 2 | y, normalized 0..65535 across the surface height |
| 8 | 2 | scroll_x, signed wheel notches (scroll only) |
| 10 | 2 | scroll_y, signed wheel notches (scroll only) |
| 12 | 4 | reserved |

Key event (`kind = 2`):

| Offset | Size | Description |
| ---: | ---: | --- |
| 0 | 1 | kind = 2 |
| 1 | 1 | key action: 1 = down, 2 = up |
| 2 | 1 | named key: 0 = char, 1 = enter, 2 = backspace, 3 = tab, 4 = escape, 5 = delete, 6-9 = arrows, 10 = home, 11 = end |
| 3 | 1 | reserved (modifiers, future) |
| 4 | 2 | Unicode code point (when named key = char) |
| 6 | 10 | reserved |

Pointer coordinates are resolution independent: the client normalizes touch
positions to `0..65535` and the host maps them directly onto the Windows
absolute-coordinate space (`MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK`), so
no per-monitor scaling is required. Text keys inject as Unicode
(`KEYEVENTF_UNICODE`); named editing keys inject via their virtual-key codes.
The shared definition lives in `protocol/src/input.rs` (Rust) and `InputEvent.kt`
(Kotlin), pinned together by matching reference-byte tests on both sides.

Today this drives the system cursor and focused-window keyboard across the
desktop. Routing input to the USBDisplay virtual monitor as a real HID
touch/pen device is a later driver-side step.

## Recovery

- CRC mismatch: drop frame and request keyframe.
- Missing fragment: drop frame and request keyframe if the dropped frame was a reference frame.
- Late frame: drop if it would violate the active latency budget.
- Heartbeat timeout: mark the USB session disconnected and enter reconnect.
- ACK gap: retransmit missing packets until acknowledged or until the session reconnects.
