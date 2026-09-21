use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
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
use crate::wifi;
use rustls::ClientConnection;

/// How many streamed frames pass between `--stats-json` lines.
const STATS_FRAME_INTERVAL: usize = 60;

/// Default bitrate/GOP when the user did not override flags.
/// USB keeps 20 Mbps / GOP 60; WiFi defaults to 12 Mbps / GOP 30.
pub const USB_DEFAULT_BITRATE_BPS: u32 = 20_000_000;
pub const WIFI_DEFAULT_BITRATE_BPS: u32 = 12_000_000;
const USB_DEFAULT_GOP: u32 = 60;
const WIFI_DEFAULT_GOP: u32 = 30;

/// Shared TLS stream: `rustls::StreamOwned` cannot be `try_clone`d like a
/// raw `TcpStream`, so both the video writer and the reverse input reader
/// share one connection behind a mutex. Each `Read`/`Write` call locks
/// briefly; throughput is dominated by the socket, not the lock.
#[derive(Clone)]
pub struct SharedTlsStream {
    inner: Arc<Mutex<rustls::StreamOwned<ClientConnection, TcpStream>>>,
}

impl SharedTlsStream {
    pub fn new(stream: rustls::StreamOwned<ClientConnection, TcpStream>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(stream)),
        }
    }
}

impl Write for SharedTlsStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // `StreamOwned::write` takes `&mut self`; lock per call.
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| std::io::Error::other("tls lock poisoned"))?;
        std::io::Write::write(&mut *guard, buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| std::io::Error::other("tls lock poisoned"))?;
        std::io::Write::flush(&mut *guard)
    }
}

impl Read for SharedTlsStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| std::io::Error::other("tls lock poisoned"))?;
        std::io::Read::read(&mut *guard, buf)
    }
}

/// Wrapper for either plain TCP or TLS stream for generic use.
pub enum VideoStream {
    Plain(TcpStream),
    Tls(SharedTlsStream),
}

impl Clone for VideoStream {
    fn clone(&self) -> Self {
        self.try_clone()
            .expect("VideoStream clone must succeed (TLS shares Arc, TCP try_clone)")
    }
}

impl Write for VideoStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            VideoStream::Plain(s) => s.write(buf),
            VideoStream::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            VideoStream::Plain(s) => s.flush(),
            VideoStream::Tls(s) => s.flush(),
        }
    }
}

impl Read for VideoStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            VideoStream::Plain(s) => s.read(buf),
            VideoStream::Tls(s) => s.read(buf),
        }
    }
}

impl VideoStream {
    fn try_clone(&self) -> std::io::Result<Self> {
        match self {
            VideoStream::Plain(s) => Ok(VideoStream::Plain(s.try_clone()?)),
            // TLS shares one session; cloning shares the Arc.
            VideoStream::Tls(s) => Ok(VideoStream::Tls(s.clone())),
        }
    }

    fn set_read_timeout(&self, dur: Option<Duration>) -> std::io::Result<()> {
        match self {
            VideoStream::Plain(s) => s.set_read_timeout(dur),
            VideoStream::Tls(s) => {
                let guard = s
                    .inner
                    .lock()
                    .map_err(|_| std::io::Error::other("tls lock poisoned"))?;
                guard.get_ref().set_read_timeout(dur)
            }
        }
    }

    fn shutdown(&self) {
        match self {
            VideoStream::Plain(s) => {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
            VideoStream::Tls(s) => {
                if let Ok(guard) = s.inner.lock() {
                    let _ = guard.get_ref().shutdown(std::net::Shutdown::Both);
                }
            }
        }
    }
}

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
    /// Live mode: always encode the newest captured frame, drop and delete the
    /// stale backlog, and send immediately with real wall-clock timestamps so
    /// the client presents with minimal latency. This is what makes the tablet
    /// usable as a real second monitor. When false, every captured frame is
    /// replayed in order at a fixed fps (demo / file-replay behavior).
    pub live: bool,
    /// Selected transport. USB is the ADB-forwarded path; WiFi is TLS LAN.
    pub transport: Transport,
    /// WiFi tablet address (bare IP, ip:port, or QR JSON). WiFi only.
    pub device_ip: Option<String>,
    /// WiFi pairing PIN shown on the tablet. WiFi only.
    pub pin: Option<String>,
    /// Print streaming stats as JSON every 60 frames.
    pub stats_json: bool,
}

