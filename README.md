# ecobee-exporter

A Prometheus exporter for ecobee thermostats, written in Rust. Talks to
ecobee's internal Beehive GraphQL API rather than the official developer
REST API.

## Read this first: terms of service

This exporter scrapes data from ecobee's mobile-app API, not the official
[developer API][dev-api]. That's a deliberate trade-off, because new
developer accounts have been closed to registration [since March 28,
2024][moratorium] and pre-existing keys are not assumed.

Using the Beehive API to back a long-running exporter is almost certainly
a violation of ecobee's terms of service. Concretely:

  - ecobee can revoke your account or rotate the mobile-app's client
    credentials at any time, breaking this exporter without notice.
  - Scraping frequency must stay reasonable. The collector defaults to
    one fetch every three minutes, matching how often the thermostat
    itself reports new data; do not turn it down.
  - This is for personal use against your own thermostats. Don't run it
    against someone else's account.

If you can recover a pre-2024 developer key, **you should use that
instead** via one of the existing exporters
([billykwooten/ecobee-exporter][bk], [cfunkhouser/promobee][cfunk],
[mrala/ecobee_prometheus_exporter][mrala]). This project exists for
people who do not have that option.

[dev-api]: https://www.ecobee.com/home/developer/api/introduction/index.shtml
[moratorium]: https://github.com/home-assistant/home-assistant.io/pull/33272
[bk]: https://github.com/billykwooten/ecobee-exporter
[cfunk]: https://github.com/cfunkhouser/promobee
[mrala]: https://github.com/mrala/ecobee_prometheus_exporter

## Current status

End-to-end functional. Fetches real thermostat + sensor + runtime data
from `api.ecobee.com/1/thermostat` using an Auth0-issued JWT bearer
token, and renders the documented [billykwooten/ecobee-exporter][bk]
metric set.

  - HTTP server on `/metrics` and `/healthz`.
  - Auth0 + PKCE one-time login bootstrap (`cargo run --bin ecobee-login`),
    persistent refresh-token rotation, mode-0600 state file.
  - Polling loop with a configurable interval and a 60-second floor.
  - Configuration via TOML file, environment variables, and CLI flags.
  - `--demo` mode that serves canned data so dashboards and scrape
    configs can be developed without credentials.

## Bootstrap

The mobile app uses Auth0 Universal Login with mandatory MFA. A
long-running headless exporter can't do MFA on every restart, so we
mint a refresh token once interactively and let the exporter reuse it.

```sh
cargo run --bin ecobee-login
```

This will print an Auth0 `/authorize` URL. Open it in your desktop
browser, complete login + MFA, and when the browser lands on a page
under `https://auth.ecobee.com/android/com.ecobee.athenamobile/callback?...`
(which on desktop will appear blank or error — that's expected; the
URL is an Android App Link with no desktop handler), copy the full
URL out of the address bar and paste it back into the prompt. The
helper exchanges the code at `/oauth/token` and writes the refresh
token to `ecobee-exporter.state.json` with mode `0600`.

After that, plain `cargo run --release` will pick up the refresh
token automatically.

## Quick start

### Demo mode (no credentials)

```sh
cargo run -- --demo
# in another shell:
curl http://localhost:9098/metrics
```

You should see a populated set of `ecobee_*` series against a synthetic
two-sensor thermostat.

### Real mode

```sh
cargo run --bin ecobee-login          # one-time interactive PKCE bootstrap
cargo run --release                   # run the exporter
curl http://localhost:9098/metrics
```

A TOML config file is optional. The defaults — base URL
`https://api.ecobee.com/1`, port `9098`, 3-minute poll cadence — are
what you want for ordinary use. Copy `ecobee-exporter.example.toml`
to `ecobee-exporter.toml` only if you need to override something.

## Configuration

Layered, lowest-to-highest precedence:

  1. Built-in defaults.
  2. `ecobee-exporter.toml` in the working directory.
  3. The file at `$ECOBEE_EXPORTER_CONFIG`, or the `--config` flag.
  4. Environment variables prefixed `ECOBEE_`. Nested keys use `__`,
     e.g. `ECOBEE_BEEHIVE__ENDPOINT=https://...`.

