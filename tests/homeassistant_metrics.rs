//! Home Assistant state translation and metrics smoke test.

use std::sync::Arc;

use ecobee_exporter::{
    collector::Collector,
    homeassistant::{client::HaState, translate::translate_states},
    metrics::Metrics,
    provider::{FakeProvider, ThermostatProvider},
};

fn fixture_states() -> Vec<HaState> {
    serde_json::from_str(include_str!("fixtures/ha_states.json")).expect("fixture JSON")
}

#[test]
fn fixture_translates_climate_and_related_entities() {
    let thermostats = translate_states(&fixture_states(), &[], &[]);
    assert_eq!(thermostats.len(), 1);
    let t = &thermostats[0];
    assert_eq!(t.identifier, "climate.living_room");
    assert_eq!(t.runtime.actual_temperature, 721);
    assert!(t.weather.is_some());
    assert!(!t.sensors.is_empty());
}

#[test]
fn ha_snapshot_renders_core_metrics() {
    let thermostats = translate_states(&fixture_states(), &[], &[]);
    let metrics = Metrics::new().expect("registry");
    metrics.record_snapshot(&thermostats, 0.05);
    let rendered = metrics.render().expect("encode");

    assert!(rendered.contains("ecobee_actual_temperature"));
    assert!(rendered.contains("current_hvac_mode=\"heat\""));
    assert!(rendered.contains("ecobee_forecast_temperature"));
    assert!(rendered.contains("ecobee_forecast_relative_humidity"));
    assert!(rendered.contains("ecobee_forecast_pressure_mb"));
    assert!(rendered.contains("ecobee_forecast_dewpoint"));
    assert!(rendered.contains("ecobee_forecast_wind_speed_mph"));
    assert!(rendered.contains("ecobee_forecast_wind_bearing_degrees"));
    assert!(rendered.contains("ecobee_forecast_visibility"));
    assert!(rendered.contains("ecobee_forecast_probability_of_precipitation"));
    assert!(rendered.contains("ecobee_forecast_temp_high"));
    assert!(rendered.contains("ecobee_forecast_temp_low"));
}

#[tokio::test]
async fn ha_provider_round_trip_via_fake_snapshot() {
    let thermostats = translate_states(&fixture_states(), &[], &[]);
    let metrics = Arc::new(Metrics::new().expect("registry"));
    let provider: Arc<dyn ThermostatProvider> = Arc::new(FakeProvider::new(thermostats));
    let collector = Collector::new(
        Arc::clone(&provider),
        Arc::clone(&metrics),
        std::time::Duration::from_mins(1),
    );
    collector.poll_once().await;
    let rendered = metrics.render().expect("encode");
    assert!(rendered.contains("ecobee_fetch_time"));
}
