//! WiFi transport implementation.
//!
//! PR-2 landed plaintext LAN (gated behind --insecure-lan).
//! PR-3 replaces it with PIN + TLS 1.3 only.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, RootCertStore, SignatureScheme};
use sha2::{Digest, Sha256};

use crate::pairing::PairingStore;

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
/// JSON form above.
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
/// Kept for tests; the live WiFi path in PR-3+ always uses TLS.
#[allow(dead_code)]
pub fn connect_plain(device: &WifiDevice) -> Result<TcpStream> {
    let addr = resolve_addr(&device.addr())?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).with_context(|| {
        format!(
            "host unreachable at {} (AP isolation? same LAN? tablet listener on 27184?) — use USB (--transport usb) instead",
            device.addr()
        )
    })?;
    stream
        .set_nodelay(true)
        .context("failed to set TCP_NODELAY")?;
    Ok(stream)
}

/// Build an owned TLS server name for an IP literal or DNS name.
fn server_name_for_host(ip: &str) -> Result<ServerName<'static>> {
    if let Ok(addr) = IpAddr::from_str(ip) {
        Ok(ServerName::IpAddress(addr.into()))
    } else {
        ServerName::try_from(ip.to_owned())
            .map_err(|e| anyhow::anyhow!("invalid wifi host name '{ip}': {e}"))
    }
}

/// PR-3 entry point: connect to the tablet over LAN with TLS 1.3 + PIN.
pub fn connect_tls(device: &WifiDevice, pin: Option<&str>) -> Result<TlsConnection> {
    let mut store = PairingStore::load().context("Failed to load pairing store")?;
    connect_tls_with_store(device, pin, &mut store)
}

/// Same as [`connect_tls`] but against a caller-provided pairing store.
/// Used by tests to avoid touching `%AppData%`.
pub fn connect_tls_with_store(
    device: &WifiDevice,
    pin: Option<&str>,
    store: &mut PairingStore,
) -> Result<TlsConnection> {
    let addr = resolve_addr(&device.addr())?;

    let tablet_id = device.addr();
    let stored_fingerprint = store.fingerprint_for(&tablet_id).cloned();
    let qr_fingerprint = device.fingerprint.clone();

    let config = build_tls_config(stored_fingerprint.as_deref(), qr_fingerprint.as_deref())?;

    let server_name = server_name_for_host(&device.ip)?;
    let connector = rustls::ClientConnection::new(Arc::new(config), server_name)
        .context("Failed to create TLS connection")?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).with_context(|| {
        format!(
            "host unreachable at {} (AP isolation? same LAN? tablet TLS listener on 27184?) — use USB (--transport usb) instead",
            device.addr()
        )
    })?;
    stream
        .set_nodelay(true)
        .context("Failed to set TCP_NODELAY")?;

    let mut tls_stream = rustls::StreamOwned::new(connector, stream);
    // Force the TLS handshake now so cert errors surface with guidance
    // before we send the pairing Hello.
    tls_stream.flush().map_err(|e| {
        let stored_or_qr = stored_fingerprint
            .as_deref()
            .or(qr_fingerprint.as_deref())
            .unwrap_or("(unknown — scan the tablet QR again)");
        anyhow::anyhow!(
            "TLS handshake with {} failed ({e}); cert mismatch? expected fingerprint {stored_or_qr} — verify the SHA256 shown on the tablet pair screen, forget stale pairings, and re-scan the QR",
            device.addr()
        )
    })?;

    perform_handshake(&mut tls_stream, pin, &tablet_id, &device.ip, store)?;

    Ok(TlsConnection { stream: tls_stream })
}

/// TLS connection wrapper.
pub struct TlsConnection {
    pub stream: rustls::StreamOwned<rustls::ClientConnection, TcpStream>,
}

