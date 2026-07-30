use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use usbdisplay_encoder::{read_bgra_bmp, select_encoder, Codec, EncoderConfig};
use usbdisplay_protocol::{EncodedFrame, FrameFlags, InputEvent};
use usbdisplay_transport::{
    Packetizer, ReceivedPacket, TransportPacket, DEFAULT_MAX_PACKET_PAYLOAD,
};

use crate::adb::{self, AdbDeviceState};
use crate::input_inject;

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

/// Reads length-prefixed transport packets from the device and injects any
/// `Control` input events into the host. Runs until the socket closes or
/// `running` is cleared. Errors are terminal for the reader only; the video
/// path keeps streaming.
fn run_input_reader(mut socket: TcpStream, running: Arc<AtomicBool>, injected: Arc<AtomicU64>) {
    let mut sink = input_inject::default_sink();
    // A short read timeout lets the loop notice `running` being cleared even
    // when the device is idle and no bytes are arriving.
    let _ = socket.set_read_timeout(Some(Duration::from_millis(200)));
    while running.load(Ordering::Relaxed) {
        let mut len_bytes = [0u8; 4];
        match read_exact_timeout_aware(&mut socket, &mut len_bytes) {
            ReadOutcome::Ok => {}
            ReadOutcome::TimedOut => continue,
            ReadOutcome::Closed => break,
        }
        let packet_len = u32::from_le_bytes(len_bytes) as usize;
        if packet_len == 0 || packet_len > DEFAULT_MAX_PACKET_PAYLOAD + 64 {
            break; // framing lost; do not try to resync a corrupt reverse channel.
        }
        let mut packet_bytes = vec![0u8; packet_len];
        match read_exact_timeout_aware(&mut socket, &mut packet_bytes) {
            ReadOutcome::Ok => {}
            // A partial packet body means we cannot trust the stream position.
            ReadOutcome::TimedOut | ReadOutcome::Closed => break,
        }
        let packet = match TransportPacket::decode(&packet_bytes, DEFAULT_MAX_PACKET_PAYLOAD) {
            Ok(packet) => packet,
            Err(_) => continue, // drop malformed packet, keep listening.
        };
        let control = match packet.into_received() {
            Ok(ReceivedPacket::Control(payload)) => payload,
            _ => continue, // only Control packets carry input today.
        };
        for chunk in control.chunks_exact(usbdisplay_protocol::INPUT_EVENT_LEN) {
            if let Ok(event) = InputEvent::decode(chunk) {
                sink.inject(&event);
                injected.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    running.store(false, Ordering::Relaxed);
}

enum ReadOutcome {
    Ok,
    TimedOut,
    Closed,
}

/// `read_exact` that reports a read timeout distinctly from a real close, so
/// the caller can re-check its run flag while idle.
fn read_exact_timeout_aware(socket: &mut TcpStream, buf: &mut [u8]) -> ReadOutcome {
    let mut filled = 0;
    while filled < buf.len() {
        match socket.read(&mut buf[filled..]) {
            Ok(0) => return ReadOutcome::Closed,
            Ok(n) => filled += n,
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // Only surface a timeout at a packet boundary; a mid-packet
                // stall should keep waiting for the rest of the bytes.
                if filled == 0 {
                    return ReadOutcome::TimedOut;
                }
            }
            Err(_) => return ReadOutcome::Closed,
        }
    }
    ReadOutcome::Ok
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
    let mut frames = collect_frames(&args.input_dir)?;
    while args.loop_forever && frames.is_empty() {
        println!("waiting_for_capture_frames=true");
        thread::sleep(Duration::from_millis(250));
        frames = collect_frames(&args.input_dir)?;
    }
    if frames.is_empty() {
        bail!(
            "no capture_*.bmp frames found in {}",
            args.input_dir.display()
        );
    }

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

    // Spawn the reverse input channel on a clone of the socket. The video path
    // below keeps ownership of `socket` for writes.
    let input_running = Arc::new(AtomicBool::new(true));
    let input_injected = Arc::new(AtomicU64::new(0));
    let input_reader = match socket.try_clone() {
        Ok(reader_socket) => {
            let running = Arc::clone(&input_running);
            let injected = Arc::clone(&input_injected);
            println!("input_return_channel=enabled");
            Some(thread::spawn(move || {
                run_input_reader(reader_socket, running, injected)
            }))
        }
        Err(err) => {
            println!("input_return_channel=disabled reason=\"clone failed: {err}\"");
            None
        }
    };

    let mut frame_sequence = 1u64;
    let frame_dur_ns = 1_000_000_000u64 / (args.fps.max(1) as u64);
    let mut source_timestamp_ns = 0u64;
    let mut packetizer = Packetizer::new(DEFAULT_MAX_PACKET_PAYLOAD);
    let mut total_packets = 0usize;
    let mut total_frames = 0usize;
    let mut total_input_frames = 0usize;
    let playback_start = Instant::now();
    let mut first_pts_ns = None::<u64>;
    let mut next_frame_index = 0usize;
    let max_input_frames = args.max_frames.unwrap_or(usize::MAX);

    loop {
        frames = collect_frames(&args.input_dir)?;
        if frames.len() <= next_frame_index {
            if args.loop_forever && total_input_frames < max_input_frames {
                thread::sleep(Duration::from_millis(16));
                continue;
            }
            break;
        }

        for path in frames.iter().skip(next_frame_index) {
            if total_input_frames >= max_input_frames {
                break;
            }
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
            let ts = source_timestamp_ns;
            source_timestamp_ns = source_timestamp_ns.saturating_add(frame_dur_ns);
            total_input_frames += 1;
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
            next_frame_index += 1;
        }
        if total_input_frames >= max_input_frames {
            break;
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

    println!(
        "streamed_input_frames={} streamed_frames={} streamed_packets={}",
        total_input_frames, total_frames, total_packets
    );

    // Stop the reverse input channel and drain the thread. Dropping the write
    // half and clearing the flag unblocks the reader's blocking read.
    input_running.store(false, Ordering::Relaxed);
    drop(socket);
    if let Some(handle) = input_reader {
        let _ = handle.join();
    }
    println!(
        "input_events_injected={}",
        input_injected.load(Ordering::Relaxed)
    );
    println!("stream_complete=true");
    Ok(())
}
