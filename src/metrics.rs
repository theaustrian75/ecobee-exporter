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
const WEATHER_LABELS: &[&str] = &["thermostat_id", "thermostat_name", "station"];
const SENSOR_LABELS: &[&str] = &[
    "thermostat_id",
    "thermostat_name",
    "sensor_id",
    "sensor_name",
    "sensor_type",
];
const HVAC_LABELS: &[&str] = &["thermostat_id", "thermostat_name", "current_hvac_mode"];
const FAN_MODE_LABELS: &[&str] = &["thermostat_id", "thermostat_name", "desired_fan_mode"];
const CLIMATE_LABELS: &[&str] = &["thermostat_id", "thermostat_name", "current_climate"];
const EVENT_TYPE_LABELS: &[&str] = &["thermostat_id", "thermostat_name", "event_type"];
const EQUIPMENT_LABELS: &[&str] = &["thermostat_id", "thermostat_name", "equipment"];
const RUNTIME_INTERVAL_LABELS: &[&str] =
    &["thermostat_id", "thermostat_name", "equipment", "interval"];
const DM_OFFSET_LABELS: &[&str] = &["thermostat_id", "thermostat_name", "interval"];
const ALERT_LABELS: &[&str] = &[
    "thermostat_id",
    "thermostat_name",
    "alert_type",
    "alert_number",
];

/// Equipment names ecobee will emit in `equipmentStatus`. We pre-register
/// a series at value 0 for every one of these on every poll so PromQL
/// queries like `ecobee_equipment_running{equipment="fan"}` always have
/// something to match — instead of relying on `absent()` / `or vector(0)`.
const KNOWN_EQUIPMENT: &[&str] = &[
    "heatPump",
    "heatPump2",
    "heatPump3",
    "compCool1",
    "compCool2",
    "auxHeat1",
    "auxHeat2",
    "auxHeat3",
    "fan",
    "humidifier",
    "dehumidifier",
    "ventilator",
    "economizer",
    "compHotWater",
    "auxHotWater",
];

pub struct Metrics {
    pub registry: Registry,
    fetch_time: Gauge,
    fetch_failures: IntCounter,

    actual_temperature: GaugeVec,
    target_temperature_min: GaugeVec,
    target_temperature_max: GaugeVec,
    current_hvac_mode: GaugeVec,
    connected: GaugeVec,

    temperature: GaugeVec,
    humidity: GaugeVec,
    occupancy: GaugeVec,
    in_use: GaugeVec,

    outdoor_temperature: GaugeVec,
    outdoor_humidity: GaugeVec,
    outdoor_pressure_mb: GaugeVec,
    outdoor_dewpoint: GaugeVec,
    outdoor_wind_speed_mph: GaugeVec,
    outdoor_wind_gust_mph: GaugeVec,
    outdoor_wind_bearing_degrees: GaugeVec,
    outdoor_visibility_meters: GaugeVec,
    outdoor_probability_of_precipitation: GaugeVec,
    outdoor_temp_high: GaugeVec,
    outdoor_temp_low: GaugeVec,

    equipment_running: GaugeVec,

    actual_humidity: GaugeVec,
    desired_humidity: GaugeVec,
    desired_dehumidity: GaugeVec,
    raw_temperature: GaugeVec,
    desired_fan_mode: GaugeVec,
    current_climate: GaugeVec,
    hold_active: GaugeVec,
    follow_me_comfort: GaugeVec,
    smart_circulation: GaugeVec,
    heat_stages: GaugeVec,
    cool_stages: GaugeVec,

    hold_heat_temp: GaugeVec,
    hold_cool_temp: GaugeVec,
    event_type: GaugeVec,

    equipment_runtime_seconds: GaugeVec,
    demand_management_offset: GaugeVec,
    current_electricity_bill: GaugeVec,
    projected_electricity_bill: GaugeVec,

    alert_active: GaugeVec,
}

