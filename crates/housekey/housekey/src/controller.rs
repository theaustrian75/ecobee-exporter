use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use rand::RngCore;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::accessory::{Accessory, AccessoryError};
use crate::discovery::{self, DiscoveredAccessory};
use crate::pairing::pair_setup::PairSetup;
use crate::pairing::pair_verify::PairVerify;
use crate::transport::IpConnection;

#[derive(Debug, Error)]
pub enum ControllerError {
    #[error("not paired with accessory {0}")]
    NotPaired(String),
    #[error("already paired with accessory {0}")]
    AlreadyPaired(String),
    #[error("pairing store error: {0}")]
    StoreError(String),
    #[error("no paired accessories")]
    NoPairings,
    #[error(transparent)]
    Discovery(#[from] discovery::DiscoveryError),
    #[error(transparent)]
    Pairing(#[from] crate::pairing::PairingError),
    #[error(transparent)]
    Accessory(#[from] AccessoryError),
    #[error(transparent)]
    Transport(#[from] crate::transport::TransportError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedDevice {
    pub alias: String,
    pub accessory_id: String,
    pub accessory_ltpk: String,
    pub controller_pairing_id: String,
    pub controller_ltsk: String,
    pub controller_ltpk: String,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PairingStore {
    #[serde(flatten)]
    devices: HashMap<String, PairedDevice>,
}

pub struct Controller {
    store_path: PathBuf,
    paired: HashMap<String, PairedDevice>,
}

impl Controller {
    pub fn new(store_path: PathBuf) -> Self {
        Self {
            store_path,
            paired: HashMap::new(),
        }
    }

    pub fn load(&mut self) -> Result<(), ControllerError> {
        if !self.store_path.exists() {
            return Ok(());
        }
        let data = std::fs::read_to_string(&self.store_path)
            .map_err(|e| ControllerError::StoreError(e.to_string()))?;
        let store: PairingStore =
            serde_json::from_str(&data).map_err(|e| ControllerError::StoreError(e.to_string()))?;
        self.paired = store.devices;
        Ok(())
    }

    pub fn save(&self) -> Result<(), ControllerError> {
        if let Some(parent) = self.store_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ControllerError::StoreError(e.to_string()))?;
        }
        let store = PairingStore {
            devices: self.paired.clone(),
        };
        let json = serde_json::to_string_pretty(&store)
            .map_err(|e| ControllerError::StoreError(e.to_string()))?;
        std::fs::write(&self.store_path, json)
            .map_err(|e| ControllerError::StoreError(e.to_string()))?;
        Ok(())
    }

    pub fn paired_devices(&self) -> impl Iterator<Item = &PairedDevice> {
        self.paired.values()
    }

    pub async fn discover(&self) -> Result<Vec<DiscoveredAccessory>, ControllerError> {
        Ok(discovery::discover(discovery::DISCOVER_TIMEOUT_SECS).await?)
    }

    pub async fn pair(
        &mut self,
        accessory: &DiscoveredAccessory,
        alias: &str,
        pin: &str,
    ) -> Result<(), ControllerError> {
        if self.paired.contains_key(alias) {
            return Err(ControllerError::AlreadyPaired(alias.to_string()));
        }

        tracing::info!(
            accessory = accessory.display_name(),
            addr = %accessory.socket_addr(),
            alias,
            "starting HomeKit pair-setup"
        );

        let controller_pairing_id = random_pairing_id();
        let mut conn = IpConnection::connect(&accessory.addr.to_string(), accessory.port).await?;

        let setup = PairSetup::new(pin, controller_pairing_id.as_bytes())?;
        let m1 = setup.build_m1();
        let m2 = conn.post_tlv("/pair-setup", &m1).await?;
        let m3 = setup.process_m2(&m2)?;
        let m3_bytes = m3.build_m3();
        let m4 = conn.post_tlv("/pair-setup", &m3_bytes).await?;
        let m5 = m3.process_m4(&m4)?;
        let (m5_bytes, ltsk) = m5.build_m5()?;
        let m6 = conn.post_tlv("/pair-setup", &m5_bytes).await?;
        let result = m5.process_m6(&m6, &ltsk)?;

        let _ltpk = ltsk.verifying_key();
        self.paired.insert(
            alias.to_string(),
            PairedDevice {
                alias: alias.to_string(),
                accessory_id: String::from_utf8_lossy(&result.accessory_pairing_id).to_string(),
                accessory_ltpk: hex::encode(result.accessory_ltpk),
                controller_pairing_id,
                controller_ltsk: hex::encode(result.controller_ltsk),
                controller_ltpk: hex::encode(result.controller_ltpk),
                host: Some(accessory.addr.to_string()),
                port: Some(accessory.port),
            },
        );
        self.save()?;
        tracing::info!(alias, "HomeKit pair-setup complete");
        Ok(())
    }

    pub async fn read_accessories(&mut self, alias: &str) -> Result<Vec<Accessory>, ControllerError> {
        self.read_accessories_with_discovered(alias, &[])
            .await
            .map(|(accessories, _)| accessories)
    }

    async fn read_accessories_with_discovered(
        &mut self,
        alias: &str,
        discovered: &[DiscoveredAccessory],
    ) -> Result<(Vec<Accessory>, bool), ControllerError> {
        let device = self
            .paired
            .get(alias)
            .ok_or_else(|| ControllerError::NotPaired(alias.to_string()))?
            .clone();

        let (host, port) = resolve_endpoint(&device, discovered).ok_or_else(|| {
            ControllerError::NotPaired(format!(
                "{alias}: accessory not found via mDNS and no stored host"
            ))
        })?;
        let mut host_updated = false;
        if Some(host.as_str()) != device.host.as_deref() || device.port != Some(port) {
            tracing::info!(
                alias,
                old_host = ?device.host,
                new_host = %host,
                port,
                "resolved HomeKit accessory address"
            );
            if let Some(entry) = self.paired.get_mut(alias) {
                entry.host = Some(host.clone());
                entry.port = Some(port);
            }
            host_updated = true;
        }

        let accessory_ltpk = parse_hex32(&device.accessory_ltpk)?;
        let controller_ltsk = parse_hex32(&device.controller_ltsk)?;

        let mut conn = IpConnection::connect(&host, port).await?;

        let verify = PairVerify::new(
            &device.accessory_id,
            accessory_ltpk,
            &device.controller_pairing_id,
            &controller_ltsk,
        );
        let (m1, secret) = verify.build_m1();
        let our_public = x25519_dalek::PublicKey::from(&secret).to_bytes();
        let m2 = conn.post_tlv("/pair-verify", &m1).await?;
        let (m3, keys) = verify.process_m2(&m2, secret, &our_public)?;
        let m4 = conn.post_tlv("/pair-verify", &m3).await?;
        PairVerify::verify_m4(&m4)?;
        conn.set_session(keys.write_key, keys.read_key);

        let json = conn.get_json("/accessories").await?;
        let accessories: Vec<Accessory> = serde_json::from_value(
            json.get("accessories")
                .cloned()
                .ok_or_else(|| {
                    crate::transport::TransportError::InvalidResponse("missing accessories".into())
                })?,
        )
        .map_err(|e| crate::transport::TransportError::InvalidResponse(e.to_string()))?;

        Ok((accessories, host_updated))
    }

    pub async fn read_all_accessories(
        &mut self,
    ) -> Result<Vec<(String, Vec<Accessory>)>, ControllerError> {
        if self.paired.is_empty() {
            return Err(ControllerError::NoPairings);
        }

        let discovered = match discovery::discover(discovery::DISCOVER_TIMEOUT_SECS).await {
            Ok(found) => found,
            Err(e) => {
                tracing::warn!(error = %e, "mDNS browse failed; using stored accessory addresses");
                Vec::new()
            }
        };

        let mut out = Vec::new();
        let mut store_dirty = false;
        for alias in self.paired.keys().cloned().collect::<Vec<_>>() {
            let alias_for_log = alias.clone();
            let read = self.read_accessories_with_discovered(&alias, &discovered);
            match tokio::time::timeout(
                Duration::from_secs(30),
                read,
            )
            .await
            {
                Ok(Ok((accessories, host_updated))) => {
                    store_dirty |= host_updated;
                    out.push((alias, accessories));
                }
                Ok(Err(e)) => {
                    tracing::warn!(alias = %alias_for_log, error = %e, "homekit read failed");
                }
                Err(_) => tracing::warn!(
                    alias = %alias_for_log,
                    "homekit read timed out after 30s"
                ),
            }
        }
        if store_dirty {
            if let Err(e) = self.save() {
                tracing::warn!(error = %e, "failed to persist updated HomeKit accessory addresses");
            }
        }
        if out.is_empty() {
            return Err(ControllerError::NoPairings);
        }
        Ok(out)
    }

    pub fn remove(&mut self, alias: &str) -> Result<(), ControllerError> {
        if self.paired.remove(alias).is_none() {
            return Err(ControllerError::NotPaired(alias.to_string()));
        }
        self.save()?;
        Ok(())
    }

    /// Insert or replace a paired device in memory (call [`Self::save`] to persist).
    pub fn insert_paired(&mut self, device: PairedDevice) {
        self.paired.insert(device.alias.clone(), device);
    }
}

fn resolve_endpoint(
    device: &PairedDevice,
    discovered: &[DiscoveredAccessory],
) -> Option<(String, u16)> {
    if let Some(found) = discovery::find_by_accessory_id(discovered, &device.accessory_id) {
        return Some((found.addr.to_string(), found.port));
    }
    let host = device.host.clone()?;
    Some((host, device.port.unwrap_or(51826)))
}

fn random_pairing_id() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

fn parse_hex32(hex_str: &str) -> Result<[u8; 32], ControllerError> {
    let bytes = hex::decode(hex_str).map_err(|e| ControllerError::StoreError(e.to_string()))?;
    bytes
        .try_into()
        .map_err(|v: Vec<u8>| ControllerError::StoreError(format!("expected 32 bytes, got {}", v.len())))
}

// hex encode/decode helper crate
mod hex {
    pub fn encode(data: impl AsRef<[u8]>) -> String {
        data.as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    pub fn decode(s: &str) -> Result<Vec<u8>, String> {
        if !s.len().is_multiple_of(2) {
            return Err("odd hex length".into());
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "housekey-controller-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0u128, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn load_succeeds_when_store_missing() {
        let dir = tempdir();
        let mut controller = Controller::new(dir.join("missing.json"));
        assert!(controller.load().is_ok());
        assert_eq!(controller.paired_devices().count(), 0);
    }

    #[test]
    fn load_reads_persisted_pairing_store() {
        let dir = tempdir();
        let path = dir.join("pairings.json");
        std::fs::write(
            &path,
            r#"{
  "ecobee": {
    "alias": "ecobee",
    "accessory_id": "AA:BB:CC:DD:EE:FF",
    "accessory_ltpk": "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
    "controller_pairing_id": "00112233-4455-6677-8899-AABBCCDDEEFF",
    "controller_ltsk": "2122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f40",
    "controller_ltpk": "4142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f60",
    "host": "192.168.1.50",
    "port": 51826
  }
}"#,
        )
        .unwrap();

        let mut controller = Controller::new(path);
        controller.load().expect("load");
        let device = controller
            .paired_devices()
            .find(|d| d.alias == "ecobee")
            .expect("paired device");
        assert_eq!(device.host.as_deref(), Some("192.168.1.50"));
        assert_eq!(device.port, Some(51826));
    }

    #[test]
    fn remove_deletes_alias_and_persists() {
        let dir = tempdir();
        let path = dir.join("pairings.json");
        std::fs::write(
            &path,
            r#"{
  "ecobee": {
    "alias": "ecobee",
    "accessory_id": "AA:BB:CC:DD:EE:FF",
    "accessory_ltpk": "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
    "controller_pairing_id": "00112233-4455-6677-8899-AABBCCDDEEFF",
    "controller_ltsk": "2122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f40",
    "controller_ltpk": "4142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f60",
    "host": "192.168.1.50",
    "port": 51826
  }
}"#,
        )
        .unwrap();

        let mut controller = Controller::new(path.clone());
        controller.load().unwrap();
        controller.remove("ecobee").expect("remove");
        assert_eq!(controller.paired_devices().count(), 0);

        let mut reloaded = Controller::new(path);
        reloaded.load().unwrap();
        assert_eq!(reloaded.paired_devices().count(), 0);
    }

    #[test]
    fn load_rejects_invalid_json() {
        let dir = tempdir();
        let path = dir.join("bad.json");
        std::fs::write(&path, "{").unwrap();
        let mut controller = Controller::new(path);
        assert!(matches!(
            controller.load(),
            Err(ControllerError::StoreError(_))
        ));
    }
}
