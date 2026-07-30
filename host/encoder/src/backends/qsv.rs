//! Intel Quick Sync (QSV) backend (stub).
//!
//! Real bring-up needs the Intel oneVPL / Media SDK. Until then this reports
//! unavailable so the selection chain falls through to the next backend. On this
//! development machine the Intel Iris Xe would drive this path.

use crate::config::EncoderConfig;
use crate::encoder::{EncoderError, VideoEncoder};

pub fn create(_config: &EncoderConfig) -> Result<Box<dyn VideoEncoder>, EncoderError> {
    Err(EncoderError::Unavailable(
        "QuickSync backend not yet implemented (needs Intel oneVPL bindings)".to_string(),
    ))
}
