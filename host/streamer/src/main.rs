mod adb;
mod encode_capture;
mod frame_section;
mod input_inject;
mod pairing;
mod stream_android;
mod wifi;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use usbdisplay_encoder::Codec as EncoderCodec;
use usbdisplay_protocol::{Codec, EncodedFrame, FrameFlags};
use usbdisplay_transport::{Packetizer, DEFAULT_MAX_PACKET_PAYLOAD};

/// Transport used by `stream-capture`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum TransportKind {
    /// ADB-forwarded USB (current behavior, default).
    Usb,
    /// WiFi LAN (PR-1: flag only, errors until PR-2 lands).
    Wifi,
}

#[derive(Debug, Parser)]
#[command(name = "usbdisplay-streamer")]
#[command(about = "USBDisplay Windows host service")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List Android devices visible over USB through adb.
    Devices,
    /// Print the negotiated stream protocol capabilities.
    Capabilities,
    /// Encode a synthetic protocol frame for transport testing.
    ProbeFrame {
        #[arg(long, default_value_t = 1920)]
        width: u16,
        #[arg(long, default_value_t = 1080)]
        height: u16,
        #[arg(long, default_value_t = 60_000)]
        refresh_millihz: u32,
    },
    /// Packetize a synthetic frame using the reliable transport layer.
    TransportProbe {
        #[arg(long, default_value_t = DEFAULT_MAX_PACKET_PAYLOAD)]
        max_packet_payload: usize,
    },
    /// Encode the driver's captured frames with a hardware encoder and validate.
    EncodeCapture {
        /// Directory of capture_*.bmp frames (default: %ProgramData%\USBDisplay\capture).
        #[arg(long)]
        input_dir: Option<PathBuf>,
        /// Output elementary-stream path.
        #[arg(long, default_value = "capture.h264")]
        out: PathBuf,
        /// Codec: h264 or h265.
        #[arg(long, default_value = "h264")]
        codec: String,
        #[arg(long, default_value_t = 20_000_000)]
        bitrate: u32,
        #[arg(long, default_value_t = 60)]
        fps: u32,
        #[arg(long, default_value_t = 60)]
        gop: u32,
        /// Cap the number of frames encoded (for quick checks).
        #[arg(long)]
        max_frames: Option<usize>,
        /// After encoding, decode the stream back to prove it is decodable.
        #[arg(long, default_value_t = false)]
        verify_decode: bool,
    },
    /// Encode captured frames and stream them to Android over ADB-forwarded TCP.
    StreamCapture {
        /// Directory of capture_*.bmp frames (default: %ProgramData%\\USBDisplay\\capture).
        #[arg(long)]
        input_dir: Option<PathBuf>,
        /// Codec: h264 or h265.
        #[arg(long, default_value = "h264")]
        codec: String,
        #[arg(long, default_value_t = 20_000_000)]
        bitrate: u32,
        #[arg(long, default_value_t = 60)]
        fps: u32,
        #[arg(long, default_value_t = 60)]
        gop: u32,
        /// TCP port used by adb forward + Android listener.
        #[arg(long, default_value_t = 27183)]
        port: u16,
        /// Optional adb serial. If omitted, the first connected device is used.
        #[arg(long)]
        serial: Option<String>,
        /// Cap the number of frames streamed.
        #[arg(long)]
        max_frames: Option<usize>,
        /// Replay the frame set continuously until interrupted.
        #[arg(long, default_value_t = false)]
        r#loop: bool,
        /// Live second-monitor mode (default): always stream the newest capture
        /// and drop the stale backlog for minimal latency. Use --no-live for
        /// ordered fixed-fps file replay.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        live: bool,
        /// Transport: usb (ADB-forwarded, default) or wifi (LAN, PR-1 errors).
        #[arg(long, value_enum, default_value_t = TransportKind::Usb)]
        transport: TransportKind,
        /// WiFi tablet address: bare IP, ip:port, or QR JSON payload.
        /// Only used with --transport wifi.
        #[arg(long)]
        device_ip: Option<String>,
        /// WiFi pairing PIN shown on the tablet. Only used with --transport wifi.
        /// May be omitted on reconnect from an already-trusted host.
        #[arg(long)]
        pin: Option<String>,
        /// Print streaming stats as JSON every 60 frames.
        #[arg(long, default_value_t = false)]
        stats_json: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Devices => {
            let devices = adb::list_devices()?;
            if devices.is_empty() {
                println!("no_android_devices=true");
            }
            for device in devices {
                println!(
                    "serial={} state={:?} model={} product={} transport_id={}",
                    device.serial,
                    device.state,
                    device.model.unwrap_or_else(|| "unknown".to_string()),
                    device.product.unwrap_or_else(|| "unknown".to_string()),
                    device.transport_id.unwrap_or_else(|| "unknown".to_string())
                );
            }
        }
        Command::Capabilities => {
            // Advertise only what the host actually implements today. The wire
            // protocol reserves AV1 and the driver reserves native USB bulk /
            // HID input as future work, but none are implemented, so they are
            // deliberately not listed here (see docs/review-findings.md 7.2).
            println!("codecs=h264,h265");
            println!("transport=adb-compat,wifi-tls");
            println!("capture=virtual-monitor-only");
            println!("input=mouse,keyboard");
        }
        Command::ProbeFrame {
            width,
            height,
            refresh_millihz,
        } => {
            let frame = EncodedFrame::new(
                1,
                0,
                Codec::H265,
                FrameFlags::KEYFRAME,
                width,
                height,
                refresh_millihz,
                b"usbdisplay-probe".to_vec(),
            )?;
            let bytes = frame.encode();
            println!("probe_frame_bytes={}", bytes.len());
            println!("payload_crc32={:#010x}", frame.header.payload_crc32);
        }
        Command::TransportProbe { max_packet_payload } => {
            let payload = vec![0x5a; 250_000];
            let frame = EncodedFrame::new(
                1,
                0,
                Codec::H265,
                FrameFlags::KEYFRAME,
                2560,
                1600,
                120_000,
                payload,
            )?;
            let mut packetizer = Packetizer::new(max_packet_payload);
            let packets = packetizer.packetize_frame(&frame)?;
            let total_bytes: usize = packets.iter().map(|packet| packet.encode().len()).sum();
            println!("transport_packets={}", packets.len());
            println!("transport_bytes={total_bytes}");
            println!("max_packet_payload={max_packet_payload}");
        }
        Command::EncodeCapture {
            input_dir,
            out,
            codec,
            bitrate,
            fps,
            gop,
            max_frames,
            verify_decode,
        } => {
            let codec = match codec.to_ascii_lowercase().as_str() {
                "h264" | "avc" => EncoderCodec::H264,
                "h265" | "hevc" => EncoderCodec::H265,
                other => anyhow::bail!("unknown codec '{other}' (use h264 or h265)"),
            };
            let input_dir = input_dir.unwrap_or_else(default_capture_dir);
            encode_capture::run(encode_capture::EncodeCaptureArgs {
                input_dir,
                out,
                codec,
                bitrate_bps: bitrate,
                fps,
                gop,
                max_frames,
                verify_decode,
            })?;
        }
        Command::StreamCapture {
            input_dir,
            codec,
            bitrate,
            fps,
            gop,
            port,
            serial,
            max_frames,
            r#loop,
            live,
            transport,
            device_ip,
            pin,
            stats_json,
        } => {
            let codec = match codec.to_ascii_lowercase().as_str() {
                "h264" | "avc" => EncoderCodec::H264,
                "h265" | "hevc" => EncoderCodec::H265,
                other => anyhow::bail!("unknown codec '{other}' (use h264 or h265)"),
            };
            let input_dir = input_dir.unwrap_or_else(default_capture_dir);
            let transport = match transport {
                TransportKind::Usb => stream_android::Transport::Usb,
                TransportKind::Wifi => stream_android::Transport::Wifi,
            };
            stream_android::run(stream_android::StreamCaptureArgs {
                input_dir,
                codec,
                bitrate_bps: bitrate,
                fps,
                gop,
                port,
                serial,
                max_frames,
                loop_forever: r#loop,
                live,
                transport,
                device_ip,
                pin,
                stats_json,
            })?;
        }
    }

    Ok(())
}

/// %ProgramData%\USBDisplay\capture, matching the driver's dump location.
fn default_capture_dir() -> std::path::PathBuf {
    let base = std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".to_string());
    std::path::PathBuf::from(base)
        .join("USBDisplay")
        .join("capture")
}
