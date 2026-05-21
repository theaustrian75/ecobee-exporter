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

The scaffolding is in place and the metrics + HTTP layers work end-to-end
in `--demo` mode. The Beehive client itself is a stub: as of mid-2026
there is no public reverse-engineering of Beehive's endpoint URL,
authentication flow, or GraphQL schema. Filling those in requires
capturing your own mobile-app traffic; see [`CAPTURE.md`](./CAPTURE.md)
for the procedure.

What works today:

  - HTTP server on `/metrics` and `/healthz`.
  - Prometheus metric layer with the full
    [billykwooten/ecobee-exporter][bk] metric set (see *Metrics* below).
  - Polling loop with a configurable interval and a 60-second floor.
  - Configuration via TOML file, environment variables, and CLI flags.
  - `--demo` mode that serves canned data so dashboards and scrape
    configs can be developed before the Beehive client is wired up.

What's stubbed and waiting for capture:

  - `src/beehive/auth.rs` — login + refresh flow.
  - `src/beehive/queries.rs` — the GraphQL operation that lists
    thermostats with their sensors, runtime, and settings.

Both are tagged `TODO(capture):` with notes on what to look for.

## Quick start

### Demo mode (no credentials)

```sh
cargo run -- --demo
# in another shell:
curl http://localhost:9098/metrics
```

You should see a populated set of `ecobee_*` series against a synthetic
two-sensor thermostat.

### Real mode (after you've completed the capture work)

```sh
cp ecobee-exporter.example.toml ecobee-exporter.toml
$EDITOR ecobee-exporter.toml   # fill in Beehive endpoint + refresh token
cargo run --release
```

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
| `beehive.endpoint`        | `null`                           | GraphQL URL from your capture.                                        |
| `beehive.user_agent`      | `ecobee-exporter/0.1.0`          | Override to mimic the official mobile app if upstream rejects yours.  |
| `beehive.extra_headers`   | `[]`                             | List of `[key, value]` pairs to add to every request.                 |
| `beehive.email`           | `null`                           | Account email, for the initial login flow.                            |
| `beehive.password`        | `null`                           | Account password, for the initial login flow.                         |
| `beehive.refresh_token`   | `null`                           | Pre-minted refresh token, if you'd rather inject one from capture.    |

Put secrets in the config file with `chmod 600`, not env vars. Env vars
leak into systemd journals and `ps`.

## Metrics

Names and labels match billykwooten/ecobee-exporter for dashboard
compatibility.

| Metric                            | Labels                                                              | Description                                                                            |
|-----------------------------------|---------------------------------------------------------------------|----------------------------------------------------------------------------------------|
| `ecobee_fetch_time`               | —                                                                   | Seconds the last upstream fetch took.                                                  |
| `ecobee_fetch_failures_total`     | —                                                                   | Counter of failed fetches since start. *(extension; not in billykwooten)*              |
| `ecobee_actual_temperature`       | `thermostat_id`, `thermostat_name`                                  | Thermostat-averaged current temperature, degrees.                                      |
| `ecobee_target_temperature_min`   | `thermostat_id`, `thermostat_name`                                  | Heating setpoint, degrees.                                                             |
| `ecobee_target_temperature_max`   | `thermostat_id`, `thermostat_name`                                  | Cooling setpoint, degrees.                                                             |
| `ecobee_currenthvacmode`          | `thermostat_id`, `thermostat_name`, `current_hvac_mode`             | Always `0`; the mode is encoded as a label, matching billykwooten.                     |
| `ecobee_temperature`              | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Per-sensor temperature, degrees.                                              |
| `ecobee_humidity`                 | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Per-sensor humidity, percent.                                                 |
| `ecobee_occupancy`                | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Per-sensor occupancy (0 or 1).                                                |
| `ecobee_in_use`                   | `thermostat_id`, `thermostat_name`, `sensor_id`, `sensor_name`, `sensor_type` | Whether the sensor is being included in thermostat averages (0 or 1).         |

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

Short-term (unblocks "real" usage):

  - Capture and implement Beehive auth (login + refresh).
  - Capture and implement the `ListThermostats` query plus its mapping
    into `model::Thermostat`.
  - Add a saved fixture in `samples/example-list-thermostats.json` and
    a parsing test in `tests/parse_sample.rs`.

Medium-term (parity-plus):

  - `ecobee_equipment_running{equipment}` if the Beehive response
    includes the live equipment-status block.
  - Outdoor temperature from the weather block.
  - Air-quality metrics on Premium models.

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
