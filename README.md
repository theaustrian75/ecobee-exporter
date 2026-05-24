# FA?Prometheus ecobee-exporter

A Prometheus exporter for ecobee thermostats, written in Rust. Talks to
ecobee's internal Beehive GraphQL API rather than the official developer
REST API.

## Read this first: terms of service

This exporter scrapes data from ecobee's mobile-app API, not the official
[developer API](https://www.ecobee.com/home/developer/api/introduction/index.shtml). That's a deliberate trade-off, because new
developer accounts have been closed to registration [since March 28,
2024](https://github.com/home-assistant/home-assistant.io/pull/33272) and pre-existing keys are not assumed.

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
([billykwooten/ecobee-exporter](https://github.com/billykwooten/ecobee-exporter), [cfunkhouser/promobee](https://github.com/cfunkhouser/promobee),
[mrala/ecobee_prometheus_exporter](https://github.com/mrala/ecobee_prometheus_exporter)). This project exists for
people who do not have that option.

## Current status

End-to-end functional. Fetches real thermostat + sensor + runtime data
from `api.ecobee.com/1/thermostat` using an Auth0-issued JWT bearer
token, and renders the documented [billykwooten/ecobee-exporter](https://github.com/billykwooten/ecobee-exporter)
metric set.

- HTTP server on `/metrics` and `/healthz`.
- Auth0 + PKCE one-time login bootstrap (`cargo run --bin ecobee-login`),
persistent refresh-token rotation, mode-0600 state file.
- Polling loop with a configurable interval and a 60-second floor.
- Configuration via TOML file, environment variables, and CLI flags.
- `--demo` mode that serves canned data so dashboards and scrape
configs can be developed without credentials.
- Optional **HomeKit backend** (`provider = "homekit"`) that reads
thermostats over the LAN via the in-tree [`housekey`](crates/housekey/housekey)
HAP client — no cloud API or Auth0 login required.

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

### Docker (Alpine)

Build the image:

```sh
docker build -t ecobee-exporter:local .
```

#### `docker run`

The image defaults `ECOBEE_STATE_FILE` to
`/var/lib/ecobee-exporter/state.json`. Mount a named volume or bind mount
there so refresh tokens survive restarts.

One-time interactive login — open the printed Auth0 URL in your desktop
browser, complete MFA, copy the full callback URL from the address bar,
and paste it at the prompt:

```sh
docker run --rm -it \
  -v ecobee-exporter-data:/var/lib/ecobee-exporter \
  --entrypoint ecobee-login \
  ecobee-exporter:local
```

Run the exporter (metrics on `:9098`, state read from the volume):

```sh
docker run -d --name ecobee-exporter --restart unless-stopped \
  -p 9098:9098 \
  -v ecobee-exporter-data:/var/lib/ecobee-exporter \
  -e ECOBEE_STATE_FILE=/var/lib/ecobee-exporter/state.json \
  -e TZ=America/New_York \
  ecobee-exporter:local

curl http://localhost:9098/metrics
```

`TZ` is the standard IANA timezone name (e.g. `America/New_York`, `UTC`).
The image ships `tzdata`; Alpine applies `TZ` to log timestamps and other
libc local-time calls. Metrics themselves are unitless gauges — timezone
only affects container-side logging.

Other `ECOBEE_*` environment variables follow the same prefix as bare-metal
runs (`ECOBEE_LISTEN_ADDR`, `ECOBEE_POLL_INTERVAL`, etc.).

Demo mode without credentials:

```sh
docker run --rm -p 9098:9098 ecobee-exporter:local --demo
```

#### `docker compose`

Copy `docker-compose.example.yml` and adjust the image tag if needed, then:

```sh
# One-time Auth0 bootstrap (interactive; same browser flow as above).
docker compose -f docker-compose.example.yml run --rm -it \
  --entrypoint ecobee-login ecobee-exporter

# Start the exporter in the background.
docker compose -f docker-compose.example.yml up -d

curl http://localhost:9098/metrics
```

Uncomment `TZ` in the compose file (or add it under `environment`) to set
the container timezone. The example file also documents
`ECOBEE_STATE_FILE` and other optional overrides.

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


| Key                     | Default                        | Notes                                                                             |
| ----------------------- | ------------------------------ | --------------------------------------------------------------------------------- |
| `listen_addr`           | `0.0.0.0:9098`                 | Where `/metrics` is served.                                                       |
| `poll_interval`         | `3m`                           | Floored to 60s; ecobee data only updates every ~3 minutes anyway.                 |
| `state_file`            | `./ecobee-exporter.state.json` | Where refresh tokens are persisted.                                               |
| `demo`                  | `false`                        | Serve canned data; no upstream calls.                                             |
| `provider`              | `beehive`                      | Data source: `beehive` (cloud) or `homekit` (local LAN).                          |
| `homekit.pairing_file`  | `./homekit-pairings.json`      | HomeKit pairing keys written by `ecobee-homekit-pair`. `chmod 600` recommended.   |
| `beehive.endpoint`      | `https://api.ecobee.com/1`     | Data API base URL. The default is the documented developer-API host.              |
| `beehive.user_agent`    | `ecobee-exporter/0.1.0`        | Override to mimic the official mobile app if upstream rejects yours.              |
| `beehive.extra_headers` | `[]`                           | List of `[key, value]` pairs to add to every request.                             |
| `beehive.refresh_token` | `null`                         | Optional refresh-token seed; normally lives in `state_file` after `ecobee-login`. |


Put secrets in the config file with `chmod 600`, not env vars. Env vars
leak into systemd journals and `ps`.

## Metrics

The core set mirrors billykwooten/ecobee-exporter for dashboard
compatibility. The extensions on top — `ecobee_connected`,
`ecobee_outdoor_*`, `ecobee_equipment_running` — are noted in the
description column.

### Thermostat + sensor


| Metric                          | Labels                                                                        | Description                                                                                |
| ------------------------------- | ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `ecobee_fetch_time`             | —                                                                             | Seconds the last upstream fetch took.                                                      |
| `ecobee_fetch_failures_total`   | —                                                                             | Counter of failed fetches since start. *(extension)*                                       |
| `ecobee_actual_temperature`     | `thermostat_id`, `thermostat_name`                                            | Thermostat-averaged current temperature, degrees.                                          |
| `ecobee_target_temperature_min` | `thermostat_id`, `thermostat_name`                                            | Heating setpoint, degrees.                                                                 |
| `ecobee_target_temperature_max` | `thermostat_id`, `thermostat_name`                                            | Cooling setpoint, degrees.                                                                 |
| `ecobee_currenthvacmode`        | `thermostat_id`, `thermostat_name`, `current_hvac_mode`                       | Always `0`; the mode is encoded as a label, matching billykwooten.                         |
| `ecobee_connected`              | `thermostat_id`, `thermostat_name`                                            | 1 if the thermostat is currently reachable by ecobee's cloud, else 0. *(extension)*        |
| `ecobee_temperature`            | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Per-sensor temperature, degrees.                                                           |
| `ecobee_humidity`               | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Per-sensor humidity, percent.                                                              |
| `ecobee_occupancy`              | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Per-sensor occupancy (0 or 1).                                                             |
| `ecobee_in_use`                 | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Whether the sensor is being included in thermostat averages (0 or 1).                      |
| `ecobee_actual_humidity`        | `thermostat_id`, `thermostat_name`                                            | Thermostat-averaged relative humidity, percent. *(extension)*                              |
| `ecobee_desired_humidity`       | `thermostat_id`, `thermostat_name`                                            | Humidifier setpoint, percent. *(extension)*                                                |
| `ecobee_desired_dehumidity`     | `thermostat_id`, `thermostat_name`                                            | Dehumidifier setpoint, percent. *(extension)*                                              |
| `ecobee_raw_temperature`        | `thermostat_id`, `thermostat_name`                                            | Dry-bulb temperature at the thermostat, degrees. *(extension)*                             |
| `ecobee_desired_fan_mode`       | `thermostat_id`, `thermostat_name`, `desired_fan_mode`                        | Always `0`; fan mode encoded as a label (`auto`, `on`). *(extension)*                      |
| `ecobee_current_climate`        | `thermostat_id`, `thermostat_name`, `current_climate`                         | Always `0`; active schedule climate encoded as a label (`home`, `sleep`, …). *(extension)* |
| `ecobee_hold_active`            | `thermostat_id`, `thermostat_name`                                            | 1 if a hold/vacation/DR event is running. *(extension)*                                    |
| `ecobee_follow_me_comfort`      | `thermostat_id`, `thermostat_name`                                            | 1 if follow-me comfort is enabled. *(extension)*                                           |
| `ecobee_smart_circulation`      | `thermostat_id`, `thermostat_name`                                            | 1 if smart circulation is enabled. *(extension)*                                           |
| `ecobee_heat_stages`            | `thermostat_id`, `thermostat_name`                                            | Number of configured heating stages. *(extension)*                                         |
| `ecobee_cool_stages`            | `thermostat_id`, `thermostat_name`                                            | Number of configured cooling stages. *(extension)*                                         |


### Outdoor weather *(extension)*

Sourced from the thermostat's associated weather station's current
forecast block. Any reading ecobee marks as missing (its `-5002`
sentinel) is suppressed rather than reported as a fake value.


| Metric                                        | Labels                                        | Description                                                |
| --------------------------------------------- | --------------------------------------------- | ---------------------------------------------------------- |
| `ecobee_outdoor_temperature`                  | `thermostat_id`, `thermostat_name`, `station` | Outdoor temperature, degrees (Fahrenheit for US accounts). |
| `ecobee_outdoor_humidity`                     | `thermostat_id`, `thermostat_name`, `station` | Outdoor relative humidity, percent.                        |
| `ecobee_outdoor_pressure_mb`                  | `thermostat_id`, `thermostat_name`, `station` | Sea-level pressure, millibars (equivalent to hPa).         |
| `ecobee_outdoor_dewpoint`                     | `thermostat_id`, `thermostat_name`, `station` | Outdoor dewpoint, degrees.                                 |
| `ecobee_outdoor_wind_speed_mph`               | `thermostat_id`, `thermostat_name`, `station` | Wind speed, mph.                                           |
| `ecobee_outdoor_wind_gust_mph`                | `thermostat_id`, `thermostat_name`, `station` | Wind gust, mph (often suppressed by ecobee).               |
| `ecobee_outdoor_wind_bearing_degrees`         | `thermostat_id`, `thermostat_name`, `station` | Wind bearing, compass degrees (0 = N, 90 = E).             |
| `ecobee_outdoor_visibility_meters`            | `thermostat_id`, `thermostat_name`, `station` | Visibility, meters.                                        |
| `ecobee_outdoor_probability_of_precipitation` | `thermostat_id`, `thermostat_name`, `station` | Probability of precipitation, percent (0–100).             |
| `ecobee_outdoor_temp_high`                    | `thermostat_id`, `thermostat_name`, `station` | Forecast daily high, degrees.                              |
| `ecobee_outdoor_temp_low`                     | `thermostat_id`, `thermostat_name`, `station` | Forecast daily low, degrees.                               |


### Equipment runtime *(extension)*

`ecobee_equipment_running{equipment}` is a 0/1 gauge with one series per
known equipment identifier per thermostat. Known values are:
`heatPump`, `heatPump2`, `heatPump3`, `compCool1`, `compCool2`,
`auxHeat1`, `auxHeat2`, `auxHeat3`, `fan`, `humidifier`, `dehumidifier`,
`ventilator`, `economizer`, `compHotWater`, `auxHotWater`. Unknown
identifiers that show up in a future ecobee build still appear as
extra series.


| Metric                     | Labels                                          | Description                                       |
| -------------------------- | ----------------------------------------------- | ------------------------------------------------- |
| `ecobee_equipment_running` | `thermostat_id`, `thermostat_name`, `equipment` | 1 if that equipment is currently running, else 0. |


### Hold / events *(extension)*

Emitted when a hold, vacation, or demand-response event is actively running.


| Metric                  | Labels                                           | Description                                |
| ----------------------- | ------------------------------------------------ | ------------------------------------------ |
| `ecobee_hold_heat_temp` | `thermostat_id`, `thermostat_name`               | Heat hold setpoint, degrees.               |
| `ecobee_hold_cool_temp` | `thermostat_id`, `thermostat_name`               | Cool hold setpoint, degrees.               |
| `ecobee_event_type`     | `thermostat_id`, `thermostat_name`, `event_type` | Always `0`; event type encoded as a label. |


### Extended runtime *(extension)*

Per-equipment runtime from the last three 5-minute intervals (interval `0` = oldest, `2` = newest). Values are seconds of runtime within each 5-minute bucket (0–300).


| Metric                              | Labels                                                      | Description                                          |
| ----------------------------------- | ----------------------------------------------------------- | ---------------------------------------------------- |
| `ecobee_equipment_runtime_seconds`  | `thermostat_id`, `thermostat_name`, `equipment`, `interval` | Equipment runtime in seconds per 5-minute bucket.    |
| `ecobee_demand_management_offset`   | `thermostat_id`, `thermostat_name`, `interval`              | Demand-management temperature offset, degrees.       |
| `ecobee_current_electricity_bill`   | `thermostat_id`, `thermostat_name`                          | Current bill from a paired utility meter (if any).   |
| `ecobee_projected_electricity_bill` | `thermostat_id`, `thermostat_name`                          | Projected bill from a paired utility meter (if any). |


Equipment names match the extended-runtime API fields: `heatPump1`, `heatPump2`, `auxHeat1`, `auxHeat2`, `auxHeat3`, `cool1`, `cool2`, `fan`, `humidifier`, `dehumidifier`, `ventilator`, `economizer`.

### Alerts *(extension)*


| Metric                | Labels                                                           | Description                                |
| --------------------- | ---------------------------------------------------------------- | ------------------------------------------ |
| `ecobee_alert_active` | `thermostat_id`, `thermostat_name`, `alert_type`, `alert_number` | 1 for each active alert on the thermostat. |


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
                                          impl 1: BeehiveProvider   (cloud)
                                          impl 2: HomeKitProvider   (local LAN)
                                          impl 3: FakeProvider      (demo/test)
```

`ThermostatProvider` is the seam: anything that can return a
`Vec<Thermostat>` can drive the exporter. HomeKit and Beehive share the
same metrics and HTTP layers.

## HomeKit backend

Ecobee thermostats expose temperature, humidity, occupancy, and HVAC
state over local HomeKit. Using this path avoids the Beehive ToS
concerns above — data stays on your LAN and no Auth0 session is needed.

Pairing is a one-time bootstrap; the exporter reuses stored keys on
every poll.

### 1. Discover the thermostat

The ecobee must be on the same LAN as the exporter host, with HomeKit
enabled in **Settings → HomeKit** (note the 8-digit setup code).

```sh
cargo run --bin ecobee-homekit-pair -- --discover-only
```

Example output:

```
Main Floor  id=AA:BB:CC:DD:EE:FF  192.168.1.42:51826  category=Thermostat  model=ecobee3 lite
```

Copy the `id=` value for the next step.

### 2. Pair and save keys

```sh
cargo run --bin ecobee-homekit-pair -- \
  --device-id AA:BB:CC:DD:EE:FF \
  --code 12345678 \
  --alias ecobee
```

This writes pairing keys to `./homekit-pairings.json` (override with
`--pairing-file` or `ECOBEE_HOMEKIT_PAIRING_FILE`). Treat the file like
a secret (`chmod 600`).

### 3. Run the exporter

In `ecobee-exporter.toml`:

```toml
provider = "homekit"

[homekit]
pairing_file = "./homekit-pairings.json"
```

Or via environment:

```sh
export ECOBEE_PROVIDER=homekit
export ECOBEE_HOMEKIT__PAIRING_FILE=./homekit-pairings.json
cargo run --release
```

Outdoor weather, extended runtime, alerts, and some Beehive-only fields
are not available over HomeKit; those metrics are simply omitted.

## Roadmap

Short-term:

- ~~Implement Auth0 + PKCE login + refresh.~~ Done.
- ~~Implement the data-API call against `api.ecobee.com/1/thermostat`.~~ Done.
- ~~Add a fixture-based parsing test for the response shape.~~ Done.
- ~~Native HomeKit `ThermostatProvider` via in-tree `housekey` crate.~~ Done.

Medium-term (parity-plus):

- ~~`ecobee_equipment_running{equipment}`.~~ Done.
- ~~Outdoor weather metrics from the weather block.~~ Done.
- ~~Tier-1 runtime/settings/program metrics (humidity setpoints, fan mode, climate, hold state).~~ Done.
- ~~Extended runtime seconds + demand-management offsets.~~ Done.
- ~~Active alerts.~~ Done.
- Air-quality metrics on Premium models (`co2`, `vocPpb`, `airQualityAccuracy`).

Long-term (operational polish):

- ~~Dockerfile + GitHub Actions image build.~~ Done.
- ~~Multi-arch container images.~~ Done.
- systemd unit file with `DynamicUser=yes` and a state directory.
- Grafana dashboard JSON checked in under `dashboards/`.

## GitHub Actions

CI runs on every push and pull request via `[.github/workflows/ci.yml](./.github/workflows/ci.yml)`:


| Job      | What it does                                                                                               |
| -------- | ---------------------------------------------------------------------------------------------------------- |
| `rust`   | `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --locked`                                  |
| `docker` | Builds the Alpine image; pushes to GitHub Container Registry on branch pushes and tags (build-only on PRs) |


After the first push to `master`, pull the image:

```sh
docker pull ghcr.io/OWNER/ecobee-exporter:master
```

Tagged releases also publish `:latest`:

```sh
git tag v0.1.0
git push origin v0.1.0
# → ghcr.io/OWNER/ecobee-exporter:v0.1.0
# → ghcr.io/OWNER/ecobee-exporter:latest
```

Make the package public under **GitHub → Packages → ecobee-exporter → Package settings** if you want anonymous pulls without `docker login ghcr.io`.

## Development

```sh
cargo test                       # FakeProvider round-trip
cargo clippy --all-targets       # warnings should be empty
cargo run -- --demo              # local smoke test
RUST_LOG=ecobee_exporter=debug cargo run -- --demo
```

## License

Apache-2.0. See `[LICENSE](./LICENSE)`.