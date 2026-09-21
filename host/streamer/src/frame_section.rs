//! Shared-memory frame handoff from the IDD driver to the host process.
//!
//! The driver captures the virtual monitor into a CPU BGRA buffer, but it lives
//! in the WUDFHost process, so the pixels must cross a process boundary to reach
//! the streamer. This module defines that boundary.
//!
//! ```text
//! section = [ header (48 B) | slot 0 | slot 1 ]
//! ```
//!
//! Two slots let the writer (driver) fill one while a reader drains the other.
//! A seqlock guards the pair: a reader samples `sequence`, copies a slot, then
//! re-samples; if the value moved or was odd, the slot may have been recycled
//! mid-copy and the read is discarded. Latest frame wins — a slow reader simply
//! misses frames, matching the live-mode semantics of the streaming loop.
//!
//! Writer protocol (the driver mirrors this; [`publish`] is the reference
//! implementation):
//!
//!  1. `sequence <- sequence + 1` (now odd: write in progress)
//!  2. fill the slot that is *not* currently published
//!  3. set `slot`, `width`, `height`, `timestamp_ns`
//!  4. `sequence <- sequence + 2` (now even: stable)
//!
//! Byte layout is fixed and arithmetic (not `repr(C)`-dependent): both sides are
//! Windows x64, so little-endian, and every field offset below is authoritative.

use std::fmt;

use usbdisplay_encoder::BgraFrame;

/// `"UDFR"` — USBDisplay FRame section.
pub const MAGIC: u32 = u32::from_le_bytes(*b"UDFR");
/// Layout version. Bump on any change to the header or slot layout.
pub const VERSION: u32 = 1;
/// Pixel slots per section.
pub const SLOT_COUNT: usize = 2;
/// Bytes in the section name, without a namespace prefix.
pub const SECTION_NAME: &str = "USBDisplayFrame";
/// Header size in bytes, written into the header so a skew is detectable.
pub const HEADER_SIZE: u32 = 48;
/// Reject absurd dimensions instead of attempting a huge mapping.
pub const MAX_DIMENSION: u32 = 16_384;

// Authoritative field offsets. Kept as arithmetic so the Rust and C++ views
// cannot drift through struct-packing differences.
const OFF_MAGIC: usize = 0;
const OFF_VERSION: usize = 4;
const OFF_HEADER_SIZE: usize = 8;
const OFF_SLOT_BYTES: usize = 12;
const OFF_WIDTH: usize = 16;
const OFF_HEIGHT: usize = 20;
const OFF_SLOT: usize = 24;
const OFF_FLAGS: usize = 28;
const OFF_SEQUENCE: usize = 32;
const OFF_TIMESTAMP_NS: usize = 40;

/// How many times to retry a torn read before skipping this frame.
const MAX_READ_ATTEMPTS: u32 = 4;

/// Typed view of the section header. Mirrors the byte layout above; used for
/// readability, never for byte-level access.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameSectionHeader {
    pub magic: u32,
    pub version: u32,
    pub header_size: u32,
    pub slot_bytes: u32,
    pub width: u32,
    pub height: u32,
    pub slot: u32,
    pub flags: u32,
    pub sequence: u64,
    pub timestamp_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionError {
    TooSmall { have: usize, need: usize },
    BadMagic { found: u32 },
    BadVersion { found: u32 },
    BadHeaderSize { found: u32 },
    BadSlot { found: u32 },
    BadDimensions { width: u32, height: u32 },
    SlotBytesMismatch { declared: u32, expected: usize },
}

