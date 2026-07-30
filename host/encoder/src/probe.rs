//! Backend selection: try the vendor encoders in preference order, falling back
//! to Media Foundation.
//!
//! The order is NVENC -> Quick Sync -> AMF -> Media Foundation. Only Media
//! Foundation is implemented today; the vendor backends report themselves
//! unavailable, but the full chain runs so the selection logic and logging are
//! exercised now and adding a real vendor backend is a one-line change here.

use crate::backends;
use crate::config::EncoderConfig;
use crate::encoder::{EncoderError, VideoEncoder};

// EncoderError is used by try_backend's return type below.

/// The candidate backends, in preference order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Nvenc,
    QuickSync,
    Amf,
    MediaFoundation,
}

impl Backend {
    pub const PREFERENCE_ORDER: [Backend; 4] = [
        Backend::Nvenc,
        Backend::QuickSync,
        Backend::Amf,
        Backend::MediaFoundation,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Backend::Nvenc => "NVENC",
            Backend::QuickSync => "QuickSync",
            Backend::Amf => "AMF",
            Backend::MediaFoundation => "MediaFoundation",
        }
    }
}

/// Result of probing one backend, for reporting which was chosen and why the
/// others were skipped.
#[derive(Debug, Clone)]
pub struct BackendStatus {
    pub backend: Backend,
    pub available: bool,
    pub detail: String,
}

/// Try to build an encoder, walking the preference order. Returns the selected
/// encoder (if any) together with the status of every backend tried, so the
/// caller can log the full chain whether or not selection succeeded.
pub fn select_encoder(
    config: &EncoderConfig,
) -> (Option<Box<dyn VideoEncoder>>, Vec<BackendStatus>) {
    let mut statuses = Vec::new();

    let mut chosen = None;
    for backend in Backend::PREFERENCE_ORDER {
        if chosen.is_some() {
            statuses.push(BackendStatus {
                backend,
                available: false,
                detail: "not tried (earlier backend selected)".to_string(),
            });
            continue;
        }
        match try_backend(backend, config) {
            Ok(encoder) => {
                statuses.push(BackendStatus {
                    backend,
                    available: true,
                    detail: "selected".to_string(),
                });
                chosen = Some(encoder);
            }
            Err(err) => {
                statuses.push(BackendStatus {
                    backend,
                    available: false,
                    detail: err.to_string(),
                });
            }
        }
    }

    (chosen, statuses)
}

fn try_backend(
    backend: Backend,
    config: &EncoderConfig,
) -> Result<Box<dyn VideoEncoder>, EncoderError> {
    match backend {
        Backend::Nvenc => backends::nvenc::create(config),
        Backend::QuickSync => backends::qsv::create(config),
        Backend::Amf => backends::amf::create(config),
        Backend::MediaFoundation => backends::mediafoundation::create(config),
    }
}
