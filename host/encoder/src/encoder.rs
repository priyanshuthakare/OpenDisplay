use thiserror::Error;

/// One encoded output unit: a single coded picture as an Annex-B byte stream
/// (start-code-delimited NAL units, including SPS/PPS on keyframes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedUnit {
    /// Annex-B bytes for this coded picture.
    pub bytes: Vec<u8>,
    /// True when this unit is an IDR / keyframe.
    pub keyframe: bool,
    /// Presentation timestamp in nanoseconds.
    pub timestamp_ns: u64,
}

#[derive(Debug, Error)]
pub enum EncoderError {
    #[error("no encoder backend is available for this configuration")]
    NoBackendAvailable,
    #[error("backend is not available: {0}")]
    Unavailable(String),
    #[error("frame dimensions {frame_w}x{frame_h} do not match encoder {enc_w}x{enc_h}")]
    DimensionMismatch {
        frame_w: u32,
        frame_h: u32,
        enc_w: u32,
        enc_h: u32,
    },
    #[error("encoder backend error: {0}")]
    Backend(String),
}

/// Object-safe encoder interface. Each backend (NVENC, QSV, AMF, Media
/// Foundation) implements this. Frames are fed in order; a single input frame
/// may yield zero or more output units depending on the backend's internal
/// pipelining, so callers must also call [`VideoEncoder::drain`] at end of
/// stream to flush any buffered pictures.
pub trait VideoEncoder {
    /// Human-readable backend name, for logging (e.g. "MediaFoundation").
    fn backend_name(&self) -> &str;

    /// Submit one BGRA frame. Returns any coded pictures that became available.
    fn encode(
        &mut self,
        frame: &crate::BgraFrame,
        timestamp_ns: u64,
    ) -> Result<Vec<EncodedUnit>, EncoderError>;

    /// Flush the pipeline at end of stream and return remaining coded pictures.
    fn drain(&mut self) -> Result<Vec<EncodedUnit>, EncoderError>;
}
