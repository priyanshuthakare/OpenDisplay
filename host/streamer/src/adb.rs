use anyhow::{Context, Result};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdbDeviceState {
    Device,
    Unauthorized,
    Offline,
    Recovery,
    Sideload,
    Bootloader,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbDevice {
    pub serial: String,
    pub state: AdbDeviceState,
    pub product: Option<String>,
    pub model: Option<String>,
    pub device: Option<String>,
    pub transport_id: Option<String>,
}

pub fn list_devices() -> Result<Vec<AdbDevice>> {
    let output = Command::new("adb")
        .args(["devices", "-l"])
        .output()
        .context(
        "failed to run adb devices -l; install Android platform-tools and ensure adb is on PATH",
    )?;

    let stdout = String::from_utf8(output.stdout).context("adb output was not valid UTF-8")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("adb devices -l failed: {stderr}");
    }

    parse_adb_devices(&stdout)
}

pub fn parse_adb_devices(output: &str) -> Result<Vec<AdbDevice>> {
    let mut devices = Vec::new();

    for line in output.lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let mut parts = trimmed.split_whitespace();
        let serial = parts
            .next()
            .context("adb device line is missing serial")?
            .to_string();
        let state = parts
            .next()
            .map(parse_state)
            .context("adb device line is missing state")?;

        let mut product = None;
        let mut model = None;
        let mut device = None;
        let mut transport_id = None;

        for part in parts {
            if let Some(value) = part.strip_prefix("product:") {
                product = Some(value.to_string());
            } else if let Some(value) = part.strip_prefix("model:") {
                model = Some(value.to_string());
            } else if let Some(value) = part.strip_prefix("device:") {
                device = Some(value.to_string());
            } else if let Some(value) = part.strip_prefix("transport_id:") {
                transport_id = Some(value.to_string());
            }
        }

        devices.push(AdbDevice {
            serial,
            state,
            product,
            model,
            device,
            transport_id,
        });
    }

    Ok(devices)
}

fn parse_state(value: &str) -> AdbDeviceState {
    match value {
        "device" => AdbDeviceState::Device,
        "unauthorized" => AdbDeviceState::Unauthorized,
        "offline" => AdbDeviceState::Offline,
        "recovery" => AdbDeviceState::Recovery,
        "sideload" => AdbDeviceState::Sideload,
        "bootloader" => AdbDeviceState::Bootloader,
        other => AdbDeviceState::Unknown(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_connected_devices() {
        let output = "\
List of devices attached
R52T70ABCDE device product:gts8 model:Galaxy_Tab_S8 device:gts8 transport_id:3
emulator-5554 offline transport_id:1
";

        let devices = parse_adb_devices(output).unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].serial, "R52T70ABCDE");
        assert_eq!(devices[0].state, AdbDeviceState::Device);
        assert_eq!(devices[0].model.as_deref(), Some("Galaxy_Tab_S8"));
        assert_eq!(devices[1].state, AdbDeviceState::Offline);
    }

    #[test]
    fn parses_unauthorized_device() {
        let output = "\
List of devices attached
R52T70ABCDE unauthorized transport_id:7
";

        let devices = parse_adb_devices(output).unwrap();
        assert_eq!(devices[0].state, AdbDeviceState::Unauthorized);
        assert_eq!(devices[0].transport_id.as_deref(), Some("7"));
    }
}