impl Metrics {
    #[allow(
        clippy::too_many_lines,
        reason = "linear list of metric definitions reads more clearly than a macro or helper"
    )]
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
        let connected = GaugeVec::new(
            Opts::new(
                "ecobee_connected",
                "1 if the thermostat is currently reachable by ecobee's cloud, else 0",
            ),
            RUNTIME_LABELS,
        )?;

        let temperature = GaugeVec::new(
            Opts::new(
                "ecobee_temperature",
                "per-sensor reported temperature in degrees",
            ),
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

        let outdoor_temperature = GaugeVec::new(
            Opts::new(
                "ecobee_outdoor_temperature",
                "outdoor temperature, degrees (Fahrenheit for US accounts)",
            ),
            WEATHER_LABELS,
        )?;
        let outdoor_humidity = GaugeVec::new(
            Opts::new(
                "ecobee_outdoor_humidity",
                "outdoor relative humidity, percent",
            ),
            WEATHER_LABELS,
        )?;
        let outdoor_pressure_mb = GaugeVec::new(
            Opts::new(
                "ecobee_outdoor_pressure_mb",
                "outdoor sea-level pressure, millibars (equivalent to hPa)",
            ),
            WEATHER_LABELS,
        )?;
        let outdoor_dewpoint = GaugeVec::new(
            Opts::new("ecobee_outdoor_dewpoint", "outdoor dewpoint, degrees"),
            WEATHER_LABELS,
        )?;
        let outdoor_wind_speed_mph = GaugeVec::new(
            Opts::new(
                "ecobee_outdoor_wind_speed_mph",
                "wind speed, miles per hour",
            ),
            WEATHER_LABELS,
        )?;
        let outdoor_wind_gust_mph = GaugeVec::new(
            Opts::new("ecobee_outdoor_wind_gust_mph", "wind gust, miles per hour"),
            WEATHER_LABELS,
        )?;
        let outdoor_wind_bearing_degrees = GaugeVec::new(
            Opts::new(
                "ecobee_outdoor_wind_bearing_degrees",
                "wind bearing, compass degrees (0=N, 90=E)",
            ),
            WEATHER_LABELS,
        )?;
        let outdoor_visibility_meters = GaugeVec::new(
            Opts::new("ecobee_outdoor_visibility_meters", "visibility, meters"),
            WEATHER_LABELS,
        )?;
        let outdoor_probability_of_precipitation = GaugeVec::new(
            Opts::new(
                "ecobee_outdoor_probability_of_precipitation",
                "probability of precipitation, percent (0-100)",
            ),
            WEATHER_LABELS,
        )?;
        let outdoor_temp_high = GaugeVec::new(
            Opts::new(
                "ecobee_outdoor_temp_high",
                "forecast daily high temperature, degrees",
            ),
            WEATHER_LABELS,
        )?;
        let outdoor_temp_low = GaugeVec::new(
            Opts::new(
                "ecobee_outdoor_temp_low",
                "forecast daily low temperature, degrees",
            ),
            WEATHER_LABELS,
        )?;

        let equipment_running = GaugeVec::new(
            Opts::new(
                "ecobee_equipment_running",
                "1 if the equipment is currently running, else 0; one series per known equipment per thermostat",
            ),
            EQUIPMENT_LABELS,
        )?;

        let actual_humidity = GaugeVec::new(
            Opts::new(
                "ecobee_actual_humidity",
                "thermostat-averaged current relative humidity, percent",
            ),
            RUNTIME_LABELS,
        )?;
        let desired_humidity = GaugeVec::new(
            Opts::new("ecobee_desired_humidity", "humidifier setpoint, percent"),
            RUNTIME_LABELS,
        )?;
        let desired_dehumidity = GaugeVec::new(
            Opts::new(
                "ecobee_desired_dehumidity",
                "dehumidifier setpoint, percent",
            ),
            RUNTIME_LABELS,
        )?;
        let raw_temperature = GaugeVec::new(
            Opts::new(
                "ecobee_raw_temperature",
                "dry-bulb temperature at the thermostat, degrees",
            ),
            RUNTIME_LABELS,
        )?;
        let desired_fan_mode = GaugeVec::new(
            Opts::new(
                "ecobee_desired_fan_mode",
                "always 0; the desired fan mode is encoded in the desired_fan_mode label",
            ),
            FAN_MODE_LABELS,
        )?;
        let current_climate = GaugeVec::new(
            Opts::new(
                "ecobee_current_climate",
                "always 0; the active schedule climate is encoded in the current_climate label",
            ),
            CLIMATE_LABELS,
        )?;
        let hold_active = GaugeVec::new(
            Opts::new(
                "ecobee_hold_active",
                "1 if a hold, vacation, or demand-response event is currently running",
            ),
            RUNTIME_LABELS,
        )?;
        let follow_me_comfort = GaugeVec::new(
            Opts::new(
                "ecobee_follow_me_comfort",
                "1 if follow-me comfort is enabled on the thermostat",
            ),
            RUNTIME_LABELS,
        )?;
        let smart_circulation = GaugeVec::new(
            Opts::new(
                "ecobee_smart_circulation",
                "1 if smart circulation fan mode is enabled",
            ),
            RUNTIME_LABELS,
        )?;
        let heat_stages = GaugeVec::new(
            Opts::new("ecobee_heat_stages", "number of configured heating stages"),
            RUNTIME_LABELS,
        )?;
        let cool_stages = GaugeVec::new(
            Opts::new("ecobee_cool_stages", "number of configured cooling stages"),
            RUNTIME_LABELS,
        )?;

        let hold_heat_temp = GaugeVec::new(
            Opts::new(
                "ecobee_hold_heat_temp",
                "heat hold setpoint while an event is running, degrees",
            ),
            RUNTIME_LABELS,
        )?;
        let hold_cool_temp = GaugeVec::new(
            Opts::new(
                "ecobee_hold_cool_temp",
                "cool hold setpoint while an event is running, degrees",
            ),
            RUNTIME_LABELS,
        )?;
        let event_type = GaugeVec::new(
            Opts::new(
                "ecobee_event_type",
                "always 0; the active event type is encoded in the event_type label",
            ),
            EVENT_TYPE_LABELS,
        )?;

        let equipment_runtime_seconds = GaugeVec::new(
            Opts::new(
                "ecobee_equipment_runtime_seconds",
                "equipment runtime in seconds for each of the last three 5-minute intervals (interval 0=oldest, 2=newest)",
            ),
            RUNTIME_INTERVAL_LABELS,
        )?;
        let demand_management_offset = GaugeVec::new(
            Opts::new(
                "ecobee_demand_management_offset",
                "demand-management temperature offset applied by the thermostat, degrees",
            ),
            DM_OFFSET_LABELS,
        )?;
        let current_electricity_bill = GaugeVec::new(
            Opts::new(
                "ecobee_current_electricity_bill",
                "current electricity bill interpolated from a paired utility meter (units per ecobee API)",
            ),
            RUNTIME_LABELS,
        )?;
        let projected_electricity_bill = GaugeVec::new(
            Opts::new(
                "ecobee_projected_electricity_bill",
                "projected electricity bill interpolated from a paired utility meter (units per ecobee API)",
            ),
            RUNTIME_LABELS,
        )?;

        let alert_active = GaugeVec::new(
            Opts::new(
                "ecobee_alert_active",
                "1 for each active thermostat alert requiring user attention",
            ),
            ALERT_LABELS,
        )?;

        registry.register(Box::new(fetch_time.clone()))?;
        registry.register(Box::new(fetch_failures.clone()))?;
        registry.register(Box::new(actual_temperature.clone()))?;
        registry.register(Box::new(target_temperature_min.clone()))?;
        registry.register(Box::new(target_temperature_max.clone()))?;
        registry.register(Box::new(current_hvac_mode.clone()))?;
        registry.register(Box::new(connected.clone()))?;
        registry.register(Box::new(temperature.clone()))?;
        registry.register(Box::new(humidity.clone()))?;
        registry.register(Box::new(occupancy.clone()))?;
        registry.register(Box::new(in_use.clone()))?;
        registry.register(Box::new(outdoor_temperature.clone()))?;
        registry.register(Box::new(outdoor_humidity.clone()))?;
        registry.register(Box::new(outdoor_pressure_mb.clone()))?;
        registry.register(Box::new(outdoor_dewpoint.clone()))?;
        registry.register(Box::new(outdoor_wind_speed_mph.clone()))?;
        registry.register(Box::new(outdoor_wind_gust_mph.clone()))?;
        registry.register(Box::new(outdoor_wind_bearing_degrees.clone()))?;
        registry.register(Box::new(outdoor_visibility_meters.clone()))?;
        registry.register(Box::new(outdoor_probability_of_precipitation.clone()))?;
        registry.register(Box::new(outdoor_temp_high.clone()))?;
        registry.register(Box::new(outdoor_temp_low.clone()))?;
        registry.register(Box::new(equipment_running.clone()))?;
        registry.register(Box::new(actual_humidity.clone()))?;
        registry.register(Box::new(desired_humidity.clone()))?;
        registry.register(Box::new(desired_dehumidity.clone()))?;
        registry.register(Box::new(raw_temperature.clone()))?;
        registry.register(Box::new(desired_fan_mode.clone()))?;
        registry.register(Box::new(current_climate.clone()))?;
        registry.register(Box::new(hold_active.clone()))?;
        registry.register(Box::new(follow_me_comfort.clone()))?;
        registry.register(Box::new(smart_circulation.clone()))?;
        registry.register(Box::new(heat_stages.clone()))?;
        registry.register(Box::new(cool_stages.clone()))?;
        registry.register(Box::new(hold_heat_temp.clone()))?;
        registry.register(Box::new(hold_cool_temp.clone()))?;
        registry.register(Box::new(event_type.clone()))?;
        registry.register(Box::new(equipment_runtime_seconds.clone()))?;
        registry.register(Box::new(demand_management_offset.clone()))?;
        registry.register(Box::new(current_electricity_bill.clone()))?;
        registry.register(Box::new(projected_electricity_bill.clone()))?;
        registry.register(Box::new(alert_active.clone()))?;

        Ok(Self {
            registry,
            fetch_time,
            fetch_failures,
            actual_temperature,
            target_temperature_min,
            target_temperature_max,
            current_hvac_mode,
            connected,
            temperature,
            humidity,
            occupancy,
            in_use,
            outdoor_temperature,
            outdoor_humidity,
            outdoor_pressure_mb,
            outdoor_dewpoint,
            outdoor_wind_speed_mph,
            outdoor_wind_gust_mph,
            outdoor_wind_bearing_degrees,
            outdoor_visibility_meters,
            outdoor_probability_of_precipitation,
            outdoor_temp_high,
            outdoor_temp_low,
            equipment_running,
            actual_humidity,
            desired_humidity,
            desired_dehumidity,
            raw_temperature,
            desired_fan_mode,
            current_climate,
            hold_active,
            follow_me_comfort,
            smart_circulation,
            heat_stages,
            cool_stages,
            hold_heat_temp,
            hold_cool_temp,
            event_type,
            equipment_runtime_seconds,
            demand_management_offset,
            current_electricity_bill,
            projected_electricity_bill,
            alert_active,
        })
    }

    /// Replace the current values with a fresh snapshot.
    ///
    /// `GaugeVec` series for previously-seen label sets are *not* removed
    /// automatically here; if a sensor disappears between polls its last
    /// value will stick around. That mirrors how billykwooten's exporter
    /// behaves and is what Grafana users expect.
    #[allow(
        clippy::too_many_lines,
        reason = "single pass over thermostat snapshot fields reads more clearly split by concern below"
    )]
    pub fn record_snapshot(&self, thermostats: &[Thermostat], fetch_secs: f64) {
        self.fetch_time.set(fetch_secs);

        for t in thermostats {
            let runtime_labels = [t.identifier.as_str(), t.name.as_str()];

            self.connected
                .with_label_values(&runtime_labels)
                .set(if t.connected { 1.0 } else { 0.0 });

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

                if let Some(v) = t.runtime.actual_humidity {
                    self.actual_humidity
                        .with_label_values(&runtime_labels)
                        .set(f64::from(v));
                }
                if let Some(v) = t.runtime.desired_humidity {
                    self.desired_humidity
                        .with_label_values(&runtime_labels)
                        .set(f64::from(v));
                }
                if let Some(v) = t.runtime.desired_dehumidity {
                    self.desired_dehumidity
                        .with_label_values(&runtime_labels)
                        .set(f64::from(v));
                }
                if let Some(v) = t.runtime.raw_temperature {
                    self.raw_temperature
                        .with_label_values(&runtime_labels)
                        .set(f64::from(v) / 10.0);
                }
                if let Some(mode) = t.runtime.desired_fan_mode.as_deref() {
                    self.desired_fan_mode
                        .with_label_values(&[t.identifier.as_str(), t.name.as_str(), mode])
                        .set(0.0);
                }
            }

            self.record_settings(t);
            self.record_program(t);
            self.record_hold(t);
            self.record_extended_runtime(t);
            self.record_alerts(t);
            self.record_weather(t);
            self.record_equipment(t);

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

    fn record_settings(&self, t: &Thermostat) {
        let labels = [t.identifier.as_str(), t.name.as_str()];
        self.follow_me_comfort
            .with_label_values(&labels)
            .set(if t.settings.follow_me_comfort {
                1.0
            } else {
                0.0
            });
        self.smart_circulation
            .with_label_values(&labels)
            .set(if t.settings.smart_circulation {
                1.0
            } else {
                0.0
            });
        if let Some(v) = t.settings.heat_stages {
            self.heat_stages
                .with_label_values(&labels)
                .set(f64::from(v));
        }
        if let Some(v) = t.settings.cool_stages {
            self.cool_stages
                .with_label_values(&labels)
                .set(f64::from(v));
        }
    }

    fn record_program(&self, t: &Thermostat) {
        let Some(program) = t.program.as_ref() else {
            return;
        };
        let climate = program.current_climate_ref.as_deref().unwrap_or("unknown");
        self.current_climate
            .with_label_values(&[t.identifier.as_str(), t.name.as_str(), climate])
            .set(0.0);
    }

    fn record_hold(&self, t: &Thermostat) {
        let labels = [t.identifier.as_str(), t.name.as_str()];
        let active = t.hold.as_ref().is_some_and(|h| h.running);
        self.hold_active
            .with_label_values(&labels)
            .set(if active { 1.0 } else { 0.0 });

        let Some(hold) = t.hold.as_ref().filter(|h| h.running) else {
            return;
        };

        self.event_type
            .with_label_values(&[
                t.identifier.as_str(),
                t.name.as_str(),
                hold.event_type.as_str(),
            ])
            .set(0.0);
        if let Some(v) = hold.heat_hold_temp {
            self.hold_heat_temp
                .with_label_values(&labels)
                .set(f64::from(v) / 10.0);
        }
        if let Some(v) = hold.cool_hold_temp {
            self.hold_cool_temp
                .with_label_values(&labels)
                .set(f64::from(v) / 10.0);
        }
    }

    fn record_extended_runtime(&self, t: &Thermostat) {
        let Some(ext) = t.extended_runtime.as_ref() else {
            return;
        };
        let id = t.identifier.as_str();
        let name = t.name.as_str();

        for eq in &ext.equipment {
            for (idx, seconds) in eq.seconds.iter().enumerate() {
                let interval = idx.to_string();
                self.equipment_runtime_seconds
                    .with_label_values(&[id, name, eq.name.as_str(), interval.as_str()])
                    .set(f64::from(*seconds));
            }
        }

        for (idx, offset) in ext.dm_offset.iter().enumerate() {
            if let Some(v) = offset {
                let interval = idx.to_string();
                self.demand_management_offset
                    .with_label_values(&[id, name, interval.as_str()])
                    .set(f64::from(*v) / 10.0);
            }
        }

        if let Some(v) = ext.current_electricity_bill {
            self.current_electricity_bill
                .with_label_values(&[id, name])
                .set(f64::from(v));
        }
        if let Some(v) = ext.projected_electricity_bill {
            self.projected_electricity_bill
                .with_label_values(&[id, name])
                .set(f64::from(v));
        }
    }

    fn record_alerts(&self, t: &Thermostat) {
        for alert in &t.alerts {
            let number = alert
                .alert_number
                .map_or_else(|| "unknown".to_owned(), |n| n.to_string());
            self.alert_active
                .with_label_values(&[
                    t.identifier.as_str(),
                    t.name.as_str(),
                    alert.alert_type.as_str(),
                    number.as_str(),
                ])
                .set(1.0);
        }
    }

    fn record_weather(&self, t: &Thermostat) {
        let Some(w) = t.weather.as_ref() else { return };
        let labels = [t.identifier.as_str(), t.name.as_str(), w.station.as_str()];

        if let Some(v) = w.temperature {
            self.outdoor_temperature.with_label_values(&labels).set(v);
        }
        if let Some(v) = w.humidity {
            self.outdoor_humidity
                .with_label_values(&labels)
                .set(f64::from(v));
        }
        if let Some(v) = w.pressure_mb {
            self.outdoor_pressure_mb
                .with_label_values(&labels)
                .set(f64::from(v));
        }
        if let Some(v) = w.dewpoint {
            self.outdoor_dewpoint.with_label_values(&labels).set(v);
        }
        if let Some(v) = w.wind_speed_mph {
            self.outdoor_wind_speed_mph
                .with_label_values(&labels)
                .set(f64::from(v));
        }
        if let Some(v) = w.wind_gust_mph {
            self.outdoor_wind_gust_mph
                .with_label_values(&labels)
                .set(f64::from(v));
        }
        if let Some(v) = w.wind_bearing_degrees {
            self.outdoor_wind_bearing_degrees
                .with_label_values(&labels)
                .set(f64::from(v));
        }
        if let Some(v) = w.visibility_meters {
            self.outdoor_visibility_meters
                .with_label_values(&labels)
                .set(f64::from(v));
        }
        if let Some(v) = w.probability_of_precipitation {
            self.outdoor_probability_of_precipitation
                .with_label_values(&labels)
                .set(f64::from(v));
        }
        if let Some(v) = w.temp_high {
            self.outdoor_temp_high.with_label_values(&labels).set(v);
        }
        if let Some(v) = w.temp_low {
            self.outdoor_temp_low.with_label_values(&labels).set(v);
        }
    }

    fn record_equipment(&self, t: &Thermostat) {
        let active: std::collections::HashSet<&str> =
            t.equipment_running.iter().map(String::as_str).collect();
        for &eq in KNOWN_EQUIPMENT {
            let v = if active.contains(eq) { 1.0 } else { 0.0 };
            self.equipment_running
                .with_label_values(&[t.identifier.as_str(), t.name.as_str(), eq])
                .set(v);
        }
        // Surface any equipment we didn't have hard-coded so an unknown
        // identifier from a future ecobee build still shows up.
        for eq in &t.equipment_running {
            if !KNOWN_EQUIPMENT.contains(&eq.as_str()) {
                self.equipment_running
                    .with_label_values(&[t.identifier.as_str(), t.name.as_str(), eq.as_str()])
                    .set(1.0);
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
