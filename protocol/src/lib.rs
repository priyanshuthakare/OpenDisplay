//! Binary protocol shared by the Windows host and Android client.
//!
//! The protocol intentionally keeps transport concerns separate from frame
//! framing. A native USB bulk endpoint, ADB compatibility bridge, or test file
//! can all carry the same byte stream.

use crc32fast::Hasher;
use thiserror::Error;

pub mod input;

pub use input::{
    InputAction, InputError, InputEvent, InputKind, KeyAction, KeyEvent, NamedKey, PointerButton,
    PointerEvent, INPUT_EVENT_LEN,
};

pub const MAGIC: [u8; 4] = *b"USBD";
pub const VERSION: u16 = 1;
pub const HEADER_LEN: usize = 50;
pub const MAX_PAYLOAD_LEN: usize = 128 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Codec {
    H264 = 1,
    H265 = 2,
    Av1 = 3,
}

impl TryFrom<u8> for Codec {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::H264),
            2 => Ok(Self::H265),
            3 => Ok(Self::Av1),
            other => Err(ProtocolError::UnknownCodec(other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameFlags(u8);

impl FrameFlags {
    pub const NONE: Self = Self(0);
    pub const KEYFRAME: Self = Self(1 << 0);
    pub const CONFIG: Self = Self(1 << 1);
    pub const END_OF_STREAM: Self = Self(1 << 2);

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0
    }

    pub const fn union(self, flag: Self) -> Self {
        Self(self.0 | flag.0)
    }
}

impl From<u8> for FrameFlags {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameHeader {
    pub sequence: u64,
    pub timestamp_ns: u64,
    pub codec: Codec,
    pub flags: FrameFlags,
    pub width: u16,
    pub height: u16,
    pub refresh_millihz: u32,
    pub payload_len: u32,
    pub payload_crc32: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    pub header: FrameHeader,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    pub frame_sequence: u64,
    pub index: u16,
    pub total: u16,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("buffer is shorter than the protocol header")]
    ShortHeader,
    #[error("invalid magic")]
    InvalidMagic,
    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u16),
    #[error("unknown codec {0}")]
    UnknownCodec(u8),
    #[error("payload length {0} exceeds maximum")]
    PayloadTooLarge(usize),
    #[error("buffer does not contain the declared payload")]
    ShortPayload,
    #[error("payload CRC mismatch: expected {expected:#010x}, got {actual:#010x}")]
    CrcMismatch { expected: u32, actual: u32 },
    #[error("fragment size must be greater than zero")]
    ZeroFragmentSize,
    #[error("frame needs {needed} fragments, exceeding u16::MAX")]
    TooManyFragments { needed: usize },
    #[error("missing fragment {0}")]
    MissingFragment(u16),
    #[error("fragment {fragment} belongs to frame sequence {actual}, expected {expected}")]
    FragmentSequenceMismatch {
        fragment: u16,
        expected: u64,
        actual: u64,
    },
}

