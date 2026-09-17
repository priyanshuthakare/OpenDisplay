//! Media Foundation H.264 backend.
//!
//! This is the universal fallback and the first real backend. It uses a
//! hardware `IMFTransform` (enumerated with `MFT_ENUM_FLAG_HARDWARE`) that
//! accepts NV12 and produces an H.264 elementary stream. On non-Windows targets
//! `create` reports unavailable so the crate still builds and its pure-Rust
//! parts (color conversion, framing, selection) can be tested anywhere.

use crate::config::EncoderConfig;
use crate::encoder::{EncoderError, VideoEncoder};

#[cfg(not(windows))]
pub fn create(_config: &EncoderConfig) -> Result<Box<dyn VideoEncoder>, EncoderError> {
    Err(EncoderError::Unavailable(
        "Media Foundation is only available on Windows".to_string(),
    ))
}

#[cfg(windows)]
pub use windows_impl::create;

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use crate::color::bgra_to_nv12;
    use crate::encoder::EncodedUnit;
    use crate::BgraFrame;

    use windows::core::{Interface, GUID};
    use windows::Win32::Foundation::E_FAIL;
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    // MF represents 100-nanosecond units for time. Our timestamps are in ns.
    const HNS_PER_SEC: i64 = 10_000_000;

    // Async MFT event types (MediaEventType values). Defined as literals to stay
    // independent of how the windows crate happens to name/scope these.
    #[allow(non_upper_case_globals)]
    const METransformNeedInput: u32 = 601;
    #[allow(non_upper_case_globals)]
    const METransformHaveOutput: u32 = 602;
    #[allow(non_upper_case_globals)]
    const METransformDrainComplete: u32 = 603;

    /// Guards MFStartup/MFShutdown for the process. MF is reference counted, but
    /// we only ever start it once per encoder and shut down on drop.
    struct MfRuntime;

    impl MfRuntime {
        fn new() -> Result<Self, EncoderError> {
            unsafe {
                // COINIT_MULTITHREADED is what MF's async model expects. Ignore
                // RPC_E_CHANGED_MODE if COM was already initialized differently.
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                MFStartup(MF_VERSION, MFSTARTUP_FULL)
                    .map_err(|e| EncoderError::Backend(format!("MFStartup failed: {e}")))?;
            }
            Ok(MfRuntime)
        }
    }

    impl Drop for MfRuntime {
        fn drop(&mut self) {
            unsafe {
                let _ = MFShutdown();
            }
        }
    }

    pub struct MediaFoundationEncoder {
        _runtime: MfRuntime,
        transform: IMFTransform,
        event_gen: IMFMediaEventGenerator,
        config: EncoderConfig,
        input_stream_id: u32,
        output_stream_id: u32,
        /// True once we've fed the first frame and pulled stream config.
        started: bool,
        frame_index: u64,
    }

    pub fn create(config: &EncoderConfig) -> Result<Box<dyn VideoEncoder>, EncoderError> {
        let enc = MediaFoundationEncoder::new(*config)?;
        Ok(Box::new(enc))
    }

    impl MediaFoundationEncoder {
        fn new(config: EncoderConfig) -> Result<Self, EncoderError> {
            if (config.width & 1) == 1 || (config.height & 1) == 1 {
                return Err(EncoderError::Backend(
                    "width and height must be even for H.264".to_string(),
                ));
            }
            let runtime = MfRuntime::new()?;

            let subtype = match config.codec {
                crate::config::Codec::H264 => MFVideoFormat_H264,
                crate::config::Codec::H265 => MFVideoFormat_HEVC,
            };

            let transform = unsafe { enumerate_hardware_encoder(subtype)? };

            // Hardware encoder MFTs are asynchronous. The client must explicitly
            // unlock async mode (via the MFT's IMFAttributes) before setting
            // types, or SetInputType fails with MF_E_TRANSFORM_ASYNC_LOCKED
            // (0xC00D6D77). After unlocking we drive it with the event model.
            unsafe {
                let attrs: IMFAttributes = transform
                    .GetAttributes()
                    .map_err(|e| EncoderError::Backend(format!("GetAttributes: {e}")))?;
                attrs
                    .SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)
                    .map_err(|e| EncoderError::Backend(format!("ASYNC_UNLOCK: {e}")))?;
            }

            // Output type must be set before input type for encoders.
            unsafe {
                Self::configure_output(&transform, &config, subtype)?;
                Self::configure_input(&transform, &config)?;
            }

            let (input_stream_id, output_stream_id) = unsafe { stream_ids(&transform)? };

            // The MFT exposes its event queue via IMFMediaEventGenerator.
            let event_gen: IMFMediaEventGenerator = transform
                .cast()
                .map_err(|e| EncoderError::Backend(format!("cast to event generator: {e}")))?;

            unsafe {
                transform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                    .map_err(|e| EncoderError::Backend(format!("BEGIN_STREAMING: {e}")))?;
                transform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                    .map_err(|e| EncoderError::Backend(format!("START_OF_STREAM: {e}")))?;
            }

            Ok(Self {
                _runtime: runtime,
                transform,
                event_gen,
                config,
                input_stream_id,
                output_stream_id,
                started: true,
                frame_index: 0,
            })
        }

        unsafe fn configure_output(
            transform: &IMFTransform,
            config: &EncoderConfig,
            subtype: GUID,
        ) -> Result<(), EncoderError> {
            let out_type: IMFMediaType = MFCreateMediaType()
                .map_err(|e| EncoderError::Backend(format!("MFCreateMediaType(out): {e}")))?;
            out_type
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .and_then(|_| out_type.SetGUID(&MF_MT_SUBTYPE, &subtype))
                .and_then(|_| out_type.SetUINT32(&MF_MT_AVG_BITRATE, config.bitrate_bps))
                .and_then(|_| {
                    out_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                })
                .map_err(|e| EncoderError::Backend(format!("output type attrs: {e}")))?;

            set_attribute_ratio(&out_type, &MF_MT_FRAME_SIZE, config.width, config.height)?;
            set_attribute_ratio(&out_type, &MF_MT_FRAME_RATE, config.fps, 1)?;
            set_attribute_ratio(&out_type, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1)?;

            transform
                .SetOutputType(0, &out_type, 0)
                .map_err(|e| EncoderError::Backend(format!("SetOutputType: {e}")))?;
            Ok(())
        }

        unsafe fn configure_input(
            transform: &IMFTransform,
            config: &EncoderConfig,
        ) -> Result<(), EncoderError> {
            let in_type: IMFMediaType = MFCreateMediaType()
                .map_err(|e| EncoderError::Backend(format!("MFCreateMediaType(in): {e}")))?;
            in_type
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .and_then(|_| in_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12))
                .and_then(|_| {
                    in_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                })
                .map_err(|e| EncoderError::Backend(format!("input type attrs: {e}")))?;

            set_attribute_ratio(&in_type, &MF_MT_FRAME_SIZE, config.width, config.height)?;
            set_attribute_ratio(&in_type, &MF_MT_FRAME_RATE, config.fps, 1)?;
            set_attribute_ratio(&in_type, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1)?;

            transform
                .SetInputType(0, &in_type, 0)
                .map_err(|e| EncoderError::Backend(format!("SetInputType: {e}")))?;
            Ok(())
        }

        /// Build an IMFSample wrapping one NV12 frame at the given timestamp.
        unsafe fn make_input_sample(
            &self,
            nv12: &[u8],
            timestamp_ns: u64,
        ) -> Result<IMFSample, EncoderError> {
            let buffer: IMFMediaBuffer = MFCreateMemoryBuffer(nv12.len() as u32)
                .map_err(|e| EncoderError::Backend(format!("MFCreateMemoryBuffer: {e}")))?;

            let mut data: *mut u8 = std::ptr::null_mut();
            let mut max_len = 0u32;
            buffer
                .Lock(&mut data, Some(&mut max_len), None)
                .map_err(|e| EncoderError::Backend(format!("buffer Lock: {e}")))?;
            std::ptr::copy_nonoverlapping(nv12.as_ptr(), data, nv12.len());
            buffer
                .Unlock()
                .map_err(|e| EncoderError::Backend(format!("buffer Unlock: {e}")))?;
            buffer
                .SetCurrentLength(nv12.len() as u32)
                .map_err(|e| EncoderError::Backend(format!("SetCurrentLength: {e}")))?;

            let sample: IMFSample = MFCreateSample()
                .map_err(|e| EncoderError::Backend(format!("MFCreateSample: {e}")))?;
            sample
                .AddBuffer(&buffer)
                .map_err(|e| EncoderError::Backend(format!("AddBuffer: {e}")))?;

            let hns = (timestamp_ns as i64) / 100;
            sample
                .SetSampleTime(hns)
                .map_err(|e| EncoderError::Backend(format!("SetSampleTime: {e}")))?;
            let dur = HNS_PER_SEC / (self.config.fps.max(1) as i64);
            sample
                .SetSampleDuration(dur)
                .map_err(|e| EncoderError::Backend(format!("SetSampleDuration: {e}")))?;

            Ok(sample)
        }

        /// Block for the next MFT event and return its type. Async hardware MFTs
        /// drive the pipeline by emitting METransformNeedInput /
        /// METransformHaveOutput / METransformDrainComplete events.
        unsafe fn next_event_type(&self) -> Result<u32, EncoderError> {
            let event = self
                .event_gen
                .GetEvent(MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS(0))
                .map_err(|e| EncoderError::Backend(format!("GetEvent: {e}")))?;
            event
                .GetType()
                .map_err(|e| EncoderError::Backend(format!("event GetType: {e}")))
        }

        /// Pull exactly one output sample (called after a HaveOutput event).
        /// Returns `None` if the MFT signalled a stream/type change or had no
        /// sample ready.
        unsafe fn process_output(&mut self) -> Result<Option<EncodedUnit>, EncoderError> {
            let stream_info = self
                .transform
                .GetOutputStreamInfo(self.output_stream_id)
                .map_err(|e| EncoderError::Backend(format!("GetOutputStreamInfo: {e}")))?;

            // If the MFT does not allocate samples, we must supply one.
            let provides_samples = (stream_info.dwFlags
                & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32
                    | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32))
                != 0;

            let mut out_buffer = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: self.output_stream_id,
                pSample: std::mem::ManuallyDrop::new(if provides_samples {
                    None
                } else {
                    Some(self.allocate_output_sample(stream_info.cbSize)?)
                }),
                dwStatus: 0,
                pEvents: std::mem::ManuallyDrop::new(None),
            };

            let mut status = 0u32;
            let hr =
                self.transform
                    .ProcessOutput(0, std::slice::from_mut(&mut out_buffer), &mut status);

            match hr {
                Ok(()) => {
                    let sample = std::mem::ManuallyDrop::take(&mut out_buffer.pSample);
                    match sample {
                        Some(sample) => Ok(Some(self.sample_to_unit(&sample)?)),
                        None => Ok(None),
                    }
                }
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    let _ = std::mem::ManuallyDrop::take(&mut out_buffer.pSample);
                    self.handle_stream_change()?;
                    Ok(None)
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                    let _ = std::mem::ManuallyDrop::take(&mut out_buffer.pSample);
                    Ok(None)
                }
                Err(e) => {
                    let _ = std::mem::ManuallyDrop::take(&mut out_buffer.pSample);
                    Err(EncoderError::Backend(format!("ProcessOutput: {e}")))
                }
            }
        }

        unsafe fn allocate_output_sample(&self, size: u32) -> Result<IMFSample, EncoderError> {
            let buffer: IMFMediaBuffer = MFCreateMemoryBuffer(size.max(1))
                .map_err(|e| EncoderError::Backend(format!("out MFCreateMemoryBuffer: {e}")))?;
            let sample: IMFSample = MFCreateSample()
                .map_err(|e| EncoderError::Backend(format!("out MFCreateSample: {e}")))?;
            sample
                .AddBuffer(&buffer)
                .map_err(|e| EncoderError::Backend(format!("out AddBuffer: {e}")))?;
            Ok(sample)
        }

        unsafe fn handle_stream_change(&mut self) -> Result<(), EncoderError> {
            let subtype = match self.config.codec {
                crate::config::Codec::H264 => MFVideoFormat_H264,
                crate::config::Codec::H265 => MFVideoFormat_HEVC,
            };
            Self::configure_output(&self.transform, &self.config, subtype)
        }

        unsafe fn sample_to_unit(&self, sample: &IMFSample) -> Result<EncodedUnit, EncoderError> {
            let buffer = sample
                .ConvertToContiguousBuffer()
                .map_err(|e| EncoderError::Backend(format!("ConvertToContiguousBuffer: {e}")))?;

            let mut data: *mut u8 = std::ptr::null_mut();
            let mut cur_len = 0u32;
            buffer
                .Lock(&mut data, None, Some(&mut cur_len))
                .map_err(|e| EncoderError::Backend(format!("out buffer Lock: {e}")))?;
            let bytes = std::slice::from_raw_parts(data, cur_len as usize).to_vec();
            let _ = buffer.Unlock();

            // Keyframe: MFSampleExtension_CleanPoint attribute is set on IDRs.
            let keyframe = sample
                .GetUINT32(&MFSampleExtension_CleanPoint)
                .map(|v| v != 0)
                .unwrap_or(false);

            let hns = sample.GetSampleTime().unwrap_or(0);
            let timestamp_ns = (hns as u64).saturating_mul(100);

            Ok(EncodedUnit {
                bytes,
                keyframe,
                timestamp_ns,
            })
        }
    }

    impl VideoEncoder for MediaFoundationEncoder {
        fn backend_name(&self) -> &str {
            "MediaFoundation"
        }

        fn encode(
            &mut self,
            frame: &BgraFrame,
            timestamp_ns: u64,
        ) -> Result<Vec<EncodedUnit>, EncoderError> {
            if frame.width != self.config.width || frame.height != self.config.height {
                return Err(EncoderError::DimensionMismatch {
                    frame_w: frame.width,
                    frame_h: frame.height,
                    enc_w: self.config.width,
                    enc_h: self.config.height,
                });
            }

            let nv12 = bgra_to_nv12(frame).ok_or_else(|| {
                EncoderError::Backend("BGRA->NV12 conversion failed (odd dims?)".to_string())
            })?;

            let mut units = Vec::new();
            unsafe {
                let sample = self.make_input_sample(&nv12.data, timestamp_ns)?;

                // Async model: pump events until the MFT asks for input, feeding
                // it this frame, and collect any output it emits along the way.
                let mut fed = false;
                while !fed {
                    let evt = self.next_event_type()?;
                    if evt == METransformNeedInput {
                        self.transform
                            .ProcessInput(self.input_stream_id, &sample, 0)
                            .map_err(|e| EncoderError::Backend(format!("ProcessInput: {e}")))?;
                        fed = true;
                    } else if evt == METransformHaveOutput {
                        if let Some(u) = self.process_output()? {
                            units.push(u);
                        }
                    }
                }
                self.frame_index += 1;
            }
            Ok(units)
        }

        fn drain(&mut self) -> Result<Vec<EncodedUnit>, EncoderError> {
            if !self.started {
                return Ok(Vec::new());
            }
            let mut units = Vec::new();
            unsafe {
                // Tell the MFT no more input is coming, then pump HaveOutput
                // events until it reports the drain is complete.
                self.transform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
                    .map_err(|e| EncoderError::Backend(format!("END_OF_STREAM: {e}")))?;
                self.transform
                    .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
                    .map_err(|e| EncoderError::Backend(format!("COMMAND_DRAIN: {e}")))?;

                loop {
                    let evt = self.next_event_type()?;
                    if evt == METransformHaveOutput {
                        if let Some(u) = self.process_output()? {
                            units.push(u);
                        }
                    } else if evt == METransformDrainComplete {
                        break;
                    }
                    // METransformNeedInput during drain is ignored (no more input).
                }
                self.started = false;
            }
            Ok(units)
        }
    }

    /// Enumerate hardware encoder MFTs for the given output subtype and return
    /// the first that activates.
    unsafe fn enumerate_hardware_encoder(subtype: GUID) -> Result<IMFTransform, EncoderError> {
        let output_info = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: subtype,
        };

        let mut activates_ptr: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count: u32 = 0;
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            None,
            Some(&output_info),
            &mut activates_ptr,
            &mut count,
        )
        .map_err(|e| EncoderError::Unavailable(format!("MFTEnumEx(hardware): {e}")))?;

        if count == 0 || activates_ptr.is_null() {
            return Err(EncoderError::Unavailable(
                "no hardware H.264 encoder MFT found".to_string(),
            ));
        }

        // Take ownership of the CoTaskMemAlloc'd array so we free it on exit.
        let activates = std::slice::from_raw_parts(activates_ptr, count as usize);
        let mut chosen: Option<IMFTransform> = None;
        for activate in activates.iter().flatten() {
            if let Ok(transform) = activate.ActivateObject::<IMFTransform>() {
                chosen = Some(transform);
                break;
            }
        }
        windows::Win32::System::Com::CoTaskMemFree(Some(activates_ptr as *const _));

        chosen.ok_or_else(|| {
            EncoderError::Unavailable("hardware encoder MFT failed to activate".to_string())
        })
    }

    unsafe fn stream_ids(transform: &IMFTransform) -> Result<(u32, u32), EncoderError> {
        // Most encoder MFTs use fixed stream id 0 for both. Query the stream
        // count; if it reports the streams by id, honor them, else default to 0.
        let mut in_ids = [0u32; 1];
        let mut out_ids = [0u32; 1];
        match transform.GetStreamIDs(&mut in_ids, &mut out_ids) {
            Ok(()) => Ok((in_ids[0], out_ids[0])),
            // E_NOTIMPL means the MFT uses sequential ids starting at 0.
            Err(_) => Ok((0, 0)),
        }
    }

    /// Pack two u32s into the hi/lo of a u64 MF ratio attribute.
    unsafe fn set_attribute_ratio(
        media_type: &IMFMediaType,
        key: &GUID,
        numerator: u32,
        denominator: u32,
    ) -> Result<(), EncoderError> {
        let packed = ((numerator as u64) << 32) | (denominator as u64);
        media_type
            .SetUINT64(key, packed)
            .map_err(|_| EncoderError::Backend(format!("SetUINT64 ratio {key:?}")))?;
        let _ = E_FAIL; // keep import used if error paths change
        Ok(())
    }
}
