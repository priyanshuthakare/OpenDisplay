//! CPU BGRA -> NV12 color conversion.
//!
//! Hardware H.264/H.265 encoders take NV12 (planar Y followed by interleaved
//! U/V at half resolution), not BGRA. This module does that conversion on the
//! CPU with BT.601 studio-swing ("limited range") coefficients, which is what
//! Media Foundation's software/hardware H.264 encoders expect by default.
//!
//! It is deliberately dependency-free and deterministic so it can be unit tested
//! without any GPU or OS support.

use crate::BgraFrame;

/// NV12 buffer: a full-resolution Y plane followed by a half-resolution
/// interleaved UV plane. Strides equal the (even) width for both planes, which
/// is what MF's default buffer layout uses when width is even.
pub struct Nv12 {
    pub width: u32,
    pub height: u32,
    /// Y plane, `width * height` bytes, followed by UV plane,
    /// `width * (height / 2)` bytes (U,V interleaved). One contiguous buffer.
    pub data: Vec<u8>,
}

impl Nv12 {
    pub fn y_len(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }
    pub fn uv_len(&self) -> usize {
        (self.width as usize) * (self.height as usize / 2)
    }
}

#[inline]
fn clamp_u8(v: i32) -> u8 {
    if v < 0 {
        0
    } else if v > 255 {
        255
    } else {
        v as u8
    }
}

/// BT.601 limited-range luma from 8-bit R,G,B.
/// Y = 16 + 0.256788 R + 0.504129 G + 0.097906 B, in 2^15 fixed point.
#[inline]
fn rgb_to_y(r: i32, g: i32, b: i32) -> u8 {
    // 16 << 15 = 524288 bias, + 16384 for rounding.
    clamp_u8((524_288 + 8_415 * r + 16_519 * g + 3_208 * b + 16_384) >> 15)
}

/// BT.601 limited-range Cb (U).
/// U = 128 - 0.148223 R - 0.290993 G + 0.439216 B, in 2^15 fixed point.
#[inline]
fn rgb_to_u(r: i32, g: i32, b: i32) -> u8 {
    // 128 << 15 = 4194304 bias, + 16384 for rounding.
    clamp_u8((4_194_304 - 4_858 * r - 9_536 * g + 14_392 * b + 16_384) >> 15)
}

/// BT.601 limited-range Cr (V).
/// V = 128 + 0.439216 R - 0.367788 G - 0.071427 B, in 2^15 fixed point.
#[inline]
fn rgb_to_v(r: i32, g: i32, b: i32) -> u8 {
    clamp_u8((4_194_304 + 14_392 * r - 12_053 * g - 2_341 * b + 16_384) >> 15)
}

/// Convert a BGRA frame to NV12. The frame's `width` and `height` must both be
/// even (H.264 macroblock/chroma requirement); callers that capture odd sizes
/// should pad first. Returns `None` if dimensions are odd or zero.
pub fn bgra_to_nv12(frame: &BgraFrame) -> Option<Nv12> {
    let w = frame.width;
    let h = frame.height;
    if w == 0 || h == 0 || (w & 1) == 1 || (h & 1) == 1 {
        return None;
    }
    let wu = w as usize;
    let hu = h as usize;

    let mut data = vec![0u8; wu * hu + wu * (hu / 2)];
    let (y_plane, uv_plane) = data.split_at_mut(wu * hu);

    // Luma: one sample per pixel.
    for y in 0..hu {
        for x in 0..wu {
            let px = frame.pixels[y * wu + x];
            // Word is 0xAARRGGBB (bytes B,G,R,A in memory).
            let b = (px & 0xFF) as i32;
            let g = ((px >> 8) & 0xFF) as i32;
            let r = ((px >> 16) & 0xFF) as i32;
            y_plane[y * wu + x] = rgb_to_y(r, g, b);
        }
    }

    // Chroma: one U,V pair per 2x2 block, averaged over the block.
    for by in 0..(hu / 2) {
        for bx in 0..(wu / 2) {
            let x0 = bx * 2;
            let y0 = by * 2;
            let mut rs = 0i32;
            let mut gs = 0i32;
            let mut bs = 0i32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let px = frame.pixels[(y0 + dy) * wu + (x0 + dx)];
                    bs += (px & 0xFF) as i32;
                    gs += ((px >> 8) & 0xFF) as i32;
                    rs += ((px >> 16) & 0xFF) as i32;
                }
            }
            let r = rs / 4;
            let g = gs / 4;
            let b = bs / 4;
            let idx = (by * (wu / 2) + bx) * 2;
            uv_plane[idx] = rgb_to_u(r, g, b);
            uv_plane[idx + 1] = rgb_to_v(r, g, b);
        }
    }

    Some(Nv12 {
        width: w,
        height: h,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, bgra: u32) -> BgraFrame {
        BgraFrame::from_words(w, h, vec![bgra; (w * h) as usize]).unwrap()
    }

    #[test]
    fn rejects_odd_dimensions() {
        assert!(bgra_to_nv12(&solid(3, 2, 0)).is_none());
        assert!(bgra_to_nv12(&solid(2, 3, 0)).is_none());
    }

    #[test]
    fn plane_sizes_are_correct() {
        let nv12 = bgra_to_nv12(&solid(4, 4, 0)).unwrap();
        assert_eq!(nv12.y_len(), 16);
        assert_eq!(nv12.uv_len(), 8);
        assert_eq!(nv12.data.len(), 24);
    }

    #[test]
    fn solid_black_is_studio_swing_floor() {
        // BGRA black (0x00000000). Y=16, U=V=128 in limited range.
        let nv12 = bgra_to_nv12(&solid(2, 2, 0x0000_0000)).unwrap();
        assert_eq!(&nv12.data[0..4], &[16, 16, 16, 16]);
        assert_eq!(&nv12.data[4..6], &[128, 128]); // U,V
    }

    #[test]
    fn solid_white_is_studio_swing_ceiling() {
        // BGRA white (0x00FFFFFF -> R=G=B=255). Y=235, U=V=128.
        let nv12 = bgra_to_nv12(&solid(2, 2, 0x00FF_FFFF)).unwrap();
        assert_eq!(&nv12.data[0..4], &[235, 235, 235, 235]);
        assert_eq!(&nv12.data[4..6], &[128, 128]);
    }

    #[test]
    fn solid_red_has_high_v() {
        // Pure red R=255. Expect Y~81, U~90, V~240 (BT.601 limited).
        let nv12 = bgra_to_nv12(&solid(2, 2, 0x00FF_0000)).unwrap();
        assert_eq!(nv12.data[0], 81);
        assert_eq!(nv12.data[4], 90); // U (Cb)
        assert_eq!(nv12.data[5], 240); // V (Cr)
    }

    #[test]
    fn solid_blue_has_high_u() {
        // Pure blue B=255. Expect Y~41, U~240, V~110.
        let nv12 = bgra_to_nv12(&solid(2, 2, 0x0000_00FF)).unwrap();
        assert_eq!(nv12.data[0], 41);
        assert_eq!(nv12.data[4], 240); // U (Cb)
        assert_eq!(nv12.data[5], 110); // V (Cr)
    }
}
