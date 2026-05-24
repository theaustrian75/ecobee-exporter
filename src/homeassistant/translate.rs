//! Map Home Assistant `/api/states` payloads into [`crate::model`] thermostats.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::model::{
    HvacMode, Program, RemoteSensor, Runtime, SensorCapability, Settings, Thermostat, Weather,
};

use super::client::{DeviceGraph, HaState};

pub fn translate_states(
    states: &[HaState],
    climate_entities: &[String],
    weather_entities: &[String],
    device_graph: &DeviceGraph,
) -> Vec<Thermostat> {
    let by_id: HashMap<&str, &HaState> = states
        .iter()
        .map(|s| (s.entity_id.as_str(), s))
        .collect();

    let climates: Vec<&HaState> = if climate_entities.is_empty() {
        states
            .iter()
            .filter(|s| domain(s) == Some("climate"))
            .collect()
    } else {
        climate_entities
            .iter()
            .filter_map(|id| by_id.get(id.as_str()).copied())
            .collect()
    };

    let weather = find_weather_states(states);

    climates
        .into_iter()
        .filter_map(|climate| {
            let stem = entity_stem(&climate.entity_id)?;
            let name = attr_string(&climate.attributes, "friendly_name")
                .unwrap_or_else(|| humanize_stem(stem));
            let sensors = collect_related_sensors(states, climate, stem, &name, device_graph);
            let linked_weather = resolve_weather_state(&weather, weather_entities, stem, &name)
                .map(translate_weather);

            Some(build_thermostat(climate, &name, sensors, linked_weather))
        })
        .collect()
}

fn build_thermostat(
    climate: &HaState,
    name: &str,
    sensors: Vec<RemoteSensor>,
    weather: Option<Weather>,
) -> Thermostat {
    let attrs = &climate.attributes;
    let unit = temperature_unit(attrs);
    let actual = attr_f64(attrs, "current_temperature").map_or(0, |t| temp_to_tenths(t, unit));
    let (desired_heat, desired_cool) = target_temps(attrs, unit, actual);

    let hvac_mode = parse_hvac_mode(&climate.state);
    let equipment_running = equipment_from_attrs(attrs, &climate.state);

    let program = program_from_attrs(attrs);
    let desired_fan_mode = attr_string(attrs, "fan_mode");

    Thermostat {
        identifier: climate.entity_id.clone(),
        name: name.to_string(),
        connected: climate.state != "unavailable",
        runtime: Runtime {
            actual_temperature: actual,
            desired_heat,
            desired_cool,
            actual_humidity: attr_f64(attrs, "humidity").map(humidity_percent),
            desired_humidity: None,
            desired_dehumidity: None,
            raw_temperature: None,
            desired_fan_mode,
        },
        settings: Settings {
            hvac_mode,
            follow_me_comfort: false,
            smart_circulation: false,
            heat_stages: None,
            cool_stages: None,
        },
        sensors,
        weather,
        equipment_running,
        program,
        hold: None,
        extended_runtime: None,
        alerts: vec![],
    }
}

fn collect_related_sensors(
    states: &[HaState],
    climate: &HaState,
    stem: &str,
    thermostat_name: &str,
    device_graph: &DeviceGraph,
) -> Vec<RemoteSensor> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let related_devices = device_graph.related_device_ids(&climate.entity_id);
    let use_device_matching = !related_devices.is_empty();

    for state in states {
        let Some(domain) = domain(state) else {
            continue;
        };
        if !matches!(domain, "sensor" | "binary_sensor") {
            continue;
        }
        if is_blocked_entity(&state.entity_id) {
            continue;
        }
        let matches = if use_device_matching {
            device_graph.entity_on_devices(&state.entity_id, &related_devices)
        } else {
            entity_relates_to_thermostat(&state.entity_id, stem, thermostat_name)
        };
        if !matches {
            continue;
        }
        if !seen.insert(state.entity_id.clone()) {
            continue;
        }
        if let Some(sensor) = sensor_from_state(state) {
            out.push(sensor);
        }
    }

    out
}

const ENTITY_ID_BLOCKLIST: &[&str] = &[
    "roku",
    "android_tv",
    "apple_tv",
    "chromecast",
    "fire_tv",
    "nvidia_shield",
    "bravia",
    "denon",
    "receiver",
    "shield",
    "harmony",
    "remote_",
];

