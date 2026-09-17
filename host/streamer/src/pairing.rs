//! TOFU (Trust On First Use) pairing store for WiFi TLS connections.
//!
//! Stores tablet certificates by fingerprint in %AppData%\USBDisplay\paired.json.
//! This allows host-side certificate pinning without a full PKI.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Path to the pairing store: %AppData%\USBDisplay\paired.json
pub fn store_path() -> Result<PathBuf> {
    let appdata = std::env::var("APPDATA")
        .context("Failed to get APPDATA environment variable")?;
    Ok(PathBuf::from(appdata).join("USBDisplay").join("paired.json"))
}

/// Pairing store data structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingStore {
    /// Map from tablet_id (ip:port) to fingerprint.
    pub tablets: HashMap<String, TabletEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabletEntry {
    pub tablet_id: String,
    pub ip: String,
    pub cert_fingerprint: String,
    pub paired_at: i64, // Unix timestamp
}

impl PairingStore {
    pub fn new() -> Self {
        Self {
            tablets: HashMap::new(),
        }
    }

    pub fn load() -> Result<Self> {
        let path = store_path()?;
        if !path.exists() {
            return Ok(Self::new());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read pairing store from {}", path.display()))?;
        let store: PairingStore = serde_json::from_str(&content)
            .with_context(|| "Failed to parse pairing store JSON")?;
        Ok(store)
    }

    pub fn save(&self) -> Result<()> {
        let path = store_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize pairing store")?;
        fs::write(&path, content)
            .with_context(|| format!("Failed to write pairing store to {}", path.display()))?;
        Ok(())
    }

    pub fn remember(&mut self, tablet_id: &str, fingerprint: &str) {
        let entry = TabletEntry {
            tablet_id: tablet_id.to_string(),
            ip: tablet_id.to_string(), // TODO: extract IP from tablet_id
            cert_fingerprint: fingerprint.to_string(),
            paired_at: chrono::Utc::now().timestamp(),
        };
        self.tablets.insert(tablet_id.to_string(), entry);
    }

    pub fn fingerprint_for(&self, tablet_id: &str) -> Option<&String> {
        self.tablets.get(tablet_id).map(|e| &e.cert_fingerprint)
    }

    pub fn is_paired(&self, tablet_id: &str) -> bool {
        self.tablets.contains_key(tablet_id)
    }
}

impl Default for PairingStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_store_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("paired.json");

        // Create a store and add an entry
        let mut store = PairingStore::new();
        store.remember("tablet1:27184", "SHA256:abcd1234");

        // Save and reload
        let content = serde_json::to_string_pretty(&store).unwrap();
        fs::write(&path, content).unwrap();

        let loaded = PairingStore::load().unwrap();
        assert_eq!(loaded.fingerprint_for("tablet1:27184"), Some(&"SHA256:abcd1234".to_string()));
    }

    #[test]
    fn test_fingerprint_lookup() {
        let mut store = PairingStore::new();
        store.remember("a:1", "SHA256:first");
        store.remember("a:2", "SHA256:second");

        assert_eq!(store.fingerprint_for("a:1"), Some(&"SHA256:first".to_string()));
        assert_eq!(store.fingerprint_for("a:2"), Some(&"SHA256:second".to_string()));
        assert_eq!(store.fingerprint_for("a:3"), None);
    }

    #[test]
    fn test_is_paired() {
        let mut store = PairingStore::new();
        assert!(!store.is_paired("unknown"));

        store.remember("known:1", "SHA256:abcd");
        assert!(store.is_paired("known:1"));
        assert!(!store.is_paired("other:1"));
    }
}
