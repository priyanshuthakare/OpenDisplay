use usbdisplay_protocol::Codec as ProtocolCodec;

/// Output codec for the encoder. Kept as its own type (rather than reusing the
/// protocol enum directly) so the encoder crate can validate and default
/// independently; [`Codec::to_protocol`] bridges to the wire type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    H264,
    H265,
}

impl Codec {
    pub fn to_protocol(self) -> ProtocolCodec {
        match self {
            Codec::H264 => ProtocolCodec::H264,
            Codec::H265 => ProtocolCodec::H265,
        }
    }
}

/// Encoder configuration. Dimensions come from the captured frames; the rest are
/// stream tuning knobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderConfig {
    pub width: u32,
    pub height: u32,
    /// Frames per second, used for MF frame-rate and rate control.
    pub fps: u32,
    /// Target average bitrate in bits per second.
    pub bitrate_bps: u32,
    /// Keyframe interval in frames (GOP length).
    pub gop: u32,
    pub codec: Codec,
}

impl EncoderConfig {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            fps: 60,
            bitrate_bps: 20_000_000,
            gop: 60,
            codec: Codec::H264,
        }
    }

    pub fn with_fps(mut self, fps: u32) -> Self {
        self.fps = fps.max(1);
        self
    }

    pub fn with_bitrate(mut self, bitrate_bps: u32) -> Self {
        self.bitrate_bps = bitrate_bps.max(1);
        self
    }

    pub fn with_gop(mut self, gop: u32) -> Self {
        self.gop = gop.max(1);
        self
    }

    pub fn with_codec(mut self, codec: Codec) -> Self {
        self.codec = codec;
        self
    }
}
