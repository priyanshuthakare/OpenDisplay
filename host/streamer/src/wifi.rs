//! WiFi transport implementation.
//!
//! PR-2 implements plaintext LAN (gated behind --insecure-lan).
//! PR-3 adds PIN + TLS 1.3.

use std::io::Read;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;
use anyhow::{bail, Context, Result};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, RootCertStore, SignatureScheme};
use sha2::{Digest, Sha256};
use usbdisplay_transport::TransportPacket;
use crate::pairing::{PairingStore, store_path};

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

/// PR-2 entry point: connect to the tablet over LAN (plaintext).
/// Returns a TcpStream for the video socket.
pub fn connect_plain(device: &WifiDevice) -> Result<TcpStream> {
    let addr = resolve_addr(&device.addr())?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .with_context(|| format!("connecting to wifi device {}", device.addr()))?;
    stream.set_nodelay(true)
        .context("failed to set TCP_NODELAY")?;
    Ok(stream)
}

/// PR-3 entry point: connect to the tablet over LAN with TLS.
pub fn connect_tls(device: &WifiDevice, pin: Option<&str>) -> Result<TlsConnection> {
    let addr = resolve_addr(&device.addr())?;

    // Load or create pairing store
    let mut store = PairingStore::load().context("Failed to load pairing store")?;

    // Build TLS config with TOFU fingerprint verifier
    let tablet_id = device.addr();
    let stored_fingerprint = store.fingerprint_for(&tablet_id);
    let qr_fingerprint = device.fingerprint.as_deref();

    let config = build_tls_config(stored_fingerprint, qr_fingerprint)?;

    // Connect with TLS
    let connector = rustls::ClientConnection::new(Arc::new(config), ServerName::try_from(device.ip.as_str())?)
        .context("Failed to create TLS connection")?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .context("Failed to connect to tablet")?;
    stream.set_nodelay(true)
        .context("Failed to set TCP_NODELAY")?;

    let mut tls_stream = rustls::StreamOwned::new(connector, stream);

    // Perform handshake: send Hello, receive Welcome
    perform_handshake(&mut tls_stream, pin, &tablet_id, &mut store)?;

    Ok(TlsConnection {
        stream: tls_stream,
        store_updated: true,
    })
}

/// TLS connection wrapper.
pub struct TlsConnection {
    pub stream: rustls::StreamOwned<rustls::ClientConnection, TcpStream>,
    pub store_updated: bool,
}

fn build_tls_config(
    stored_fingerprint: Option<&String>,
    qr_fingerprint: Option<&str>,
) -> Result<ClientConfig> {
    let mut root_store = RootCertStore::empty();
    // Add system certificates
    root_store.add_parsable_certificates(rustls_native_certs::load_native_certs()?);

    let verifier = Arc::new(TofuVerifier {
        stored_fingerprint: stored_fingerprint.cloned(),
        qr_fingerprint: qr_fingerprint.map(|s| s.to_string()),
    });

    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();

    Ok(config)
}

#[derive(Debug)]
struct TofuVerifier {
    stored_fingerprint: Option<String>,
    qr_fingerprint: Option<String>,
}

impl rustls::client::danger::ServerCertVerifier for TofuVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        // Extract certificate fingerprint
        let cert = _end_entity;
        let fingerprint = certificate_fingerprint(cert.as_ref());

        // Check against stored fingerprint or QR fingerprint
        let accepted = match (&self.stored_fingerprint, &self.qr_fingerprint) {
            (Some(stored), _) => fingerprint == *stored,
            (None, Some(qr)) => fingerprint == *qr,
            (None, None) => {
                // New pairing: accept and fingerprint will be stored after handshake
                true
            }
        };

        if accepted {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::NotValidForName,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ED25519,
        ]
    }
}

fn certificate_fingerprint(cert_der: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(cert_der);
    let result = hasher.finalize();
    format!("SHA256:{}", hex::encode(&result[..16])) // First 16 hex chars for display
}

fn perform_handshake(
    tls_stream: &mut rustls::StreamOwned<rustls::ClientConnection, TcpStream>,
    pin: Option<&str>,
    tablet_id: &str,
    store: &mut PairingStore,
) -> Result<()> {
    use usbdisplay_transport::{TransportPacket, PacketKind};
    use crate::stream_android::write_framed_packet;

    // Generate host ID
    let host_id = format!("host-{}", std::process::id());

    // Build Hello handshake
    let hello_payload = serde_json::json!({
        "v": 1,
        "pin": pin.unwrap_or(""),
        "host_id": host_id,
        "codecs": ["h264", "h265"]
    }).to_string();

    let hello_packet = TransportPacket::handshake(1, hello_payload.into_bytes());
    write_framed_packet(tls_stream, &hello_packet)?;

    // Read Welcome response
    let welcome_packet = read_framed_packet(tls_stream)?;
    if welcome_packet.header.kind != PacketKind::Handshake {
        bail!("Expected Handshake packet, got {:?}", welcome_packet.header.kind);
    }

    let welcome_json: serde_json::Value = serde_json::from_slice(&welcome_packet.payload)
        .context("Failed to parse Welcome JSON")?;

    if welcome_json["accept"].as_bool() != Some(true) {
        bail!("Handshake rejected by tablet: {}", welcome_json);
    }

    // Store pairing if new
    if !store.is_paired(tablet_id) {
        // Extract fingerprint from the certificate we verified
        // (In a real implementation, we'd get this from the TLS session)
        // For now, use the QR fingerprint if available
        if let Some(qr_fp) = welcome_json["fp"].as_str() {
            store.remember(tablet_id, qr_fp);
            store.save(store_path()?)?;
        }
    }

    Ok(())
}

fn read_framed_packet(
    tls_stream: &mut rustls::StreamOwned<rustls::ClientConnection, TcpStream>,
) -> Result<TransportPacket> {
    let mut len_bytes = [0u8; 4];
    tls_stream.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    let mut packet_bytes = vec![0u8; len];
    tls_stream.read_exact(&mut packet_bytes)?;
    TransportPacket::decode(&packet_bytes, 64 * 1024)
        .map_err(|e| anyhow::anyhow!("Failed to decode packet: {}", e))
}

fn resolve_addr(addr: &str) -> Result<SocketAddr> {
    addr.to_socket_addrs()
        .with_context(|| format!("resolving wifi device address '{}'", addr))?
        .next()
        .ok_or_else(|| anyhow::anyhow!("no address resolved for '{}'", addr))
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

    #[test]
    fn connect_plain_resolves_and_connects() {
        let device = parse_device("127.0.0.1:27184").unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:27184").unwrap();
        let stream = connect_plain(&device).unwrap();
        assert_eq!(stream.peer_addr().unwrap().port(), 27184);
        assert!(listener.accept().is_ok());
    }
}
