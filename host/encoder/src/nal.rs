//! Annex-B NAL-unit inspection for validating encoder output.
//!
//! Just enough parsing to prove the stream is well-formed: split on start codes
//! (`00 00 01` / `00 00 00 01`) and classify each NAL. This is the structural
//! half of the milestone's deterministic validation (the other half is a
//! decode round-trip). Supports H.264 and H.265 NAL type encodings.

use crate::config::Codec;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NalSummary {
    pub total_nals: usize,
    pub has_sps: bool,
    pub has_pps: bool,
    /// H.265 only; H.264 has no VPS.
    pub has_vps: bool,
    /// Number of IDR (keyframe) coded slices.
    pub idr_slices: usize,
    /// Number of non-IDR coded slices.
    pub non_idr_slices: usize,
}

impl NalSummary {
    /// A decodable stream must carry at least one parameter set of each required
    /// kind and at least one IDR to start from.
    pub fn is_playable(&self, codec: Codec) -> bool {
        let params = match codec {
            Codec::H264 => self.has_sps && self.has_pps,
            Codec::H265 => self.has_sps && self.has_pps && self.has_vps,
        };
        params && self.idr_slices > 0
    }
}

/// Iterate the NAL payloads in an Annex-B buffer (start codes stripped).
fn nal_units(stream: &[u8]) -> Vec<&[u8]> {
    let mut nals = Vec::new();
    let mut i = 0;
    let n = stream.len();
    // Find first start code.
    let mut start = None;
    while i + 3 <= n {
        if stream[i] == 0 && stream[i + 1] == 0 && stream[i + 2] == 1 {
            start = Some(i + 3);
            i += 3;
            break;
        }
        i += 1;
    }
    let mut cur = match start {
        Some(s) => s,
        None => return nals,
    };
    // Walk subsequent start codes.
    while i + 3 <= n {
        if stream[i] == 0 && stream[i + 1] == 0 && stream[i + 2] == 1 {
            // Trim a trailing 0 that belongs to a 4-byte start code.
            let mut end = i;
            if end > cur && stream[end - 1] == 0 {
                end -= 1;
            }
            nals.push(&stream[cur..end]);
            cur = i + 3;
            i += 3;
        } else {
            i += 1;
        }
    }
    if cur < n {
        nals.push(&stream[cur..n]);
    }
    nals
}

pub fn summarize(stream: &[u8], codec: Codec) -> NalSummary {
    let mut s = NalSummary::default();
    for nal in nal_units(stream) {
        if nal.is_empty() {
            continue;
        }
        s.total_nals += 1;
        match codec {
            Codec::H264 => {
                let t = nal[0] & 0x1f;
                match t {
                    7 => s.has_sps = true,
                    8 => s.has_pps = true,
                    5 => s.idr_slices += 1,
                    1 => s.non_idr_slices += 1,
                    _ => {}
                }
            }
            Codec::H265 => {
                let t = (nal[0] >> 1) & 0x3f;
                match t {
                    33 => s.has_sps = true,
                    34 => s.has_pps = true,
                    32 => s.has_vps = true,
                    // IDR_W_RADL (19) and IDR_N_LP (20) are keyframe slices.
                    19 | 20 => s.idr_slices += 1,
                    // Trailing/other VCL slice types 0..=9, 16..=18, 21.
                    0..=9 | 16..=18 | 21 => s.non_idr_slices += 1,
                    _ => {}
                }
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_and_classifies_h264() {
        // SPS(7), PPS(8), IDR(5), non-IDR(1) with mixed start-code lengths.
        let stream = [
            0, 0, 0, 1, 0x67, 0xAA, // SPS
            0, 0, 1, 0x68, 0xBB, // PPS
            0, 0, 0, 1, 0x65, 0xCC, // IDR
            0, 0, 1, 0x61, 0xDD, // non-IDR
        ];
        let s = summarize(&stream, Codec::H264);
        assert_eq!(s.total_nals, 4);
        assert!(s.has_sps && s.has_pps);
        assert_eq!(s.idr_slices, 1);
        assert_eq!(s.non_idr_slices, 1);
        assert!(s.is_playable(Codec::H264));
    }

    #[test]
    fn not_playable_without_idr() {
        let stream = [0, 0, 0, 1, 0x67, 0xAA, 0, 0, 1, 0x68, 0xBB];
        let s = summarize(&stream, Codec::H264);
        assert!(!s.is_playable(Codec::H264));
    }
}
