//! Internal domain types used by the collector and exposed via `Prometheus`.
//!
//! These intentionally do *not* try to mirror Beehive's GraphQL response
//! shapes 1:1 — they're a stable interface that the Beehive client (or any
//! future provider translates into. Keeping this layer
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
    /// Current outdoor weather observation, if the thermostat reported one.
    #[serde(default)]
    pub weather: Option<Weather>,
    /// Identifiers of equipment currently running (e.g. `"fan"`, `"compCool1"`).
    ///
    /// Comes from the upstream `equipmentStatus` CSV; empty means idle.
    #[serde(default)]
    pub equipment_running: Vec<String>,
    /// Active schedule slot, if program data was included.
    #[serde(default)]
    pub program: Option<Program>,
    /// The currently running hold/vacation/DR event, if any.
    #[serde(default)]
    pub hold: Option<HoldEvent>,
    /// Last three 5-minute equipment runtime buckets, if requested.
    #[serde(default)]
    pub extended_runtime: Option<ExtendedRuntime>,
    /// Active thermostat alerts requiring user attention.
    #[serde(default)]
    pub alerts: Vec<Alert>,
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
    pub desired_humidity: Option<i32>,
    pub desired_dehumidity: Option<i32>,
    /// Dry-bulb temperature; differs from `actual_temperature` when
    /// "feels like" mode is enabled.
    pub raw_temperature: Option<i32>,
    /// `"auto"`, `"on"`, or absent when HVAC is off.
    pub desired_fan_mode: Option<String>,
}

/// User-facing settings that affect what the thermostat is currently doing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub hvac_mode: HvacMode,
    pub follow_me_comfort: bool,
    pub smart_circulation: bool,
    pub heat_stages: Option<i32>,
    pub cool_stages: Option<i32>,
}

/// Active schedule program state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    /// Reference to the active climate, e.g. `"home"`, `"sleep"`, `"away"`.
    pub current_climate_ref: Option<String>,
}

/// A hold, vacation, or demand-response event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldEvent {
    pub running: bool,
    pub event_type: String,
    pub name: String,
    pub heat_hold_temp: Option<i32>,
    pub cool_hold_temp: Option<i32>,
}

/// Per-equipment runtime from the extended runtime block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipmentRuntime {
    /// API field name, e.g. `"cool1"`, `"fan"`, `"heatPump1"`.
    pub name: String,
    /// Seconds of runtime in each of the last three 5-minute intervals.
    /// Index 0 = oldest, index 2 = newest.
    pub seconds: [i32; 3],
}

/// Extended runtime telemetry: 5-minute equipment buckets and utility data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendedRuntime {
    pub equipment: Vec<EquipmentRuntime>,
    /// Demand-management temperature offsets in tenths of a degree.
    pub dm_offset: [Option<i32>; 3],
    pub current_electricity_bill: Option<i32>,
    pub projected_electricity_bill: Option<i32>,
}

/// An active alert on the thermostat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub alert_type: String,
    pub alert_number: Option<i32>,
    pub severity: String,
    pub text: String,
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

/// Current outdoor conditions reported by the thermostat's associated
/// weather station, with units already normalized.
///
/// `None` on any individual field means ecobee returned its `-5002`
/// "no data" sentinel; we filter those at translation time so callers
/// don't need to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weather {
    /// Weather station identifier ecobee used (e.g. `"FI:KCDW"`).
    pub station: String,
    /// Human-readable condition (e.g. `"Cloudy"`).
    pub condition: String,
    /// Outdoor temperature in degrees (Fahrenheit for US accounts).
    pub temperature: Option<f64>,
    /// Outdoor relative humidity, percent.
    pub humidity: Option<i32>,
    /// Sea-level pressure, millibars (equivalent to hectopascals).
    pub pressure_mb: Option<i32>,
    /// Outdoor dewpoint in degrees.
    pub dewpoint: Option<f64>,
    /// Wind speed in mph.
    pub wind_speed_mph: Option<i32>,
    /// Wind gust in mph.
    pub wind_gust_mph: Option<i32>,
    /// Wind bearing, compass degrees (0 = N, 90 = E).
    pub wind_bearing_degrees: Option<i32>,
    /// Visibility in meters.
    pub visibility_meters: Option<i32>,
    /// Probability of precipitation, percent.
    pub probability_of_precipitation: Option<i32>,
    /// Forecast daily high in degrees.
    pub temp_high: Option<f64>,
    /// Forecast daily low in degrees.
    pub temp_low: Option<f64>,
    /// Sky code (ecobee enum 0-9-ish; mostly redundant with `condition`).
    pub sky: Option<i32>,
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
