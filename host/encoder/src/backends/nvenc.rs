//! NVENC backend (stub).
//!
//! Real bring-up needs the NVIDIA Video Codec SDK (`nvEncodeAPI`) via FFI plus
//! D3D11 interop to feed the captured surface without a CPU round-trip. Until
//! then this reports unavailable so the selection chain falls through to the
//! next backend.

use crate::config::EncoderConfig;
use crate::encoder::{EncoderError, VideoEncoder};

pub fn create(_config: &EncoderConfig) -> Result<Box<dyn VideoEncoder>, EncoderError> {
    Err(EncoderError::Unavailable(
        "NVENC backend not yet implemented (needs NVIDIA Video Codec SDK bindings)".to_string(),
    ))
}
