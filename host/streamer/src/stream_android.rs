use std::fs;
use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use usbdisplay_encoder::{read_bgra_bmp, select_encoder, Codec, EncoderConfig};
use usbdisplay_protocol::{EncodedFrame, FrameFlags};
use usbdisplay_transport::{Packetizer, DEFAULT_MAX_PACKET_PAYLOAD};

use crate::adb::{self, AdbDeviceState};

pub struct StreamCaptureArgs {
    pub input_dir: PathBuf,
    pub codec: Codec,
    pub bitrate_bps: u32,
    pub fps: u32,
    pub gop: u32,
    pub port: u16,
    pub serial: Option<String>,
    pub max_frames: Option<usize>,
    pub loop_forever: bool,
}

struct AdbForwardGuard {
    serial: String,
    port: u16,
}

impl Drop for AdbForwardGuard {
    fn drop(&mut self) {
        let _ = Command::new("adb")
            .args([
                "-s",
                &self.serial,
                "forward",
                "--remove",
                &format!("tcp:{}", self.port),
            ])
            .output();
    }
}

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

fn pick_serial(requested: Option<&str>) -> Result<String> {
    let devices = adb::list_devices()?;
    let connected: Vec<_> = devices
        .into_iter()
        .filter(|d| d.state == AdbDeviceState::Device)
        .collect();
    if connected.is_empty() {
        bail!("no connected adb devices found");
    }
    if let Some(serial) = requested {
        if connected.iter().any(|d| d.serial == serial) {
            return Ok(serial.to_string());
        }
        bail!("adb device '{serial}' is not connected");
    }
    Ok(connected[0].serial.clone())
}

fn ensure_adb_forward(serial: &str, port: u16) -> Result<AdbForwardGuard> {
    let output = Command::new("adb")
        .args([
            "-s",
            serial,
            "forward",
            &format!("tcp:{port}"),
            &format!("tcp:{port}"),
        ])
        .output()
        .context("failed to run adb forward")?;
    if !output.status.success() {
        bail!(
            "adb forward failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(AdbForwardGuard {
        serial: serial.to_string(),
        port,
    })
}

fn connect_with_retry(port: u16) -> Result<TcpStream> {
    let mut last_err = None;
    for _ in 0..100 {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => return Ok(stream),
            Err(err) => {
                last_err = Some(err);
                thread::sleep(Duration::from_millis(200));
            }
        }
    }
    bail!("failed to connect to 127.0.0.1:{port}: {last_err:?}")
}

pub fn run(args: StreamCaptureArgs) -> Result<()> {
    let frames = collect_frames(&args.input_dir)?;
    if frames.is_empty() {
        bail!(
            "no capture_*.bmp frames found in {}",
            args.input_dir.display()
        );
    }
    let take = args.max_frames.unwrap_or(frames.len()).min(frames.len());
    println!("stream_input_frames={take}");

    let first = read_bgra_bmp(&fs::read(&frames[0])?)
        .with_context(|| format!("decoding {}", frames[0].display()))?;
    println!("stream_frame_size={}x{}", first.width, first.height);

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
    println!("stream_encoder_backend={}", encoder.backend_name());

    let serial = pick_serial(args.serial.as_deref())?;
    println!("adb_serial={serial}");
    let _forward = ensure_adb_forward(&serial, args.port)?;
    println!("adb_forward=tcp:{}->tcp:{}", args.port, args.port);
    println!("waiting_for_android_listener=true");
    let mut socket = connect_with_retry(args.port)?;
    socket
        .set_nodelay(true)
        .context("failed to set TCP_NODELAY")?;
    println!("android_connection=established");

    let mut frame_sequence = 1u64;
    let mut timestamp_offset_ns = 0u64;
    let frame_dur_ns = 1_000_000_000u64 / (args.fps.max(1) as u64);
    let mut packetizer = Packetizer::new(DEFAULT_MAX_PACKET_PAYLOAD);
    let mut total_packets = 0usize;
    let mut total_frames = 0usize;
    let playback_start = Instant::now();
    let mut first_pts_ns = None::<u64>;

    loop {
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
            let ts = timestamp_offset_ns + (i as u64) * frame_dur_ns;
            let units = encoder.encode(&frame, ts)?;
            for unit in units {
                let base = *first_pts_ns.get_or_insert(unit.timestamp_ns);
                let target_ns = unit.timestamp_ns.saturating_sub(base);
                let elapsed_ns = playback_start.elapsed().as_nanos() as u64;
                if target_ns > elapsed_ns {
                    thread::sleep(Duration::from_nanos(target_ns - elapsed_ns));
                }

                let flags = if unit.keyframe {
                    FrameFlags::KEYFRAME
                } else {
                    FrameFlags::NONE
                };
                let encoded_frame = EncodedFrame::new(
                    frame_sequence,
                    unit.timestamp_ns,
                    args.codec.to_protocol(),
                    flags,
                    first.width as u16,
                    first.height as u16,
                    args.fps * 1000,
                    unit.bytes,
                )?;
                frame_sequence = frame_sequence.wrapping_add(1).max(1);

                let packets = packetizer.packetize_frame(&encoded_frame)?;
                for packet in packets {
                    let bytes = packet.encode();
                    let packet_len = (bytes.len() as u32).to_le_bytes();
                    socket.write_all(&packet_len)?;
                    socket.write_all(&bytes)?;
                    total_packets += 1;
                }
                total_frames += 1;
            }
        }

        let drained = encoder.drain()?;
        for unit in drained {
            let base = *first_pts_ns.get_or_insert(unit.timestamp_ns);
            let target_ns = unit.timestamp_ns.saturating_sub(base);
            let elapsed_ns = playback_start.elapsed().as_nanos() as u64;
            if target_ns > elapsed_ns {
                thread::sleep(Duration::from_nanos(target_ns - elapsed_ns));
            }

            let flags = if unit.keyframe {
                FrameFlags::KEYFRAME
            } else {
                FrameFlags::NONE
            };
            let encoded_frame = EncodedFrame::new(
                frame_sequence,
                unit.timestamp_ns,
                args.codec.to_protocol(),
                flags,
                first.width as u16,
                first.height as u16,
                args.fps * 1000,
                unit.bytes,
            )?;
            frame_sequence = frame_sequence.wrapping_add(1).max(1);
            let packets = packetizer.packetize_frame(&encoded_frame)?;
            for packet in packets {
                let bytes = packet.encode();
                let packet_len = (bytes.len() as u32).to_le_bytes();
                socket.write_all(&packet_len)?;
                socket.write_all(&bytes)?;
                total_packets += 1;
            }
            total_frames += 1;
        }

        println!("streamed_frames={total_frames} streamed_packets={total_packets}");
        timestamp_offset_ns = timestamp_offset_ns
            .saturating_add((take as u64).saturating_mul(frame_dur_ns));
        if !args.loop_forever {
            break;
        }
    }

    println!("stream_complete=true");
    Ok(())
}
