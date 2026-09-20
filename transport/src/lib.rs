//! Reliable packet layer used by USBDisplay transports.
//!
//! This crate does not open USB devices by itself. It defines the packet format
//! and retransmission bookkeeping shared by the ADB compatibility bridge and
//! the future native USB bulk backend.

use crc32fast::Hasher;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};
use thiserror::Error;
use usbdisplay_protocol::{EncodedFrame, Fragment, ProtocolError};

pub const PACKET_MAGIC: [u8; 4] = *b"USBT";
pub const PACKET_VERSION: u16 = 1;
pub const PACKET_HEADER_LEN: usize = 40;
pub const DEFAULT_MAX_PACKET_PAYLOAD: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketKind {
    FrameFragment = 1,
    Ack = 2,
    Heartbeat = 3,
    KeyframeRequest = 4,
    Control = 5,
    /// Connection-setup handshake (WiFi pairing / capability negotiation).
    ///
    /// Carries an opaque handshake payload negotiated inside the (TLS)
    /// transport. Receivers that do not understand the handshake version
    /// must ignore the packet — the same rule as other non-Fragment kinds.
    Handshake = 6,
}

impl TryFrom<u8> for PacketKind {
    type Error = TransportError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::FrameFragment),
            2 => Ok(Self::Ack),
            3 => Ok(Self::Heartbeat),
            4 => Ok(Self::KeyframeRequest),
            5 => Ok(Self::Control),
            6 => Ok(Self::Handshake),
            other => Err(TransportError::UnknownPacketKind(other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketHeader {
    pub kind: PacketKind,
    pub packet_sequence: u64,
    pub frame_sequence: u64,
    pub fragment_index: u16,
    pub fragment_total: u16,
    pub payload_len: u32,
    pub payload_crc32: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportPacket {
    pub header: PacketHeader,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ack {
    pub through_packet_sequence: u64,
    pub missing_packet_sequences: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceivedPacket {
    FrameFragment(Fragment),
    Ack(Ack),
    Heartbeat { packet_sequence: u64 },
    KeyframeRequest { frame_sequence: u64 },
    Control(Vec<u8>),
    Handshake(Vec<u8>),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransportError {
    #[error("packet buffer is shorter than the transport header")]
    ShortHeader,
    #[error("invalid transport magic")]
    InvalidMagic,
    #[error("unsupported transport version {0}")]
    UnsupportedVersion(u16),
    #[error("unknown packet kind {0}")]
    UnknownPacketKind(u8),
    #[error("transport payload length {len} exceeds configured maximum {max}")]
    PayloadTooLarge { len: usize, max: usize },
    #[error("packet buffer does not contain the declared payload")]
    ShortPayload,
    #[error("packet payload CRC mismatch: expected {expected:#010x}, got {actual:#010x}")]
    CrcMismatch { expected: u32, actual: u32 },
    #[error("ack payload length must be a multiple of eight")]
    InvalidAckPayload,
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),
}

impl TransportPacket {
    pub fn new(
        kind: PacketKind,
        packet_sequence: u64,
        frame_sequence: u64,
        fragment_index: u16,
        fragment_total: u16,
        payload: Vec<u8>,
    ) -> Self {
        let payload_crc32 = crc32(&payload);
        Self {
            header: PacketHeader {
                kind,
                packet_sequence,
                frame_sequence,
                fragment_index,
                fragment_total,
                payload_len: payload.len() as u32,
                payload_crc32,
            },
            payload,
        }
    }

    pub fn heartbeat(packet_sequence: u64) -> Self {
        Self::new(PacketKind::Heartbeat, packet_sequence, 0, 0, 0, Vec::new())
    }

    pub fn handshake(packet_sequence: u64, payload: Vec<u8>) -> Self {
        Self::new(PacketKind::Handshake, packet_sequence, 0, 0, 0, payload)
    }

    pub fn keyframe_request(packet_sequence: u64, frame_sequence: u64) -> Self {
        Self::new(
            PacketKind::KeyframeRequest,
            packet_sequence,
            frame_sequence,
            0,
            0,
            Vec::new(),
        )
    }

    pub fn ack(packet_sequence: u64, ack: Ack) -> Self {
        let mut payload = Vec::with_capacity(8 + ack.missing_packet_sequences.len() * 8);
        payload.extend_from_slice(&ack.through_packet_sequence.to_le_bytes());
        for missing in ack.missing_packet_sequences {
            payload.extend_from_slice(&missing.to_le_bytes());
        }
        Self::new(PacketKind::Ack, packet_sequence, 0, 0, 0, payload)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(PACKET_HEADER_LEN + self.payload.len());
        out.extend_from_slice(&PACKET_MAGIC);
        out.extend_from_slice(&PACKET_VERSION.to_le_bytes());
        out.extend_from_slice(&(PACKET_HEADER_LEN as u16).to_le_bytes());
        out.push(self.header.kind as u8);
        out.extend_from_slice(&[0u8; 3]);
        out.extend_from_slice(&self.header.packet_sequence.to_le_bytes());
        out.extend_from_slice(&self.header.frame_sequence.to_le_bytes());
        out.extend_from_slice(&self.header.fragment_index.to_le_bytes());
        out.extend_from_slice(&self.header.fragment_total.to_le_bytes());
        out.extend_from_slice(&self.header.payload_len.to_le_bytes());
        out.extend_from_slice(&self.header.payload_crc32.to_le_bytes());
        debug_assert_eq!(out.len(), PACKET_HEADER_LEN);
        out.extend_from_slice(&self.payload);
        out
    }

    pub fn decode(bytes: &[u8], max_payload_len: usize) -> Result<Self, TransportError> {
        if bytes.len() < PACKET_HEADER_LEN {
            return Err(TransportError::ShortHeader);
        }
        if bytes[0..4] != PACKET_MAGIC {
            return Err(TransportError::InvalidMagic);
        }

        let version = u16_at(bytes, 4);
        if version != PACKET_VERSION {
            return Err(TransportError::UnsupportedVersion(version));
        }

        let header_len = u16_at(bytes, 6) as usize;
        if header_len != PACKET_HEADER_LEN || bytes.len() < header_len {
            return Err(TransportError::ShortHeader);
        }

        let kind = PacketKind::try_from(bytes[8])?;
        let packet_sequence = u64_at(bytes, 12);
        let frame_sequence = u64_at(bytes, 20);
        let fragment_index = u16_at(bytes, 28);
        let fragment_total = u16_at(bytes, 30);
        let payload_len = u32_at(bytes, 32);
        let payload_crc32 = u32_at(bytes, 36);

        let payload_len_usize = payload_len as usize;
        if payload_len_usize > max_payload_len {
            return Err(TransportError::PayloadTooLarge {
                len: payload_len_usize,
                max: max_payload_len,
            });
        }

        let payload_end = header_len + payload_len_usize;
        if bytes.len() < payload_end {
            return Err(TransportError::ShortPayload);
        }

        let payload = bytes[header_len..payload_end].to_vec();
        let actual = crc32(&payload);
        if actual != payload_crc32 {
            return Err(TransportError::CrcMismatch {
                expected: payload_crc32,
                actual,
            });
        }

        Ok(Self {
            header: PacketHeader {
                kind,
                packet_sequence,
                frame_sequence,
                fragment_index,
                fragment_total,
                payload_len,
                payload_crc32,
            },
            payload,
        })
    }

    pub fn into_received(self) -> Result<ReceivedPacket, TransportError> {
        match self.header.kind {
            PacketKind::FrameFragment => Ok(ReceivedPacket::FrameFragment(Fragment {
                frame_sequence: self.header.frame_sequence,
                index: self.header.fragment_index,
                total: self.header.fragment_total,
                bytes: self.payload,
            })),
            PacketKind::Ack => {
                if self.payload.len() < 8 || !self.payload.len().is_multiple_of(8) {
                    return Err(TransportError::InvalidAckPayload);
                }
                let through_packet_sequence = u64_at(&self.payload, 0);
                let missing_packet_sequences = self.payload[8..]
                    .chunks_exact(8)
                    .map(|chunk| u64::from_le_bytes(chunk.try_into().expect("chunk is 8 bytes")))
                    .collect();
                Ok(ReceivedPacket::Ack(Ack {
                    through_packet_sequence,
                    missing_packet_sequences,
                }))
            }
            PacketKind::Heartbeat => Ok(ReceivedPacket::Heartbeat {
                packet_sequence: self.header.packet_sequence,
            }),
            PacketKind::KeyframeRequest => Ok(ReceivedPacket::KeyframeRequest {
                frame_sequence: self.header.frame_sequence,
            }),
            PacketKind::Control => Ok(ReceivedPacket::Control(self.payload)),
            PacketKind::Handshake => Ok(ReceivedPacket::Handshake(self.payload)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Packetizer {
    max_packet_payload: usize,
    next_packet_sequence: u64,
}

impl Packetizer {
    pub fn new(max_packet_payload: usize) -> Self {
        Self {
            max_packet_payload: max_packet_payload.max(1),
            next_packet_sequence: 1,
        }
    }

    pub fn packetize_frame(
        &mut self,
        frame: &EncodedFrame,
    ) -> Result<Vec<TransportPacket>, TransportError> {
        let fragments = frame.fragment(self.max_packet_payload)?;
        Ok(fragments
            .into_iter()
            .map(|fragment| {
                TransportPacket::new(
                    PacketKind::FrameFragment,
                    self.take_sequence(),
                    fragment.frame_sequence,
                    fragment.index,
                    fragment.total,
                    fragment.bytes,
                )
            })
            .collect())
    }

    pub fn heartbeat(&mut self) -> TransportPacket {
        TransportPacket::heartbeat(self.take_sequence())
    }

    pub fn keyframe_request(&mut self, frame_sequence: u64) -> TransportPacket {
        TransportPacket::keyframe_request(self.take_sequence(), frame_sequence)
    }

    pub fn ack(&mut self, ack: Ack) -> TransportPacket {
        TransportPacket::ack(self.take_sequence(), ack)
    }

    fn take_sequence(&mut self) -> u64 {
        let sequence = self.next_packet_sequence;
        self.next_packet_sequence = self.next_packet_sequence.wrapping_add(1).max(1);
        sequence
    }
}

#[derive(Debug, Clone)]
pub struct ReassemblyBuffer {
    frame_sequence: u64,
    fragments: BTreeMap<u16, Fragment>,
    total: Option<u16>,
}

impl ReassemblyBuffer {
    pub fn new(frame_sequence: u64) -> Self {
        Self {
            frame_sequence,
            fragments: BTreeMap::new(),
            total: None,
        }
    }

    pub fn push(&mut self, fragment: Fragment) -> Result<Option<EncodedFrame>, TransportError> {
        if fragment.frame_sequence != self.frame_sequence {
            return Err(ProtocolError::FragmentSequenceMismatch {
                fragment: fragment.index,
                expected: self.frame_sequence,
                actual: fragment.frame_sequence,
            }
            .into());
        }

        self.total = Some(fragment.total);
        self.fragments.entry(fragment.index).or_insert(fragment);

        let total = self.total.unwrap_or(0);
        if total == 0 || self.fragments.len() != total as usize {
            return Ok(None);
        }

        let ordered: Vec<Fragment> = self.fragments.values().cloned().collect();
        Ok(Some(EncodedFrame::reassemble(
            self.frame_sequence,
            &ordered,
        )?))
    }
}

#[derive(Debug, Clone)]
pub struct RetransmitWindow {
    timeout: Duration,
    in_flight: BTreeMap<u64, QueuedPacket>,
}

#[derive(Debug, Clone)]
struct QueuedPacket {
    packet: TransportPacket,
    last_sent: Instant,
}

impl RetransmitWindow {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            in_flight: BTreeMap::new(),
        }
    }

    pub fn track_sent(&mut self, packet: TransportPacket, now: Instant) {
        if packet.header.kind == PacketKind::FrameFragment {
            self.in_flight.insert(
                packet.header.packet_sequence,
                QueuedPacket {
                    packet,
                    last_sent: now,
                },
            );
        }
    }

    pub fn apply_ack(&mut self, ack: &Ack) {
        let missing: BTreeSet<u64> = ack.missing_packet_sequences.iter().copied().collect();
        let acked: Vec<u64> = self
            .in_flight
            .keys()
            .copied()
            .filter(|sequence| {
                *sequence <= ack.through_packet_sequence && !missing.contains(sequence)
            })
            .collect();

        for sequence in acked {
            self.in_flight.remove(&sequence);
        }
    }

    pub fn expired(&self, now: Instant) -> Vec<TransportPacket> {
        self.in_flight
            .values()
            .filter(|queued| now.duration_since(queued.last_sent) >= self.timeout)
            .map(|queued| queued.packet.clone())
            .collect()
    }

    pub fn mark_resent(&mut self, packet_sequence: u64, now: Instant) {
        if let Some(queued) = self.in_flight.get_mut(&packet_sequence) {
            queued.last_sent = now;
        }
    }

    pub fn len(&self) -> usize {
        self.in_flight.len()
    }

    pub fn is_empty(&self) -> bool {
        self.in_flight.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct ReceiverAcks {
    highest_contiguous: u64,
    received_out_of_order: BTreeSet<u64>,
}

impl ReceiverAcks {
    pub fn new() -> Self {
        Self {
            highest_contiguous: 0,
            received_out_of_order: BTreeSet::new(),
        }
    }

    pub fn observe(&mut self, packet_sequence: u64) -> Ack {
        if packet_sequence == self.highest_contiguous + 1 {
            self.highest_contiguous = packet_sequence;
            while self
                .received_out_of_order
                .remove(&(self.highest_contiguous + 1))
            {
                self.highest_contiguous += 1;
            }
        } else if packet_sequence > self.highest_contiguous + 1 {
            self.received_out_of_order.insert(packet_sequence);
        }

        let missing_packet_sequences = ((self.highest_contiguous + 1)
            ..self
                .received_out_of_order
                .last()
                .copied()
                .unwrap_or(self.highest_contiguous))
            .filter(|sequence| !self.received_out_of_order.contains(sequence))
            .collect();

        Ack {
            through_packet_sequence: self.highest_contiguous,
            missing_packet_sequences,
        }
    }
}

impl Default for ReceiverAcks {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct HeartbeatMonitor {
    timeout: Duration,
    last_seen: Instant,
}

impl HeartbeatMonitor {
    pub fn new(timeout: Duration, now: Instant) -> Self {
        Self {
            timeout,
            last_seen: now,
        }
    }

    pub fn observe(&mut self, now: Instant) {
        self.last_seen = now;
    }

    pub fn is_disconnected(&self, now: Instant) -> bool {
        now.duration_since(self.last_seen) >= self.timeout
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(bytes);
    hasher.finalize()
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use usbdisplay_protocol::{Codec, FrameFlags};

    #[test]
    fn packet_round_trips() {
        let packet =
            TransportPacket::new(PacketKind::FrameFragment, 10, 20, 1, 3, b"payload".to_vec());

        let decoded =
            TransportPacket::decode(&packet.encode(), DEFAULT_MAX_PACKET_PAYLOAD).unwrap();
        assert_eq!(decoded, packet);
    }

    #[test]
    fn handshake_packet_round_trips() {
        let packet = TransportPacket::handshake(7, b"hello-wifi".to_vec());

        let decoded =
            TransportPacket::decode(&packet.encode(), DEFAULT_MAX_PACKET_PAYLOAD).unwrap();
        assert_eq!(decoded, packet);
        match decoded.into_received().unwrap() {
            ReceivedPacket::Handshake(payload) => assert_eq!(payload, b"hello-wifi"),
            other => panic!("expected Handshake, got {other:?}"),
        }
    }

    #[test]
    fn packet_crc_rejects_corruption() {
        let packet = TransportPacket::heartbeat(1);
        let mut bytes = packet.encode();
        bytes[PACKET_HEADER_LEN - 1] ^= 0xff;

        assert!(matches!(
            TransportPacket::decode(&bytes, DEFAULT_MAX_PACKET_PAYLOAD),
            Err(TransportError::CrcMismatch { .. })
        ));
    }

    #[test]
    fn packetizes_and_reassembles_frame() {
        let payload = (0..50_000).map(|v| (v % 251) as u8).collect();
        let frame = EncodedFrame::new(
            44,
            1_000,
            Codec::H265,
            FrameFlags::KEYFRAME,
            2560,
            1600,
            120_000,
            payload,
        )
        .unwrap();

        let mut packetizer = Packetizer::new(1200);
        let packets = packetizer.packetize_frame(&frame).unwrap();
        assert!(packets.len() > 10);

        let mut reassembly = ReassemblyBuffer::new(44);
        let mut completed = None;
        for packet in packets {
            match packet.into_received().unwrap() {
                ReceivedPacket::FrameFragment(fragment) => {
                    completed = reassembly.push(fragment).unwrap();
                }
                _ => unreachable!(),
            }
        }

        assert_eq!(completed.unwrap(), frame);
    }

    #[test]
    fn receiver_ack_reports_gap_then_closes_it() {
        let mut receiver = ReceiverAcks::new();
        assert_eq!(receiver.observe(1).through_packet_sequence, 1);
        let ack = receiver.observe(3);
        assert_eq!(ack.through_packet_sequence, 1);
        assert_eq!(ack.missing_packet_sequences, vec![2]);
        let ack = receiver.observe(2);
        assert_eq!(ack.through_packet_sequence, 3);
        assert!(ack.missing_packet_sequences.is_empty());
    }

    #[test]
    fn retransmit_window_expires_and_acks_packets() {
        let now = Instant::now();
        let mut window = RetransmitWindow::new(Duration::from_millis(10));
        let packet = TransportPacket::new(PacketKind::FrameFragment, 1, 5, 0, 1, b"x".to_vec());
        window.track_sent(packet, now);

        assert!(window.expired(now + Duration::from_millis(5)).is_empty());
        assert_eq!(window.expired(now + Duration::from_millis(10)).len(), 1);

        window.apply_ack(&Ack {
            through_packet_sequence: 1,
            missing_packet_sequences: Vec::new(),
        });
        assert!(window.is_empty());
    }

    #[test]
    fn heartbeat_monitor_detects_disconnect() {
        let now = Instant::now();
        let mut monitor = HeartbeatMonitor::new(Duration::from_secs(2), now);
        assert!(!monitor.is_disconnected(now + Duration::from_secs(1)));
        assert!(monitor.is_disconnected(now + Duration::from_secs(2)));
        monitor.observe(now + Duration::from_secs(3));
        assert!(!monitor.is_disconnected(now + Duration::from_secs(4)));
    }

    proptest! {
        #[test]
        fn arbitrary_packet_payloads_round_trip(payload in proptest::collection::vec(any::<u8>(), 0..65536)) {
            let packet = TransportPacket::new(PacketKind::Control, 99, 0, 0, 0, payload);
            prop_assert_eq!(
                TransportPacket::decode(&packet.encode(), DEFAULT_MAX_PACKET_PAYLOAD).unwrap(),
                packet
            );
        }
    }
}
