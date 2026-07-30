//! `encode-capture` subcommand: encode the driver's captured frames.
//!
//! Reads the `capture_*.bmp` frames the IDD writes to
//! `%ProgramData%\USBDisplay\capture`, encodes them with the best available
//! hardware backend (Media Foundation today), writes the H.264/H.265 elementary
//! stream to disk, validates the stream structurally, and pushes each coded
//! picture through the protocol + transport layers to prove the full
//! encode -> frame -> packetize path.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use usbdisplay_encoder::{
    nal, read_bgra_bmp, select_encoder, Codec, EncodedUnit, EncoderConfig,
};
use usbdisplay_protocol::{EncodedFrame, FrameFlags};
use usbdisplay_transport::{Packetizer, DEFAULT_MAX_PACKET_PAYLOAD};

pub struct EncodeCaptureArgs {
    pub input_dir: PathBuf,
    pub out: PathBuf,
    pub codec: Codec,
    pub bitrate_bps: u32,
    pub fps: u32,
    pub gop: u32,
    pub max_frames: Option<usize>,
    pub verify_decode: bool,
}

/// Collect and sort `capture_*.bmp` files by name (their frame index is
/// zero-padded, so lexicographic == numeric order).
fn collect_frames(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        bail!("input dir does not exist: {}", dir.display());
    }
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().map(|x| x == "bmp").unwrap_or(false)
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("capture_"))
                    .unwrap_or(false)
        })
        .collect();
    files.sort();
    Ok(files)
}

pub fn run(args: EncodeCaptureArgs) -> Result<()> {
    let frames = collect_frames(&args.input_dir)?;
    if frames.is_empty() {
        bail!(
            "no capture_*.bmp frames found in {}",
            args.input_dir.display()
        );
    }
    let take = args.max_frames.unwrap_or(frames.len()).min(frames.len());
    println!("input_frames={}", take);

    // First frame establishes dimensions for the whole run.
    let first = read_bgra_bmp(&fs::read(&frames[0])?)
        .with_context(|| format!("decoding {}", frames[0].display()))?;
    println!("frame_size={}x{}", first.width, first.height);

    let config = EncoderConfig::new(first.width, first.height)
        .with_fps(args.fps)
        .with_bitrate(args.bitrate_bps)
        .with_gop(args.gop)
        .with_codec(args.codec);

    let (encoder, statuses) = select_encoder(&config);
    for s in &statuses {
        println!(
            "backend {}: {}",
            s.backend.name(),
            if s.available { "SELECTED" } else { &s.detail }
        );
    }
    let mut encoder = match encoder {
        Some(e) => e,
        None => bail!("no encoder backend available (see backend lines above)"),
    };
    println!("encoder_backend={}", encoder.backend_name());

    // Encode every frame, accumulating coded pictures.
    let frame_dur_ns = 1_000_000_000u64 / (args.fps.max(1) as u64);
    let mut units: Vec<EncodedUnit> = Vec::new();
    for (i, path) in frames.iter().take(take).enumerate() {
        let frame = read_bgra_bmp(&fs::read(path)?)
            .with_context(|| format!("decoding {}", path.display()))?;
        if frame.width != first.width || frame.height != first.height {
            bail!(
                "frame {} size {}x{} differs from first {}x{}",
                path.display(),
                frame.width,
                frame.height,
                first.width,
                first.height
            );
        }
        let ts = (i as u64) * frame_dur_ns;
        units.extend(encoder.encode(&frame, ts)?);
    }
    units.extend(encoder.drain()?);

    if units.is_empty() {
        bail!("encoder produced no output units");
    }

    // Write the elementary stream: concatenated Annex-B access units.
    let mut stream = Vec::new();
    for u in &units {
        stream.extend_from_slice(&u.bytes);
    }
    fs::write(&args.out, &stream)
        .with_context(|| format!("writing {}", args.out.display()))?;
    println!("encoded_units={}", units.len());
    println!("output_bytes={}", stream.len());
    println!("output_path={}", args.out.display());

    // Structural validation of the whole stream.
    let summary = nal::summarize(&stream, args.codec);
    println!(
        "nal_total={} sps={} pps={} vps={} idr={} non_idr={}",
        summary.total_nals,
        summary.has_sps,
        summary.has_pps,
        summary.has_vps,
        summary.idr_slices,
        summary.non_idr_slices
    );
    let playable = summary.is_playable(args.codec);
    println!("stream_playable={}", playable);

    // Push each coded picture through protocol framing + transport packetizer.
    let mut packetizer = Packetizer::new(DEFAULT_MAX_PACKET_PAYLOAD);
    let mut total_packets = 0usize;
    let mut total_packet_bytes = 0usize;
    for (seq, u) in units.iter().enumerate() {
        let flags = if u.keyframe {
            FrameFlags::KEYFRAME
        } else {
            FrameFlags::NONE
        };
        let frame = EncodedFrame::new(
            seq as u64,
            u.timestamp_ns,
            args.codec.to_protocol(),
            flags,
            first.width as u16,
            first.height as u16,
            args.fps * 1000,
            u.bytes.clone(),
        )?;
        let packets = packetizer.packetize_frame(&frame)?;
        total_packets += packets.len();
        total_packet_bytes += packets.iter().map(|p| p.encode().len()).sum::<usize>();
    }
    println!("transport_packets={}", total_packets);
    println!("transport_bytes={}", total_packet_bytes);

    if !playable {
        bail!("stream failed structural validation (missing SPS/PPS/IDR)");
    }

    // Optional decode round-trip: prove the bytes actually decode, not just that
    // the NAL headers are well-formed.
    if args.verify_decode {
        match usbdisplay_encoder::validate::decode_h264_stream(&stream, first.width, first.height) {
            Ok(report) => {
                println!(
                    "decoded_frames={} decoded_size={}x{}",
                    report.decoded_frames, report.width, report.height
                );
                if report.decoded_frames == 0 {
                    bail!("decode round-trip produced zero frames");
                }
                // H.264 codes in 16px macroblocks, so the decoder's coded size
                // is the captured size rounded up to a multiple of 16 (e.g.
                // 1080 -> 1088). Accept any size within one macroblock.
                let w_ok = report.width >= first.width && report.width < first.width + 16;
                let h_ok = report.height >= first.height && report.height < first.height + 16;
                if !w_ok || !h_ok {
                    bail!(
                        "decoded size {}x{} not within a macroblock of captured {}x{}",
                        report.width,
                        report.height,
                        first.width,
                        first.height
                    );
                }
                if report.decoded_frames as usize != take {
                    println!(
                        "warning: decoded {} frames but encoded {}",
                        report.decoded_frames, take
                    );
                }
                println!("decode_roundtrip=PASS");
            }
            Err(e) => bail!("decode round-trip failed: {e}"),
        }
    }

    println!("OVERALL: PASS -- captured frames encoded and validated");
    Ok(())
}