| Key                       | Default                          | Notes                                                                 |
|---------------------------|----------------------------------|-----------------------------------------------------------------------|
| `listen_addr`             | `0.0.0.0:9098`                   | Where `/metrics` is served.                                           |
| `poll_interval`           | `3m`                             | Floored to 60s; ecobee data only updates every ~3 minutes anyway.     |
| `state_file`              | `./ecobee-exporter.state.json`   | Where refresh tokens are persisted.                                   |
| `demo`                    | `false`                          | Serve canned data; no upstream calls.                                 |
| `beehive.endpoint`        | `https://api.ecobee.com/1`       | Data API base URL. The default is the documented developer-API host. |
| `beehive.user_agent`      | `ecobee-exporter/0.1.0`          | Override to mimic the official mobile app if upstream rejects yours.  |
| `beehive.extra_headers`   | `[]`                             | List of `[key, value]` pairs to add to every request.                 |
| `beehive.refresh_token`   | `null`                           | Optional refresh-token seed; normally lives in `state_file` after `ecobee-login`. |

Put secrets in the config file with `chmod 600`, not env vars. Env vars
leak into systemd journals and `ps`.

## Metrics

The core set mirrors billykwooten/ecobee-exporter for dashboard
compatibility. The extensions on top — `ecobee_connected`,
`ecobee_outdoor_*`, `ecobee_equipment_running` — are noted in the
description column.

### Thermostat + sensor

| Metric                            | Labels                                                              | Description                                                                            |
|-----------------------------------|---------------------------------------------------------------------|----------------------------------------------------------------------------------------|
| `ecobee_fetch_time`               | —                                                                   | Seconds the last upstream fetch took.                                                  |
| `ecobee_fetch_failures_total`     | —                                                                   | Counter of failed fetches since start. *(extension)*                                   |
| `ecobee_actual_temperature`       | `thermostat_id`, `thermostat_name`                                  | Thermostat-averaged current temperature, degrees.                                      |
| `ecobee_target_temperature_min`   | `thermostat_id`, `thermostat_name`                                  | Heating setpoint, degrees.                                                             |
| `ecobee_target_temperature_max`   | `thermostat_id`, `thermostat_name`                                  | Cooling setpoint, degrees.                                                             |
| `ecobee_currenthvacmode`          | `thermostat_id`, `thermostat_name`, `current_hvac_mode`             | Always `0`; the mode is encoded as a label, matching billykwooten.                     |
| `ecobee_connected`                | `thermostat_id`, `thermostat_name`                                  | 1 if the thermostat is currently reachable by ecobee's cloud, else 0. *(extension)*    |
| `ecobee_temperature`              | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Per-sensor temperature, degrees.                                              |
| `ecobee_humidity`                 | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Per-sensor humidity, percent.                                                 |
| `ecobee_occupancy`                | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Per-sensor occupancy (0 or 1).                                                |
| `ecobee_in_use`                   | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Whether the sensor is being included in thermostat averages (0 or 1).         |

### Outdoor weather *(extension)*

Sourced from the thermostat's associated weather station's current
forecast block. Any reading ecobee marks as missing (its `-5002`
sentinel) is suppressed rather than reported as a fake value.

| Metric                                            | Labels                                                | Description                                                                |
|---------------------------------------------------|-------------------------------------------------------|----------------------------------------------------------------------------|
| `ecobee_outdoor_temperature`                      | `thermostat_id`, `thermostat_name`, `station`         | Outdoor temperature, degrees (Fahrenheit for US accounts).                 |
| `ecobee_outdoor_humidity`                         | `thermostat_id`, `thermostat_name`, `station`         | Outdoor relative humidity, percent.                                        |
| `ecobee_outdoor_pressure_mb`                      | `thermostat_id`, `thermostat_name`, `station`         | Sea-level pressure, millibars (equivalent to hPa).                         |
| `ecobee_outdoor_dewpoint`                         | `thermostat_id`, `thermostat_name`, `station`         | Outdoor dewpoint, degrees.                                                 |
| `ecobee_outdoor_wind_speed_mph`                   | `thermostat_id`, `thermostat_name`, `station`         | Wind speed, mph.                                                           |
| `ecobee_outdoor_wind_gust_mph`                    | `thermostat_id`, `thermostat_name`, `station`         | Wind gust, mph (often suppressed by ecobee).                               |
| `ecobee_outdoor_wind_bearing_degrees`             | `thermostat_id`, `thermostat_name`, `station`         | Wind bearing, compass degrees (0 = N, 90 = E).                             |
| `ecobee_outdoor_visibility_meters`                | `thermostat_id`, `thermostat_name`, `station`         | Visibility, meters.                                                        |
| `ecobee_outdoor_probability_of_precipitation`     | `thermostat_id`, `thermostat_name`, `station`         | Probability of precipitation, percent (0–100).                             |
| `ecobee_outdoor_temp_high`                        | `thermostat_id`, `thermostat_name`, `station`         | Forecast daily high, degrees.                                              |
| `ecobee_outdoor_temp_low`                         | `thermostat_id`, `thermostat_name`, `station`         | Forecast daily low, degrees.                                               |

