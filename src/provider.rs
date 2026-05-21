//! Abstraction over "things that can return thermostat snapshots."
//!
//! The collector depends only on this trait, so swapping the Beehive
//! implementation for a future local-HomeKit one, or for tests, is a
//! single-line change.

use async_trait::async_trait;

use crate::model::Thermostat;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("upstream returned an error: {0}")]
    Upstream(String),
    #[error("response parse failed: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("not yet implemented: {0}")]
    NotImplemented(&'static str),
}

#[async_trait]
pub trait ThermostatProvider: Send + Sync {
    async fn fetch(&self) -> Result<Vec<Thermostat>, ProviderError>;
}

/// In-memory provider that always returns the same snapshot. Used in tests
/// and for `--demo` mode so the binary is observable without real
/// credentials.
pub struct FakeProvider {
    snapshot: Vec<Thermostat>,
}

impl FakeProvider {
    pub fn new(snapshot: Vec<Thermostat>) -> Self {
        Self { snapshot }
    }

    /// A representative two-sensor snapshot useful for smoke tests and demos.
    pub fn demo() -> Self {
        use crate::model::{
            HvacMode, RemoteSensor, Runtime, SensorCapability, Settings, Thermostat, Weather,
        };
        let demo = Thermostat {
            identifier: "411111111111".into(),
            name: "Main Floor".into(),
            connected: true,
            runtime: Runtime {
                actual_temperature: 721,
                desired_heat: 680,
                desired_cool: 760,
                actual_humidity: Some(43),
            },
            settings: Settings { hvac_mode: HvacMode::Auto },
            sensors: vec![
                RemoteSensor {
                    id: "ei:0".into(),
                    name: "Main Floor".into(),
                    sensor_type: "thermostat".into(),
                    in_use: true,
                    capabilities: vec![
                        SensorCapability { kind: "temperature".into(), value: "721".into() },
                        SensorCapability { kind: "humidity".into(),    value: "43".into() },
                        SensorCapability { kind: "occupancy".into(),   value: "true".into() },
                    ],
                },
                RemoteSensor {
                    id: "rs:100".into(),
                    name: "Bedroom".into(),
                    sensor_type: "ecobee3_remote_sensor".into(),
                    in_use: false,
                    capabilities: vec![
                        SensorCapability { kind: "temperature".into(), value: "693".into() },
                        SensorCapability { kind: "occupancy".into(),   value: "false".into() },
                    ],
                },
            ],
            weather: Some(Weather {
                station: "FI:KDEMO".into(),
                condition: "Cloudy".into(),
                temperature: Some(64.5),
                humidity: Some(78),
                pressure_mb: Some(1017),
                dewpoint: Some(57.5),
                wind_speed_mph: Some(4),
                wind_gust_mph: None,
                wind_bearing_degrees: Some(327),
                visibility_meters: Some(24000),
                probability_of_precipitation: Some(0),
                temp_high: Some(64.5),
                temp_low: Some(56.6),
                sky: Some(5),
            }),
            equipment_running: vec!["fan".into(), "compCool1".into()],
        };
        Self::new(vec![demo])
    }
}

#[async_trait]
impl ThermostatProvider for FakeProvider {
    async fn fetch(&self) -> Result<Vec<Thermostat>, ProviderError> {
        Ok(self.snapshot.clone())
    }
}
