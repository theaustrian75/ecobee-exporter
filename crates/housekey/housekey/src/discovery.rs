use std::collections::HashMap;
use std::fmt;
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

impl fmt::Display for AccessoryCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Other => "Other",
            Self::Bridge => "Bridge",
            Self::Fan => "Fan",
            Self::GarageDoorOpener => "Garage",
            Self::Lightbulb => "Light",
            Self::DoorLock => "Lock",
            Self::Outlet => "Outlet",
            Self::Switch => "Switch",
            Self::Thermostat => "Thermostat",
            Self::Sensor => "Sensor",
            Self::SecuritySystem => "Security",
            Self::Door => "Door",
            Self::Window => "Window",
            Self::WindowCovering => "Covering",
            Self::ProgrammableSwitch => "Switch",
            Self::IpCamera => "Camera",
            Self::VideoDoorbell => "Doorbell",
            Self::AirPurifier => "Purifier",
            Self::Heater => "Heater",
            Self::AirConditioner => "AC",
            Self::Humidifier => "Humidifier",
            Self::Dehumidifier => "Dehumidifier",
            Self::Sprinkler => "Sprinkler",
            Self::Faucet => "Faucet",
            Self::ShowerSystem => "Shower",
            Self::Router => "Router",
        };
        f.write_str(label)
    }
}

impl DiscoveredAccessory {
    /// Short hostname (no trailing dot or `.local`).
    pub fn display_name(&self) -> &str {
        let host = self.name.trim_end_matches('.');
        host.strip_suffix(".local").unwrap_or(host)
    }

    /// `host:port`, bracketed when `host` is IPv6.
    pub fn socket_addr(&self) -> String {
        format_socket_addr(self.addr, self.port)
    }

    /// True when the accessory reports an ecobee model over HAP.
    pub fn is_ecobee(&self) -> bool {
        self.model.to_ascii_lowercase().contains("ecobee")
    }
}

fn format_socket_addr(addr: IpAddr, port: u16) -> String {
    match addr {
        IpAddr::V4(v4) => format!("{v4}:{port}"),
        IpAddr::V6(v6) => format!("[{v6}]:{port}"),
    }
}

fn pick_address(addrs: &std::collections::HashSet<IpAddr>) -> Option<IpAddr> {
    addrs
        .iter()
        .find(|addr| addr.is_ipv4())
        .or_else(|| addrs.iter().next())
        .copied()
}

/// Default mDNS browse duration used by [`Controller::discover`].
pub const DISCOVER_TIMEOUT_SECS: u64 = 5;

/// Browse `_hap._tcp` for HomeKit accessories on the LAN.
pub async fn discover(timeout_secs: u64) -> Result<Vec<DiscoveredAccessory>, DiscoveryError> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};

    let timeout_secs = timeout_secs.max(1);
    tracing::info!(timeout_secs, "browsing _hap._tcp.local");

    let mdns = ServiceDaemon::new().map_err(|e| DiscoveryError::BrowseFailed(e.to_string()))?;
    let receiver = mdns
        .browse("_hap._tcp.local.")
        .map_err(|e| DiscoveryError::BrowseFailed(e.to_string()))?;

    let mut found: HashMap<String, DiscoveredAccessory> = HashMap::new();
    let deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, receiver.recv_async()).await {
            Ok(Ok(ServiceEvent::ServiceResolved(info))) => {
                let id = info
                    .get_property_val_str("id")
                    .unwrap_or("")
                    .to_string();
                if id.is_empty() {
                    tracing::debug!(
                        hostname = info.get_hostname(),
                        "skipping HAP service without id"
                    );
                    continue;
                }
                let Some(addr) = pick_address(info.get_addresses()) else {
                    tracing::debug!(
                        hostname = info.get_hostname(),
                        id = %id,
                        "skipping HAP service without address"
                    );
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

                let accessory = DiscoveredAccessory {
                        name: info.get_hostname().to_string(),
                        id: id.clone(),
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
                    };
                tracing::debug!(
                    name = accessory.display_name(),
                    id = %accessory.id,
                    addr = %accessory.socket_addr(),
                    model = %accessory.model,
                    category = %accessory.category,
                    "resolved HAP accessory"
                );
                found.insert(id, accessory);
            }
            Ok(Ok(other)) => {
                tracing::trace!(?other, "mDNS event");
            }
            Ok(Err(e)) => return Err(DiscoveryError::BrowseFailed(e.to_string())),
            Err(_) => break,
        }
    }

    tracing::info!(accessories = found.len(), "mDNS browse complete");

    Ok(found.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn format_socket_ipv4() {
        let addr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        assert_eq!(format_socket_addr(addr, 51826), "192.168.1.10:51826");
    }

    #[test]
    fn format_socket_ipv6() {
        let addr = IpAddr::V6(Ipv6Addr::new(
            0x2601, 0x8c, 0xc200, 0x9f12, 0x4661, 0x32ff, 0xfe12, 0x9a0f,
        ));
        assert_eq!(
            format_socket_addr(addr, 55118),
            "[2601:8c:c200:9f12:4661:32ff:fe12:9a0f]:55118"
        );
    }

    #[test]
    fn pick_address_prefers_ipv4() {
        use std::collections::HashSet;
        let mut addrs = HashSet::new();
        addrs.insert(IpAddr::V6(Ipv6Addr::LOCALHOST));
        addrs.insert(IpAddr::V4(Ipv4Addr::new(172, 30, 0, 102)));
        assert_eq!(
            pick_address(&addrs),
            Some(IpAddr::V4(Ipv4Addr::new(172, 30, 0, 102)))
        );
    }

    #[test]
    fn is_ecobee_matches_model_string() {
        let item = DiscoveredAccessory {
            name: "Living-Room.local".into(),
            id: "18:E2:7F:FE:8D:24".into(),
            addr: IpAddr::V4(Ipv4Addr::new(172, 30, 0, 102)),
            port: 55118,
            model: "ecobee3 lite".into(),
            state_number: 1,
            feature_flags: 0,
            status_flags: 0,
            category: AccessoryCategory::Thermostat,
            txt_records: HashMap::new(),
        };
        assert!(item.is_ecobee());
    }
}
