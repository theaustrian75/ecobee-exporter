use std::{path::PathBuf, sync::Arc};

use anyhow::Context;
use clap::{ArgAction, Parser};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use ecobee_exporter::{
    beehive::BeehiveProvider,
    collector::Collector,
    config::{CliOverrides, Config, ProviderKind, parse_header_pair, parse_poll_interval},
    homeassistant::HomeAssistantProvider,
    homekit::HomeKitProvider,
    metrics::Metrics,
    provider::{FakeProvider, ThermostatProvider},
    server::{AppState, router, run as serve},
};

#[derive(Debug, Parser)]
#[command(
    name = "ecobee-exporter",
    version,
    about = "Prometheus exporter for ecobee thermostats (Beehive or HomeKit backend)"
)]
struct Cli {
    /// Path to a TOML config file.
    #[arg(long, short = 'c', env = "ECOBEE_EXPORTER_CONFIG")]
    config: Option<PathBuf>,

    /// Force demo mode regardless of config.
    #[arg(long, env = "ECOBEE_DEMO", action = ArgAction::SetTrue)]
    demo: bool,

    /// Data source: `beehive`, `homekit`, or `homeassistant`.
    #[arg(long, value_parser = clap::value_parser!(ProviderKind), env = "ECOBEE_PROVIDER")]
    provider: Option<ProviderKind>,

    /// Where the `/metrics` HTTP server listens.
    #[arg(long, env = "ECOBEE_LISTEN_ADDR")]
    listen_addr: Option<std::net::SocketAddr>,

    /// How often the collector polls upstream (e.g. `3m`, `90s`).
    #[arg(long, env = "ECOBEE_POLL_INTERVAL", value_parser = parse_poll_interval)]
    poll_interval: Option<std::time::Duration>,

    /// Where to persist refresh tokens / session state between restarts.
    #[arg(long, env = "ECOBEE_STATE_FILE")]
    state_file: Option<PathBuf>,

    #[command(flatten)]
    beehive: BeehiveCli,

    #[command(flatten)]
    homekit: HomeKitCli,

    #[command(flatten)]
    homeassistant: HomeAssistantCli,

    #[command(flatten)]
    tls: TlsCli,
}

#[derive(Debug, Parser)]
struct TlsCli {
    /// PEM certificate chain for the `/metrics` HTTPS server.
    #[arg(long = "tls-cert-file", env = "ECOBEE_TLS__CERT_FILE")]
    cert_file: Option<PathBuf>,

    /// PEM private key for the `/metrics` HTTPS server.
    #[arg(long = "tls-key-file", env = "ECOBEE_TLS__KEY_FILE")]
    key_file: Option<PathBuf>,
}

#[derive(Debug, Parser)]
struct BeehiveCli {
    /// Beehive data API base URL.
    #[arg(long = "beehive-endpoint", env = "ECOBEE_BEEHIVE__ENDPOINT")]
    endpoint: Option<String>,

    /// Outgoing `User-Agent` header for Beehive requests.
    #[arg(long = "beehive-user-agent", env = "ECOBEE_BEEHIVE__USER_AGENT")]
    user_agent: Option<String>,

    /// Pre-minted Beehive refresh token (normally stored in `state_file`).
    #[arg(long = "beehive-refresh-token", env = "ECOBEE_BEEHIVE__REFRESH_TOKEN")]
    refresh_token: Option<String>,

    /// Extra Beehive request header (`KEY=VALUE`). Repeat for multiple headers.
    #[arg(long = "beehive-header", value_name = "KEY=VALUE", action = ArgAction::Append)]
    header: Vec<String>,
}

#[derive(Debug, Parser)]
struct HomeKitCli {
    /// JSON file storing HomeKit pairing keys.
    #[arg(long = "homekit-pairing-file", env = "ECOBEE_HOMEKIT__PAIRING_FILE")]
    pairing_file: Option<PathBuf>,
}

#[derive(Debug, Parser)]
struct HomeAssistantCli {
    /// Home Assistant base URL, e.g. `http://homeassistant.local:8123`.
    #[arg(long = "homeassistant-url", env = "ECOBEE_HOMEASSISTANT__URL")]
    url: Option<String>,

