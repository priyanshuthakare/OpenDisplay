//! AMD AMF backend (stub).
//!
//! Real bring-up needs the AMD Advanced Media Framework SDK via FFI. Until then
//! this reports unavailable so the selection chain falls through to Media
//! Foundation.

use crate::config::EncoderConfig;
use crate::encoder::{EncoderError, VideoEncoder};

pub fn create(_config: &EncoderConfig) -> Result<Box<dyn VideoEncoder>, EncoderError> {
    Err(EncoderError::Unavailable(
        "AMF backend not yet implemented (needs AMD AMF SDK bindings)".to_string(),
    ))
}
