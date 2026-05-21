//! Prometheus metric registry.
//!
//! Metric names and labels deliberately mirror `billykwooten/ecobee-exporter`
//! so existing Grafana dashboards and alert rules transfer cleanly:
//!
//! | metric                              | labels                                                  |
//! |-------------------------------------|---------------------------------------------------------|
//! | `ecobee_fetch_time`                 | —                                                       |
//! | `ecobee_actual_temperature`         | thermostat_id, thermostat_name                          |
//! | `ecobee_target_temperature_min`     | thermostat_id, thermostat_name                          |
//! | `ecobee_target_temperature_max`     | thermostat_id, thermostat_name                          |
//! | `ecobee_currenthvacmode`            | thermostat_id, thermostat_name, current_hvac_mode       |
//! | `ecobee_temperature`                | thermostat_id, thermostat_name, sensor_id, sensor_name, sensor_type |
//! | `ecobee_humidity`                   | thermostat_id, thermostat_name, sensor_id, sensor_name, sensor_type |
//! | `ecobee_occupancy`                  | thermostat_id, thermostat_name, sensor_id, sensor_name, sensor_type |
//! | `ecobee_in_use`                     | thermostat_id, thermostat_name, sensor_id, sensor_name, sensor_type |
//!
//! `ecobee_currenthvacmode` follows the billykwooten convention of always
//! emitting value `0` and encoding the mode in a label so PromQL can match
//! on it directly.

use prometheus::{Gauge, GaugeVec, IntCounter, Opts, Registry, TextEncoder};

use crate::model::Thermostat;

const RUNTIME_LABELS: &[&str] = &["thermostat_id", "thermostat_name"];
const SENSOR_LABELS: &[&str] = &[
    "thermostat_id",
    "thermostat_name",
    "sensor_id",
    "sensor_name",
    "sensor_type",
];
const HVAC_LABELS: &[&str] = &["thermostat_id", "thermostat_name", "current_hvac_mode"];

pub struct Metrics {
    pub registry: Registry,
    fetch_time: Gauge,
    fetch_failures: IntCounter,

    actual_temperature: GaugeVec,
    target_temperature_min: GaugeVec,
    target_temperature_max: GaugeVec,
    current_hvac_mode: GaugeVec,

    temperature: GaugeVec,
    humidity: GaugeVec,
    occupancy: GaugeVec,
    in_use: GaugeVec,
}

impl Metrics {
    pub fn new() -> Result<Self, prometheus::Error> {
        let registry = Registry::new();

        let fetch_time = Gauge::with_opts(Opts::new(
            "ecobee_fetch_time",
            "elapsed seconds fetching data from the upstream API",
        ))?;
        let fetch_failures = IntCounter::new(
            "ecobee_fetch_failures_total",
            "number of failed fetches since exporter start",
        )?;

        let actual_temperature = GaugeVec::new(
            Opts::new(
                "ecobee_actual_temperature",
                "thermostat-averaged current temperature (degrees, as reported)",
            ),
            RUNTIME_LABELS,
        )?;
        let target_temperature_min = GaugeVec::new(
            Opts::new(
                "ecobee_target_temperature_min",
                "lower setpoint the thermostat is currently maintaining (degrees)",
            ),
            RUNTIME_LABELS,
        )?;
        let target_temperature_max = GaugeVec::new(
            Opts::new(
                "ecobee_target_temperature_max",
                "upper setpoint the thermostat is currently maintaining (degrees)",
            ),
            RUNTIME_LABELS,
        )?;
        let current_hvac_mode = GaugeVec::new(
            Opts::new(
                "ecobee_currenthvacmode",
                "always 0; the active HVAC mode is encoded in the current_hvac_mode label",
            ),
            HVAC_LABELS,
        )?;

        let temperature = GaugeVec::new(
            Opts::new("ecobee_temperature", "per-sensor reported temperature in degrees"),
            SENSOR_LABELS,
        )?;
        let humidity = GaugeVec::new(
            Opts::new("ecobee_humidity", "per-sensor reported humidity in percent"),
            SENSOR_LABELS,
        )?;
        let occupancy = GaugeVec::new(
            Opts::new("ecobee_occupancy", "per-sensor occupancy (0 or 1)"),
            SENSOR_LABELS,
        )?;
        let in_use = GaugeVec::new(
            Opts::new(
                "ecobee_in_use",
                "whether the sensor is being included in thermostat averages (0 or 1)",
            ),
            SENSOR_LABELS,
        )?;

        registry.register(Box::new(fetch_time.clone()))?;
        registry.register(Box::new(fetch_failures.clone()))?;
        registry.register(Box::new(actual_temperature.clone()))?;
        registry.register(Box::new(target_temperature_min.clone()))?;
        registry.register(Box::new(target_temperature_max.clone()))?;
        registry.register(Box::new(current_hvac_mode.clone()))?;
        registry.register(Box::new(temperature.clone()))?;
        registry.register(Box::new(humidity.clone()))?;
        registry.register(Box::new(occupancy.clone()))?;
        registry.register(Box::new(in_use.clone()))?;

        Ok(Self {
            registry,
            fetch_time,
            fetch_failures,
            actual_temperature,
            target_temperature_min,
            target_temperature_max,
            current_hvac_mode,
            temperature,
            humidity,
            occupancy,
            in_use,
        })
    }