    /// Home Assistant long-lived access token.
    #[arg(long = "homeassistant-token", env = "ECOBEE_HOMEASSISTANT__TOKEN")]
    token: Option<String>,

    /// Climate entity ID to export (repeat for multiple thermostats).
    #[arg(long = "homeassistant-climate-entity", action = ArgAction::Append)]
    climate_entities: Vec<String>,

    /// Weather entity ID to attach (repeat for priority order). When unset, auto-links
    /// `weather.ecobee`, per-thermostat stems, or the only `weather.*` entity.
    #[arg(long = "homeassistant-weather-entity", action = ArgAction::Append)]
    weather_entities: Vec<String>,
}

impl Cli {
    fn overrides(&self) -> anyhow::Result<CliOverrides> {
        let beehive_headers = self
            .beehive
            .header
            .iter()
            .map(|pair| parse_header_pair(pair).map_err(anyhow::Error::msg))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(CliOverrides {
            demo: self.demo,
            provider: self.provider,
            listen_addr: self.listen_addr,
            poll_interval: self.poll_interval,
            state_file: self.state_file.clone(),
            beehive_endpoint: self.beehive.endpoint.clone(),
            beehive_user_agent: self.beehive.user_agent.clone(),
            beehive_refresh_token: self.beehive.refresh_token.clone(),
            beehive_headers,
            homekit_pairing_file: self.homekit.pairing_file.clone(),
            homeassistant_url: self.homeassistant.url.clone(),
            homeassistant_token: self.homeassistant.token.clone(),
            homeassistant_climate_entities: self.homeassistant.climate_entities.clone(),
            homeassistant_weather_entities: self.homeassistant.weather_entities.clone(),
            tls_cert_file: self.tls.cert_file.clone(),
            tls_key_file: self.tls.key_file.clone(),
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let cli = Cli::parse();
    let mut cfg = Config::load(cli.config.as_deref()).context("loading config")?;
    cfg.apply_cli_overrides(&cli.overrides()?);
    cfg.validate_tls()
        .map_err(anyhow::Error::msg)
        .context("invalid TLS configuration")?;

    tracing::info!(
        listen = %cfg.listen_addr,
        poll_interval = ?cfg.poll_interval,
        demo = cfg.demo,
        provider = ?cfg.provider,
        tls = cfg.tls.is_some(),
        "starting ecobee-exporter"
    );

    let metrics = Arc::new(Metrics::new().context("setting up Prometheus registry")?);

    let provider: Arc<dyn ThermostatProvider> = if cfg.demo {
        tracing::warn!("demo mode: serving canned data, no upstream calls will be made");
        Arc::new(FakeProvider::demo())
    } else {
        match cfg.provider {
            ProviderKind::Beehive => Arc::new(
                BeehiveProvider::new(&cfg.beehive, cfg.state_file.clone())
                    .context("initializing Beehive provider")?,
            ),
            ProviderKind::Homekit => {
                tracing::warn!(
                    "homekit provider is untested; prefer provider=homeassistant if HA already polls your ecobees"
                );
                tracing::info!(
                    pairing_file = %cfg.homekit.pairing_file.display(),
                    "using native HomeKit provider"
                );
                Arc::new(
                    HomeKitProvider::new(&cfg.homekit).context("initializing HomeKit provider")?,
                )
            }
            ProviderKind::Homeassistant => {
                tracing::info!(
                    url = %cfg.homeassistant.url,
                    climate_entities = cfg.homeassistant.climate_entities.len(),
                    "using Home Assistant REST provider"
                );
                Arc::new(
                    HomeAssistantProvider::new(&cfg.homeassistant)
                        .context("initializing Home Assistant provider")?,
                )
            }
        }
    };

    let collector = Collector::new(
        Arc::clone(&provider),
        Arc::clone(&metrics),
        cfg.poll_interval,
    );
    let collector_task = tokio::spawn(collector.run());

    let app = router(AppState { metrics });

    tokio::select! {
        res = serve(app, cfg.listen_addr, cfg.tls.as_ref()) => {
            res.context("metrics server crashed")?;
        }
        _ = collector_task => {
            anyhow::bail!("collector task exited unexpectedly");
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("received Ctrl-C, shutting down");
        }
    }

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("ecobee_exporter=info,info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .init();
}
