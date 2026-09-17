//! Minimal BMP reader for the driver's captured frames.
//!
//! The driver's `FrameCapturer::DumpBmp` writes a very specific format:
//! `BITMAPFILEHEADER` + `BITMAPINFOHEADER`, 32 bpp, `BI_RGB`, negative height
//! (top-down), no palette. This reader targets exactly that layout rather than
//! being a general BMP decoder -- anything else is rejected so a malformed or
//! unexpected file is a hard error, not a silent misread.

use crate::BgraFrame;

#[derive(Debug)]
pub enum BmpError {
    TooShort,
    NotBmp,
    Unsupported(String),
    SizeMismatch,
}

impl std::fmt::Display for BmpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BmpError::TooShort => write!(f, "file shorter than BMP headers"),
            BmpError::NotBmp => write!(f, "missing 'BM' signature"),
            BmpError::Unsupported(s) => write!(f, "unsupported BMP: {s}"),
            BmpError::SizeMismatch => write!(f, "pixel data size does not match dimensions"),
        }
    }
}

impl std::error::Error for BmpError {}

const FILE_HEADER_LEN: usize = 14;
const INFO_HEADER_LEN: usize = 40;

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Parse a 32-bpp top-down BI_RGB BMP (the format our driver dumps) into a
/// [`BgraFrame`]. The pixel bytes are already B,G,R,A, matching BgraFrame.
pub fn read_bgra_bmp(bytes: &[u8]) -> Result<BgraFrame, BmpError> {
    if bytes.len() < FILE_HEADER_LEN + INFO_HEADER_LEN {
        return Err(BmpError::TooShort);
    }
    if &bytes[0..2] != b"BM" {
        return Err(BmpError::NotBmp);
    }

    let pixel_offset = u32_at(bytes, 10) as usize;
    let info_size = u32_at(bytes, 14) as usize;
    if info_size < INFO_HEADER_LEN {
        return Err(BmpError::Unsupported(format!(
            "info header size {info_size}"
        )));
    }

    let width = i32_at(bytes, 18);
    let raw_height = i32_at(bytes, 22);
    let bpp = u16_at(bytes, 28);
    let compression = u32_at(bytes, 30);

    if bpp != 32 {
        return Err(BmpError::Unsupported(format!("{bpp} bpp (expected 32)")));
    }
    if compression != 0 {
        return Err(BmpError::Unsupported(format!(
            "compression {compression} (expected BI_RGB)"
        )));
    }
    if width <= 0 {
        return Err(BmpError::Unsupported(format!("width {width}")));
    }

    // Negative height = top-down (our driver's layout); positive = bottom-up.
    let top_down = raw_height < 0;
    let height = raw_height.unsigned_abs();
    let w = width as usize;
    let h = height as usize;

    let needed = pixel_offset
        .checked_add(w * h * 4)
        .ok_or(BmpError::SizeMismatch)?;
    if bytes.len() < needed {
        return Err(BmpError::SizeMismatch);
    }

    let mut pixels = vec![0u32; w * h];
    for row in 0..h {
        let src_row = if top_down { row } else { h - 1 - row };
        let src = pixel_offset + src_row * w * 4;
        for x in 0..w {
            let o = src + x * 4;
            pixels[row * w + x] =
                u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
        }
    }

    BgraFrame::from_words(width as u32, height as u32, pixels).ok_or(BmpError::SizeMismatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Build a 1x1 top-down 32bpp BMP with a known pixel and round-trip it.
    #[test]
    fn reads_top_down_single_pixel() {
        let mut b = vec![0u8; 14 + 40 + 4];
        b[0] = b'B';
        b[1] = b'M';
        b[10..14].copy_from_slice(&54u32.to_le_bytes()); // pixel offset
        b[14..18].copy_from_slice(&40u32.to_le_bytes()); // info size
        b[18..22].copy_from_slice(&1i32.to_le_bytes()); // width
        b[22..26].copy_from_slice(&(-1i32).to_le_bytes()); // height (top-down)
        b[28..30].copy_from_slice(&32u16.to_le_bytes()); // bpp
        b[30..34].copy_from_slice(&0u32.to_le_bytes()); // BI_RGB
        b[54..58].copy_from_slice(&[0x11, 0x22, 0x33, 0x44]); // B,G,R,A
        let frame = read_bgra_bmp(&b).unwrap();
        assert_eq!(frame.width, 1);
        assert_eq!(frame.height, 1);
        assert_eq!(frame.pixels[0], 0x44332211);
    }

    #[test]
    fn rejects_non_bmp() {
        let b = vec![0u8; 100];
        assert!(matches!(read_bgra_bmp(&b), Err(BmpError::NotBmp)));
    }

    #[test]
    fn rejects_wrong_bpp() {
        let mut b = vec![0u8; 14 + 40 + 4];
        b[0] = b'B';
        b[1] = b'M';
        b[10..14].copy_from_slice(&54u32.to_le_bytes());
        b[14..18].copy_from_slice(&40u32.to_le_bytes());
        b[18..22].copy_from_slice(&1i32.to_le_bytes());
        b[22..26].copy_from_slice(&(-1i32).to_le_bytes());
        b[28..30].copy_from_slice(&24u16.to_le_bytes()); // 24bpp -> unsupported
        let err = read_bgra_bmp(&b).unwrap_err();
        assert!(matches!(err, BmpError::Unsupported(_)));
    }
}
