//! WiFi transport scaffolding (PR-1: not yet connected).
//!
//! PR-2 will implement the plaintext LAN path, PR-3 adds PIN + TLS.
//! This module intentionally contains only the argument shape and QR
//! payload parsing so the `--transport wifi` flag can be plumbed
//! without changing USB behavior.

use anyhow::{bail, Result};

/// Default TCP port the Android app will listen on for WiFi (LAN).
/// USB keeps 27183 (adb-forwarded loopback); WiFi uses 27184 so both
/// listeners can run side by side during development.
pub const DEFAULT_WIFI_PORT: u16 = 27184;

/// Parsed `usbdisplay-wifi://` QR / manual payload.
///
/// QR JSON shape (v1): `{"v":1,"ip":"192.168.1.42","port":27184,"fp":"SHA256:…"}`
/// The PIN itself is entered separately on the host and never trusted
/// from the QR alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiDevice {
    pub ip: String,
    pub port: u16,
    pub fingerprint: Option<String>,
}

impl WifiDevice {
    pub fn addr(&self) -> String {
        format!("{}:{}", self.ip, self.port)
    }
}

/// Parse a minimal QR payload. Accepts either `ip`, `ip:port`, or the
/// JSON form above. Full validation (TLS fingerprint pinning) lands in PR-3.
pub fn parse_device(s: &str) -> Result<WifiDevice> {
    let s = s.trim();
    if s.is_empty() {
        bail!("empty wifi device address");
    }
    if s.starts_with('{') {
        return parse_json_device(s);
    }
    if let Some((ip, port_s)) = s.rsplit_once(':') {
        let port: u16 = port_s
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid port in '{s}'"))?;
        if ip.is_empty() {
            bail!("invalid wifi device address '{s}'");
        }
        return Ok(WifiDevice {
            ip: ip.to_string(),
            port,
            fingerprint: None,
        });
    }
    Ok(WifiDevice {
        ip: s.to_string(),
        port: DEFAULT_WIFI_PORT,
        fingerprint: None,
    })
}

fn parse_json_device(s: &str) -> Result<WifiDevice> {
    // Minimal hand-rolled extraction to avoid a serde dependency in PR-1.
    // PR-2 will replace this with a proper serde struct.
    let ip = extract_json_string(s, "\"ip\"")
        .ok_or_else(|| anyhow::anyhow!("wifi QR payload missing \"ip\""))?;
    let port = extract_json_u16(s, "\"port\"").unwrap_or(DEFAULT_WIFI_PORT);
    let fingerprint = extract_json_string(s, "\"fp\"");
    Ok(WifiDevice {
        ip,
        port,
        fingerprint,
    })
}

fn extract_json_string(s: &str, key: &str) -> Option<String> {
    let key_pos = s.find(key)?;
    let after = &s[key_pos + key.len()..];
    let colon = after.find(':')?;
    let value = after[colon + 1..].trim_start();
    if !value.starts_with('"') {
        return None;
    }
    let end = value[1..].find('"')?;
    Some(value[1..1 + end].to_string())
}

fn extract_json_u16(s: &str, key: &str) -> Option<u16> {
    let key_pos = s.find(key)?;
    let after = &s[key_pos + key.len()..];
    let colon = after.find(':')?;
    let value = after[colon + 1..].trim_start();
    let digits: String = value.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// PR-2 entry point: connect to the tablet over LAN.
/// Currently always errors so `--transport wifi` fails loudly instead of
/// silently falling back to USB.
pub fn connect(_device: &WifiDevice, _pin: Option<&str>) -> Result<()> {
    bail!(
        "wifi transport not yet implemented (PR-2 plaintext LAN, PR-3 PIN+TLS). \
         Use --transport usb for now."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_ip_with_default_port() {
        let d = parse_device("192.168.1.42").unwrap();
        assert_eq!(d.ip, "192.168.1.42");
        assert_eq!(d.port, DEFAULT_WIFI_PORT);
        assert_eq!(d.fingerprint, None);
    }

    #[test]
    fn parses_ip_colon_port() {
        let d = parse_device("192.168.1.42:27184").unwrap();
        assert_eq!(d.addr(), "192.168.1.42:27184");
    }

    #[test]
    fn parses_json_qr_payload() {
        let d =
            parse_device(r#"{"v":1,"ip":"192.168.1.42","port":27184,"fp":"SHA256:abcd"}"#).unwrap();
        assert_eq!(d.ip, "192.168.1.42");
        assert_eq!(d.port, 27184);
        assert_eq!(d.fingerprint.as_deref(), Some("SHA256:abcd"));
    }

    #[test]
    fn rejects_empty_address() {
        assert!(parse_device("  ").is_err());
    }
}
