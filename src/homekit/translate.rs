//! Map `housekey` HAP accessory trees into [`crate::model`] types.

use housekey::accessory::{Accessory, Service};

use crate::model::{HvacMode, RemoteSensor, Runtime, SensorCapability, Settings, Thermostat};

const UUID_THERMOSTAT: &str = "0000004a-0000-1000-8000-0026bb765291";
const UUID_TEMP_SENSOR: &str = "0000008a-0000-1000-8000-0026bb765291";
const UUID_HUMIDITY_SENSOR: &str = "00000082-0000-1000-8000-0026bb765291";
const UUID_OCCUPANCY_SENSOR: &str = "00000086-0000-1000-8000-0026bb765291";
const UUID_ACCESSORY_INFO: &str = "0000003e-0000-1000-8000-0026bb765291";

const CHAR_CURRENT_TEMP: &str = "00000011-0000-1000-8000-0026bb765291";
const CHAR_TARGET_HEAT: &str = "0000000c-0000-1000-8000-0026bb765291";
const CHAR_TARGET_COOL: &str = "0000000d-0000-1000-8000-0026bb765291";
const CHAR_CURRENT_HUMIDITY: &str = "00000010-0000-1000-8000-0026bb765291";
const CHAR_TARGET_HVAC: &str = "00000033-0000-1000-8000-0026bb765291";
const CHAR_CURRENT_HVAC: &str = "0000000f-0000-1000-8000-0026bb765291";
const CHAR_NAME: &str = "00000023-0000-1000-8000-0026bb765291";
const CHAR_SERIAL: &str = "00000030-0000-1000-8000-0026bb765291";
const CHAR_OCCUPANCY: &str = "00000071-0000-1000-8000-0026bb765291";

pub fn translate_accessories(alias: &str, accessories: &[Accessory]) -> Vec<Thermostat> {
    accessories
        .iter()
        .filter_map(|accessory| translate_accessory(alias, accessory))
        .collect()
}

fn translate_accessory(alias: &str, accessory: &Accessory) -> Option<Thermostat> {
    let thermostat = accessory
        .services
        .iter()
        .find(|s| uuid_eq(&s.service_type, UUID_THERMOSTAT))?;

    let info = accessory
        .services
        .iter()
        .find(|s| uuid_eq(&s.service_type, UUID_ACCESSORY_INFO));

    let name = info
        .and_then(|s| char_string(s, CHAR_NAME))
        .unwrap_or_else(|| alias.to_string());
    let identifier = info
        .and_then(|s| char_string(s, CHAR_SERIAL))
        .unwrap_or_else(|| format!("{alias}-{}", accessory.aid));

    let actual_c = char_f64(thermostat, CHAR_CURRENT_TEMP);
    let heat_c = char_f64(thermostat, CHAR_TARGET_HEAT);
    let cool_c = char_f64(thermostat, CHAR_TARGET_COOL);
    let actual = actual_c.map_or(0, celsius_to_tenths_f);
    let desired_heat = heat_c.map_or(actual, celsius_to_tenths_f);
    let desired_cool = cool_c.map_or(actual, celsius_to_tenths_f);

    let target_mode = char_u8(thermostat, CHAR_TARGET_HVAC).map(parse_target_hvac);
    let current_action = char_u8(thermostat, CHAR_CURRENT_HVAC).map(parse_current_hvac);

    let mut sensors = Vec::new();
    for service in &accessory.services {
        let sensor_name = char_string(service, CHAR_NAME).unwrap_or_else(|| "sensor".into());
        let sensor_id = format!("{}:{}", accessory.aid, service.iid);
        let mut sensor = RemoteSensor {
            id: sensor_id,
            name: sensor_name,
            sensor_type: "homekit_sensor".into(),
            in_use: false,
            capabilities: vec![],
        };

        if uuid_eq(&service.service_type, UUID_TEMP_SENSOR) {
            if let Some(temp) = char_f64(service, CHAR_CURRENT_TEMP) {
                sensor.capabilities.push(SensorCapability {
                    kind: "temperature".into(),
                    value: celsius_to_tenths_f(temp).to_string(),
                });
            }
            sensors.push(sensor);
        } else if uuid_eq(&service.service_type, UUID_HUMIDITY_SENSOR) {
            if let Some(h) = char_f64(service, CHAR_CURRENT_HUMIDITY) {
                sensor.capabilities.push(SensorCapability {
                    kind: "humidity".into(),
                    value: humidity_percent(h).to_string(),
                });
            }
            sensors.push(sensor);
        } else if uuid_eq(&service.service_type, UUID_OCCUPANCY_SENSOR) {
            if let Some(occ) = char_bool(service, CHAR_OCCUPANCY) {
                sensor.capabilities.push(SensorCapability {
                    kind: "occupancy".into(),
                    value: occ.to_string(),
                });
            }
            sensors.push(sensor);
        }
    }

    Some(Thermostat {
        identifier,
        name,
        connected: true,
        runtime: Runtime {
            actual_temperature: actual,
            desired_heat,
            desired_cool,
            actual_humidity: char_f64(thermostat, CHAR_CURRENT_HUMIDITY).map(humidity_percent),
            desired_humidity: None,
            desired_dehumidity: None,
            raw_temperature: None,
            desired_fan_mode: None,
        },
        settings: Settings {
            hvac_mode: target_mode.unwrap_or(HvacMode::Off),
            follow_me_comfort: false,
            smart_circulation: false,
            heat_stages: None,
            cool_stages: None,
        },
        sensors,
        weather: None,
        equipment_running: equipment_from_action(current_action),
        program: None,
        hold: None,
        extended_runtime: None,
        alerts: vec![],
    })
}