fn is_blocked_entity(entity_id: &str) -> bool {
    let id = entity_id.to_ascii_lowercase();
    ENTITY_ID_BLOCKLIST.iter().any(|needle| id.contains(needle))
}

fn entity_relates_to_thermostat(entity_id: &str, stem: &str, thermostat_name: &str) -> bool {
    if is_blocked_entity(entity_id) {
        return false;
    }
    let id = entity_id.to_ascii_lowercase();
    let stem = stem.to_ascii_lowercase();
    if id.contains(&stem) {
        return is_climate_sensor_entity(entity_id);
    }
    let name_slug = slugify(thermostat_name);
    !name_slug.is_empty() && id.contains(&name_slug) && is_climate_sensor_entity(entity_id)
}

fn is_climate_sensor_entity(entity_id: &str) -> bool {
    let id = entity_id.to_ascii_lowercase();
    id.contains("temperature")
        || id.contains("humidity")
        || id.contains("occupancy")
        || id.contains("motion")
        || id.contains("presence")
        || id.ends_with("_contact")
}

fn sensor_from_state(state: &HaState) -> Option<RemoteSensor> {
    let name = attr_string(&state.attributes, "friendly_name")
        .unwrap_or_else(|| state.entity_id.clone());
    let device_class = attr_string(&state.attributes, "device_class");
    let unit = state
        .attributes
        .get("unit_of_measurement")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut capabilities = Vec::new();

    match device_class.as_deref() {
        Some("temperature") => {
            if let Ok(temp) = state.state.parse::<f64>() {
                capabilities.push(SensorCapability {
                    kind: "temperature".into(),
                    value: temp_to_tenths(temp, unit).to_string(),
                });
            }
        }
        Some("humidity") => {
            if let Ok(h) = state.state.parse::<f64>() {
                capabilities.push(SensorCapability {
                    kind: "humidity".into(),
                    value: humidity_percent(h).to_string(),
                });
            }
        }
        Some("occupancy" | "motion" | "presence") => {
            capabilities.push(SensorCapability {
                kind: "occupancy".into(),
                value: state.state.eq_ignore_ascii_case("on").to_string(),
            });
        }
        _ => {
            if unit.contains('F') || unit.contains('C') || unit.contains('°') {
                if let Ok(temp) = state.state.parse::<f64>() {
                    capabilities.push(SensorCapability {
                        kind: "temperature".into(),
                        value: temp_to_tenths(temp, unit).to_string(),
                    });
                }
            } else if state.entity_id.contains("humidity") {
                if let Ok(h) = state.state.parse::<f64>() {
                    capabilities.push(SensorCapability {
                        kind: "humidity".into(),
                        value: humidity_percent(h).to_string(),
                    });
                }
            } else if state.entity_id.contains("occupancy")
                && domain(state) == Some("binary_sensor")
            {
                capabilities.push(SensorCapability {
                    kind: "occupancy".into(),
                    value: state.state.eq_ignore_ascii_case("on").to_string(),
                });
            }
        }
    }

    if capabilities.is_empty() {
        return None;
    }

    Some(RemoteSensor {
        id: state.entity_id.clone(),
        name,
        sensor_type: device_class.unwrap_or_else(|| "homeassistant".into()),
        in_use: false,
        capabilities,
    })
}

fn find_weather_states(states: &[HaState]) -> Vec<&HaState> {
    states
        .iter()
        .filter(|s| domain(s) == Some("weather"))
        .collect()
}

fn is_global_weather_entity(entity_id: &str) -> bool {
    let id = entity_id.to_ascii_lowercase();
    id == "weather.ecobee" || id.contains("ecobee")
}

fn weather_specific_match(weather: &HaState, stem: &str, thermostat_name: &str) -> bool {
    if is_global_weather_entity(&weather.entity_id) {
        return false;
    }
    weather_matches_thermostat(weather, stem, thermostat_name)
}

