//! Local HomeKit provider for ecobee thermostats (native Rust HAP controller).
//!
//! **Status: untested.** Pair-verify against real ecobees has not been validated
//! in production; concurrent Home Assistant polling with HA-imported keys is a
//! known failure mode. Prefer `provider = "homeassistant"` or `provider = "beehive"`
//! until this backend is confirmed working in your environment.

pub mod ha_import;
pub mod translate;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use housekey::Controller;
use tokio::sync::Mutex;

use crate::{
    config::HomeKitConfig,
    model::Thermostat,
    provider::{ProviderError, ThermostatProvider},
};

/// Reads paired ecobee thermostats over the LAN via the in-tree `housekey` HAP client.
pub struct HomeKitProvider {
    controller: Arc<Mutex<Controller>>,
}

impl HomeKitProvider {
    pub fn new(cfg: &HomeKitConfig) -> Result<Self, ProviderError> {
        let mut controller = Controller::new(cfg.pairing_file.clone());
        controller
            .load()
            .map_err(|e| ProviderError::Auth(format!("loading homekit pairings: {e}")))?;
        Ok(Self {
            controller: Arc::new(Mutex::new(controller)),
        })
    }

    pub fn pairing_file(cfg: &HomeKitConfig) -> PathBuf {
        cfg.pairing_file.clone()
    }
}

#[async_trait]
impl ThermostatProvider for HomeKitProvider {
    async fn fetch(&self) -> Result<Vec<Thermostat>, ProviderError> {
        let mut controller = self.controller.lock().await;
        let snapshots = controller
            .read_all_accessories()
            .await
            .map_err(|e| ProviderError::Upstream(format!("homekit read failed: {e}")))?;

        Ok(snapshots
            .into_iter()
            .flat_map(|(alias, accessories)| translate::translate_accessories(&alias, &accessories))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "ecobee-homekit-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0u128, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn new_succeeds_when_pairing_file_missing() {
        let dir = tempdir();
        let cfg = HomeKitConfig {
            pairing_file: dir.join("missing-pairings.json"),
        };
        assert!(HomeKitProvider::new(&cfg).is_ok());
    }

    #[test]
    fn new_loads_existing_pairing_store() {
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

        let cfg = HomeKitConfig { pairing_file: path };
        let provider = HomeKitProvider::new(&cfg).expect("valid pairing store");
        let controller = provider.controller.blocking_lock();
        assert_eq!(controller.paired_devices().count(), 1);
    }

    #[test]
    fn new_rejects_invalid_pairing_json() {
        let dir = tempdir();
        let path = dir.join("bad.json");
        std::fs::write(&path, "not-json").unwrap();
        let cfg = HomeKitConfig { pairing_file: path };
        assert!(matches!(
            HomeKitProvider::new(&cfg),
            Err(ProviderError::Auth(_))
        ));
    }
}
