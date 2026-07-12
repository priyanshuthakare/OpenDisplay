mod adb;

use anyhow::Result;
use clap::{Parser, Subcommand};
use usbdisplay_protocol::{Codec, EncodedFrame, FrameFlags};
use usbdisplay_transport::{Packetizer, DEFAULT_MAX_PACKET_PAYLOAD};

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
            println!("codecs=h264,h265,av1");
            println!("transport=adb-compat,native-usb-bulk");
            println!("capture=virtual-monitor-only");
            println!("input=hid-touch,hid-pen,keyboard,mouse");
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
    }

    Ok(())
}
