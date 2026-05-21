//! Data-API request/response shapes.
//!
//! Hypothesis: the mobile app's data API at `prod.ecobee.com/api/v1`
//! reuses the same Selection-based REST contract that's been documented
//! for the developer API at `api.ecobee.com/1/...` for the last decade.
//! Same backend, same wire format, just a different host name.
//!
//! Concretely we send:
//!
//!   GET /thermostat?json={"selection":{...}}
//!   Authorization: Bearer <auth0_access_token>
//!
//! and expect a response shaped like:
//!
//!   { "page": {...},
//!     "thermostatList": [ { ... per-thermostat ... } ],
//!     "status": { "code": 0, "message": "" } }
//!
//! `status.code == 0` is success; non-zero is an application-level
//! error even when the HTTP response is 200. We surface those as
//! `ProviderError::Upstream`.
//!
//! If the hypothesis is wrong, this whole module needs to be rewritten
//! around whatever your capture shows. See `CAPTURE.md`.

use serde::{Deserialize, Serialize};

use crate::{
    model::{HvacMode, RemoteSensor, Runtime, SensorCapability, Settings, Thermostat, Weather},
    provider::ProviderError,
};

use super::client::BeehiveClient;

/// Ecobee uses this value to mean "no reading available" in integer fields.
const NO_DATA_SENTINEL: i32 = -5002;

fn maybe(v: i32) -> Option<i32> {
    if v == NO_DATA_SENTINEL { None } else { Some(v) }
}

fn maybe_tenths(v: i32) -> Option<f64> {
    maybe(v).map(|n| f64::from(n) / 10.0)
}

/// The Selection object that filters which thermostats and which sub-blocks
/// to include in the response. Mirrors the ecobee REST `Selection` type.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(
    clippy::struct_excessive_bools,
    reason = "mirrors ecobee's REST Selection contract one-for-one"
)]
pub struct Selection {
    pub selection_type: &'static str,
    pub selection_match: &'static str,
    pub include_runtime: bool,
    pub include_sensors: bool,
    pub include_settings: bool,
    pub include_weather: bool,
    pub include_equipment_status: bool,
}

impl Selection {
    pub fn registered_with_sensors() -> Self {
        Self {
            selection_type: "registered",
            selection_match: "",
            include_runtime: true,
            include_sensors: true,
            include_settings: true,
            include_weather: true,
            include_equipment_status: true,
        }
    }
}

#[derive(Debug, Serialize)]
struct SelectionEnvelope {
    selection: Selection,
}

