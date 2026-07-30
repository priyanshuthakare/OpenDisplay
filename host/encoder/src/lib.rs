//! Hardware video encoding for the USBDisplay host.
//!
//! The captured USBDisplay monitor frames (BGRA, produced by the driver's
//! `FrameCapturer`) are encoded to a compressed video stream here. Encoding is
//! done on the host -- not in the sandboxed UMDF driver -- so heavy vendor SDKs
//! stay out of the restricted driver host process.
//!
//! # Backend selection
//!
//! The intended preference order is NVENC, then Intel Quick Sync (QSV), then
//! AMD AMF, then Media Foundation as the universal fallback. See [`probe`].
//! Media Foundation is implemented first; the other three are stubs that report
//! themselves unavailable until their SDK bring-up lands, so the selection chain
//! is already exercised end to end.
//!
//! # Layers
//!
//! * [`BgraFrame`] -- one captured frame, the input to the encoder.
//! * [`EncoderConfig`] -- resolution / fps / bitrate / GOP / codec.
//! * [`VideoEncoder`] -- the object-safe trait every backend implements.
//! * [`color`] -- CPU BGRA -> NV12 conversion (pure Rust, unit tested).
//! * [`probe::select_encoder`] -- picks the best available backend.

mod color;
mod config;
mod encoder;
mod frame;
pub mod bmp;
pub mod nal;
pub mod probe;
pub mod validate;

pub mod backends;

pub use bmp::read_bgra_bmp;
pub use color::bgra_to_nv12;
pub use config::{Codec, EncoderConfig};
pub use encoder::{EncodedUnit, EncoderError, VideoEncoder};
pub use frame::BgraFrame;
pub use nal::{summarize, NalSummary};
pub use probe::{select_encoder, Backend, BackendStatus};