/// Transport selector for `stream-capture`. Mirrors the CLI `--transport` flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Usb,
    Wifi,
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

/// Adaptive bitrate ladder (bps). WiFi starts one step down from USB.
const BITRATE_LADDER: [u32; 4] = [20_000_000, 12_000_000, 8_000_000, 4_000_000];

/// Congestion-adaptive bitrate controller (PR-4).
///
/// Inputs: per-send write-stall time + KeyframeRequest rate from the input
/// thread. Steps down 20→12→8→4 Mbps when p95 write-stall >50 ms over a
/// 60-frame window OR kf_req_rate >2/s; steps back up when clean for
/// 600 frames.
pub struct RateController {
    level: usize,
    stalls: Vec<f64>,
    /// Monotonic KF counter sampled per frame; `kf_window[i]` aligns with
    /// `stalls[i]` so `kf_total - kf_window[0]` is the exact count in the
    /// current 60-frame window (≈ per-second at 60 fps).
    kf_window: Vec<u64>,
    clean_frames: usize,
}

impl RateController {
    pub fn new(start_bitrate: u32) -> Self {
        Self {
            level: level_for_bitrate(start_bitrate),
            stalls: Vec::with_capacity(60),
            kf_window: Vec::with_capacity(60),
            clean_frames: 0,
        }
    }

    #[allow(dead_code)]
    pub fn current_bitrate(&self) -> u32 {
        BITRATE_LADDER[self.level]
    }

    /// Observe one streamed frame. Returns `Some(new_bitrate)` when the
    /// encoder should be re-created at a different bitrate.
    ///
    /// Congestion = p95 write-stall >50 ms over a 60-frame window OR more
    /// than 2 keyframe requests in the current window (≈>2/s at 60 fps).
    /// Recovery requires 600 consecutive clean frames.
    pub fn observe(&mut self, stall_ms: f64, kf_total: u64) -> Option<u32> {
        self.stalls.push(stall_ms);
        self.kf_window.push(kf_total);
        if self.stalls.len() > 60 {
            self.stalls.remove(0);
            self.kf_window.remove(0);
        }
        let p95 = percentile(&self.stalls, 95.0);
        let kf_in_window = kf_total.saturating_sub(*self.kf_window.first().unwrap_or(&kf_total));

        let window_full = self.stalls.len() >= 60;
        // Stall needs a full window; KF bursts react immediately (>2 in
        // whatever frames we have so far) so loss storms cut bitrate fast.
        let stall_congested = window_full && p95 > 50.0;
        let kf_congested = kf_in_window > 2;

        if stall_congested || kf_congested {
            self.clean_frames = 0;
            if self.level + 1 < BITRATE_LADDER.len() {
                self.level += 1;
                self.stalls.clear();
                self.kf_window.clear();
                return Some(BITRATE_LADDER[self.level]);
            }
            return None;
        }

        if p95 <= 50.0 && !kf_congested {
            self.clean_frames += 1;
        } else {
            self.clean_frames = 0;
        }
        if self.clean_frames >= 600 && self.level > 0 && window_full {
            self.level -= 1;
            self.clean_frames = 0;
            self.stalls.clear();
            self.kf_window.clear();
            return Some(BITRATE_LADDER[self.level]);
        }
        None
    }
}

fn level_for_bitrate(bitrate: u32) -> usize {
    let mut best = 0;
    let mut best_diff = u32::MAX;
    for (i, level) in BITRATE_LADDER.iter().enumerate() {
        let diff = level.abs_diff(bitrate);
        if diff < best_diff {
            best_diff = diff;
            best = i;
        }
    }
    best
}

fn percentile(samples: &[f64], p: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = (((p / 100.0) * sorted.len() as f64).ceil() as usize).saturating_sub(1);
    sorted[idx.min(sorted.len() - 1)]
}

