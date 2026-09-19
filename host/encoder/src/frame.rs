use std::fmt;

/// One captured frame in 32-bpp BGRA, top-down, tightly packed.
///
/// This mirrors the layout the driver's `FrameCapturer::Pixels()` produces:
/// each pixel is a little-endian `0xAARRGGBB` word whose bytes in memory are
/// `B, G, R, A`, row-major from the top-left. `pixels.len()` must be
/// `width * height`.
#[derive(Clone, PartialEq, Eq)]
pub struct BgraFrame {
    pub width: u32,
    pub height: u32,
    /// BGRA words, row-major, top-down. Length == width * height.
    pub pixels: Vec<u32>,
}

impl BgraFrame {
    /// Construct from BGRA words. Returns `None` if the buffer length does not
    /// match `width * height` (a mismatch means a corrupt or misparsed frame).
    pub fn from_words(width: u32, height: u32, pixels: Vec<u32>) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        if pixels.len() != (width as usize) * (height as usize) {
            return None;
        }
        Some(Self {
            width,
            height,
            pixels,
        })
    }

    /// Construct from a raw BGRA byte buffer (4 bytes per pixel, B,G,R,A order).
    /// Returns `None` on any size mismatch.
    pub fn from_bytes(width: u32, height: u32, bytes: &[u8]) -> Option<Self> {
        let expected = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        if bytes.len() != expected {
            return None;
        }
        let pixels = bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        Self::from_words(width, height, pixels)
    }
}

impl fmt::Debug for BgraFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Don't dump millions of pixels.
        f.debug_struct("BgraFrame")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("pixels", &format_args!("[{} words]", self.pixels.len()))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_length_mismatch() {
        assert!(BgraFrame::from_words(2, 2, vec![0; 3]).is_none());
        assert!(BgraFrame::from_words(0, 2, vec![]).is_none());
    }

    #[test]
    fn accepts_matching_length() {
        let f = BgraFrame::from_words(2, 2, vec![0xFF00FF00; 4]).unwrap();
        assert_eq!(f.width, 2);
        assert_eq!(f.pixels.len(), 4);
    }

    #[test]
    fn from_bytes_parses_bgra_order() {
        // One pixel: B=0x11 G=0x22 R=0x33 A=0x44 -> word 0x44332211.
        let f = BgraFrame::from_bytes(1, 1, &[0x11, 0x22, 0x33, 0x44]).unwrap();
        assert_eq!(f.pixels[0], 0x44332211);
    }
}
