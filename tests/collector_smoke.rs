//! End-to-end smoke test: stand up a `FakeProvider`, run one poll cycle,
//! render the registry, and verify the expected metric series are present
//! and well-formed. This catches schema regressions in the metric layer
//! without needing a live Beehive endpoint.

use std::sync::Arc;

use ecobee_exporter::{
    collector::Collector,
    metrics::Metrics,
    provider::{FakeProvider, ThermostatProvider},
};

#[tokio::test]
async fn fake_provider_round_trip_renders_billykwooten_parity_metrics() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    let provider: Arc<dyn ThermostatProvider> = Arc::new(FakeProvider::demo());

    let collector = Collector::new(
        Arc::clone(&provider),
        Arc::clone(&metrics),
        std::time::Duration::from_mins(1),
    );
    collector.poll_once().await;

    let rendered = metrics.render().expect("encode");

    for needle in [
        "ecobee_fetch_time",
        "ecobee_actual_temperature",
        "ecobee_target_temperature_min",
        "ecobee_target_temperature_max",
        "ecobee_currenthvacmode",
        "ecobee_connected",
        "ecobee_temperature",
        "ecobee_humidity",
        "ecobee_occupancy",
        "ecobee_in_use",
        "ecobee_outdoor_temperature",
        "ecobee_outdoor_humidity",
        "ecobee_outdoor_pressure_mb",
        "ecobee_outdoor_dewpoint",
        "ecobee_outdoor_wind_speed_mph",
        "ecobee_outdoor_wind_bearing_degrees",
        "ecobee_outdoor_visibility_meters",
        "ecobee_outdoor_probability_of_precipitation",
        "ecobee_outdoor_temp_high",
        "ecobee_outdoor_temp_low",
        "ecobee_equipment_running",
    ] {
        assert!(
            rendered.contains(needle),
            "missing metric `{needle}` in:\n{rendered}"
        );
    }

    assert!(
        rendered.contains("station=\"FI:KDEMO\""),
        "weather station label missing"
    );
    assert!(
        rendered.contains("equipment=\"fan\"") && rendered.contains("equipment=\"compCool1\""),
        "expected fan + compCool1 equipment series"
    );
    // Demo windGust is None — series must NOT be emitted.
    assert!(
        !rendered.contains("ecobee_outdoor_wind_gust_mph{"),
        "wind gust should be suppressed when not reported"
    );

    // Demo data has actual_temperature = 721 tenths-of-a-degree => 72.1
    assert!(
        rendered.contains("ecobee_actual_temperature{") && rendered.contains("} 72.1"),
        "expected actual_temperature=72.1 in:\n{rendered}"
    );

    // hvac mode label is encoded as a label, value is always 0
    assert!(
        rendered.contains("current_hvac_mode=\"auto\""),
        "expected current_hvac_mode=\"auto\" label in:\n{rendered}"
    );

    // occupancy=true on the main floor sensor should render as 1
    assert!(
        rendered.contains("ecobee_occupancy{") && rendered.contains("sensor_name=\"Main Floor\""),
        "expected an ecobee_occupancy series for Main Floor in:\n{rendered}"
    );
}

#[tokio::test]
async fn empty_provider_does_not_panic() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    let provider: Arc<dyn ThermostatProvider> = Arc::new(FakeProvider::new(vec![]));
    let collector = Collector::new(
        Arc::clone(&provider),
        Arc::clone(&metrics),
        std::time::Duration::from_mins(1),
    );
    collector.poll_once().await;
    let rendered = metrics.render().expect("encode");
    assert!(rendered.contains("ecobee_fetch_time"));
}