fn uuid_eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn char_value<'a>(service: &'a Service, char_type: &str) -> Option<&'a serde_json::Value> {
    service
        .characteristics
        .iter()
        .find(|c| uuid_eq(&c.char_type, char_type))
        .and_then(|c| c.value.as_ref())
}

fn char_f64(service: &Service, char_type: &str) -> Option<f64> {
    char_value(service, char_type).and_then(serde_json::Value::as_f64)
}

fn char_u8(service: &Service, char_type: &str) -> Option<u8> {
    char_value(service, char_type)
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| u8::try_from(n).ok())
}

fn char_bool(service: &Service, char_type: &str) -> Option<bool> {
    char_value(service, char_type).and_then(serde_json::Value::as_bool)
}

fn char_string(service: &Service, char_type: &str) -> Option<String> {
    char_value(service, char_type).and_then(|v| v.as_str().map(str::to_string))
}

fn celsius_to_tenths_f(c: f64) -> i32 {
    let f = c.mul_add(9.0 / 5.0, 32.0);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "thermostat temps fit i32 tenths"
    )]
    {
        f.mul_add(10.0, 0.0).round() as i32
    }
}

fn humidity_percent(h: f64) -> i32 {
    #[allow(clippy::cast_possible_truncation, reason = "humidity is 0-100 percent")]
    {
        h.round() as i32
    }
}

fn parse_target_hvac(mode: u8) -> HvacMode {
    match mode {
        0 => HvacMode::Off,
        1 => HvacMode::Heat,
        2 => HvacMode::Cool,
        3 => HvacMode::Auto,
        other => HvacMode::Other(other.to_string()),
    }
}

fn parse_current_hvac(mode: u8) -> &'static str {
    match mode {
        1 => "heating",
        2 => "cooling",
        _ => "idle",
    }
}

