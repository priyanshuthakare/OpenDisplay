//! Decode round-trip validation of an encoded elementary stream.
//!
//! Structural NAL inspection ([`crate::nal`]) proves the byte layout is
//! well-formed, but not that the stream actually decodes. This module feeds the
//! encoded Annex-B bytes straight into the Media Foundation H.264 **decoder
//! MFT** and counts the frames it emits -- a true round-trip of the exact bytes
//! we produced, with no container in between.
//!
//! On non-Windows targets [`decode_h264_stream`] reports unavailable.

/// Result of a decode round-trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeReport {
    pub decoded_frames: u64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub enum DecodeError {
    Unavailable(String),
    Backend(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Unavailable(s) => write!(f, "decode unavailable: {s}"),
            DecodeError::Backend(s) => write!(f, "decode backend error: {s}"),
        }
    }
}

impl std::error::Error for DecodeError {}

#[cfg(not(windows))]
pub fn decode_h264_stream(
    _stream: &[u8],
    _width: u32,
    _height: u32,
) -> Result<DecodeReport, DecodeError> {
    Err(DecodeError::Unavailable(
        "Media Foundation decode is only available on Windows".to_string(),
    ))
}

#[cfg(windows)]
pub use windows_impl::decode_h264_stream;

#[cfg(windows)]
mod windows_impl {
    use super::*;

    use windows::core::Interface;
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    const HNS_PER_SEC: i64 = 10_000_000;