fn build_tls_config(
    stored_fingerprint: Option<&str>,
    qr_fingerprint: Option<&str>,
) -> Result<ClientConfig> {
    let mut root_store = RootCertStore::empty();
    // System roots are irrelevant for self-signed tablet certs, but loading
    // them keeps the verifier well-formed; TOFU pinning below is authoritative.
    // A failure to load natives must not break pairing.
    if let Ok(natives) = rustls_native_certs::load_native_certs() {
        root_store.add_parsable_certificates(natives);
    }

    let verifier = Arc::new(TofuVerifier {
        stored_fingerprint: stored_fingerprint.map(|s| s.to_string()),
        qr_fingerprint: qr_fingerprint.map(|s| s.to_string()),
    });

    let mut config = ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    config.alpn_protocols.clear();
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
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let fingerprint = certificate_fingerprint(end_entity.as_ref());

        let accepted = match (&self.stored_fingerprint, &self.qr_fingerprint) {
            (Some(stored), _) => fingerprint_equal(&fingerprint, stored),
            (None, Some(qr)) => fingerprint_equal(&fingerprint, qr),
            (None, None) => true,
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

fn fingerprint_equal(a: &str, b: &str) -> bool {
    // Case-insensitive compare; fingerprints are `SHA256:<hex>`.
    a.eq_ignore_ascii_case(b)
}

/// SHA-256 of the DER cert, `SHA256:<64 lowercase hex>`.
pub fn certificate_fingerprint(cert_der: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(cert_der);
    let result = hasher.finalize();
    format!("SHA256:{}", hex::encode(result))
}

/// Short display form: `SHA256:xxxx…` (first 16 hex chars).
#[allow(dead_code)]
pub fn fingerprint_short(fp: &str) -> String {
    let hexpart = fp.strip_prefix("SHA256:").unwrap_or(fp);
    let shown: String = hexpart.chars().take(16).collect();
    format!("SHA256:{shown}…")
}

fn perform_handshake(
    tls_stream: &mut rustls::StreamOwned<rustls::ClientConnection, TcpStream>,
    pin: Option<&str>,
    tablet_id: &str,
    tablet_ip: &str,
    store: &mut PairingStore,
) -> Result<()> {
    use crate::stream_android::write_framed_packet;
    use usbdisplay_transport::{PacketKind, TransportPacket};

    let host_id = crate::pairing::stable_host_id();

    let hello_payload = serde_json::json!({
        "v": 1,
        "pin": pin.unwrap_or(""),
        "host_id": host_id,
        "codecs": ["h264", "h265"]
    })
    .to_string();

    let hello_packet = TransportPacket::handshake(1, hello_payload.into_bytes());
    write_framed_packet(tls_stream, &hello_packet)?;

    let welcome_packet = read_framed_packet(tls_stream)?;
    if welcome_packet.header.kind != PacketKind::Handshake {
        bail!(
            "Expected Handshake packet, got {:?}",
            welcome_packet.header.kind
        );
    }

    let welcome_json: serde_json::Value =
        serde_json::from_slice(&welcome_packet.payload).context("Failed to parse Welcome JSON")?;

    if welcome_json["accept"].as_bool() != Some(true) {
        let reason = welcome_json["reason"].as_str().unwrap_or("rejected");
        if reason.contains("lockout") || reason.contains("pin") {
            bail!(
                "Handshake rejected by tablet ({reason}); wrong PIN? 3 strikes triggers a 30s lockout — re-run with the current PIN from the tablet pair screen"
            );
        }
        bail!("Handshake rejected by tablet: {welcome_json}");
    }

    // Pin the cert fingerprint on first successful pairing. Prefer the live
    // peer cert; fall back to the QR / Welcome fp if unavailable.
    if !store.is_paired(tablet_id) {
        let fp = tls_stream
            .conn
            .peer_certificates()
            .and_then(|certs| certs.first())
            .map(|cert| certificate_fingerprint(cert.as_ref()))
            .or_else(|| {
                welcome_json["fp"]
                    .as_str()
                    .map(|s| s.to_string())
                    .or_else(|| store.fingerprint_for(tablet_id).cloned())
            });
        if let Some(fp) = fp {
            store.remember(tablet_id, tablet_ip, &fp);
            if let Err(e) = store.save() {
                eprintln!("warning: failed to save pairing store: {e:#}");
            }
        }
    }

    Ok(())
}

fn read_framed_packet(
    tls_stream: &mut rustls::StreamOwned<rustls::ClientConnection, TcpStream>,
) -> Result<usbdisplay_transport::TransportPacket> {
    let mut len_bytes = [0u8; 4];
    tls_stream.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    let mut packet_bytes = vec![0u8; len];
    tls_stream.read_exact(&mut packet_bytes)?;
    usbdisplay_transport::TransportPacket::decode(&packet_bytes, 64 * 1024)
        .map_err(|e| anyhow::anyhow!("Failed to decode packet: {e}"))
}

fn resolve_addr(addr: &str) -> Result<SocketAddr> {
    addr.to_socket_addrs()
        .with_context(|| format!("resolving wifi device address '{addr}'"))?
        .next()
        .ok_or_else(|| anyhow::anyhow!("no address resolved for '{addr}'"))
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
    fn connect_plain_uses_tcp_nodelay() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        let device = parse_device(&format!("127.0.0.1:{port}")).unwrap();
        let handle = std::thread::spawn(move || listener.accept().is_ok());
        let stream = connect_plain(&device).expect("plaintext connect");
        assert_eq!(stream.peer_addr().unwrap().port(), port);
        assert!(handle.join().unwrap());
    }

    #[test]
    fn fingerprint_is_full_sha256() {
        let fp = certificate_fingerprint(b"test-cert-bytes");
        assert!(fp.starts_with("SHA256:"));
        assert_eq!(fp.len(), "SHA256:".len() + 64);
        let short = fingerprint_short(&fp);
        assert!(short.starts_with("SHA256:"));
    }

    #[test]
    fn server_name_accepts_ip_and_dns() {
        assert!(server_name_for_host("192.168.1.42").is_ok());
        assert!(server_name_for_host("tablet.local").is_ok());
    }

    /// End-to-end loopback validation of the WiFi pairing path against a
    /// tablet-like TLS server (P-256 self-signed cert, TLS 1.3, one framed
    /// Hello/Welcome exchange mirroring `WifiListener` + `Handshake`).
    #[test]
    fn tls_pairing_handshake_against_tablet_like_server() {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        use std::io::Write;
        use usbdisplay_transport::{PacketKind, TransportPacket};

        fn spawn_tablet_like_server(
            cert_der: Vec<u8>,
            key_der: Vec<u8>,
            expected_pin: &str,
        ) -> (u16, std::thread::JoinHandle<Option<String>>) {
            let expected_pin = expected_pin.to_string();
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let handle = std::thread::spawn(move || -> Option<String> {
                let server_config = rustls::ServerConfig::builder()
                    .with_no_client_auth()
                    .with_single_cert(
                        vec![CertificateDer::from(cert_der)],
                        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der)),
                    )
                    .ok()?;
                let (sock, _) = listener.accept().ok()?;
                sock.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
                let conn =
                    rustls::ServerConnection::new(std::sync::Arc::new(server_config)).ok()?;
                let mut tls = rustls::StreamOwned::new(conn, sock);
                let mut len = [0u8; 4];
                tls.read_exact(&mut len).ok()?;
                let len = u32::from_le_bytes(len) as usize;
                if len == 0 || len > 64 * 1024 {
                    return None;
                }
                let mut body = vec![0u8; len];
                tls.read_exact(&mut body).ok()?;
                // The tablet negotiates TLS 1.3 only.
                assert_eq!(
                    tls.conn.protocol_version(),
                    Some(rustls::ProtocolVersion::TLSv1_3)
                );
                let packet = TransportPacket::decode(&body, 64 * 1024).ok()?;
                if packet.header.kind != PacketKind::Handshake {
                    return None;
                }
                let hello = String::from_utf8(packet.payload).ok()?;
                let value: serde_json::Value = serde_json::from_str(&hello).ok()?;
                let accept = value["pin"].as_str() == Some(expected_pin.as_str());
                let welcome = if accept {
                    serde_json::json!({"v":1,"accept":true,"tablet_id":"tablet-test","fp":"SHA256:00"})
                } else {
                    serde_json::json!({"v":1,"accept":false,"reason":"bad-pin"})
                }
                .to_string();
                let reply = TransportPacket::handshake(1, welcome.into_bytes()).encode();
                tls.write_all(&(reply.len() as u32).to_le_bytes()).ok()?;
                tls.write_all(&reply).ok()?;
                tls.flush().ok()?;
                accept.then_some(hello)
            });
            (port, handle)
        }

        fn tablet_like_identity() -> (Vec<u8>, Vec<u8>) {
            // Same shape as Android TabletIdentity: P-256 self-signed cert.
            let certified =
                rcgen::generate_simple_self_signed(vec!["usbdisplay-tablet".to_string()]).unwrap();
            let cert_der: Vec<u8> = certified.cert.der().as_ref().to_vec();
            let key_der: Vec<u8> = certified.key_pair.serialize_der();
            (cert_der, key_der)
        }

        // 1. Happy path: correct PIN over a TOFU QR fingerprint.
        let (cert_der, key_der) = tablet_like_identity();
        let expected_fp = certificate_fingerprint(&cert_der);
        let (port, server) = spawn_tablet_like_server(cert_der.clone(), key_der.clone(), "123456");
        let device = WifiDevice {
            ip: "127.0.0.1".to_string(),
            port,
            fingerprint: Some(expected_fp.clone()),
        };
        let mut store = PairingStore::new();
        let conn = connect_tls_with_store(&device, Some("123456"), &mut store).unwrap();
        drop(conn);
        let hello = server.join().unwrap().expect("server saw a valid Hello");
        assert!(hello.contains("\"pin\":\"123456\""), "{hello}");
        assert!(hello.contains("\"host_id\":\""), "{hello}");
        assert!(hello.contains("\"codecs\":[\"h264\",\"h265\"]"), "{hello}");
        // TOFU pinning recorded the live peer fingerprint.
        assert_eq!(
            store.fingerprint_for(&device.addr()).map(String::as_str),
            Some(expected_fp.as_str())
        );

        // 2. Wrong PIN is rejected with actionable guidance.
        let (port, server) = spawn_tablet_like_server(cert_der.clone(), key_der.clone(), "123456");
        let device = WifiDevice {
            ip: "127.0.0.1".to_string(),
            port,
            fingerprint: Some(expected_fp.clone()),
        };
        let mut store = PairingStore::new();
        let err = connect_tls_with_store(&device, Some("000000"), &mut store)
            .err()
            .expect("wrong PIN must fail");
        assert!(err.to_string().contains("wrong PIN"), "{err:#}");
        assert!(server.join().unwrap().is_none());

        // 3. Cert mismatch (rotated tablet cert, stale QR fp) fails pre-Hello.
        let (rotated_der, rotated_key) = tablet_like_identity();
        assert_ne!(certificate_fingerprint(&rotated_der), expected_fp);
        let (port, server) = spawn_tablet_like_server(rotated_der, rotated_key, "123456");
        let device = WifiDevice {
            ip: "127.0.0.1".to_string(),
            port,
            fingerprint: Some(expected_fp.clone()),
        };
        let mut store = PairingStore::new();
        let err = connect_tls_with_store(&device, Some("123456"), &mut store)
            .err()
            .expect("cert mismatch must fail");
        assert!(err.to_string().contains("fingerprint"), "{err:#}");
        assert!(server.join().unwrap().is_none());
        assert!(!store.is_paired(&device.addr()));
    }
}
