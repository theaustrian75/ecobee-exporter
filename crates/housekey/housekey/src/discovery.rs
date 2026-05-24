use std::collections::HashMap;
use std::net::IpAddr;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("mDNS browse failed: {0}")]
    BrowseFailed(String),
    #[error("accessory not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone)]
pub struct DiscoveredAccessory {
    pub name: String,
    pub id: String,
    pub addr: IpAddr,
    pub port: u16,
    pub model: String,
    pub state_number: u8,
    pub feature_flags: u8,
    pub status_flags: u8,
    pub category: AccessoryCategory,
    pub txt_records: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AccessoryCategory {
    Other = 1,
    Bridge = 2,
    Fan = 3,
    GarageDoorOpener = 4,
    Lightbulb = 5,
    DoorLock = 6,
    Outlet = 7,
    Switch = 8,
    Thermostat = 9,
    Sensor = 10,
    SecuritySystem = 11,
    Door = 12,
    Window = 13,
    WindowCovering = 14,
    ProgrammableSwitch = 15,
    IpCamera = 17,
    VideoDoorbell = 18,
    AirPurifier = 19,
    Heater = 20,
    AirConditioner = 21,
    Humidifier = 22,
    Dehumidifier = 23,
    Sprinkler = 28,
    Faucet = 29,
    ShowerSystem = 30,
    Router = 32,
}

impl AccessoryCategory {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => Self::Other,
            2 => Self::Bridge,
            3 => Self::Fan,
            4 => Self::GarageDoorOpener,
            5 => Self::Lightbulb,
            6 => Self::DoorLock,
            7 => Self::Outlet,
            8 => Self::Switch,
            9 => Self::Thermostat,
            10 => Self::Sensor,
            11 => Self::SecuritySystem,
            12 => Self::Door,
            13 => Self::Window,
            14 => Self::WindowCovering,
            15 => Self::ProgrammableSwitch,
            17 => Self::IpCamera,
            18 => Self::VideoDoorbell,
            19 => Self::AirPurifier,
            20 => Self::Heater,
            21 => Self::AirConditioner,
            22 => Self::Humidifier,
            23 => Self::Dehumidifier,
            28 => Self::Sprinkler,
            29 => Self::Faucet,
            30 => Self::ShowerSystem,
            32 => Self::Router,
            _ => Self::Other,
        }
    }
}

/// Browse `_hap._tcp` for HomeKit accessories on the LAN.
pub async fn discover(timeout_secs: u64) -> Result<Vec<DiscoveredAccessory>, DiscoveryError> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};

    let mdns = ServiceDaemon::new().map_err(|e| DiscoveryError::BrowseFailed(e.to_string()))?;
    let receiver = mdns
        .browse("_hap._tcp.local.")
        .map_err(|e| DiscoveryError::BrowseFailed(e.to_string()))?;

    let mut found: HashMap<String, DiscoveredAccessory> = HashMap::new();
    let deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs.max(1));

    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, receiver.recv_async()).await {
            Ok(Ok(ServiceEvent::ServiceResolved(info))) => {
                let id = info
                    .get_property_val_str("id")
                    .unwrap_or("")
                    .to_string();
                if id.is_empty() {
                    continue;
                }
                let Some(addr) = info.get_addresses().iter().next().copied() else {
                    continue;
                };
                let category = info
                    .get_property_val_str("ci")
                    .and_then(|s| s.parse::<u8>().ok())
                    .map(AccessoryCategory::from_u8)
                    .unwrap_or(AccessoryCategory::Other);
                let txt_records = info
                    .get_properties()
                    .iter()
                    .map(|prop| (prop.key().to_string(), prop.val_str().to_string()))
                    .collect();

                found.insert(
                    id.clone(),
                    DiscoveredAccessory {
                        name: info.get_fullname().to_string(),
                        id,
                        addr,
                        port: info.get_port(),
                        model: info
                            .get_property_val_str("md")
                            .unwrap_or("")
                            .to_string(),
                        state_number: info
                            .get_property_val_str("s#")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0),
                        feature_flags: info
                            .get_property_val_str("ff")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0),
                        status_flags: info
                            .get_property_val_str("sf")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0),
                        category,
                        txt_records,
                    },
                );
            }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => return Err(DiscoveryError::BrowseFailed(e.to_string())),
            Err(_) => break,
        }
    }

    Ok(found.into_values().collect())
}
