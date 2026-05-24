//! HomeKit translate → collector → Prometheus smoke test using a HAP fixture.

use std::sync::Arc;

use ecobee_exporter::{
    collector::Collector,
    homekit::translate::translate_accessories,
    metrics::Metrics,
    model::HvacMode,
    provider::{FakeProvider, ThermostatProvider},
};
use housekey::accessory::Accessory;

fn fixture_accessories() -> Vec<Accessory> {
    serde_json::from_str(include_str!("fixtures/homekit_accessories.json"))
        .expect("homekit fixture JSON")
}

#[test]
fn fixture_deserializes_and_translates() {
    let thermostats = translate_accessories("ecobee", &fixture_accessories());
    assert_eq!(
        thermostats.len(),
        1,
        "non-thermostat accessories are skipped"
    );

    let t = &thermostats[0];
    assert_eq!(t.identifier, "ecobee-homekit-1");
    assert_eq!(t.name, "Main Floor");
    assert_eq!(t.runtime.actual_temperature, 721);
    assert_eq!(t.runtime.actual_humidity, Some(43));
    assert_eq!(t.settings.hvac_mode, HvacMode::Heat);
    assert_eq!(t.equipment_running, vec!["heatPump1"]);

    assert_eq!(t.sensors.len(), 3);
    let bedroom = t
        .sensors
        .iter()
        .find(|s| s.name == "Bedroom")
        .expect("bedroom temp sensor");
    assert_eq!(bedroom.capabilities[0].kind, "temperature");
    assert_eq!(bedroom.capabilities[0].value, "700");

    let basement = t
        .sensors
        .iter()
        .find(|s| s.name == "Basement")
        .expect("basement humidity sensor");
    assert_eq!(basement.capabilities[0].kind, "humidity");
    assert_eq!(basement.capabilities[0].value, "55");

    let occupancy = t
        .sensors
        .iter()
        .find(|s| s.capabilities.iter().any(|c| c.kind == "occupancy"))
        .expect("occupancy sensor");
    assert_eq!(occupancy.capabilities[0].value, "true");
}

#[tokio::test]
async fn homekit_snapshot_renders_core_metrics_without_weather() {
    let thermostats = translate_accessories("ecobee", &fixture_accessories());
    let metrics = Arc::new(Metrics::new().expect("registry"));
    let provider: Arc<dyn ThermostatProvider> = Arc::new(FakeProvider::new(thermostats));
    let collector = Collector::new(
        Arc::clone(&provider),
        Arc::clone(&metrics),
        std::time::Duration::from_mins(1),
    );
    collector.poll_once().await;

    let rendered = metrics.render().expect("encode");

    for needle in [
        "ecobee_actual_temperature",
        "ecobee_target_temperature_min",
        "ecobee_target_temperature_max",
        "ecobee_currenthvacmode",
        "ecobee_connected",
        "ecobee_temperature",
        "ecobee_humidity",
        "ecobee_occupancy",
        "ecobee_actual_humidity",
        "ecobee_equipment_running",
    ] {
        assert!(
            rendered.contains(needle),
            "missing metric `{needle}` in:\n{rendered}"
        );
    }

    assert!(
        rendered.contains("current_hvac_mode=\"heat\""),
        "expected heat mode label in:\n{rendered}"
    );
    assert!(
        rendered.contains("equipment=\"heatPump1\""),
        "expected heatPump1 equipment series"
    );
    assert!(
        rendered.contains("sensor_name=\"Bedroom\""),
        "expected bedroom sensor series"
    );
    assert!(
        !rendered.contains("ecobee_forecast_temperature"),
        "HomeKit snapshot should not emit weather metrics"
    );
}