    /// Replace the current values with a fresh snapshot.
    ///
    /// `GaugeVec` series for previously-seen label sets are *not* removed
    /// automatically here; if a sensor disappears between polls its last
    /// value will stick around. That mirrors how billykwooten's exporter
    /// behaves and is what Grafana users expect.
    pub fn record_snapshot(&self, thermostats: &[Thermostat], fetch_secs: f64) {
        self.fetch_time.set(fetch_secs);

        for t in thermostats {
            let runtime_labels = [t.identifier.as_str(), t.name.as_str()];

            if t.connected {
                self.actual_temperature
                    .with_label_values(&runtime_labels)
                    .set(f64::from(t.runtime.actual_temperature) / 10.0);
                self.target_temperature_min
                    .with_label_values(&runtime_labels)
                    .set(f64::from(t.runtime.desired_heat) / 10.0);
                self.target_temperature_max
                    .with_label_values(&runtime_labels)
                    .set(f64::from(t.runtime.desired_cool) / 10.0);
                self.current_hvac_mode
                    .with_label_values(&[
                        t.identifier.as_str(),
                        t.name.as_str(),
                        t.settings.hvac_mode.as_label(),
                    ])
                    .set(0.0);
            }

            for s in &t.sensors {
                let sensor_labels = [
                    t.identifier.as_str(),
                    t.name.as_str(),
                    s.id.as_str(),
                    s.name.as_str(),
                    s.sensor_type.as_str(),
                ];

                self.in_use
                    .with_label_values(&sensor_labels)
                    .set(if s.in_use { 1.0 } else { 0.0 });

                for cap in &s.capabilities {
                    match cap.kind.as_str() {
                        "temperature" => {
                            if let Ok(v) = cap.value.parse::<f64>() {
                                self.temperature
                                    .with_label_values(&sensor_labels)
                                    .set(v / 10.0);
                            } else {
                                tracing::warn!(value = %cap.value, "unparseable temperature");
                            }
                        }
                        "humidity" => {
                            if let Ok(v) = cap.value.parse::<f64>() {
                                self.humidity.with_label_values(&sensor_labels).set(v);
                            } else {
                                tracing::warn!(value = %cap.value, "unparseable humidity");
                            }
                        }
                        "occupancy" => {
                            let v = match cap.value.as_str() {
                                "true" => Some(1.0),
                                "false" => Some(0.0),
                                _ => None,
                            };
                            if let Some(v) = v {
                                self.occupancy.with_label_values(&sensor_labels).set(v);
                            } else {
                                tracing::warn!(value = %cap.value, "unparseable occupancy");
                            }
                        }
                        other => {
                            tracing::trace!(capability = %other, "ignoring sensor capability");
                        }
                    }
                }
            }
        }
    }

    pub fn record_fetch_failure(&self) {
        self.fetch_failures.inc();
    }

    /// Render the registry as Prometheus text-exposition format.
    pub fn render(&self) -> Result<String, prometheus::Error> {
        let encoder = TextEncoder::new();
        let families = self.registry.gather();
        encoder.encode_to_string(&families)
    }
}