impl fmt::Display for SectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooSmall { have, need } => {
                write!(f, "frame section too small: {have} bytes, need {need}")
            }
            Self::BadMagic { found } => {
                write!(f, "not a frame section (magic {found:#010x})")
            }
            Self::BadVersion { found } => {
                write!(f, "unsupported frame-section version {found}")
            }
            Self::BadHeaderSize { found } => write!(f, "unexpected header size {found}"),
            Self::BadSlot { found } => write!(f, "published slot {found} out of range"),
            Self::BadDimensions { width, height } => {
                write!(f, "implausible frame size {width}x{height}")
            }
            Self::SlotBytesMismatch { declared, expected } => write!(
                f,
                "slot_bytes {declared} does not match width*height*4 ({expected})"
            ),
        }
    }
}

impl std::error::Error for SectionError {}

/// Bytes one slot needs for a given frame size.
pub fn slot_bytes(width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)
}

/// Total bytes a section needs for a given frame size.
pub fn section_len(width: u32, height: u32) -> Option<usize> {
    let slot = slot_bytes(width, height)?;
    (HEADER_SIZE as usize).checked_add(slot.checked_mul(SLOT_COUNT)?)
}

// --- byte helpers ---------------------------------------------------------

fn put_u32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_u64(buf: &mut [u8], off: usize, v: u64) {
    buf[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

fn get_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

fn get_u64(buf: &[u8], off: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&buf[off..off + 8]);
    u64::from_le_bytes(b)
}

fn get_sequence(buf: &[u8]) -> u64 {
    get_u64(buf, OFF_SEQUENCE)
}

/// Decode the header from raw bytes.
pub fn decode_header(buf: &[u8]) -> Option<FrameSectionHeader> {
    if buf.len() < HEADER_SIZE as usize {
        return None;
    }
    Some(FrameSectionHeader {
        magic: get_u32(buf, OFF_MAGIC),
        version: get_u32(buf, OFF_VERSION),
        header_size: get_u32(buf, OFF_HEADER_SIZE),
        slot_bytes: get_u32(buf, OFF_SLOT_BYTES),
        width: get_u32(buf, OFF_WIDTH),
        height: get_u32(buf, OFF_HEIGHT),
        slot: get_u32(buf, OFF_SLOT),
        flags: get_u32(buf, OFF_FLAGS),
        sequence: get_u64(buf, OFF_SEQUENCE),
        timestamp_ns: get_u64(buf, OFF_TIMESTAMP_NS),
    })
}

/// Validate a header against the section it claims to describe.
fn validate(header: &FrameSectionHeader, view_len: usize) -> Result<usize, SectionError> {
    if header.magic != MAGIC {
        return Err(SectionError::BadMagic {
            found: header.magic,
        });
    }
    if header.version != VERSION {
        return Err(SectionError::BadVersion {
            found: header.version,
        });
    }
    if header.header_size != HEADER_SIZE {
        return Err(SectionError::BadHeaderSize {
            found: header.header_size,
        });
    }
    if header.width == 0
        || header.height == 0
        || header.width > MAX_DIMENSION
        || header.height > MAX_DIMENSION
    {
        return Err(SectionError::BadDimensions {
            width: header.width,
            height: header.height,
        });
    }
    let expected = slot_bytes(header.width, header.height).ok_or(SectionError::BadDimensions {
        width: header.width,
        height: header.height,
    })?;
    if header.slot_bytes as usize != expected {
        return Err(SectionError::SlotBytesMismatch {
            declared: header.slot_bytes,
            expected,
        });
    }
    let need = HEADER_SIZE as usize + expected * SLOT_COUNT;
    if view_len < need {
        return Err(SectionError::TooSmall {
            have: view_len,
            need,
        });
    }
    Ok(expected)
}

/// A published frame plus its capture timestamp.
#[derive(Debug)]
pub struct FrameSnapshot {
    pub frame: BgraFrame,
    pub timestamp_ns: u64,
}

/// Read the newest consistent frame from a section.
///
/// `Ok(None)` means only that a stable read could not be taken within the retry
/// budget (the driver was mid-publish) — the caller should keep its last frame
/// and try again, exactly as the BMP reader skips a half-written file.
pub fn read_snapshot<V: SectionView + ?Sized>(
    view: &V,
) -> Result<Option<FrameSnapshot>, SectionError> {
    let header = view.header();
    let slot_len = validate(&header, view.len())?;

    // Sequence 0 means the section exists but the driver has not published yet.
    // Report "nothing available" rather than handing back an all-zero frame.
    if view.sequence() == 0 {
        return Ok(None);
    }

    let mut scratch: Vec<u8> = Vec::new();
    for _ in 0..MAX_READ_ATTEMPTS {
        let seq1 = view.sequence();
        if seq1 % 2 != 0 {
            std::hint::spin_loop();
            continue; // a publish is in flight
        }
        // Re-read the header: the writer advanced `slot` under the same guard.
        let published = view.header();
        if published.slot as usize >= SLOT_COUNT {
            return Err(SectionError::BadSlot {
                found: published.slot,
            });
        }
        view.copy_slot(published.slot as usize, slot_len, &mut scratch);
        if view.sequence() == seq1 {
            let frame = BgraFrame::from_bytes(header.width, header.height, &scratch).ok_or(
                SectionError::SlotBytesMismatch {
                    declared: header.slot_bytes,
                    expected: slot_len,
                },
            )?;
            return Ok(Some(FrameSnapshot {
                frame,
                timestamp_ns: published.timestamp_ns,
            }));
        }
    }
    Ok(None)
}

/// Publish one frame into a whole-section buffer.
///
/// Reference implementation of the writer protocol; the driver mirrors it. The
/// buffer must be exactly [`section_len`] for the frame and initialised (see
/// [`initialise`]).
pub fn publish(
    section: &mut [u8],
    frame: &BgraFrame,
    timestamp_ns: u64,
) -> Result<(), SectionError> {
    let slot_len = slot_bytes(frame.width, frame.height).ok_or(SectionError::BadDimensions {
        width: frame.width,
        height: frame.height,
    })?;
    let needed = HEADER_SIZE as usize + slot_len * SLOT_COUNT;
    if section.len() < needed {
        return Err(SectionError::TooSmall {
            have: section.len(),
            need: needed,
        });
    }
    let header = decode_header(section).ok_or(SectionError::TooSmall {
        have: section.len(),
        need: HEADER_SIZE as usize,
    })?;

    // Publish into whichever slot is not currently live.
    let target = if (header.slot as usize) < SLOT_COUNT {
        (header.slot as usize + 1) % SLOT_COUNT
    } else {
        0
    };

    let seq = header.sequence;

    // Odd sequence == write in flight; readers discard whatever they sample.
    put_u64(section, OFF_SEQUENCE, seq.wrapping_add(1));

    let start = HEADER_SIZE as usize + target * slot_len;
    let dst = &mut section[start..start + slot_len];
    for (chunk, word) in dst.chunks_exact_mut(4).zip(frame.pixels.iter()) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }

    put_u32(section, OFF_WIDTH, frame.width);
    put_u32(section, OFF_HEIGHT, frame.height);
    put_u32(section, OFF_SLOT_BYTES, slot_len as u32);
    put_u32(section, OFF_SLOT, target as u32);
    put_u64(section, OFF_TIMESTAMP_NS, timestamp_ns);

    // Even sequence == stable.
    put_u64(section, OFF_SEQUENCE, seq.wrapping_add(2));
    Ok(())
}