fn equipment_from_action(action: Option<&str>) -> Vec<String> {
    match action {
        Some("heating") => vec!["heatPump1".into()],
        Some("cooling") => vec!["compCool1".into()],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekey::accessory::{Accessory, Characteristic, Service};

    const FIXTURE: &str = include_str!("../../tests/fixtures/homekit_accessories.json");

    fn char(iid: u64, char_type: &str, value: serde_json::Value) -> Characteristic {
        Characteristic {
            iid,
            char_type: char_type.into(),
            perms: vec!["pr".into()],
            value: Some(value),
            format: None,
            unit: None,
            min_value: None,
            max_value: None,
            min_step: None,
        }
    }

    fn thermostat_service(
        current_c: f64,
        heat_c: f64,
        cool_c: f64,
        humidity: f64,
        target_hvac: u8,
        current_hvac: u8,
    ) -> Service {
        Service {
            iid: 10,
            service_type: UUID_THERMOSTAT.into(),
            characteristics: vec![
                char(1, CHAR_CURRENT_TEMP, serde_json::json!(current_c)),
                char(2, CHAR_TARGET_HEAT, serde_json::json!(heat_c)),
                char(3, CHAR_TARGET_COOL, serde_json::json!(cool_c)),
                char(4, CHAR_CURRENT_HUMIDITY, serde_json::json!(humidity)),
                char(5, CHAR_TARGET_HVAC, serde_json::json!(target_hvac)),
                char(6, CHAR_CURRENT_HVAC, serde_json::json!(current_hvac)),
            ],
        }
    }

    fn fixture_accessories() -> Vec<Accessory> {
        serde_json::from_str(FIXTURE).expect("fixture JSON")
    }

    #[test]
    fn celsius_to_tenths_f_rounds_correctly() {
        assert_eq!(celsius_to_tenths_f(0.0), 320);
        assert_eq!(celsius_to_tenths_f(22.277_777_777_777_78), 721);
        assert_eq!(celsius_to_tenths_f(21.111_111_111_111_11), 700);
    }

    #[test]
    fn parse_target_hvac_maps_known_modes() {
        assert_eq!(parse_target_hvac(0), HvacMode::Off);
        assert_eq!(parse_target_hvac(1), HvacMode::Heat);
        assert_eq!(parse_target_hvac(2), HvacMode::Cool);
        assert_eq!(parse_target_hvac(3), HvacMode::Auto);
        assert_eq!(parse_target_hvac(9), HvacMode::Other("9".into()));
    }

    #[test]
    fn skips_accessories_without_thermostat_service() {
        let accessories = fixture_accessories();
        assert_eq!(accessories.len(), 2);
        let thermostats = translate_accessories("ecobee", &accessories);
        assert_eq!(thermostats.len(), 1);
    }

    #[test]
    fn translate_fixture_accessory() {
        let thermostats = translate_accessories("ecobee", &fixture_accessories());
        let t = &thermostats[0];
        assert_eq!(t.identifier, "ecobee-homekit-1");
        assert_eq!(t.name, "Main Floor");
        assert_eq!(t.runtime.actual_temperature, 721);
        assert_eq!(t.runtime.desired_heat, 680);
        assert_eq!(t.runtime.desired_cool, 760);
        assert_eq!(t.runtime.actual_humidity, Some(43));
        assert_eq!(t.settings.hvac_mode, HvacMode::Heat);
        assert_eq!(t.equipment_running, vec!["heatPump1"]);
        assert_eq!(t.sensors.len(), 3);
    }

    #[test]
    fn falls_back_to_alias_when_accessory_info_missing() {
        let accessory = Accessory {
            aid: 9,
            services: vec![thermostat_service(20.0, 20.0, 24.0, 40.0, 3, 0)],
        };
        let t = translate_accessories("my-alias", &[accessory])
            .pop()
            .expect("thermostat");
        assert_eq!(t.name, "my-alias");
        assert_eq!(t.identifier, "my-alias-9");
        assert_eq!(t.settings.hvac_mode, HvacMode::Auto);
        assert!(t.equipment_running.is_empty());
    }

    #[test]
    fn maps_cooling_equipment_and_auto_mode() {
        let accessory = Accessory {
            aid: 1,
            services: vec![thermostat_service(
                22.277_777_777_777_78,
                20.0,
                24.444_444_444_444_443,
                43.0,
                3,
                2,
            )],
        };
        let t = translate_accessories("ecobee", &[accessory])
            .pop()
            .expect("thermostat");
        assert_eq!(t.settings.hvac_mode, HvacMode::Auto);
        assert_eq!(t.equipment_running, vec!["compCool1"]);
    }

    #[test]
    fn translated_snapshot_renders_core_metrics() {
        use crate::metrics::Metrics;

        let thermostats = translate_accessories("ecobee", &fixture_accessories());
        let metrics = Metrics::new().expect("registry");
        metrics.record_snapshot(&thermostats, 0.05);
        let rendered = metrics.render().expect("encode");

        assert!(rendered.contains("ecobee_actual_temperature"));
        assert!(rendered.contains("current_hvac_mode=\"heat\""));
        assert!(rendered.contains("sensor_name=\"Bedroom\""));
        assert!(!rendered.contains("ecobee_outdoor_temperature"));
    }
}
