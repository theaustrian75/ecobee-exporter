//! Internal domain types used by the collector and exposed via `Prometheus`.
//!
//! These intentionally do *not* try to mirror Beehive's GraphQL response
//! shapes 1:1 — they're a stable interface that the Beehive client (or any
//! future provider, e.g. a HomeKit one) translates into. Keeping this layer
//! small means we don't have to redesign metrics every time the upstream
//! API changes.
//!
//! Naming and units mirror `billykwooten/ecobee-exporter` where it makes
//! sense, so a Grafana dashboard built against that exporter is mostly
//! transferable.

use serde::{Deserialize, Serialize};

/// Top-level snapshot for a single thermostat at a single point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thermostat {
    /// Opaque ecobee thermostat identifier. Stable across polls.
    pub identifier: String,
    /// User-set thermostat name (e.g. "Upstairs").
    pub name: String,
    /// Whether the thermostat is currently reachable by ecobee's cloud.
    pub connected: bool,
    pub runtime: Runtime,
    pub settings: Settings,
    pub sensors: Vec<RemoteSensor>,
}

/// Live runtime metrics for the thermostat itself (not the remote sensors).
///
/// Temperatures are reported in tenths-of-a-degree (matching the ecobee REST
/// shape), e.g. `actual_temperature = 721` means 72.1°F. The Prometheus
/// exporter divides by 10 before publishing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Runtime {
    pub actual_temperature: i32,
    pub desired_heat: i32,
    pub desired_cool: i32,
    pub actual_humidity: Option<i32>,
}

/// User-facing settings that affect what the thermostat is currently doing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub hvac_mode: HvacMode,
}

/// Subset of HVAC modes ecobee supports. Anything else falls through to
/// `Other` so we never panic on unknown values from a rotated API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HvacMode {
    Off,
    Heat,
    Cool,
    Auto,
    AuxHeatOnly,
    #[serde(untagged)]
    Other(String),
}

impl HvacMode {
    pub fn as_label(&self) -> &str {
        match self {
            Self::Off => "off",
            Self::Heat => "heat",
            Self::Cool => "cool",
            Self::Auto => "auto",
            Self::AuxHeatOnly => "auxHeatOnly",
            Self::Other(s) => s.as_str(),
        }
    }
}

/// A physical sensor: the thermostat itself or a paired remote sensor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteSensor {
    pub id: String,
    pub name: String,
    pub sensor_type: String,
    /// Whether this sensor is currently being included in the thermostat's
    /// averaged readings.
    pub in_use: bool,
    pub capabilities: Vec<SensorCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorCapability {
    /// One of `"temperature"`, `"humidity"`, `"occupancy"`, plus others we
    /// currently ignore. Stored as a string so unknown capabilities pass
    /// through harmlessly.
    pub kind: String,
    /// Raw string value as reported. ecobee historically uses `"true"`/`"false"`
    /// for booleans and integer tenths for temperatures.
    pub value: String,
}