/// Resolve effective bitrate/GOP: WiFi defaults (12 Mbps / GOP 30) apply
/// unless the user overrode the flag (detected by comparing to the USB
/// defaults, per spec).
pub fn effective_wifi_tuning(transport: Transport, bitrate_bps: u32, gop: u32) -> (u32, u32) {
    if transport == Transport::Wifi {
        let bitrate = if bitrate_bps == USB_DEFAULT_BITRATE_BPS {
            WIFI_DEFAULT_BITRATE_BPS
        } else {
            bitrate_bps
        };
        let gop = if gop == USB_DEFAULT_GOP {
            WIFI_DEFAULT_GOP
        } else {
            gop
        };
        (bitrate, gop)
    } else {
        (bitrate_bps, gop)
    }
}

/// Re-create the encoder at a new bitrate (same codec/resolution/fps/GOP).
/// Encoders are fixed-size today, so bitrate changes require re-init;
/// resolution changes keep the existing bail-and-restart behavior.
fn recreate_encoder(
    width: u32,
    height: u32,
    fps: u32,
    bitrate_bps: u32,
    gop: u32,
    codec: Codec,
) -> Result<Box<dyn usbdisplay_encoder::VideoEncoder>> {
    let config = EncoderConfig::new(width, height)
        .with_fps(fps)
        .with_bitrate(bitrate_bps)
        .with_gop(gop)
        .with_codec(codec);
    let (encoder, _) = select_encoder(&config);
    encoder.ok_or_else(|| anyhow::anyhow!("no encoder backend available for bitrate {bitrate_bps}"))
}

/// Apply a mid-stream capture size change.
///
/// A Windows mode switch (Display Settings, Extend/Duplicate, resolution
/// change) alters the virtual monitor's size while we are streaming. Instead of
/// treating that as fatal, rebuild the encoder at the new dimensions and retag
/// the sender so the next frame carries the correct WxH. Android's
/// `StreamPipeline` already recreates its `MediaCodec` when the frame WxH
/// changes, so the switch becomes a brief hiccup rather than a dead stream.
#[allow(clippy::too_many_arguments)]
fn apply_size_change<W: Write>(
    sender: &mut FrameSender<W>,
    encoder: &mut Box<dyn usbdisplay_encoder::VideoEncoder>,
    old: (u32, u32),
    new: (u32, u32),
    fps: u32,
    bitrate_bps: u32,
    gop: u32,
    codec: Codec,
) -> Result<()> {
    *encoder = recreate_encoder(new.0, new.1, fps, bitrate_bps, gop, codec)?;
    sender.width = new.0 as u16;
    sender.height = new.1 as u16;
    println!(
        "stream_resolution_change old={}x{} new={}x{}",
        old.0, old.1, new.0, new.1
    );
    Ok(())
}

fn emit_stats_json(sender: &FrameSender<&mut VideoStream>, injected: &Arc<AtomicU64>) {
    println!("{}", sender.stats_line(injected.load(Ordering::Relaxed)));
}

#[allow(clippy::too_many_arguments)]
fn maybe_adapt(
    rate: &mut RateController,
    stall_ms: f64,
    kf_total: u64,
    current_bitrate: &mut u32,
    width: u32,
    height: u32,
    fps: u32,
    gop: u32,
    codec: Codec,
    encoder: &mut Box<dyn usbdisplay_encoder::VideoEncoder>,
) {
    if let Some(new_bitrate) = rate.observe(stall_ms, kf_total) {
        let old = *current_bitrate;
        match recreate_encoder(width, height, fps, new_bitrate, gop, codec) {
            Ok(new_enc) => {
                *encoder = new_enc;
                *current_bitrate = new_bitrate;
                if new_bitrate < old {
                    println!("bitrate_step_down old_bitrate={old} new_bitrate={new_bitrate}");
                } else {
                    println!("bitrate_step_up old_bitrate={old} new_bitrate={new_bitrate}");
                }
            }
            Err(e) => {
                eprintln!("warning: encoder re-init to {new_bitrate} failed: {e:#}");
            }
        }
    }
}