impl EncodedFrame {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sequence: u64,
        timestamp_ns: u64,
        codec: Codec,
        flags: FrameFlags,
        width: u16,
        height: u16,
        refresh_millihz: u32,
        payload: Vec<u8>,
    ) -> Result<Self, ProtocolError> {
        if payload.len() > MAX_PAYLOAD_LEN {
            return Err(ProtocolError::PayloadTooLarge(payload.len()));
        }

        let payload_crc32 = crc32(&payload);
        let payload_len = payload.len() as u32;

        Ok(Self {
            header: FrameHeader {
                sequence,
                timestamp_ns,
                codec,
                flags,
                width,
                height,
                refresh_millihz,
                payload_len,
                payload_crc32,
            },
            payload,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.payload.len());
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(HEADER_LEN as u16).to_le_bytes());
        out.extend_from_slice(&self.header.sequence.to_le_bytes());
        out.extend_from_slice(&self.header.timestamp_ns.to_le_bytes());
        out.push(self.header.codec as u8);
        out.push(self.header.flags.bits());
        out.extend_from_slice(&self.header.width.to_le_bytes());
        out.extend_from_slice(&self.header.height.to_le_bytes());
        out.extend_from_slice(&self.header.refresh_millihz.to_le_bytes());
        out.extend_from_slice(&self.header.payload_len.to_le_bytes());
        out.extend_from_slice(&self.header.payload_crc32.to_le_bytes());
        out.extend_from_slice(&[0u8; 8]);
        debug_assert_eq!(out.len(), HEADER_LEN);
        out.extend_from_slice(&self.payload);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() < HEADER_LEN {
            return Err(ProtocolError::ShortHeader);
        }
        if bytes[0..4] != MAGIC {
            return Err(ProtocolError::InvalidMagic);
        }

        let version = u16_at(bytes, 4);
        if version != VERSION {
            return Err(ProtocolError::UnsupportedVersion(version));
        }

        let header_len = u16_at(bytes, 6) as usize;
        if header_len != HEADER_LEN || bytes.len() < header_len {
            return Err(ProtocolError::ShortHeader);
        }

        let sequence = u64_at(bytes, 8);
        let timestamp_ns = u64_at(bytes, 16);
        let codec = Codec::try_from(bytes[24])?;
        let flags = FrameFlags::from(bytes[25]);
        let width = u16_at(bytes, 26);
        let height = u16_at(bytes, 28);
        let refresh_millihz = u32_at(bytes, 30);
        let payload_len = u32_at(bytes, 34);
        let payload_crc32 = u32_at(bytes, 38);

        let payload_len_usize = payload_len as usize;
        if payload_len_usize > MAX_PAYLOAD_LEN {
            return Err(ProtocolError::PayloadTooLarge(payload_len_usize));
        }

        let payload_end = header_len + payload_len_usize;
        if bytes.len() < payload_end {
            return Err(ProtocolError::ShortPayload);
        }

        let payload = bytes[header_len..payload_end].to_vec();
        let actual = crc32(&payload);
        if actual != payload_crc32 {
            return Err(ProtocolError::CrcMismatch {
                expected: payload_crc32,
                actual,
            });
        }

        Ok(Self {
            header: FrameHeader {
                sequence,
                timestamp_ns,
                codec,
                flags,
                width,
                height,
                refresh_millihz,
                payload_len,
                payload_crc32,
            },
            payload,
        })
    }

    pub fn fragment(&self, max_fragment_payload: usize) -> Result<Vec<Fragment>, ProtocolError> {
        if max_fragment_payload == 0 {
            return Err(ProtocolError::ZeroFragmentSize);
        }

        let encoded = self.encode();
        let total = encoded.len().div_ceil(max_fragment_payload);
        if total > u16::MAX as usize {
            return Err(ProtocolError::TooManyFragments { needed: total });
        }

        Ok(encoded
            .chunks(max_fragment_payload)
            .enumerate()
            .map(|(index, chunk)| Fragment {
                frame_sequence: self.header.sequence,
                index: index as u16,
                total: total as u16,
                bytes: chunk.to_vec(),
            })
            .collect())
    }

    pub fn reassemble(frame_sequence: u64, fragments: &[Fragment]) -> Result<Self, ProtocolError> {
        let total = fragments
            .first()
            .map(|fragment| fragment.total)
            .unwrap_or(0);
        let mut ordered: Vec<Option<&Fragment>> = vec![None; total as usize];

        for fragment in fragments {
            if fragment.frame_sequence != frame_sequence {
                return Err(ProtocolError::FragmentSequenceMismatch {
                    fragment: fragment.index,
                    expected: frame_sequence,
                    actual: fragment.frame_sequence,
                });
            }
            if usize::from(fragment.index) < ordered.len() {
                ordered[usize::from(fragment.index)] = Some(fragment);
            }
        }

        let mut bytes = Vec::new();
        for (index, fragment) in ordered.into_iter().enumerate() {
            let fragment = fragment.ok_or(ProtocolError::MissingFragment(index as u16))?;
            bytes.extend_from_slice(&fragment.bytes);
        }

        Self::decode(&bytes)
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

    #[test]
    fn round_trips_encoded_frame() {
        let frame = EncodedFrame::new(
            42,
            1_000_000,
            Codec::H265,
            FrameFlags::KEYFRAME,
            2560,
            1600,
            120_000,
            b"encoded frame".to_vec(),
        )
        .unwrap();

        let decoded = EncodedFrame::decode(&frame.encode()).unwrap();
        assert_eq!(decoded, frame);
        assert!(decoded.header.flags.contains(FrameFlags::KEYFRAME));
    }

    #[test]
    fn rejects_corrupted_payload() {
        let frame = EncodedFrame::new(
            1,
            2,
            Codec::H264,
            FrameFlags::NONE,
            1920,
            1080,
            60_000,
            b"abc".to_vec(),
        )
        .unwrap();

        let mut bytes = frame.encode();
        let last = bytes.last_mut().unwrap();
        *last ^= 0xff;

        assert!(matches!(
            EncodedFrame::decode(&bytes),
            Err(ProtocolError::CrcMismatch { .. })
        ));
    }

    #[test]
    fn fragments_and_reassembles() {
        let payload = (0..10_000).map(|v| (v % 251) as u8).collect();
        let frame = EncodedFrame::new(
            7,
            99,
            Codec::Av1,
            FrameFlags::KEYFRAME.union(FrameFlags::CONFIG),
            3840,
            2160,
            90_000,
            payload,
        )
        .unwrap();

        let fragments = frame.fragment(513).unwrap();
        assert!(fragments.len() > 10);

        let decoded = EncodedFrame::reassemble(7, &fragments).unwrap();
        assert_eq!(decoded, frame);
    }

    proptest! {
        #[test]
        fn arbitrary_payloads_round_trip(payload in proptest::collection::vec(any::<u8>(), 0..65536)) {
            let frame = EncodedFrame::new(
                123,
                456,
                Codec::H264,
                FrameFlags::NONE,
                1920,
                1080,
                60_000,
                payload,
            ).unwrap();

            prop_assert_eq!(EncodedFrame::decode(&frame.encode()).unwrap(), frame);
        }
    }
}