fn resolve_weather_state<'a>(
    weather_states: &[&'a HaState],
    weather_entities: &[String],
    stem: &str,
    thermostat_name: &str,
) -> Option<&'a HaState> {
    if !weather_entities.is_empty() {
        let configured: Vec<&HaState> = weather_entities
            .iter()
            .filter_map(|id| weather_states.iter().copied().find(|s| s.entity_id == *id))
            .collect();
        if let Some(w) = configured
            .iter()
            .copied()
            .find(|w| weather_specific_match(w, stem, thermostat_name))
        {
            return Some(w);
        }
        if let Some(w) = configured
            .iter()
            .copied()
            .find(|w| is_global_weather_entity(&w.entity_id))
        {
            return Some(w);
        }
        return configured.first().copied();
    }

    if let Some(w) = weather_states
        .iter()
        .copied()
        .find(|w| weather_specific_match(w, stem, thermostat_name))
    {
        return Some(w);
    }

    if let Some(w) = weather_states
        .iter()
        .copied()
        .find(|w| is_global_weather_entity(&w.entity_id))
    {
        return Some(w);
    }

    if weather_states.len() == 1 {
        return weather_states.first().copied();
    }

    weather_states
        .iter()
        .copied()
        .find(|w| weather_matches_thermostat(w, stem, thermostat_name))
}

fn weather_matches_thermostat(weather: &HaState, stem: &str, thermostat_name: &str) -> bool {
    let id = weather.entity_id.to_ascii_lowercase();
    if id == "weather.ecobee" || id.contains("ecobee") {
        return true;
    }
    let stem = stem.to_ascii_lowercase();
    if id.contains(&stem) {
        return true;
    }
    let name_slug = slugify(thermostat_name);
    !name_slug.is_empty() && id.contains(&name_slug)
}

fn translate_weather(state: &HaState) -> Weather {
    let attrs = &state.attributes;
    let temp_unit = weather_temp_unit(attrs);
    let wind_unit = weather_wind_unit(attrs);
    let visibility_unit = weather_visibility_unit(attrs);
    let forecast = forecast_first(attrs);

    Weather {
        station: attr_string(attrs, "friendly_name").unwrap_or_else(|| state.entity_id.clone()),
        condition: if state.state.is_empty() || state.state == "unknown" {
            "unknown".into()
        } else {
            state.state.clone()
        },
        temperature: attr_f64(attrs, "temperature").map(|t| weather_temp_degrees(t, temp_unit)),
        humidity: attr_f64(attrs, "humidity").map(humidity_percent),
        pressure_mb: attr_f64(attrs, "pressure").map(round_i32),
        dewpoint: attr_f64(attrs, "dew_point")
            .or_else(|| attr_f64(attrs, "dewpoint"))
            .map(|t| weather_temp_degrees(t, temp_unit)),
        wind_speed_mph: attr_f64(attrs, "wind_speed").map(|w| wind_to_mph(w, wind_unit)),
        wind_gust_mph: attr_f64(attrs, "wind_gust_speed")
            .or_else(|| attr_f64(attrs, "wind_gust"))
            .map(|w| wind_to_mph(w, wind_unit)),
        wind_bearing_degrees: attr_f64(attrs, "wind_bearing").map(round_i32),
        visibility_meters: attr_f64(attrs, "visibility").map(|v| {
            visibility_to_meters(v, visibility_unit)
        }),
        probability_of_precipitation: attr_f64(attrs, "precipitation_probability")
            .or_else(|| forecast.and_then(|day| attr_f64(day, "precipitation_probability")))
            .map(round_i32),
        temp_high: forecast
            .and_then(|day| attr_f64(day, "temperature"))
            .map(|t| weather_temp_degrees(t, temp_unit)),
        temp_low: forecast
            .and_then(|day| attr_f64(day, "templow"))
            .map(|t| weather_temp_degrees(t, temp_unit)),
        sky: None,
    }
}

fn forecast_first(attrs: &Value) -> Option<&Value> {
    attrs
        .get("forecast")
        .and_then(|f| f.as_array())
        .and_then(|items| items.first())
}

fn program_from_attrs(attrs: &Value) -> Option<Program> {
    let current = attr_string(attrs, "preset_mode")
        .or_else(|| attr_string(attrs, "climate_mode"))
        .filter(|mode| !mode.eq_ignore_ascii_case("unknown"));
    current.map(|current_climate_ref| Program {
        current_climate_ref: Some(current_climate_ref),
    })
}