/// Reads length-prefixed transport packets from the device and injects any
/// `Control` input events into the host. Runs until the socket closes or
/// `running` is cleared. Errors are terminal for the reader only; the video
/// path keeps streaming.
///
/// Also counts `KeyframeRequest` packets into `kf_requests` for the PR-4
/// `RateController`.
fn run_input_reader(
    mut socket: VideoStream,
    running: Arc<AtomicBool>,
    injected: Arc<AtomicU64>,
    kf_requests: Arc<AtomicU64>,
) {
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
        match packet.into_received() {
            Ok(ReceivedPacket::Control(payload)) => {
                for chunk in payload.chunks_exact(usbdisplay_protocol::INPUT_EVENT_LEN) {
                    if let Ok(event) = InputEvent::decode(chunk) {
                        sink.inject(&event);
                        injected.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            Ok(ReceivedPacket::KeyframeRequest { .. }) => {
                kf_requests.fetch_add(1, Ordering::Relaxed);
            }
            _ => continue,
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
fn read_exact_timeout_aware(socket: &mut VideoStream, buf: &mut [u8]) -> ReadOutcome {
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

/// Wraps one coded picture in a protocol `EncodedFrame`, packetizes it, and
/// writes the length-prefixed transport packets to the socket. Shared by the
/// live and replay paths — and by USB and (future) WiFi transports — so
/// framing stays identical.
///
/// Generic over `Write` so the ADB `TcpStream` today and the TLS WiFi stream
/// in PR-3 share the exact same send path.
struct FrameSender<W: Write> {
    socket: W,
    packetizer: Packetizer,
    codec: Codec,
    width: u16,
    height: u16,
    fps: u32,
    frame_sequence: u64,
    total_packets: usize,
    total_frames: usize,
    write_stall_ms_max: f64,
    last_stall_ms: f64,
}

impl<W: Write> FrameSender<W> {
    fn send_unit(&mut self, unit: &usbdisplay_encoder::EncodedUnit) -> Result<()> {
        let flags = if unit.keyframe {
            FrameFlags::KEYFRAME
        } else {
            FrameFlags::NONE
        };
        let encoded_frame = EncodedFrame::new(
            self.frame_sequence,
            unit.timestamp_ns,
            self.codec.to_protocol(),
            flags,
            self.width,
            self.height,
            self.fps * 1000,
            unit.bytes.clone(),
        )?;
        self.frame_sequence = self.frame_sequence.wrapping_add(1).max(1);

        let start = Instant::now();
        for packet in self.packetizer.packetize_frame(&encoded_frame)? {
            write_framed_packet(&mut self.socket, &packet)?;
            self.total_packets += 1;
        }
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        self.write_stall_ms_max = self.write_stall_ms_max.max(elapsed_ms);
        self.last_stall_ms = elapsed_ms;
        self.total_frames += 1;
        Ok(())
    }

    fn last_stall_ms(&self) -> f64 {
        self.last_stall_ms
    }

    fn stats_line(&self, input_events_injected: u64) -> String {
        format!(
            r#"{{"streamed_frames":{},"streamed_packets":{},"write_stall_ms_max":{:.2},"input_events_injected":{}}}"#,
            self.total_frames, self.total_packets, self.write_stall_ms_max, input_events_injected
        )
    }
}

/// Encode one transport packet with the shared `u32LE(len) + bytes` framing
/// and write it to any video sink (USB TCP today, TLS WiFi in PR-3).
pub fn write_framed_packet<W: Write>(socket: &mut W, packet: &TransportPacket) -> Result<()> {
    let bytes = packet.encode();
    let packet_len = (bytes.len() as u32).to_le_bytes();
    socket.write_all(&packet_len)?;
    socket.write_all(&bytes)?;
    Ok(())
}

pub fn run(args: StreamCaptureArgs) -> Result<()> {
    // WiFi: fail fast with actionable guidance before touching the encoder
    // or ADB so `--transport wifi` never silently falls back to USB.
    if args.transport == Transport::Wifi {
        let raw = args.device_ip.as_deref().unwrap_or_default();
        if raw.trim().is_empty() {
            bail!(
                "wifi transport needs --device-ip <ip|ip:port|QR-JSON> \
                 (tablet WiFi Pair screen shows the QR; scan it or pass --device-ip <tablet-lan-ip>)"
            );
        }
        let device = wifi::parse_device(raw)?;
        println!("wifi_device={}", device.addr());
        if let Some(fp) = &device.fingerprint {
            println!("wifi_fingerprint={fp}");
        }

        // PR-3+: plaintext refused always; PIN+TLS 1.3 required.
        // --pin may be omitted on reconnect: a tablet that already trusts
        // this host_id skips the PIN check (TLS fingerprint still enforced).
        let env_pin = std::env::var("USBDISPLAY_WIFI_PIN").ok();
        let pin = args.pin.as_deref().or(env_pin.as_deref());
        if pin.is_none() {
            println!("wifi_pin=omitted (ok only for already-trusted hosts)");
        }
        println!("wifi_encryption=tls-1.3");

        let connection = wifi::connect_tls(&device, pin)?;
        println!("android_connection=established");

        // Wrap TLS stream for video streaming (shared for write + input read).
        let mut video_stream = VideoStream::Tls(SharedTlsStream::new(connection.stream));

        // Spawn the reverse input channel on a clone of the socket.
        let input_running = Arc::new(AtomicBool::new(true));
        let input_injected = Arc::new(AtomicU64::new(0));
        let kf_requests = Arc::new(AtomicU64::new(0));
        let input_reader = match video_stream.try_clone() {
            Ok(reader_socket) => {
                let running = Arc::clone(&input_running);
                let injected = Arc::clone(&input_injected);
                let kf = Arc::clone(&kf_requests);
                println!("input_return_channel=enabled");
                Some(thread::spawn(move || {
                    run_input_reader(reader_socket, running, injected, kf)
                }))
            }
            Err(err) => {
                println!("input_return_channel=disabled reason=\"clone failed: {err}\"");
                None
            }
        };

        // Run the existing encode loop over the WiFi socket
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

        let (eff_bitrate, eff_gop) =
            effective_wifi_tuning(args.transport, args.bitrate_bps, args.gop);
        if eff_bitrate != args.bitrate_bps || eff_gop != args.gop {
            println!("wifi_defaults_applied bitrate={eff_bitrate} gop={eff_gop}");
        }
        let mut current_bitrate = eff_bitrate;
        let current_gop = eff_gop;
        let config = EncoderConfig::new(first.width, first.height)
            .with_fps(args.fps)
            .with_bitrate(current_bitrate)
            .with_gop(current_gop)
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
        println!("stream_bitrate={current_bitrate} stream_gop={current_gop}");
        let mut rate = RateController::new(current_bitrate);

        let mut sender = FrameSender {
            socket: &mut video_stream,
            packetizer: Packetizer::new(DEFAULT_MAX_PACKET_PAYLOAD),
            codec: args.codec,
            width: first.width as u16,
            height: first.height as u16,
            fps: args.fps,
            frame_sequence: 1,
            total_packets: 0,
            total_frames: 0,
            write_stall_ms_max: 0.0,
            last_stall_ms: 0.0,
        };
        let mut total_input_frames = 0usize;
        let max_input_frames = args.max_frames.unwrap_or(usize::MAX);
        // Live capture size; updated in place when Windows changes the mode.
        let mut current_size = (first.width, first.height);

        if args.live {
            let playback_start = Instant::now();
            let mut last_streamed: Option<PathBuf> = None;
            loop {
                if total_input_frames >= max_input_frames {
                    break;
                }
                frames = collect_frames(&args.input_dir)?;
                let newest = match frames.last() {
                    Some(p) => p.clone(),
                    None => {
                        if args.loop_forever {
                            thread::sleep(Duration::from_millis(4));
                            continue;
                        }
                        break;
                    }
                };

                if last_streamed.as_ref() == Some(&newest) {
                    if args.loop_forever {
                        thread::sleep(Duration::from_millis(4));
                        continue;
                    }
                    break;
                }

                for stale in frames.iter().take(frames.len().saturating_sub(1)) {
                    let _ = fs::remove_file(stale);
                }

                let frame = match read_bgra_bmp(&fs::read(&newest)?) {
                    Ok(f) => f,
                    Err(_) => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                };
                if (frame.width, frame.height) != current_size {
                    apply_size_change(
                        &mut sender,
                        &mut encoder,
                        current_size,
                        (frame.width, frame.height),
                        args.fps,
                        current_bitrate,
                        current_gop,
                        args.codec,
                    )?;
                    current_size = (frame.width, frame.height);
                }

                let ts = playback_start.elapsed().as_nanos() as u64;
                total_input_frames += 1;
                let units = encoder.encode(&frame, ts)?;
                for unit in units {
                    sender.send_unit(&unit)?;
                    maybe_adapt(
                        &mut rate,
                        sender.last_stall_ms(),
                        kf_requests.load(Ordering::Relaxed),
                        &mut current_bitrate,
                        current_size.0,
                        current_size.1,
                        args.fps,
                        current_gop,
                        args.codec,
                        &mut encoder,
                    );
                }
                if args.stats_json && sender.total_frames % STATS_FRAME_INTERVAL == 0 {
                    emit_stats_json(&sender, &input_injected);
                }
                last_streamed = Some(newest);
            }
        } else {
            let frame_dur_ns = 1_000_000_000u64 / (args.fps.max(1) as u64);
            let mut source_timestamp_ns = 0u64;
            let playback_start = Instant::now();
            let mut first_pts_ns = None::<u64>;
            let mut next_frame_index = 0usize;

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
                    if (frame.width, frame.height) != current_size {
                        // A size change mid-replay (mixed capture set). Rebuild
                        // the encoder rather than aborting the run.
                        apply_size_change(
                            &mut sender,
                            &mut encoder,
                            current_size,
                            (frame.width, frame.height),
                            args.fps,
                            current_bitrate,
                            current_gop,
                            args.codec,
                        )?;
                        current_size = (frame.width, frame.height);
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
                        sender.send_unit(&unit)?;
                        maybe_adapt(
                            &mut rate,
                            sender.last_stall_ms(),
                            kf_requests.load(Ordering::Relaxed),
                            &mut current_bitrate,
                            current_size.0,
                            current_size.1,
                            args.fps,
                            current_gop,
                            args.codec,
                            &mut encoder,
                        );
                    }
                    if args.stats_json && sender.total_frames % STATS_FRAME_INTERVAL == 0 {
                        emit_stats_json(&sender, &input_injected);
                    }
                    next_frame_index += 1;
                }
                if total_input_frames >= max_input_frames {
                    break;
                }
            }
        }

        for unit in encoder.drain()? {
            sender.send_unit(&unit)?;
        }

        // Snapshot before the sender's borrow of the socket ends below.
        let final_stats = sender.stats_line(input_injected.load(Ordering::Relaxed));
        println!(
            "streamed_input_frames={} streamed_frames={} streamed_packets={}",
            total_input_frames, sender.total_frames, sender.total_packets
        );

        input_running.store(false, Ordering::Relaxed);
        video_stream.shutdown();
        if let Some(handle) = input_reader {
            let _ = handle.join();
        }
        println!(
            "input_events_injected={}",
            input_injected.load(Ordering::Relaxed)
        );
        if args.stats_json {
            println!("{}", final_stats);
        }
        println!("stream_complete=true");
        return Ok(());
    }

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
    let plain_socket = connect_with_retry(args.port)?;
    plain_socket
        .set_nodelay(true)
        .context("failed to set TCP_NODELAY")?;
    println!("android_connection=established");

    let mut video_stream = VideoStream::Plain(plain_socket);

    // Spawn the reverse input channel on a clone of the socket. The video path
    // below keeps ownership of `socket` for writes.
    let input_running = Arc::new(AtomicBool::new(true));
    let input_injected = Arc::new(AtomicU64::new(0));
    let kf_requests = Arc::new(AtomicU64::new(0));
    let input_reader = match video_stream.try_clone() {
        Ok(reader_socket) => {
            let running = Arc::clone(&input_running);
            let injected = Arc::clone(&input_injected);
            let kf = Arc::clone(&kf_requests);
            println!("input_return_channel=enabled");
            Some(thread::spawn(move || {
                run_input_reader(reader_socket, running, injected, kf)
            }))
        }
        Err(err) => {
            println!("input_return_channel=disabled reason=\"clone failed: {err}\"");
            None
        }
    };

    // USB keeps user bitrate/GOP verbatim; effective helper is identity here.
    let (eff_bitrate, eff_gop) = effective_wifi_tuning(args.transport, args.bitrate_bps, args.gop);
    let mut current_bitrate = eff_bitrate;
    let current_gop = eff_gop;
    // Rebuild config with effective values (USB: unchanged) so logs match WiFi.
    let mut rate = RateController::new(current_bitrate);
    let _ = current_gop;

    let mut sender = FrameSender {
        socket: &mut video_stream,
        packetizer: Packetizer::new(DEFAULT_MAX_PACKET_PAYLOAD),
        codec: args.codec,
        width: first.width as u16,
        height: first.height as u16,
        fps: args.fps,
        frame_sequence: 1,
        total_packets: 0,
        total_frames: 0,
        write_stall_ms_max: 0.0,
        last_stall_ms: 0.0,
    };
    let mut total_input_frames = 0usize;
    let max_input_frames = args.max_frames.unwrap_or(usize::MAX);
    // Live capture size; updated in place when Windows changes the mode.
    let mut current_size = (first.width, first.height);

    if args.live {
        // Live second-monitor mode: latency beats completeness. Each tick we
        // take only the NEWEST capture, delete the stale backlog, and time it
        // with the real wall clock so the client's pacer presents it promptly.
        let playback_start = Instant::now();
        let mut last_streamed: Option<PathBuf> = None;
        loop {
            if total_input_frames >= max_input_frames {
                break;
            }
            frames = collect_frames(&args.input_dir)?;
            let newest = match frames.last() {
                Some(p) => p.clone(),
                None => {
                    if args.loop_forever {
                        thread::sleep(Duration::from_millis(4));
                        continue;
                    }
                    break;
                }
            };

            // Nothing new since last tick: wait briefly rather than re-encode
            // an identical frame.
            if last_streamed.as_ref() == Some(&newest) {
                if args.loop_forever {
                    thread::sleep(Duration::from_millis(4));
                    continue;
                }
                break;
            }

            // Drop + delete every capture older than the newest so disk stays
            // bounded and we never fall behind streaming stale frames.
            for stale in frames.iter().take(frames.len().saturating_sub(1)) {
                let _ = fs::remove_file(stale);
            }

            let frame = match read_bgra_bmp(&fs::read(&newest)?) {
                Ok(f) => f,
                // A half-written BMP (driver mid-dump) fails to parse; skip and
                // retry next tick.
                Err(_) => {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                }
            };
            if (frame.width, frame.height) != current_size {
                // Resolution changed under us (Windows mode switch). Rebuild the
                // encoder at the new size and keep going; Android recreates its
                // MediaCodec on a WxH change.
                apply_size_change(
                    &mut sender,
                    &mut encoder,
                    current_size,
                    (frame.width, frame.height),
                    args.fps,
                    current_bitrate,
                    current_gop,
                    args.codec,
                )?;
                current_size = (frame.width, frame.height);
            }

            let ts = playback_start.elapsed().as_nanos() as u64;
            total_input_frames += 1;
            let units = encoder.encode(&frame, ts)?;
            for unit in units {
                sender.send_unit(&unit)?;
                maybe_adapt(
                    &mut rate,
                    sender.last_stall_ms(),
                    kf_requests.load(Ordering::Relaxed),
                    &mut current_bitrate,
                    current_size.0,
                    current_size.1,
                    args.fps,
                    current_gop,
                    args.codec,
                    &mut encoder,
                );
            }
            if args.stats_json && sender.total_frames % STATS_FRAME_INTERVAL == 0 {
                emit_stats_json(&sender, &input_injected);
            }
            last_streamed = Some(newest);
        }
    } else {
        // Replay mode: stream every captured frame in order at a fixed fps.
        let frame_dur_ns = 1_000_000_000u64 / (args.fps.max(1) as u64);
        let mut source_timestamp_ns = 0u64;
        let playback_start = Instant::now();
        let mut first_pts_ns = None::<u64>;
        let mut next_frame_index = 0usize;

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
                if (frame.width, frame.height) != current_size {
                    // A size change mid-replay (mixed capture set). Rebuild the
                    // encoder rather than aborting the run.
                    apply_size_change(
                        &mut sender,
                        &mut encoder,
                        current_size,
                        (frame.width, frame.height),
                        args.fps,
                        current_bitrate,
                        current_gop,
                        args.codec,
                    )?;
                    current_size = (frame.width, frame.height);
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
                    sender.send_unit(&unit)?;
                    maybe_adapt(
                        &mut rate,
                        sender.last_stall_ms(),
                        kf_requests.load(Ordering::Relaxed),
                        &mut current_bitrate,
                        current_size.0,
                        current_size.1,
                        args.fps,
                        current_gop,
                        args.codec,
                        &mut encoder,
                    );
                }
                if args.stats_json && sender.total_frames % STATS_FRAME_INTERVAL == 0 {
                    emit_stats_json(&sender, &input_injected);
                }
                next_frame_index += 1;
            }
            if total_input_frames >= max_input_frames {
                break;
            }
        }
    }

    for unit in encoder.drain()? {
        sender.send_unit(&unit)?;
    }

    println!(
        "streamed_input_frames={} streamed_frames={} streamed_packets={}",
        total_input_frames, sender.total_frames, sender.total_packets
    );

    // Stop the reverse input channel and drain the thread. Dropping the write
    // half and clearing the flag unblocks the reader's blocking read.
    input_running.store(false, Ordering::Relaxed);
    video_stream.shutdown();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wifi_defaults_apply_unless_overridden() {
        let (b, g) =
            effective_wifi_tuning(Transport::Wifi, USB_DEFAULT_BITRATE_BPS, USB_DEFAULT_GOP);
        assert_eq!(b, WIFI_DEFAULT_BITRATE_BPS);
        assert_eq!(g, WIFI_DEFAULT_GOP);
        let (b2, g2) = effective_wifi_tuning(Transport::Wifi, 8_000_000, 30);
        assert_eq!(b2, 8_000_000);
        assert_eq!(g2, 30);
        let (b3, g3) =
            effective_wifi_tuning(Transport::Usb, USB_DEFAULT_BITRATE_BPS, USB_DEFAULT_GOP);
        assert_eq!(b3, USB_DEFAULT_BITRATE_BPS);
        assert_eq!(g3, USB_DEFAULT_GOP);
    }

    #[test]
    fn rate_controller_starts_at_given_bitrate() {
        let r = RateController::new(12_000_000);
        assert_eq!(r.current_bitrate(), 12_000_000);
        let r2 = RateController::new(20_000_000);
        assert_eq!(r2.current_bitrate(), 20_000_000);
    }

    #[test]
    fn rate_controller_steps_down_on_stall() {
        let mut r = RateController::new(20_000_000);
        let mut stepped: Option<u32> = None;
        for _ in 0..60 {
            stepped = r.observe(80.0, 0);
        }
        assert_eq!(stepped, Some(12_000_000));
        assert_eq!(r.current_bitrate(), 12_000_000);
    }

    #[test]
    fn rate_controller_steps_down_on_keyframe_storm() {
        let mut r = RateController::new(12_000_000);
        let mut seen: Option<u32> = None;
        for i in 1..=5u64 {
            if let Some(b) = r.observe(5.0, i) {
                seen = Some(b);
            }
        }
        assert_eq!(seen, Some(8_000_000));
    }

    #[test]
    fn rate_controller_steps_up_after_clean_window() {
        let mut r = RateController::new(12_000_000);
        for _ in 0..60 {
            let _ = r.observe(80.0, 0);
        }
        // 12M -> 8M on sustained stall.
        assert_eq!(r.current_bitrate(), 8_000_000);
        let mut up: Option<u32> = None;
        for _ in 0..600 {
            up = r.observe(5.0, 0);
            if up.is_some() {
                break;
            }
        }
        assert_eq!(up, Some(12_000_000));
    }

    #[test]
    fn frame_sender_tracks_stall_and_stats_json() {
        let mut sender = FrameSender {
            socket: Vec::new(),
            packetizer: Packetizer::new(1024),
            codec: Codec::H264,
            width: 64,
            height: 64,
            fps: 60,
            frame_sequence: 1,
            total_packets: 0,
            total_frames: 0,
            write_stall_ms_max: 0.0,
            last_stall_ms: 0.0,
        };
        let unit = usbdisplay_encoder::EncodedUnit {
            bytes: vec![0u8; 64],
            keyframe: true,
            timestamp_ns: 0,
        };
        sender.send_unit(&unit).unwrap();
        assert_eq!(sender.total_frames, 1);
        assert!(sender.total_packets >= 1);
        let line = sender.stats_line(7);
        assert!(line.contains("\"streamed_frames\":1"), "{line}");
        assert!(line.contains("\"input_events_injected\":7"), "{line}");
        assert!(line.contains("write_stall_ms_max"), "{line}");
    }
}