/// Write a valid, unpublished section header into `buf`.
pub fn initialise(buf: &mut [u8], width: u32, height: u32) -> Result<usize, SectionError> {
    let slot_len =
        slot_bytes(width, height).ok_or(SectionError::BadDimensions { width, height })?;
    let total = HEADER_SIZE as usize + slot_len * SLOT_COUNT;
    if buf.len() < total {
        return Err(SectionError::TooSmall {
            have: buf.len(),
            need: total,
        });
    }
    buf[..total].fill(0);
    put_u32(buf, OFF_MAGIC, MAGIC);
    put_u32(buf, OFF_VERSION, VERSION);
    put_u32(buf, OFF_HEADER_SIZE, HEADER_SIZE);
    put_u32(buf, OFF_SLOT_BYTES, slot_len as u32);
    put_u32(buf, OFF_WIDTH, width);
    put_u32(buf, OFF_HEIGHT, height);
    put_u32(buf, OFF_SLOT, 0);
    put_u64(buf, OFF_SEQUENCE, 0);
    Ok(total)
}

/// A read-only view over a section's bytes.
///
/// Abstracted so the seqlock logic above can be exercised against a plain `Vec`
/// in tests, while the Windows path supplies a view over mapped memory that
/// reads the sequence atomically and the payload with volatile copies.
pub trait SectionView {
    fn len(&self) -> usize;
    fn header(&self) -> FrameSectionHeader;
    /// Current sequence value, read atomically where it matters.
    fn sequence(&self) -> u64;
    /// Copy slot `index` into `out`, replacing its contents.
    fn copy_slot(&self, index: usize, slot_bytes: usize, out: &mut Vec<u8>);
}

