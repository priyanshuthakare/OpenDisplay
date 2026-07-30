//! Encoder backend implementations.
//!
//! Each submodule exposes `create(&EncoderConfig) -> Result<Box<dyn
//! VideoEncoder>, EncoderError>`. Media Foundation is implemented; NVENC, Quick
//! Sync, and AMF are stubs that report `Unavailable` until their SDK bring-up
//! lands. Keeping them as real modules (rather than omitting them) means the
//! selection chain in `probe` is complete and each one becomes a drop-in.

pub mod amf;
pub mod mediafoundation;
pub mod nvenc;
pub mod qsv;
