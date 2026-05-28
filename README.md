# Prometheus ecobee-exporter

A Prometheus exporter for ecobee thermostats, written in Rust. Two backends share the same `/metrics` endpoint and metric names:


| Backend            | Provider            | Data path                      | Metric coverage                                    |
| ------------------ | ------------------- | ------------------------------ | -------------------------------------------------- |
| **Beehive**        | `beehive` (default) | ecobee cloud API via Auth0 JWT | Full — see [metrics](#metrics)                     |
| **Home Assistant** | `homeassistant`     | HA REST API (`/api/states`)    | Core + sensors + weather — see [metrics](#metrics) |


Pick one backend per exporter instance. See [Beehive backend](#beehive-backend-cloud-api) or [Home Assistant backend](#home-assistant-backend) below for setup.

## Current status

End-to-end functional for both backends.

- HTTP(S) server on `/metrics`, `/healthz`, `/readiness`, and `/liveness` (optional TLS via PEM certificate + key).
- Polling loop with a configurable interval and a 60-second floor.
- Configuration via TOML file, environment variables, and CLI flags.
- `--demo` mode that serves canned data without credentials.
- **Beehive:** Auth0 + PKCE one-time login (`ecobee-login`), refresh-token rotation, mode-0600 state file, full metric coverage.
- **Home Assistant:** long-lived access token, auto-discovers `climate.`* entities (or an explicit allow-list), maps climate + related sensor/weather entities into the same Prometheus schema.

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
  -e UID="$(id -u)" -e GID="$(id -g)" \
  --entrypoint /usr/local/bin/docker-entrypoint.sh \
  ecobee-exporter:local ecobee-login
```

Run the exporter:

```sh
docker run -d --name ecobee-exporter --restart unless-stopped \
  -p 9098:9098 \
  -v ecobee-exporter-data:/var/lib/ecobee-exporter \
  -e ECOBEE_STATE_FILE=/var/lib/ecobee-exporter/state.json \
  -e UID="$(id -u)" -e GID="$(id -g)" \
  -e TZ=America/New_York \
  ecobee-exporter:local
```

Set `UID` / `GID` (or `PUID` / `PGID`) to the host user that owns mounted volumes. The entrypoint recreates the container's `ecobee` user with those ids before starting (default `1000`).

See [Docker compose](#docker-compose) below for a compose-based workflow. `TZ` sets the container timezone for log timestamps (metrics are unitless gauges). Other `ECOBEE_*` variables follow the same prefix as bare-metal runs.

---

## Home Assistant backend

Reads ecobee data that Home Assistant already exposes — from the **ecobee** cloud integration and/or **HomeKit Device** (`homekit_controller`) — via the [REST API](https://developers.home-assistant.io/docs/api/rest/). This is the recommended path when HA is already polling your thermostats.

### Prerequisites

- Home Assistant reachable from the exporter host (HTTP/S).
- A [long-lived access token](https://www.home-assistant.io/docs/authentication/#your-account-profile) (Profile → Security → Long-Lived Access Tokens).
- Ecobee thermostats already integrated in HA (`climate.`* entities).

### Setup

**1. Configure**

```toml
provider = "homeassistant"

[homeassistant]
url = "http://homeassistant.local:8123"
token = "YOUR_LONG_LIVED_TOKEN"
# Optional: restrict to specific thermostats (default: every climate.* entity)
# climate_entities = ["climate.living_room", "climate.master_bedroom"]
```

Or via environment / CLI:

```sh
export ECOBEE_PROVIDER=homeassistant
export ECOBEE_HOMEASSISTANT__URL=http://homeassistant.local:8123
export ECOBEE_HOMEASSISTANT__TOKEN=YOUR_LONG_LIVED_TOKEN
# or:
cargo run --release -- \
  --provider homeassistant \
  --homeassistant-url http://homeassistant.local:8123 \
  --homeassistant-token YOUR_LONG_LIVED_TOKEN \
  --homeassistant-climate-entity climate.living_room
```

**2. Run the exporter**

```sh
cargo run --release
curl http://localhost:9098/metrics
```

### What HA provides

See the [metrics tables](#metrics) for per-series availability. In short, Home Assistant covers core thermostat readings, matched remote sensors, linked outdoor weather, and partial equipment/schedule fields when HA exposes them:


| HA source                              | Mapped metrics / fields                                                                      |
| -------------------------------------- | -------------------------------------------------------------------------------------------- |
| `climate.*`                            | Current/target temps, HVAC mode/action, humidity, fan mode, preset/climate mode              |
| ecobee cloud `climate` attrs           | `equipment_running` (CSV), richer preset/climate metadata                                    |
| Related `sensor.*` / `binary_sensor.*` | Remote sensor temperature, humidity, occupancy (matched by entity-id stem)                   |
| `weather.*`                            | Outdoor temperature, humidity, pressure, dewpoint, wind, visibility, forecast high/low / PoP |


Extended runtime, alerts, hold/events, and demand-management offsets are **not** available via Home Assistant unless you add matching entities yourself.

---

## Configuration reference

Layered, lowest-to-highest precedence:

1. Built-in defaults.
2. `ecobee-exporter.toml` in the working directory.
3. The file at `$ECOBEE_EXPORTER_CONFIG`, or the `--config` flag.
4. Environment variables prefixed `ECOBEE_`. Nested keys use `__`,
  e.g. `ECOBEE_BEEHIVE__ENDPOINT=https://…`.
5. CLI flags on `ecobee-exporter` (each mirrors its `ECOBEE_*` env var; run `--help`).


| Key                              | Default                        | CLI flag / env                                                                   |
| -------------------------------- | ------------------------------ | -------------------------------------------------------------------------------- |
| `listen_addr`                    | `0.0.0.0:9098`                 | `--listen-addr` / `ECOBEE_LISTEN_ADDR`                                           |
| `tls.cert_file`                  | *(disabled)*                   | `--tls-cert-file` / `ECOBEE_TLS__CERT_FILE` — PEM certificate chain              |
| `tls.key_file`                   | *(disabled)*                   | `--tls-key-file` / `ECOBEE_TLS__KEY_FILE` — PEM private key                      |
| `poll_interval`                  | `3m`                           | `--poll-interval` / `ECOBEE_POLL_INTERVAL` (floored to 60s)                      |
| `state_file`                     | `./ecobee-exporter.state.json` | `--state-file` / `ECOBEE_STATE_FILE`                                             |
| `demo`                           | `false`                        | `--demo` / `ECOBEE_DEMO`                                                         |
| `provider`                       | `beehive`                      | `--provider` / `ECOBEE_PROVIDER` (`beehive`, `homeassistant`)                    |
| `homeassistant.url`              | *(required)*                   | `--homeassistant-url` / `ECOBEE_HOMEASSISTANT__URL`                              |
| `homeassistant.token`            | *(required)*                   | `--homeassistant-token` / `ECOBEE_HOMEASSISTANT__TOKEN`                          |
| `homeassistant.climate_entities` | `[]` (all climates)            | `--homeassistant-climate-entity` (repeat) / TOML array                           |
| `homeassistant.weather_entities` | `[]` (auto-link)               | `--homeassistant-weather-entity` (repeat) / TOML array — e.g. `weather.ecobee`   |
| `beehive.endpoint`               | `https://api.ecobee.com/1`     | `--beehive-endpoint` / `ECOBEE_BEEHIVE__ENDPOINT`                                |
| `beehive.user_agent`             | `ecobee-exporter/0.1.0`        | `--beehive-user-agent` / `ECOBEE_BEEHIVE__USER_AGENT`                            |
| `beehive.extra_headers`          | `[]`                           | `--beehive-header KEY=VALUE` (repeat); or TOML / `ECOBEE_BEEHIVE__EXTRA_HEADERS` |
| `beehive.refresh_token`          | `null`                         | `--beehive-refresh-token` / `ECOBEE_BEEHIVE__REFRESH_TOKEN`                      |


Put secrets in the config file with `chmod 600`, not env vars — env vars leak into systemd journals and `ps`.

Copy `ecobee-exporter.example.toml` to `ecobee-exporter.toml` only when you need to override defaults.

### TLS

Set `[tls]` (or `--tls-cert-file` / `--tls-key-file`) to serve `/metrics` and `/healthz` over HTTPS instead of plain HTTP. Both paths must point at PEM files (e.g. Let's Encrypt `fullchain.pem` + `privkey.pem`). When TLS is enabled, configure Prometheus with `scheme: https`.

```toml
listen_addr = "0.0.0.0:9098"

[tls]
cert_file = "/etc/letsencrypt/live/example/fullchain.pem"
key_file = "/etc/letsencrypt/live/example/privkey.pem"
```

### Docker compose

Copy `docker-compose.example.yml` and adjust the image tag if needed:

```sh
# Beehive: one-time Auth0 bootstrap (interactive).
docker compose -f docker-compose.example.yml run --rm -it \
  --entrypoint /usr/local/bin/docker-entrypoint.sh ecobee-exporter ecobee-login

docker compose -f docker-compose.example.yml up -d
curl http://localhost:9098/metrics
```

The image healthcheck script (`docker-healthcheck.sh`) probes `/healthz` on `127.0.0.1` and the listen port from `ECOBEE_LISTEN_ADDR` (default `9098`; HTTP by default, HTTPS when `ECOBEE_TLS__CERT_FILE` and `ECOBEE_TLS__KEY_FILE` point at existing PEM files). The container is marked unhealthy when ecobee or Home Assistant is unreachable. Use `/liveness` for a process-only probe (always `200 ok`) in Kubernetes liveness checks.

**Podman:** GHCR images are built with `provenance: false`, `sbom: false`, and `oci-mediatypes=false` so embedded `HEALTHCHECK` metadata is preserved ([podman#18904](https://github.com/containers/podman/issues/18904)). If Podman still reports `has no defined healthcheck`, use `docker-compose.example.yml` (explicit healthcheck), `--health-cmd=/usr/local/bin/docker-healthcheck.sh`, or `podman build --format docker`.

**TLS:** use the `tls` profile and mount PEM files:

```sh
mkdir -p certs
# copy or symlink fullchain.pem and privkey.pem into ./certs/
docker compose -f docker-compose.example.yml --profile tls up -d
curl https://localhost:9098/metrics
```

Configure Prometheus with `scheme: https`. The healthcheck script passes `--no-check-certificate` when exporter TLS is enabled.

**Home Assistant:** create a `.env` file with your long-lived token, then start the `homeassistant` profile:

```sh
echo 'ECOBEE_HOMEASSISTANT__TOKEN=your_long_lived_token' >> .env
docker compose -f docker-compose.example.yml --profile homeassistant up -d
curl http://localhost:9098/metrics
```

Point `ECOBEE_HOMEASSISTANT__URL` at wherever HA listens (default `http://host.docker.internal:8123` reaches the Docker host). See [Home Assistant backend](#home-assistant-backend) for entity filtering and weather linking.

---

## Metrics

Metric names mirror [billykwooten/ecobee-exporter](https://github.com/billykwooten/ecobee-exporter) for dashboard compatibility. Extensions beyond that baseline are marked *(extension)* in the description column.

Every metric row lists availability for each backend:


| Symbol      | Meaning                                                                  |
| ----------- | ------------------------------------------------------------------------ |
| **Yes**     | Populated when upstream data exists                                      |
| **Partial** | Emitted with reduced, mapped, or placeholder semantics (see description) |
| **No**      | Not sourced from that backend; series omitted                            |


### Availability summary


| Category                                   | Beehive         | Home Assistant                               |
| ------------------------------------------ | --------------- | -------------------------------------------- |
| Core thermostat temps / HVAC               | Yes             | Yes                                          |
| Remote sensor temps / humidity / occupancy | Yes             | Yes (matched `sensor.`* / `binary_sensor.`*) |
| Outdoor / forecast weather                 | Yes             | Yes (linked `weather.*`)                     |
| Equipment running                          | Yes (full list) | Partial (ecobee cloud attrs or `hvac_action` map) |
| Schedule / comfort settings                | Yes             | Partial                                      |
| Hold / events                              | Yes             | No                                           |
| Extended runtime / utility bills           | Yes             | No                                           |
| Alerts                                     | Yes             | No                                           |


### Thermostat + sensor


| Metric                          | Labels                                                                        | Beehive | Home Assistant | Description                                                                                                                                  |
| ------------------------------- | ----------------------------------------------------------------------------- | ------- | -------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `ecobee_fetch_time`             | —                                                                             | Yes     | Yes            | Seconds the last upstream fetch took.                                                                                                        |
| `ecobee_fetch_failures_total`   | —                                                                             | Yes     | Yes            | Counter of failed fetches since start. *(extension)*                                                                                         |
| `ecobee_actual_temperature`     | `thermostat_id`, `thermostat_name`                                            | Yes     | Yes            | Current temperature, degrees.                                                                                                                |
| `ecobee_target_temperature_min` | `thermostat_id`, `thermostat_name`                                            | Yes     | Yes            | Heating setpoint, degrees.                                                                                                                   |
| `ecobee_target_temperature_max` | `thermostat_id`, `thermostat_name`                                            | Yes     | Yes            | Cooling setpoint, degrees.                                                                                                                   |
| `ecobee_currenthvacmode`        | `thermostat_id`, `thermostat_name`, `current_hvac_mode`                       | Yes     | Yes            | Always `0`; mode encoded as a label (billykwooten convention).                                                                               |
| `ecobee_connected`              | `thermostat_id`, `thermostat_name`                                            | Yes     | Partial        | Beehive: cloud reachability. Home Assistant: `1` when climate entity is not `unavailable`. *(extension)*                                   |
| `ecobee_temperature`            | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Yes     | Yes            | Per-sensor temperature, degrees. Home Assistant: related entities matched by climate stem.                                                   |
| `ecobee_humidity`               | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Yes     | Yes            | Per-sensor humidity, percent.                                                                                                                |
| `ecobee_occupancy`              | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Yes     | Yes            | Per-sensor occupancy (0 or 1). Home Assistant: when matching `binary_sensor.`* exists.                                                       |
| `ecobee_in_use`                 | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Yes     | Partial        | Beehive: included in thermostat average. Home Assistant: always `0`.                                                                         |
| `ecobee_actual_humidity`        | `thermostat_id`, `thermostat_name`                                            | Yes     | Yes            | Thermostat humidity, percent. *(extension)*                                                                                                  |
| `ecobee_desired_humidity`       | `thermostat_id`, `thermostat_name`                                            | Yes     | No             | Humidifier setpoint, percent. *(extension)*                                                                                                  |
| `ecobee_desired_dehumidity`     | `thermostat_id`, `thermostat_name`                                            | Yes     | No             | Dehumidifier setpoint, percent. *(extension)*                                                                                                |
| `ecobee_raw_temperature`        | `thermostat_id`, `thermostat_name`                                            | Yes     | No             | Dry-bulb temperature at the thermostat. *(extension)*                                                                                        |
| `ecobee_desired_fan_mode`       | `thermostat_id`, `thermostat_name`, `desired_fan_mode`                        | Yes     | Partial        | Fan mode as label (`auto`, `on`). Home Assistant: when `fan_mode` attribute is present. *(extension)*                                        |
| `ecobee_current_climate`        | `thermostat_id`, `thermostat_name`, `current_climate`                         | Yes     | Partial        | Active schedule climate as label. Home Assistant: from `preset_mode` / `climate_mode`. *(extension)*                                         |
| `ecobee_hold_active`            | `thermostat_id`, `thermostat_name`                                            | Yes     | Partial        | Beehive: real hold/DR state. Home Assistant: always `0`. *(extension)*                                                                       |
| `ecobee_follow_me_comfort`      | `thermostat_id`, `thermostat_name`                                            | Yes     | Partial        | Beehive: real setting. Home Assistant: always `0`. *(extension)*                                                                             |
| `ecobee_smart_circulation`      | `thermostat_id`, `thermostat_name`                                            | Yes     | Partial        | Beehive: real setting. Home Assistant: always `0`. *(extension)*                                                                             |
| `ecobee_heat_stages`            | `thermostat_id`, `thermostat_name`                                            | Yes     | No             | Configured heating stages. *(extension)*                                                                                                       |
| `ecobee_cool_stages`            | `thermostat_id`, `thermostat_name`                                            | Yes     | No             | Configured cooling stages. *(extension)*                                                                                                     |


### Forecast / outdoor weather *(extension)*

Beehive reads the thermostat's paired weather station. Home Assistant maps linked `weather.`* entities (auto-discovered or via `weather_entities`). Missing readings are suppressed. Metric names use the `ecobee_forecast_*` prefix for Grafana dashboard compatibility.


| Metric                                         | Labels                                        | Beehive | Home Assistant | Description                                                        |
| ---------------------------------------------- | --------------------------------------------- | ------- | -------------- | ------------------------------------------------------------------ |
| `ecobee_forecast_temperature`                  | `thermostat_id`, `thermostat_name`, `station` | Yes     | Yes            | Outdoor temperature, degrees.                                      |
| `ecobee_forecast_relative_humidity`            | `thermostat_id`, `thermostat_name`, `station` | Yes     | Yes            | Outdoor relative humidity, percent.                                |
| `ecobee_forecast_pressure_mb`                  | `thermostat_id`, `thermostat_name`, `station` | Yes     | Yes            | Sea-level pressure, millibars.                                     |
| `ecobee_forecast_dewpoint`                     | `thermostat_id`, `thermostat_name`, `station` | Yes     | Yes            | Outdoor dewpoint, degrees.                                         |
| `ecobee_forecast_wind_speed_mph`               | `thermostat_id`, `thermostat_name`, `station` | Yes     | Yes            | Wind speed, mph.                                                   |
| `ecobee_forecast_wind_gust_mph`                | `thermostat_id`, `thermostat_name`, `station` | Yes     | Partial        | Wind gust, mph. Home Assistant: when HA exposes `wind_gust_speed`. |
| `ecobee_forecast_wind_bearing_degrees`         | `thermostat_id`, `thermostat_name`, `station` | Yes     | Yes            | Wind bearing, degrees.                                             |
| `ecobee_forecast_visibility`                   | `thermostat_id`, `thermostat_name`, `station` | Yes     | Yes            | Visibility, meters (`* 0.000621371` for miles in Grafana).         |
| `ecobee_forecast_probability_of_precipitation` | `thermostat_id`, `thermostat_name`, `station` | Yes     | Yes            | Precipitation probability, percent.                                |
| `ecobee_forecast_temp_high`                    | `thermostat_id`, `thermostat_name`, `station` | Yes     | Yes            | Forecast daily high, degrees.                                      |
| `ecobee_forecast_temp_low`                     | `thermostat_id`, `thermostat_name`, `station` | Yes     | Yes            | Forecast daily low, degrees.                                       |


### Equipment runtime *(extension)*


| Metric                     | Labels                                          | Beehive | Home Assistant | Description                                                                                                                                                  |
| -------------------------- | ----------------------------------------------- | ------- | -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `ecobee_equipment_running` | `thermostat_id`, `thermostat_name`, `equipment` | Yes     | Partial        | Replaces legacy `ecobee_last_interval_energized_state`. Beehive: full `equipmentStatus` list. Home Assistant: ecobee cloud `equipment_running` CSV or `hvac_action` map. |


Known equipment identifiers: `heatPump`, `heatPump2`, `heatPump3`, `compCool1`, `compCool2`, `auxHeat1`, `auxHeat2`, `auxHeat3`, `fan`, `humidifier`, `dehumidifier`, `ventilator`, `economizer`, `compHotWater`, `auxHotWater`.

### Hold / events *(extension)*


| Metric                  | Labels                                           | Beehive | Home Assistant | Description                  |
| ----------------------- | ------------------------------------------------ | ------- | -------------- | ---------------------------- |
| `ecobee_hold_heat_temp` | `thermostat_id`, `thermostat_name`               | Yes     | No             | Heat hold setpoint, degrees. |
| `ecobee_hold_cool_temp` | `thermostat_id`, `thermostat_name`               | Yes     | No             | Cool hold setpoint, degrees. |
| `ecobee_event_type`     | `thermostat_id`, `thermostat_name`, `event_type` | Yes     | No             | Event type as label.         |


### Extended runtime *(extension)*

Per-equipment runtime from the last three 5-minute intervals (interval `0` = oldest, `2` = newest). Values are seconds within each bucket (0–300).


| Metric                              | Labels                                                      | Beehive | Home Assistant | Description                                                                                                      |
| ----------------------------------- | ----------------------------------------------------------- | ------- | -------------- | ---------------------------------------------------------------------------------------------------------------- |
| `ecobee_equipment_runtime_seconds`  | `thermostat_id`, `thermostat_name`, `equipment`, `interval` | Yes     | No             | Replaces legacy `ecobee_last_interval_runtime`. Equipment runtime per 5-minute bucket (`interval="2"` = newest). |
| `ecobee_demand_management_offset`   | `thermostat_id`, `thermostat_name`, `interval`              | Yes     | No             | Demand-management temperature offset, degrees.                                                                   |
| `ecobee_current_electricity_bill`   | `thermostat_id`, `thermostat_name`                          | Yes     | No             | Current utility meter bill (if paired).                                                                          |
| `ecobee_projected_electricity_bill` | `thermostat_id`, `thermostat_name`                          | Yes     | No             | Projected utility meter bill (if paired).                                                                        |


Equipment names in extended runtime: `heatPump1`, `heatPump2`, `auxHeat1`, `auxHeat2`, `auxHeat3`, `cool1`, `cool2`, `fan`, `humidifier`, `dehumidifier`, `ventilator`, `economizer`.

### Alerts *(extension)*


| Metric                | Labels                                                           | Beehive | Home Assistant | Description                  |
| --------------------- | ---------------------------------------------------------------- | ------- | -------------- | ---------------------------- |
| `ecobee_alert_active` | `thermostat_id`, `thermostat_name`, `alert_type`, `alert_number` | Yes     | No             | One series per active alert. |


---

## Architecture

```
                +---------------------+
                |  axum HTTP server   |  /metrics, /healthz, /readiness, /liveness
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
                                          impl 1: BeehiveProvider        (cloud)
                                          impl 2: HomeAssistantProvider  (HA REST)
                                          impl 3: FakeProvider           (demo/test)
```

`ThermostatProvider` is the seam: anything that can return a
`Vec<Thermostat>` can drive the exporter. All backends share the
same metrics and HTTP layers.

### Health probes

By default the collector tracks whether the last upstream poll succeeded:

| Endpoint | Behavior |
|----------|----------|
| `/healthz` | Upstream readiness probe — live-probes connectivity; `503` when the target is unreachable |
| `/readiness` | `200 ok` after a successful fetch; `503` when the last fetch failed or none has succeeded yet (cached mode, default) |
| `/liveness` | Always `200 ok` — process is running |
| `/metrics` | `503` when upstream is unhealthy (stale data is not served); `200` with Prometheus text otherwise |

Set `health_probe_mode = "live"` (or `ECOBEE_HEALTH_PROBE_MODE=live`) to fetch upstream on each `/readiness` request instead of using the cached collector status. `/healthz` always performs a live connectivity check. Tune the probe timeout with `health_check_timeout` (default `10s`).

On startup (non-demo mode), the exporter retries the first fetch up to three times and exits if upstream stays unreachable. Shutdown on `SIGINT`/`SIGTERM` stops the HTTP server gracefully, then the collector loop.

## Roadmap

Short-term:

- ~~Implement Auth0 + PKCE login + refresh.~~ Done.
- ~~Implement the data-API call against `api.ecobee.com/1/thermostat`.~~ Done.
- ~~Add a fixture-based parsing test for the response shape.~~ Done.

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


After the first push to `main`:

```sh
docker pull ghcr.io/OWNER/ecobee-exporter:main
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