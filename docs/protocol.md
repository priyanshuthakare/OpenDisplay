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

This keeps the frame format stable while allowing the ADB compatibility bridge and native USB bulk backend to share reliability behavior.

## Recovery

- CRC mismatch: drop frame and request keyframe.
- Missing fragment: drop frame and request keyframe if the dropped frame was a reference frame.
- Late frame: drop if it would violate the active latency budget.
- Heartbeat timeout: mark the USB session disconnected and enter reconnect.
- ACK gap: retransmit missing packets until acknowledged or until the session reconnects.