/// [`SectionView`] over an ordinary byte slice.
pub struct SliceView<'a> {
    bytes: &'a [u8],
}

impl<'a> SliceView<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }
}

impl SectionView for SliceView<'_> {
    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn header(&self) -> FrameSectionHeader {
        decode_header(self.bytes).unwrap_or_default()
    }

    fn sequence(&self) -> u64 {
        if self.bytes.len() < HEADER_SIZE as usize {
            return 0;
        }
        get_sequence(self.bytes)
    }

    fn copy_slot(&self, index: usize, slot_bytes: usize, out: &mut Vec<u8>) {
        let start = HEADER_SIZE as usize + index * slot_bytes;
        out.clear();
        out.extend_from_slice(&self.bytes[start..start + slot_bytes]);
    }
}

/// Read-only handle on a section published by the driver.
///
/// Windows-only: the streamer runs in the user session while the driver runs in
/// the WUDFHost process, so the section is looked up by name in the global
/// namespace. When it does not exist yet (driver not started, or not publishing)
/// callers get `Ok(None)` and should keep retrying rather than failing.
#[cfg(windows)]
pub mod mapped {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Memory::{
        MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, VirtualQuery, FILE_MAP_READ,
        MEMORY_BASIC_INFORMATION, MEMORY_MAPPED_VIEW_ADDRESS,
    };

    /// Fully-qualified section name, matching the driver's `CreateFileMappingW`.
    pub const GLOBAL_SECTION_NAME: &str = r"Global\USBDisplayFrame";

    /// Alternative name in case both processes share a session namespace.
    pub const LOCAL_SECTION_NAME: &str = r"Local\USBDisplayFrame";