fn equipment_from_attrs(attrs: &Value, hvac_state: &str) -> Vec<String> {
    if let Some(raw) = attr_string(attrs, "equipment_running") {
        return raw
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect();
    }

    match attr_string(attrs, "hvac_action").as_deref() {
        Some("heating") => vec!["heatPump1".into()],
        Some("cooling") => vec!["compCool1".into()],
        _ if hvac_state.eq_ignore_ascii_case("heat") => vec!["heatPump1".into()],
        _ if hvac_state.eq_ignore_ascii_case("cool") => vec!["compCool1".into()],
        _ => vec![],
    }
}

fn target_temps(attrs: &Value, unit: &str, fallback: i32) -> (i32, i32) {
    let low = attr_f64(attrs, "target_temp_low")
        .or_else(|| attr_f64(attrs, "target_temperature_low"))
        .map(|t| temp_to_tenths(t, unit));
    let high = attr_f64(attrs, "target_temp_high")
        .or_else(|| attr_f64(attrs, "target_temperature_high"))
        .map(|t| temp_to_tenths(t, unit));
    if let (Some(low), Some(high)) = (low, high) {
        return (low, high);
    }

    let single = attr_f64(attrs, "temperature").map(|t| temp_to_tenths(t, unit));
    match (single, low, high) {
        (Some(t), _, _) if attrs.get("target_temp_low").is_none() => (t, t),
        (Some(t), None, Some(h)) => (t, h),
        (Some(t), Some(l), None) => (l, t),
        (_, Some(l), Some(h)) => (l, h),
        (Some(t), _, _) => (t, t),
        _ => (fallback, fallback),
    }
}

fn parse_hvac_mode(state: &str) -> HvacMode {
    match state.to_ascii_lowercase().as_str() {
        "off" => HvacMode::Off,
        "heat" => HvacMode::Heat,
        "cool" => HvacMode::Cool,
        "auto" | "heat_cool" => HvacMode::Auto,
        "aux_heat_only" => HvacMode::AuxHeatOnly,
        other if other.is_empty() || other == "unknown" || other == "unavailable" => HvacMode::Off,
        other => HvacMode::Other(other.to_string()),
    }
}

fn temperature_unit(attrs: &Value) -> &str {
    attrs
        .get("unit_of_measurement")
        .or_else(|| attrs.get("temperature_unit"))
        .and_then(|v| v.as_str())
        .unwrap_or("°F")
}

fn temp_to_tenths(temp: f64, unit: &str) -> i32 {
    let fahrenheit = if unit.contains('C') {
        temp.mul_add(9.0 / 5.0, 32.0)
    } else {
        temp
    };
    #[allow(clippy::cast_possible_truncation, reason = "thermostat temps fit i32 tenths")]
    {
        (fahrenheit * 10.0).round() as i32
    }
}

fn humidity_percent(h: f64) -> i32 {
    #[allow(clippy::cast_possible_truncation, reason = "humidity is 0-100 percent")]
    {
        h.round() as i32
    }
}

fn round_i32(v: f64) -> i32 {
    #[allow(clippy::cast_possible_truncation, reason = "weather values fit i32 after rounding")]
    {
        v.round() as i32
    }
}

fn weather_temp_unit(attrs: &Value) -> &str {
    attrs
        .get("temperature_unit")
        .and_then(|v| v.as_str())
        .unwrap_or("°F")
}

fn weather_wind_unit(attrs: &Value) -> &str {
    attrs
        .get("wind_speed_unit")
        .and_then(|v| v.as_str())
        .unwrap_or("mph")
}

fn weather_visibility_unit(attrs: &Value) -> &str {
    attrs
        .get("visibility_unit")
        .and_then(|v| v.as_str())
        .unwrap_or("km")
}

fn weather_temp_degrees(temp: f64, unit: &str) -> f64 {
    if unit.contains('C') {
        temp.mul_add(9.0 / 5.0, 32.0)
    } else {
        temp
    }
}

fn wind_to_mph(speed: f64, unit: &str) -> i32 {
    let mph = if unit.contains("km") {
        speed * 0.621_371
    } else if unit.contains("m/s") || unit == "mps" {
        speed * 2.236_94
    } else if unit.contains("ft") {
        speed * 0.681_818
    } else {
        speed
    };
    round_i32(mph)
}