### Equipment runtime *(extension)*

`ecobee_equipment_running{equipment}` is a 0/1 gauge with one series per
known equipment identifier per thermostat. Known values are:
`heatPump`, `heatPump2`, `heatPump3`, `compCool1`, `compCool2`,
`auxHeat1`, `auxHeat2`, `auxHeat3`, `fan`, `humidifier`, `dehumidifier`,
`ventilator`, `economizer`, `compHotWater`, `auxHotWater`. Unknown
identifiers that show up in a future ecobee build still appear as
extra series.

| Metric                       | Labels                                                | Description                                       |
|------------------------------|-------------------------------------------------------|---------------------------------------------------|
| `ecobee_equipment_running`   | `thermostat_id`, `thermostat_name`, `equipment`       | 1 if that equipment is currently running, else 0. |

## Architecture

```
                +---------------------+
                |  axum HTTP server   |  /metrics, /healthz
                +----------+----------+
                           ^ render()
                +----------+----------+
                |  prometheus::Reg.   |
                +----------+----------+
                           ^ record_snapshot()
                +----------+----------+        +-------------------+
                |  Collector loop     |<------>|  ThermostatProv.  |  trait
                +---------------------+ fetch  +---------+---------+
                                                         |
                                          impl 1: BeehiveProvider  (real)
                                          impl 2: FakeProvider     (demo/test)
```

`ThermostatProvider` is the seam: anything that can return a
`Vec<Thermostat>` can drive the exporter. If a HomeKit or Matter
controller path becomes practical later, it slots in alongside Beehive
without touching the metrics or HTTP layers.

## Why not local HomeKit / Matter?

A reasonable alternative path. ecobee thermostats expose much of this
data over local HomeKit (and Matter on newer Premium/Enhanced models),
which would avoid the ToS issue entirely. The current Rust HomeKit
Accessory Protocol *controller* ecosystem is thin compared to the
Python `aiohomekit` library that Home Assistant uses, so the
implementation cost is high. If Beehive turns out to be too hostile or
the capture work doesn't pan out, falling back to a HomeKit-based
`ThermostatProvider` is the natural next step — the rest of the
exporter is unchanged.

## Roadmap

Short-term:

  - ~~Implement Auth0 + PKCE login + refresh.~~ Done.
  - ~~Implement the data-API call against `api.ecobee.com/1/thermostat`.~~ Done.
  - ~~Add a fixture-based parsing test for the response shape.~~ Done.

Medium-term (parity-plus):

  - ~~`ecobee_equipment_running{equipment}`.~~ Done.
  - ~~Outdoor weather metrics from the weather block.~~ Done.
  - Per-equipment runtime minutes from the `extendedRuntime` block
    (heat-pump, aux, cool, fan, humidifier, dehumidifier, ventilator
    minute counters).
  - Air-quality metrics on Premium models (`co2`, `vocPpb`,
    `airQualityAccuracy`).

Long-term (operational polish):

  - Dockerfile + multi-arch CI.
  - systemd unit file with `DynamicUser=yes` and a state directory.
  - Grafana dashboard JSON checked in under `dashboards/`.

## Development

```sh
cargo test                       # FakeProvider round-trip
cargo clippy --all-targets       # warnings should be empty
cargo run -- --demo              # local smoke test
RUST_LOG=ecobee_exporter=debug cargo run -- --demo
```

## License

Apache-2.0. See [`LICENSE`](./LICENSE).