#[derive(Debug, Deserialize)]
pub struct ThermostatListResponse {
    #[serde(default)]
    pub thermostat_list: Vec<RawThermostat>,
    pub status: ApiStatus,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThermostatListResponseWire {
    #[serde(default)]
    thermostat_list: Vec<RawThermostat>,
    status: ApiStatus,
}

impl From<ThermostatListResponseWire> for ThermostatListResponse {
    fn from(w: ThermostatListResponseWire) -> Self {
        Self { thermostat_list: w.thermostat_list, status: w.status }
    }
}

#[derive(Debug, Deserialize)]
pub struct ApiStatus {
    pub code: i32,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawThermostat {
    pub identifier: String,
    pub name: String,
    #[serde(default)]
    pub runtime: Option<RawRuntime>,
    #[serde(default)]
    pub settings: Option<RawSettings>,
    #[serde(default)]
    pub remote_sensors: Vec<RawRemoteSensor>,
    #[serde(default)]
    pub weather: Option<RawWeather>,
    /// CSV of currently-running equipment, e.g. `"compCool1,fan"`. Empty
    /// or absent when idle.
    #[serde(default)]
    pub equipment_status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawWeather {
    #[serde(default)]
    pub weather_station: String,
    #[serde(default)]
    pub forecasts: Vec<RawForecast>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawForecast {
    #[serde(default)]
    pub condition: String,
    #[serde(default = "no_data")]
    pub temperature: i32,
    #[serde(default = "no_data")]
    pub pressure: i32,
    #[serde(default = "no_data")]
    pub relative_humidity: i32,
    #[serde(default = "no_data")]
    pub dewpoint: i32,
    #[serde(default = "no_data")]
    pub visibility: i32,
    #[serde(default = "no_data")]
    pub wind_speed: i32,
    #[serde(default = "no_data")]
    pub wind_gust: i32,
    #[serde(default = "no_data")]
    pub wind_bearing: i32,
    #[serde(default = "no_data")]
    pub pop: i32,
    #[serde(default = "no_data")]
    pub temp_high: i32,
    #[serde(default = "no_data")]
    pub temp_low: i32,
    #[serde(default = "no_data")]
    pub sky: i32,
}

fn no_data() -> i32 {
    NO_DATA_SENTINEL
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawRuntime {
    #[serde(default)]
    pub connected: bool,
    #[serde(default)]
    pub actual_temperature: i32,
    #[serde(default)]
    pub desired_heat: i32,
    #[serde(default)]
    pub desired_cool: i32,
    #[serde(default)]
    pub actual_humidity: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawSettings {
    #[serde(default)]
    pub hvac_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawRemoteSensor {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub sensor_type: String,
    #[serde(default)]
    pub in_use: bool,
    #[serde(default, alias = "capabilities")]
    pub capability: Vec<RawSensorCapability>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawSensorCapability {
    #[serde(rename = "type")]
    pub kind: String,
    pub value: String,
}

/// Issue the list-thermostats call against the configured base URL.
pub async fn list_thermostats(
    client: &BeehiveClient,
    bearer: &str,
) -> Result<ThermostatListResponse, ProviderError> {
    let url = format!("{}/thermostat", client.base_url());
    let selection_json = serde_json::to_string(&SelectionEnvelope {
        selection: Selection::registered_with_sensors(),
    })
    .map_err(ProviderError::Parse)?;

    let resp = client
        .http()
        .get(&url)
        .bearer_auth(bearer)
        .query(&[("json", selection_json.as_str())])
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(200).collect();
        return Err(ProviderError::Upstream(format!(
            "{url} returned HTTP {status}: {snippet}"
        )));
    }

    let wire: ThermostatListResponseWire = resp.json().await?;
    let parsed: ThermostatListResponse = wire.into();
    if parsed.status.code != 0 {
        return Err(ProviderError::Upstream(format!(
            "API status {}: {}",
            parsed.status.code, parsed.status.message
        )));
    }
    Ok(parsed)
}

/// Translate the raw response into the exporter's domain model.
///
/// Kept as a free function so it's unit-testable against captured JSON
/// without doing any network I/O.
pub fn translate(resp: &ThermostatListResponse) -> Vec<Thermostat> {
    resp.thermostat_list
        .iter()
        .map(|t| {
            let runtime = t.runtime.as_ref();
            let connected = runtime.is_some_and(|r| r.connected);
            let runtime_model = Runtime {
                actual_temperature: runtime.map_or(0, |r| r.actual_temperature),
                desired_heat: runtime.map_or(0, |r| r.desired_heat),
                desired_cool: runtime.map_or(0, |r| r.desired_cool),
                actual_humidity: runtime.and_then(|r| r.actual_humidity),
            };
            let hvac_mode = t
                .settings
                .as_ref()
                .and_then(|s| s.hvac_mode.as_deref())
                .map_or(HvacMode::Off, parse_hvac_mode);
            let sensors = t
                .remote_sensors
                .iter()
                .map(|s| RemoteSensor {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    sensor_type: s.sensor_type.clone(),
                    in_use: s.in_use,
                    capabilities: s
                        .capability
                        .iter()
                        .map(|c| SensorCapability {
                            kind: c.kind.clone(),
                            value: c.value.clone(),
                        })
                        .collect(),
                })
                .collect();
            let weather = t.weather.as_ref().and_then(translate_weather);
            let equipment_running = t
                .equipment_status
                .as_deref()
                .map(parse_equipment_csv)
                .unwrap_or_default();
            Thermostat {
                identifier: t.identifier.clone(),
                name: t.name.clone(),
                connected,
                runtime: runtime_model,
                settings: Settings { hvac_mode },
                sensors,
                weather,
                equipment_running,
            }
        })
        .collect()
}

fn translate_weather(w: &RawWeather) -> Option<Weather> {
    let current = w.forecasts.first()?;
    Some(Weather {
        station: w.weather_station.clone(),
        condition: current.condition.clone(),
        temperature: maybe_tenths(current.temperature),
        humidity: maybe(current.relative_humidity),
        pressure_mb: maybe(current.pressure),
        dewpoint: maybe_tenths(current.dewpoint),
        wind_speed_mph: maybe(current.wind_speed),
        wind_gust_mph: maybe(current.wind_gust),
        wind_bearing_degrees: maybe(current.wind_bearing),
        visibility_meters: maybe(current.visibility),
        probability_of_precipitation: maybe(current.pop),
        temp_high: maybe_tenths(current.temp_high),
        temp_low: maybe_tenths(current.temp_low),
        sky: maybe(current.sky),
    })
}

fn parse_equipment_csv(csv: &str) -> Vec<String> {
    csv.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_hvac_mode(s: &str) -> HvacMode {
    match s {
        "off" => HvacMode::Off,
        "heat" => HvacMode::Heat,
        "cool" => HvacMode::Cool,
        "auto" => HvacMode::Auto,
        "auxHeatOnly" => HvacMode::AuxHeatOnly,
        other => HvacMode::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic but shape-faithful response. Once the user does a real
    /// capture, this should be replaced with the sanitized fixture.
    const SAMPLE: &str = r#"{
        "page": {"page": 1, "totalPages": 1, "pageSize": 1, "total": 1},
        "thermostatList": [{
            "identifier": "411111111111",
            "name": "Main Floor",
            "runtime": {
                "connected": true,
                "actualTemperature": 721,
                "desiredHeat": 680,
                "desiredCool": 760,
                "actualHumidity": 43
            },
            "settings": { "hvacMode": "auto" },
            "remoteSensors": [
                {
                    "id": "ei:0",
                    "name": "Main Floor",
                    "type": "thermostat",
                    "inUse": true,
                    "capability": [
                        {"id": "1", "type": "temperature", "value": "721"},
                        {"id": "2", "type": "humidity", "value": "43"},
                        {"id": "3", "type": "occupancy", "value": "true"}
                    ]
                },
                {
                    "id": "rs:100",
                    "name": "Bedroom",
                    "type": "ecobee3_remote_sensor",
                    "inUse": false,
                    "capability": [
                        {"id": "1", "type": "temperature", "value": "693"},
                        {"id": "2", "type": "occupancy", "value": "false"}
                    ]
                }
            ]
        }],
        "status": {"code": 0, "message": ""}
    }"#;

    #[test]
    fn parses_documented_developer_api_shape() {
        let wire: ThermostatListResponseWire = serde_json::from_str(SAMPLE).unwrap();
        let parsed: ThermostatListResponse = wire.into();
        assert_eq!(parsed.status.code, 0);
        assert_eq!(parsed.thermostat_list.len(), 1);
        let t = &parsed.thermostat_list[0];
        assert_eq!(t.identifier, "411111111111");
        let runtime = t.runtime.as_ref().unwrap();
        assert!(runtime.connected);
        assert_eq!(runtime.actual_temperature, 721);
        assert_eq!(t.remote_sensors.len(), 2);
        assert_eq!(t.remote_sensors[0].capability.len(), 3);
    }

    #[test]
    fn translate_round_trip_to_domain_model() {
        let wire: ThermostatListResponseWire = serde_json::from_str(SAMPLE).unwrap();
        let parsed: ThermostatListResponse = wire.into();
        let domain = translate(&parsed);
        assert_eq!(domain.len(), 1);
        let t = &domain[0];
        assert_eq!(t.identifier, "411111111111");
        assert_eq!(t.name, "Main Floor");
        assert!(t.connected);
        assert_eq!(t.runtime.actual_temperature, 721);
        assert_eq!(t.runtime.desired_heat, 680);
        assert_eq!(t.runtime.desired_cool, 760);
        assert_eq!(t.runtime.actual_humidity, Some(43));
        assert_eq!(t.settings.hvac_mode.as_label(), "auto");
        assert_eq!(t.sensors.len(), 2);
        assert!(t.sensors[0].in_use);
        let cap_kinds: Vec<&str> = t.sensors[0].capabilities.iter().map(|c| c.kind.as_str()).collect();
        assert_eq!(cap_kinds, vec!["temperature", "humidity", "occupancy"]);
    }

    #[test]
    fn unknown_hvac_mode_falls_through() {
        assert!(matches!(parse_hvac_mode("eco"), HvacMode::Other(_)));
        assert_eq!(parse_hvac_mode("eco").as_label(), "eco");
    }

    #[test]
    fn weather_block_parses_units_and_filters_sentinels() {
        let json = r#"{
            "thermostatList": [{
                "identifier": "1",
                "name": "X",
                "runtime": {"connected": true, "actualTemperature": 700, "desiredHeat": 0, "desiredCool": 0},
                "equipmentStatus": "compCool1,fan",
                "weather": {
                    "weatherStation": "FI:KCDW",
                    "forecasts": [{
                        "condition": "Cloudy",
                        "temperature": 645,
                        "pressure": 1017,
                        "relativeHumidity": 78,
                        "dewpoint": 575,
                        "visibility": 24000,
                        "windSpeed": 4,
                        "windGust": -5002,
                        "windBearing": 327,
                        "pop": 0,
                        "tempHigh": 645,
                        "tempLow": 566,
                        "sky": 5
                    }]
                }
            }],
            "status": {"code": 0, "message": ""}
        }"#;
        let wire: ThermostatListResponseWire = serde_json::from_str(json).unwrap();
        let parsed: ThermostatListResponse = wire.into();
        let domain = translate(&parsed);
        let t = &domain[0];

        let w = t.weather.as_ref().expect("weather populated");
        assert_eq!(w.station, "FI:KCDW", "camelCase weatherStation must parse");
        assert_eq!(w.condition, "Cloudy");
        assert_eq!(w.temperature, Some(64.5));
        assert_eq!(w.humidity, Some(78));
        assert_eq!(w.pressure_mb, Some(1017));
        assert_eq!(w.dewpoint, Some(57.5));
        assert_eq!(w.wind_speed_mph, Some(4));
        assert_eq!(w.wind_gust_mph, None, "-5002 sentinel must become None");
        assert_eq!(w.wind_bearing_degrees, Some(327));
        assert_eq!(w.visibility_meters, Some(24000));
        assert_eq!(w.probability_of_precipitation, Some(0));
        assert_eq!(w.temp_high, Some(64.5));
        assert_eq!(w.temp_low, Some(56.6));

        assert_eq!(t.equipment_running, vec!["compCool1", "fan"]);
    }

    #[test]
    fn missing_weather_block_is_none() {
        let wire: ThermostatListResponseWire = serde_json::from_str(SAMPLE).unwrap();
        let parsed: ThermostatListResponse = wire.into();
        let domain = translate(&parsed);
        assert!(domain[0].weather.is_none());
        assert!(domain[0].equipment_running.is_empty());
    }

    #[test]
    fn equipment_csv_parsing_trims_and_drops_empties() {
        assert!(parse_equipment_csv("").is_empty());
        assert_eq!(parse_equipment_csv("fan"), vec!["fan"]);
        assert_eq!(
            parse_equipment_csv(" fan , compCool1 ,"),
            vec!["fan", "compCool1"]
        );
    }

    #[test]
    fn missing_runtime_does_not_panic() {
        let json = r#"{
            "thermostatList": [
                {"identifier": "1", "name": "No Runtime"}
            ],
            "status": {"code": 0, "message": ""}
        }"#;
        let wire: ThermostatListResponseWire = serde_json::from_str(json).unwrap();
        let parsed: ThermostatListResponse = wire.into();
        let domain = translate(&parsed);
        assert_eq!(domain.len(), 1);
        assert!(!domain[0].connected);
        assert_eq!(domain[0].runtime.actual_temperature, 0);
    }
}