fn visibility_to_meters(visibility: f64, unit: &str) -> i32 {
    let meters = if unit.contains("mi") {
        visibility * 1609.34
    } else if unit.contains("km") {
        visibility * 1000.0
    } else if unit.contains("ft") {
        visibility * 0.3048
    } else {
        visibility
    };
    round_i32(meters)
}

fn domain(state: &HaState) -> Option<&str> {
    state.entity_id.split('.').next()
}

fn entity_stem(entity_id: &str) -> Option<&str> {
    entity_id.split('.').nth(1)
}

fn attr_string(attrs: &Value, key: &str) -> Option<String> {
    attrs
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn attr_f64(attrs: &Value, key: &str) -> Option<f64> {
    attrs.get(key).and_then(|v| match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    })
}

fn humanize_stem(stem: &str) -> String {
    stem.replace('_', " ")
}

fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('_');
            last_dash = true;
        }
    }
    out.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::homeassistant::client::{DeviceGraph, HaState};

    fn state(entity_id: &str, state: &str, attributes: Value) -> HaState {
        HaState {
            entity_id: entity_id.into(),
            state: state.into(),
            attributes,
        }
    }

    #[test]
    fn maps_ecobee_cloud_climate_and_sensors() {
        let states = vec![
            state(
                "climate.living_room",
                "heat",
                serde_json::json!({
                    "friendly_name": "Living Room",
                    "current_temperature": 72.1,
                    "temperature": 70.0,
                    "target_temp_high": 74.0,
                    "target_temp_low": 68.0,
                    "humidity": 43,
                    "hvac_action": "heating",
                    "equipment_running": "heatPump,fan",
                    "preset_mode": "home",
                    "temperature_unit": "°F"
                }),
            ),
            state(
                "sensor.living_room_bedroom_temperature",
                "68.5",
                serde_json::json!({
                    "friendly_name": "Bedroom",
                    "device_class": "temperature",
                    "unit_of_measurement": "°F"
                }),
            ),
            state(
                "binary_sensor.living_room_bedroom_occupancy",
                "on",
                serde_json::json!({
                    "friendly_name": "Bedroom Occupancy",
                    "device_class": "occupancy"
                }),
            ),
            state(
                "weather.ecobee",
                "cloudy",
                serde_json::json!({
                    "friendly_name": "Ecobee Weather",
                    "temperature": 64.5,
                    "humidity": 78,
                    "pressure": 1017.0,
                    "wind_speed": 4.0,
                    "wind_bearing": 327.0,
                    "dew_point": 58.2,
                    "visibility": 16.0,
                    "visibility_unit": "km",
                    "wind_speed_unit": "mph",
                    "temperature_unit": "°F",
                    "forecast": [{
                        "temperature": 72.0,
                        "templow": 55.0,
                        "precipitation_probability": 20
                    }]
                }),
            ),
        ];

        let thermostats = translate_states(&states, &[], &[], &DeviceGraph::default());
        assert_eq!(thermostats.len(), 1);
        let t = &thermostats[0];
        assert_eq!(t.identifier, "climate.living_room");
        assert_eq!(t.name, "Living Room");
        assert_eq!(t.runtime.actual_temperature, 721);
        assert_eq!(t.runtime.desired_heat, 680);
        assert_eq!(t.runtime.desired_cool, 740);
        assert_eq!(t.settings.hvac_mode, HvacMode::Heat);
        assert_eq!(t.equipment_running, vec!["heatPump", "fan"]);
        assert_eq!(
            t.program.as_ref().and_then(|p| p.current_climate_ref.as_deref()),
            Some("home")
        );
        assert_eq!(t.sensors.len(), 2);
        assert!(t.weather.is_some());
        let w = t.weather.as_ref().expect("weather");
        assert_eq!(w.station, "Ecobee Weather");
        assert_eq!(w.temperature, Some(64.5));
        assert_eq!(w.humidity, Some(78));
        assert_eq!(w.pressure_mb, Some(1017));
        assert_eq!(w.dewpoint, Some(58.2));
        assert_eq!(w.wind_speed_mph, Some(4));
        assert_eq!(w.wind_bearing_degrees, Some(327));
        assert_eq!(w.visibility_meters, Some(16000));
        assert_eq!(w.probability_of_precipitation, Some(20));
        assert_eq!(w.temp_high, Some(72.0));
        assert_eq!(w.temp_low, Some(55.0));
    }

    #[test]
    fn honors_explicit_climate_entity_filter() {
        let states = vec![
            state("climate.one", "off", serde_json::json!({"current_temperature": 70.0})),
            state("climate.two", "off", serde_json::json!({"current_temperature": 71.0})),
        ];
        let thermostats = translate_states(&states, &["climate.two".into()], &[], &DeviceGraph::default());
        assert_eq!(thermostats.len(), 1);
        assert_eq!(thermostats[0].identifier, "climate.two");
    }

    #[test]
    fn maps_homekit_climate_hvac_action() {
        let states = vec![state(
            "climate.upstairs_hallway",
            "cool",
            serde_json::json!({
                "friendly_name": "Upstairs Hallway",
                "current_temperature": 74.0,
                "temperature": 74.0,
                "humidity": 50,
                "hvac_action": "cooling",
                "temperature_unit": "°F"
            }),
        )];
        let t = translate_states(&states, &[], &[], &DeviceGraph::default()).pop().expect("thermostat");
        assert_eq!(t.settings.hvac_mode, HvacMode::Cool);
        assert_eq!(t.equipment_running, vec!["compCool1"]);
    }

    fn device_graph(entity_json: &str, device_json: &str) -> DeviceGraph {
        DeviceGraph::parse(entity_json, device_json).expect("device graph")
    }

    #[test]
    fn excludes_roku_sensors_even_when_stem_matches() {
        let states = vec![
            state(
                "climate.living_room",
                "heat",
                serde_json::json!({
                    "friendly_name": "Living Room",
                    "current_temperature": 72.0,
                    "temperature_unit": "°F"
                }),
            ),
            state(
                "sensor.living_room_bedroom_temperature",
                "68.0",
                serde_json::json!({
                    "friendly_name": "Bedroom",
                    "device_class": "temperature",
                    "unit_of_measurement": "°F"
                }),
            ),
            state(
                "sensor.living_room_roku_active",
                "on",
                serde_json::json!({
                    "friendly_name": "Living Room Roku",
                    "device_class": "power"
                }),
            ),
        ];

        let t = translate_states(&states, &[], &[], &DeviceGraph::default())
            .pop()
            .expect("thermostat");
        assert_eq!(t.sensors.len(), 1);
        assert_eq!(t.sensors[0].name, "Bedroom");
    }

    #[test]
    fn includes_homekit_remote_sensors_via_device_graph() {
        let states = vec![
            state(
                "climate.living_room",
                "heat",
                serde_json::json!({
                    "friendly_name": "Living Room",
                    "current_temperature": 72.0,
                    "temperature_unit": "°F"
                }),
            ),
            state(
                "sensor.bedroom_temperature",
                "68.0",
                serde_json::json!({
                    "friendly_name": "Bedroom",
                    "device_class": "temperature",
                    "unit_of_measurement": "°F"
                }),
            ),
            state(
                "binary_sensor.bedroom_occupancy",
                "on",
                serde_json::json!({
                    "friendly_name": "Bedroom Occupancy",
                    "device_class": "occupancy"
                }),
            ),
            state(
                "sensor.living_room_roku_active",
                "on",
                serde_json::json!({
                    "friendly_name": "Living Room Roku",
                    "device_class": "power"
                }),
            ),
        ];
        let graph = device_graph(
            r#"[
              {"entity_id":"climate.living_room","device_id":"therm"},
              {"entity_id":"sensor.bedroom_temperature","device_id":"remote1"},
              {"entity_id":"binary_sensor.bedroom_occupancy","device_id":"remote1"},
              {"entity_id":"sensor.living_room_roku_active","device_id":"roku1"}
            ]"#,
            r#"[
              {"id":"therm","via_device_id":null},
              {"id":"remote1","via_device_id":"therm"},
              {"id":"roku1","via_device_id":null}
            ]"#,
        );

        let t = translate_states(&states, &[], &[], &graph)
            .pop()
            .expect("thermostat");
        assert_eq!(t.sensors.len(), 2);
        let names: Vec<_> = t.sensors.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Bedroom"));
        assert!(names.contains(&"Bedroom Occupancy"));
    }
}