    /// Decode an H.264 Annex-B elementary stream by driving the decoder MFT
    /// directly. `width`/`height` seed the input media type. Returns the number
    /// of decoded frames and the decoder's reported dimensions.
    pub fn decode_h264_stream(
        stream: &[u8],
        width: u32,
        height: u32,
    ) -> Result<DecodeReport, DecodeError> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL)
                .map_err(|e| DecodeError::Backend(format!("MFStartup: {e}")))?;
            let result = decode_inner(stream, width, height);
            let _ = MFShutdown();
            result
        }
    }

    unsafe fn decode_inner(
        stream: &[u8],
        width: u32,
        height: u32,
    ) -> Result<DecodeReport, DecodeError> {
        let transform = enumerate_decoder()?;

        // Input: H.264 with the source dimensions/frame rate.
        let in_type: IMFMediaType =
            MFCreateMediaType().map_err(|e| DecodeError::Backend(format!("in type: {e}")))?;
        in_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .and_then(|_| in_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264))
            .map_err(|e| DecodeError::Backend(format!("in type attrs: {e}")))?;
        set_ratio(&in_type, &MF_MT_FRAME_SIZE, width, height)?;
        set_ratio(&in_type, &MF_MT_FRAME_RATE, 60, 1)?;
        transform
            .SetInputType(0, &in_type, 0)
            .map_err(|e| DecodeError::Backend(format!("SetInputType: {e}")))?;

        // Output: NV12. Enumerate available output types and pick NV12.
        let out_type: IMFMediaType =
            MFCreateMediaType().map_err(|e| DecodeError::Backend(format!("out type: {e}")))?;
        out_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .and_then(|_| out_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12))
            .map_err(|e| DecodeError::Backend(format!("out type attrs: {e}")))?;
        set_ratio(&out_type, &MF_MT_FRAME_SIZE, width, height)?;
        transform
            .SetOutputType(0, &out_type, 0)
            .map_err(|e| DecodeError::Backend(format!("SetOutputType: {e}")))?;

        transform
            .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
            .map_err(|e| DecodeError::Backend(format!("BEGIN_STREAMING: {e}")))?;

        // Feed the whole elementary stream as one input sample; the decoder
        // internally frames on NAL boundaries. Then drain all outputs.
        let sample = make_sample(stream)?;
        let mut decoded = 0u64;

        match transform.ProcessInput(0, &sample, 0) {
            Ok(()) => {}
            Err(e) if e.code() == MF_E_NOTACCEPTING => {
                decoded += pump_output(&transform)?;
                transform
                    .ProcessInput(0, &sample, 0)
                    .map_err(|e| DecodeError::Backend(format!("ProcessInput retry: {e}")))?;
            }
            Err(e) => return Err(DecodeError::Backend(format!("ProcessInput: {e}"))),
        }
        decoded += pump_output(&transform)?;

        transform
            .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
            .map_err(|e| DecodeError::Backend(format!("END_OF_STREAM: {e}")))?;
        transform
            .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
            .map_err(|e| DecodeError::Backend(format!("DRAIN: {e}")))?;
        decoded += pump_output(&transform)?;

        // Report the decoder's negotiated dimensions.
        let (mut w, mut h) = (width, height);
        if let Ok(cur) = transform.GetOutputCurrentType(0) {
            if let Ok(packed) = cur.GetUINT64(&MF_MT_FRAME_SIZE) {
                w = (packed >> 32) as u32;
                h = (packed & 0xFFFF_FFFF) as u32;
            }
        }

        Ok(DecodeReport {
            decoded_frames: decoded,
            width: w,
            height: h,
        })
    }

    /// Pull all currently-available decoded frames (synchronous decoder MFT).
    unsafe fn pump_output(transform: &IMFTransform) -> Result<u64, DecodeError> {
        let mut count = 0u64;
        loop {
            let info = transform
                .GetOutputStreamInfo(0)
                .map_err(|e| DecodeError::Backend(format!("GetOutputStreamInfo: {e}")))?;
            let provides = (info.dwFlags
                & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32
                    | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32))
                != 0;

            let mut out = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: std::mem::ManuallyDrop::new(if provides {
                    None
                } else {
                    Some(alloc_sample(info.cbSize.max(1))?)
                }),
                dwStatus: 0,
                pEvents: std::mem::ManuallyDrop::new(None),
            };
            let mut status = 0u32;
            let hr = transform.ProcessOutput(0, std::slice::from_mut(&mut out), &mut status);
            match hr {
                Ok(()) => {
                    let _ = std::mem::ManuallyDrop::take(&mut out.pSample);
                    count += 1;
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                    let _ = std::mem::ManuallyDrop::take(&mut out.pSample);
                    break;
                }
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    // Decoder discovered the real format from the SPS. Adopt the
                    // first NV12 output type it now offers (querying it carries
                    // the correct dimensions/stride the decoder expects).
                    let _ = std::mem::ManuallyDrop::take(&mut out.pSample);
                    let mut idx = 0u32;
                    loop {
                        match transform.GetOutputAvailableType(0, idx) {
                            Ok(t) => {
                                let sub = t.GetGUID(&MF_MT_SUBTYPE).unwrap_or_default();
                                if sub == MFVideoFormat_NV12 {
                                    transform.SetOutputType(0, &t, 0).map_err(|e| {
                                        DecodeError::Backend(format!("stream change set: {e}"))
                                    })?;
                                    break;
                                }
                                idx += 1;
                            }
                            Err(_) => {
                                // No NV12 offered; fall back to a bare NV12 type.
                                let nv12: IMFMediaType = MFCreateMediaType()
                                    .map_err(|e| DecodeError::Backend(format!("sc type: {e}")))?;
                                nv12.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                                    .and_then(|_| nv12.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12))
                                    .map_err(|e| DecodeError::Backend(format!("sc attrs: {e}")))?;
                                transform.SetOutputType(0, &nv12, 0).map_err(|e| {
                                    DecodeError::Backend(format!("sc set fallback: {e}"))
                                })?;
                                break;
                            }
                        }
                    }
                    continue;
                }
                Err(e) => {
                    let _ = std::mem::ManuallyDrop::take(&mut out.pSample);
                    return Err(DecodeError::Backend(format!("ProcessOutput: {e}")));
                }
            }
        }
        Ok(count)
    }

    unsafe fn make_sample(bytes: &[u8]) -> Result<IMFSample, DecodeError> {
        let buffer: IMFMediaBuffer = MFCreateMemoryBuffer(bytes.len() as u32)
            .map_err(|e| DecodeError::Backend(format!("MFCreateMemoryBuffer: {e}")))?;
        let mut data: *mut u8 = std::ptr::null_mut();
        buffer
            .Lock(&mut data, None, None)
            .map_err(|e| DecodeError::Backend(format!("Lock: {e}")))?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
        let _ = buffer.Unlock();
        buffer
            .SetCurrentLength(bytes.len() as u32)
            .map_err(|e| DecodeError::Backend(format!("SetCurrentLength: {e}")))?;
        let sample: IMFSample =
            MFCreateSample().map_err(|e| DecodeError::Backend(format!("MFCreateSample: {e}")))?;
        sample
            .AddBuffer(&buffer)
            .map_err(|e| DecodeError::Backend(format!("AddBuffer: {e}")))?;
        sample.SetSampleTime(0).ok();
        sample.SetSampleDuration(HNS_PER_SEC / 60).ok();
        Ok(sample)
    }

    unsafe fn alloc_sample(size: u32) -> Result<IMFSample, DecodeError> {
        let buffer: IMFMediaBuffer = MFCreateMemoryBuffer(size)
            .map_err(|e| DecodeError::Backend(format!("out MFCreateMemoryBuffer: {e}")))?;
        let sample: IMFSample = MFCreateSample()
            .map_err(|e| DecodeError::Backend(format!("out MFCreateSample: {e}")))?;
        sample
            .AddBuffer(&buffer)
            .map_err(|e| DecodeError::Backend(format!("out AddBuffer: {e}")))?;
        Ok(sample)
    }

    /// Find a software H.264 decoder MFT (sync model keeps validation simple).
    unsafe fn enumerate_decoder() -> Result<IMFTransform, DecodeError> {
        let input_info = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };
        let mut ptr: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input_info),
            None,
            &mut ptr,
            &mut count,
        )
        .map_err(|e| DecodeError::Backend(format!("MFTEnumEx(decoder): {e}")))?;

        if count == 0 || ptr.is_null() {
            return Err(DecodeError::Unavailable(
                "no H.264 decoder MFT found".to_string(),
            ));
        }
        let activates = std::slice::from_raw_parts(ptr, count as usize);
        let mut chosen = None;
        for a in activates.iter().flatten() {
            if let Ok(t) = a.ActivateObject::<IMFTransform>() {
                chosen = Some(t);
                break;
            }
        }
        windows::Win32::System::Com::CoTaskMemFree(Some(ptr as *const _));
        chosen.ok_or_else(|| DecodeError::Backend("decoder MFT failed to activate".to_string()))
    }

    unsafe fn set_ratio(
        t: &IMFMediaType,
        key: &windows::core::GUID,
        n: u32,
        d: u32,
    ) -> Result<(), DecodeError> {
        let packed = ((n as u64) << 32) | (d as u64);
        t.SetUINT64(key, packed)
            .map_err(|e| DecodeError::Backend(format!("SetUINT64: {e}")))
    }

    // Keep Interface in scope for potential casts (decoder MFT is sync here).
    #[allow(unused_imports)]
    use Interface as _KeepInterface;
}
