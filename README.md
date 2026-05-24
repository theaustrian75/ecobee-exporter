# Prometheus ecobee-exporter

A Prometheus exporter for ecobee thermostats, written in Rust. Two backends share the same `/metrics` endpoint and metric names:


| Backend     | `provider` value    | Data path                                                   | Best for                            |
| ----------- | ------------------- | ----------------------------------------------------------- | ----------------------------------- |
| **Beehive** | `beehive` (default) | ecobee cloud API via Auth0 JWT                              | Full metric coverage, remote access |
| **HomeKit** | `homekit`           | Local HAP over LAN (`[housekey](crates/housekey/housekey)`) | No cloud login, data stays on LAN   |


Pick one backend per exporter instance. See [Beehive backend](#beehive-backend-cloud-api)
or [HomeKit backend](#homekit-backend-local-lan) below for setup.

## Current status

End-to-end functional for both backends.

- HTTP server on `/metrics` and `/healthz`.
- Polling loop with a configurable interval and a 60-second floor.
- Configuration via TOML file, environment variables, and CLI flags.
- `--demo` mode that serves canned data without credentials.
- **Beehive:** Auth0 + PKCE one-time login (`ecobee-login`), refresh-token rotation, mode-0600 state file, full billykwooten-compatible metric set.
- **HomeKit:** one-time LAN pairing (`ecobee-homekit-pair`), persistent pairing keys, core thermostat + sensor metrics over HAP.

## Demo mode (no credentials)

Works with either backend configured; upstream is never contacted.

```sh
cargo run -- --demo
curl http://localhost:9098/metrics
```

You should see a populated set of `ecobee_*` series against a synthetic
two-sensor thermostat (Beehive-shaped demo data).

---

## Beehive backend (cloud API)

Reads thermostat, sensor, weather, runtime, and alert data from Ecobee's internal Beehive mobile-app API at `api.ecobee.com/1/thermostat` using an Auth0-issued JWT bearer token.

### Important: terms of service

This path scrapes ecobee's mobile-app API, not the official [developer API](https://www.ecobee.com/home/developer/api/introduction/index.shtml). New developer accounts have been closed to registration [since March 28, 2024](https://github.com/home-assistant/home-assistant.io/pull/33272) and pre-existing keys are not assumed.

Using the Beehive API to back a long-running exporter is almost certainly a violation of ecobee's terms of service:

- Ecobee can revoke your account or rotate mobile-app client credentials at any time, breaking this exporter without notice.
- Keep scrape frequency reasonable. The default poll interval is three minutes, matching how often the thermostat reports new data.
- Personal use against your own thermostats only. 

If you have a pre-2024 developer key, prefer an official-API exporter ([billykwooten/ecobee-exporter](https://github.com/billykwooten/ecobee-exporter), [cfunkhouser/promobee](https://github.com/cfunkhouser/promobee), [mrala/ecobee_prometheus_exporter](https://github.com/mrala/ecobee_prometheus_exporter)).

### Setup

The mobile app uses Auth0 Universal Login with mandatory MFA. A headless exporter cannot complete MFA on every restart, so you mint a refresh token once interactively and the exporter reuses it.

**1. One-time login bootstrap**

```sh
cargo run --bin ecobee-login
```

Prints an Auth0 `/authorize` URL. Open it in a desktop browser, complete login + MFA, then copy the full callback URL from the address bar when the browser lands on `https://auth.ecobee.com/android/com.ecobee.athenamobile/callback?...` (a blank or error page on desktop is expected). Paste the URL at the prompt.

The helper exchanges the code and writes the refresh token to `ecobee-exporter.state.json` with mode `0600`.

**2. Configure (optional — Beehive is the default)**

```toml
provider = "beehive"

[beehive]
# endpoint = "https://api.ecobee.com/1"
# refresh_token normally lives in state_file after ecobee-login
```

Or via environment:

```sh
export ECOBEE_PROVIDER=beehive   # optional; this is the default
# or: cargo run --release -- --provider beehive
```

**3. Run the exporter**

```sh
cargo run --release
curl http://localhost:9098/metrics
```

### Beehive Docker

Build:

```sh
docker build -t ecobee-exporter:local .
```

One-time interactive login (mount a volume for token persistence):

```sh
docker run --rm -it \
  -v ecobee-exporter-data:/var/lib/ecobee-exporter \
  --entrypoint ecobee-login \
  ecobee-exporter:local
```

Run the exporter:

```sh
docker run -d --name ecobee-exporter --restart unless-stopped \
  -p 9098:9098 \
  -v ecobee-exporter-data:/var/lib/ecobee-exporter \
  -e ECOBEE_STATE_FILE=/var/lib/ecobee-exporter/state.json \
  -e TZ=America/New_York \
  ecobee-exporter:local
```

See [Docker compose](#docker-compose) below for a compose-based workflow. `TZ` sets the container timezone for log timestamps (metrics are unitless gauges). Other `ECOBEE_*` variables follow the same prefix as bare-metal runs.

---

## HomeKit backend (local LAN)

Reads temperature, humidity, occupancy, HVAC mode, and coarse equipment state directly from the thermostat over Apple HomeKit (HAP). No Auth0 login, no cloud API, and no Beehive ToS concerns — traffic stays on your LAN.

Pairing is a one-time bootstrap; the exporter reuses stored keys on every poll.

### Prerequisites

- Ecobee thermostat with HomeKit enabled (**Settings → HomeKit**).
- 8-digit HomeKit setup code from that screen.
- Exporter host on the **same LAN** as the thermostat (mDNS + TCP to port 51826).
- For Docker: run pairing on the host (or use `network_mode: host`) so mDNS discovery works; the running exporter must reach the thermostat IP.

### Setup

**1. Discover the thermostat**

```sh
cargo run --bin ecobee-homekit-pair -- --discover-only
```

Example output:

```
Main Floor  id=AA:BB:CC:DD:EE:FF  192.168.1.42:51826  category=Thermostat  model=ecobee3 lite
```

Copy the `id=` value.

**2. Pair and save keys**

```sh
cargo run --bin ecobee-homekit-pair -- \
  --device-id AA:BB:CC:DD:EE:FF \
  --code 12345678 \
  --alias ecobee
```

Writes pairing keys to `./homekit-pairings.json`. Override with `--pairing-file` or `ECOBEE_HOMEKIT_PAIRING_FILE`. Treat the file like a secret (`chmod 600`).

**3. Configure**

```toml
provider = "homekit"

[homekit]
pairing_file = "./homekit-pairings.json"
```

Or via environment or CLI flag:

```sh
export ECOBEE_PROVIDER=homekit
export ECOBEE_HOMEKIT__PAIRING_FILE=./homekit-pairings.json
# or: cargo run --release -- --provider homekit
```

**4. Run the exporter**

```sh
cargo run --release
curl http://localhost:9098/metrics
```

Re-pairing is only needed if you reset HomeKit on the thermostat or delete the pairing file. Outdoor weather, extended runtime, alerts, and other Beehive-only fields are not available over HomeKit — see the [metrics tables](#metrics) for details.

### HomeKit Docker notes

- Run `ecobee-homekit-pair` on the host (recommended) or in a container with `network_mode: host` so `_hap._tcp` mDNS discovery works.
- Mount `homekit-pairings.json` into the exporter container at the path configured in `homekit.pairing_file`.
- Set `ECOBEE_PROVIDER=homekit` and `ECOBEE_HOMEKIT__PAIRING_FILE=…`.

---

## Configuration reference

Layered, lowest-to-highest precedence:

1. Built-in defaults.
2. `ecobee-exporter.toml` in the working directory.
3. The file at `$ECOBEE_EXPORTER_CONFIG`, or the `--config` flag.
4. Environment variables prefixed `ECOBEE_`. Nested keys use `__`,
  e.g. `ECOBEE_BEEHIVE__ENDPOINT=https://…`.


| Key                     | Default                        | Notes                                                             |
| ----------------------- | ------------------------------ | ----------------------------------------------------------------- |
| `listen_addr`           | `0.0.0.0:9098`                 | Where `/metrics` is served.                                       |
| `poll_interval`         | `3m`                           | Floored to 60s.                                                   |
| `state_file`            | `./ecobee-exporter.state.json` | Beehive refresh tokens (`ecobee-login`).                          |
| `demo`                  | `false`                        | Serve canned data; no upstream calls.                             |
| `provider`              | `beehive`                      | `beehive` (cloud) or `homekit` (local LAN). Also `--provider` or `ECOBEE_PROVIDER`. |
| `homekit.pairing_file`  | `./homekit-pairings.json`      | HomeKit keys from `ecobee-homekit-pair`. `chmod 600` recommended. |
| `beehive.endpoint`      | `https://api.ecobee.com/1`     | Data API base URL.                                                |
| `beehive.user_agent`    | `ecobee-exporter/0.1.0`        | Override to mimic the mobile app if needed.                       |
| `beehive.extra_headers` | `[]`                           | `[key, value]` pairs added to every request.                      |
| `beehive.refresh_token` | `null`                         | Normally in `state_file` after `ecobee-login`.                    |


Put secrets in the config file with `chmod 600`, not env vars — env vars leak into systemd journals and `ps`.

Copy `ecobee-exporter.example.toml` to `ecobee-exporter.toml` only when you need to override defaults.

### Docker compose

Copy `docker-compose.example.yml` and adjust the image tag if needed:

```sh
# Beehive: one-time Auth0 bootstrap (interactive).
docker compose -f docker-compose.example.yml run --rm -it \
  --entrypoint ecobee-login ecobee-exporter

docker compose -f docker-compose.example.yml up -d
curl http://localhost:9098/metrics
```

For HomeKit, pair on the host first and mount `homekit-pairings.json`, then set `ECOBEE_PROVIDER=homekit` in the compose environment.

---

## Metrics

Metric names mirror [billykwooten/ecobee-exporter](https://github.com/billykwooten/ecobee-exporter) for dashboard compatibility. Extensions beyond that baseline are marked *(extension)* in the description column.

**Backend availability**


| Symbol  | Meaning                                                    |
| ------- | ---------------------------------------------------------- |
| Yes     | Populated from that backend when data exists               |
| Partial | Emitted with reduced or mapped semantics (see description) |
| No      | Not sourced from that backend; series omitted              |


### Thermostat + sensor


| Metric                          | Labels                                                                        | Beehive | HomeKit | Description                                                                                  |
| ------------------------------- | ----------------------------------------------------------------------------- | ------- | ------- | -------------------------------------------------------------------------------------------- |
| `ecobee_fetch_time`             | —                                                                             | Yes     | Yes     | Seconds the last upstream fetch took.                                                        |
| `ecobee_fetch_failures_total`   | —                                                                             | Yes     | Yes     | Counter of failed fetches since start. *(extension)*                                         |
| `ecobee_actual_temperature`     | `thermostat_id`, `thermostat_name`                                            | Yes     | Yes     | Current temperature, degrees.                                                                |
| `ecobee_target_temperature_min` | `thermostat_id`, `thermostat_name`                                            | Yes     | Yes     | Heating setpoint, degrees.                                                                   |
| `ecobee_target_temperature_max` | `thermostat_id`, `thermostat_name`                                            | Yes     | Yes     | Cooling setpoint, degrees.                                                                   |
| `ecobee_currenthvacmode`        | `thermostat_id`, `thermostat_name`, `current_hvac_mode`                       | Yes     | Yes     | Always `0`; mode encoded as a label (billykwooten convention).                               |
| `ecobee_connected`              | `thermostat_id`, `thermostat_name`                                            | Yes     | Partial | Beehive: cloud reachability. HomeKit: always `1` when paired (LAN, not cloud). *(extension)* |
| `ecobee_temperature`            | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Yes     | Yes     | Per-sensor temperature, degrees.                                                             |
| `ecobee_humidity`               | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Yes     | Yes     | Per-sensor humidity, percent.                                                                |
| `ecobee_occupancy`              | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Yes     | Yes     | Per-sensor occupancy (0 or 1).                                                               |
| `ecobee_in_use`                 | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Yes     | Partial | Beehive: included in thermostat average. HomeKit: always `0`.                                |
| `ecobee_actual_humidity`        | `thermostat_id`, `thermostat_name`                                            | Yes     | Yes     | Thermostat-averaged humidity, percent. *(extension)*                                         |
| `ecobee_desired_humidity`       | `thermostat_id`, `thermostat_name`                                            | Yes     | No      | Humidifier setpoint, percent. *(extension)*                                                  |
| `ecobee_desired_dehumidity`     | `thermostat_id`, `thermostat_name`                                            | Yes     | No      | Dehumidifier setpoint, percent. *(extension)*                                                |
| `ecobee_raw_temperature`        | `thermostat_id`, `thermostat_name`                                            | Yes     | No      | Dry-bulb temperature at the thermostat. *(extension)*                                        |
| `ecobee_desired_fan_mode`       | `thermostat_id`, `thermostat_name`, `desired_fan_mode`                        | Yes     | No      | Fan mode as label (`auto`, `on`). *(extension)*                                              |
| `ecobee_current_climate`        | `thermostat_id`, `thermostat_name`, `current_climate`                         | Yes     | No      | Active schedule climate as label. *(extension)*                                              |
| `ecobee_hold_active`            | `thermostat_id`, `thermostat_name`                                            | Yes     | Partial | Beehive: real hold/DR state. HomeKit: always `0`. *(extension)*                              |
| `ecobee_follow_me_comfort`      | `thermostat_id`, `thermostat_name`                                            | Yes     | Partial | Beehive: real setting. HomeKit: always `0`. *(extension)*                                    |
| `ecobee_smart_circulation`      | `thermostat_id`, `thermostat_name`                                            | Yes     | Partial | Beehive: real setting. HomeKit: always `0`. *(extension)*                                    |
| `ecobee_heat_stages`            | `thermostat_id`, `thermostat_name`                                            | Yes     | No      | Configured heating stages. *(extension)*                                                     |
| `ecobee_cool_stages`            | `thermostat_id`, `thermostat_name`                                            | Yes     | No      | Configured cooling stages. *(extension)*                                                     |


### Outdoor weather *(extension)*

Sourced from the thermostat's weather station (Beehive only). Missing readings (ecobee `-5002` sentinel) are suppressed.


| Metric                                        | Labels                                        | Beehive | HomeKit | Description                         |
| --------------------------------------------- | --------------------------------------------- | ------- | ------- | ----------------------------------- |
| `ecobee_outdoor_temperature`                  | `thermostat_id`, `thermostat_name`, `station` | Yes     | No      | Outdoor temperature, degrees.       |
| `ecobee_outdoor_humidity`                     | `thermostat_id`, `thermostat_name`, `station` | Yes     | No      | Outdoor relative humidity, percent. |
| `ecobee_outdoor_pressure_mb`                  | `thermostat_id`, `thermostat_name`, `station` | Yes     | No      | Sea-level pressure, millibars.      |
| `ecobee_outdoor_dewpoint`                     | `thermostat_id`, `thermostat_name`, `station` | Yes     | No      | Outdoor dewpoint, degrees.          |
| `ecobee_outdoor_wind_speed_mph`               | `thermostat_id`, `thermostat_name`, `station` | Yes     | No      | Wind speed, mph.                    |
| `ecobee_outdoor_wind_gust_mph`                | `thermostat_id`, `thermostat_name`, `station` | Yes     | No      | Wind gust, mph.                     |
| `ecobee_outdoor_wind_bearing_degrees`         | `thermostat_id`, `thermostat_name`, `station` | Yes     | No      | Wind bearing, degrees.              |
| `ecobee_outdoor_visibility_meters`            | `thermostat_id`, `thermostat_name`, `station` | Yes     | No      | Visibility, meters.                 |
| `ecobee_outdoor_probability_of_precipitation` | `thermostat_id`, `thermostat_name`, `station` | Yes     | No      | Precipitation probability, percent. |
| `ecobee_outdoor_temp_high`                    | `thermostat_id`, `thermostat_name`, `station` | Yes     | No      | Forecast daily high, degrees.       |
| `ecobee_outdoor_temp_low`                     | `thermostat_id`, `thermostat_name`, `station` | Yes     | No      | Forecast daily low, degrees.        |


### Equipment runtime *(extension)*


| Metric                     | Labels                                          | Beehive | HomeKit | Description                                                                                                                                |
| -------------------------- | ----------------------------------------------- | ------- | ------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `ecobee_equipment_running` | `thermostat_id`, `thermostat_name`, `equipment` | Yes     | Partial | Beehive: full `equipmentStatus` list. HomeKit: maps heating/cooling to `heatPump1` / `compCool1` only; other equipment series stay at `0`. |


Known equipment identifiers: `heatPump`, `heatPump2`, `heatPump3`, `compCool1`, `compCool2`, `auxHeat1`, `auxHeat2`, `auxHeat3`, `fan`, `humidifier`, `dehumidifier`, `ventilator`, `economizer`, `compHotWater`, `auxHotWater`.

### Hold / events *(extension)*


| Metric                  | Labels                                           | Beehive | HomeKit | Description                  |
| ----------------------- | ------------------------------------------------ | ------- | ------- | ---------------------------- |
| `ecobee_hold_heat_temp` | `thermostat_id`, `thermostat_name`               | Yes     | No      | Heat hold setpoint, degrees. |
| `ecobee_hold_cool_temp` | `thermostat_id`, `thermostat_name`               | Yes     | No      | Cool hold setpoint, degrees. |
| `ecobee_event_type`     | `thermostat_id`, `thermostat_name`, `event_type` | Yes     | No      | Event type as label.         |


### Extended runtime *(extension)*

Per-equipment runtime from the last three 5-minute intervals (interval `0` = oldest, `2` = newest). Values are seconds within each bucket (0–300).


| Metric                              | Labels                                                      | Beehive | HomeKit | Description                                    |
| ----------------------------------- | ----------------------------------------------------------- | ------- | ------- | ---------------------------------------------- |
| `ecobee_equipment_runtime_seconds`  | `thermostat_id`, `thermostat_name`, `equipment`, `interval` | Yes     | No      | Equipment runtime per 5-minute bucket.         |
| `ecobee_demand_management_offset`   | `thermostat_id`, `thermostat_name`, `interval`              | Yes     | No      | Demand-management temperature offset, degrees. |
| `ecobee_current_electricity_bill`   | `thermostat_id`, `thermostat_name`                          | Yes     | No      | Current utility meter bill (if paired).        |
| `ecobee_projected_electricity_bill` | `thermostat_id`, `thermostat_name`                          | Yes     | No      | Projected utility meter bill (if paired).      |


Equipment names in extended runtime: `heatPump1`, `heatPump2`, `auxHeat1`, `auxHeat2`, `auxHeat3`, `cool1`, `cool2`, `fan`, `humidifier`, `dehumidifier`, `ventilator`, `economizer`.

### Alerts *(extension)*


| Metric                | Labels                                                           | Beehive | HomeKit | Description                  |
| --------------------- | ---------------------------------------------------------------- | ------- | ------- | ---------------------------- |
| `ecobee_alert_active` | `thermostat_id`, `thermostat_name`, `alert_type`, `alert_number` | Yes     | No      | One series per active alert. |


---

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

## Roadmap

Short-term:

- ~~Implement Auth0 + PKCE login + refresh.~~ Done.
- ~~Implement the data-API call against `api.ecobee.com/1/thermostat`.~~ Done.
- ~~Add a fixture-based parsing test for the response shape.~~ Done.
- ~~Native HomeKit `ThermostatProvider` via in-tree `housekey` crate.~~ Done.

Medium-term (parity-plus):

- ~~`ecobee_equipment_running{equipment}`.~~ Done.
- ~~Outdoor weather metrics from the weather block.~~ Done.
- ~~Tier-1 runtime/settings/program metrics.~~ Done.
- ~~Extended runtime seconds + demand-management offsets.~~ Done.
- ~~Active alerts.~~ Done.
- Air-quality metrics on Premium models (`co2`, `vocPpb`, `airQualityAccuracy`).

Long-term (operational polish):

- ~~Dockerfile + GitHub Actions image build.~~ Done.
- ~~Multi-arch container images.~~ Done.
- systemd unit file with `DynamicUser=yes` and a state directory.
- Grafana dashboard JSON checked in under `dashboards/`.

## GitHub Actions

CI runs on every push and pull request via
`[.github/workflows/ci.yml](./.github/workflows/ci.yml)`:


| Job      | What it does                                                              |
| -------- | ------------------------------------------------------------------------- |
| `rust`   | `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --locked` |
| `docker` | Builds the Alpine image; pushes to GHCR on branch pushes and tags         |


After the first push to `master`:

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

Make the package public under **GitHub → Packages → ecobee-exporter → Package settings** for anonymous pulls without `docker login ghcr.io`.

## Development

```sh
cargo test                       # unit + integration tests
cargo clippy --all-targets       # warnings should be empty
cargo run -- --demo              # local smoke test
RUST_LOG=ecobee_exporter=debug cargo run -- --demo
```

## License

Apache-2.0. See `[LICENSE](./LICENSE)`.