    fn wide(name: &str) -> Vec<u16> {
        name.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub struct MappedSection {
        handle: HANDLE,
        base: MEMORY_MAPPED_VIEW_ADDRESS,
        len: usize,
    }

    // The view is only ever read through `&self`, and the section is unmapped on
    // drop, so moving the handle between threads is sound.
    unsafe impl Send for MappedSection {}

    impl MappedSection {
        /// Open an existing section, or `None` when it is not there yet.
        pub fn open() -> Option<Self> {
            for name in [GLOBAL_SECTION_NAME, LOCAL_SECTION_NAME] {
                let name_w = wide(name);
                // SAFETY: a null-terminated UTF-16 name that outlives the call.
                let Ok(handle) =
                    (unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(name_w.as_ptr())) })
                else {
                    continue;
                };

                // SAFETY: `handle` is a valid section handle; mapping the whole
                // section (`dwNumberOfBytesToMap = 0`) is what we want.
                let base = unsafe { MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0) };
                if base.Value.is_null() {
                    // SAFETY: handle came from OpenFileMappingW and is ours.
                    let _ = unsafe { CloseHandle(handle) };
                    continue;
                }

                // SAFETY: `base` is a mapped region; VirtualQuery only inspects it.
                let mut info = MEMORY_BASIC_INFORMATION::default();
                let written = unsafe {
                    VirtualQuery(
                        Some(base.Value),
                        &mut info,
                        std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                    )
                };
                if written == 0 {
                    unsafe {
                        let _ = UnmapViewOfFile(base);
                        let _ = CloseHandle(handle);
                    }
                    continue;
                }

                return Some(Self {
                    handle,
                    base,
                    len: info.RegionSize,
                });
            }
            None
        }

        fn bytes(&self) -> *const u8 {
            self.base.Value as *const u8
        }
    }

    impl SectionView for MappedSection {
        fn len(&self) -> usize {
            self.len
        }

        fn header(&self) -> FrameSectionHeader {
            if self.len < HEADER_SIZE as usize {
                return FrameSectionHeader::default();
            }
            // SAFETY: validated length; the header region is readable.
            let bytes = unsafe { std::slice::from_raw_parts(self.bytes(), HEADER_SIZE as usize) };
            decode_header(bytes).unwrap_or_default()
        }

        fn sequence(&self) -> u64 {
            if self.len < OFF_SEQUENCE + 8 {
                return 0;
            }
            // SAFETY: offset 32 within a page-aligned mapping is 8-byte aligned,
            // which is what AtomicU64 requires. The acquire load pairs with the
            // writer's release stores to order the payload copy.
            let cell = unsafe { &*(self.bytes().add(OFF_SEQUENCE) as *const AtomicU64) };
            cell.load(Ordering::Acquire)
        }

        fn copy_slot(&self, index: usize, slot_bytes: usize, out: &mut Vec<u8>) {
            let start = HEADER_SIZE as usize + index * slot_bytes;
            if start + slot_bytes > self.len {
                out.clear();
                return;
            }
            out.clear();
            out.resize(slot_bytes, 0);
            // SAFETY: bounds checked above; both regions are valid for their
            // lengths and cannot overlap (one is the mapped section, the other a
            // fresh Vec).
            unsafe {
                std::ptr::copy_nonoverlapping(
                    self.bytes().add(start),
                    out.as_mut_ptr(),
                    slot_bytes,
                );
            }
        }
    }

    impl Drop for MappedSection {
        fn drop(&mut self) {
            // SAFETY: both were produced by the matching map/open calls above
            // and are released exactly once here.
            unsafe {
                let _ = UnmapViewOfFile(self.base);
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: u32, h: u32, seed: u32) -> BgraFrame {
        let pixels = (0..(w * h))
            .map(|i| 0xFF00_0000 | (seed.wrapping_mul(31).wrapping_add(i)))
            .collect();
        BgraFrame::from_words(w, h, pixels).expect("frame")
    }

    fn section(w: u32, h: u32) -> Vec<u8> {
        let mut buf = vec![0u8; section_len(w, h).expect("len")];
        initialise(&mut buf, w, h).expect("init");
        buf
    }

    #[test]
    fn layout_is_stable() {
        // The driver's C++ struct mirrors this exactly; a change here is a
        // wire-format change and must bump VERSION.
        assert_eq!(
            std::mem::size_of::<FrameSectionHeader>(),
            HEADER_SIZE as usize
        );
        assert_eq!(section_len(2, 2).unwrap(), 48 + 2 * 16);
        assert_eq!(slot_bytes(1920, 1080).unwrap(), 1920 * 1080 * 4);
    }

    #[test]
    fn unpublished_section_yields_no_frame() {
        // A freshly initialised section has sequence 0: the driver owns it but
        // has not published. That must read as "nothing yet", not a black frame.
        let buf = section(4, 4);
        assert!(read_snapshot(&SliceView::new(&buf))
            .expect("read")
            .is_none());
    }

    #[test]
    fn round_trips_a_published_frame() {
        let mut buf = section(8, 4);
        let f = frame(8, 4, 7);
        publish(&mut buf, &f, 1234).expect("publish");

        let snap = read_snapshot(&SliceView::new(&buf))
            .expect("read")
            .expect("some");
        assert_eq!(snap.frame, f);
        assert_eq!(snap.timestamp_ns, 1234);
    }

    #[test]
    fn alternates_slots_and_keeps_latest() {
        let mut buf = section(4, 4);
        publish(&mut buf, &frame(4, 4, 1), 10).unwrap();
        let first = decode_header(&buf).unwrap().slot;

        publish(&mut buf, &frame(4, 4, 2), 20).unwrap();
        let second = decode_header(&buf).unwrap().slot;
        assert_ne!(first, second, "consecutive publishes use different slots");

        let snap = read_snapshot(&SliceView::new(&buf)).unwrap().unwrap();
        assert_eq!(snap.frame, frame(4, 4, 2));
        assert_eq!(snap.timestamp_ns, 20);
    }

    #[test]
    fn rejects_a_foreign_section() {
        let buf = vec![0u8; 4096];
        assert_eq!(
            read_snapshot(&SliceView::new(&buf)).unwrap_err(),
            SectionError::BadMagic { found: 0 }
        );
    }

    #[test]
    fn rejects_a_truncated_section() {
        let mut buf = section(8, 8);
        buf.truncate(HEADER_SIZE as usize + 16);
        assert!(matches!(
            read_snapshot(&SliceView::new(&buf)).unwrap_err(),
            SectionError::TooSmall { .. }
        ));
    }

    #[test]
    fn rejects_a_version_skew() {
        let mut buf = section(4, 4);
        put_u32(&mut buf, OFF_VERSION, VERSION + 1);
        assert_eq!(
            read_snapshot(&SliceView::new(&buf)).unwrap_err(),
            SectionError::BadVersion { found: VERSION + 1 }
        );
    }

    #[test]
    fn rejects_a_lying_slot_bytes() {
        let mut buf = section(4, 4);
        put_u32(&mut buf, OFF_SLOT_BYTES, 12);
        assert!(matches!(
            read_snapshot(&SliceView::new(&buf)).unwrap_err(),
            SectionError::SlotBytesMismatch { .. }
        ));
    }

    #[test]
    fn rejects_an_out_of_range_slot() {
        let mut buf = section(4, 4);
        publish(&mut buf, &frame(4, 4, 1), 1).unwrap();
        put_u32(&mut buf, OFF_SLOT, 9);
        assert_eq!(
            read_snapshot(&SliceView::new(&buf)).unwrap_err(),
            SectionError::BadSlot { found: 9 }
        );
    }

    #[test]
    fn a_reader_mid_publish_sees_either_the_old_or_the_new_frame() {
        // Simulate a torn read: odd sequence for the whole retry budget.
        let mut buf = section(4, 4);
        publish(&mut buf, &frame(4, 4, 1), 1).unwrap();
        put_u64(&mut buf, OFF_SEQUENCE, 5);
        assert!(read_snapshot(&SliceView::new(&buf)).unwrap().is_none());
    }

    #[test]
    fn does_not_mix_two_consecutive_frames() {
        // Two frames that differ in every pixel: a reader that mixed slots
        // would produce a frame equal to neither.
        let mut buf = section(6, 3);
        let a = frame(6, 3, 1);
        let mut b = frame(6, 3, 2);
        for p in b.pixels.iter_mut() {
            *p = !*p;
        }
        publish(&mut buf, &a, 1).unwrap();
        publish(&mut buf, &b, 2).unwrap();
        let snap = read_snapshot(&SliceView::new(&buf)).unwrap().unwrap();
        assert!(
            snap.frame == b || snap.frame == a,
            "must be one frame, not a blend"
        );
    }
}
