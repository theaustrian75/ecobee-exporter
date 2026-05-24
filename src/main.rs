use std::{path::PathBuf, sync::Arc};

use anyhow::Context;
use clap::Parser;
use tokio::net::TcpListener;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use ecobee_exporter::{
    beehive::BeehiveProvider,
    collector::Collector,
    config::{Config, ProviderKind},
    homekit::HomeKitProvider,
    metrics::Metrics,
    provider::{FakeProvider, ThermostatProvider},
    server::{AppState, router},
};

#[derive(Debug, Parser)]
#[command(
    name = "ecobee-exporter",
    version,
    about = "Prometheus exporter for ecobee thermostats (Beehive or HomeKit backend)"
)]
struct Cli {
    /// Path to a TOML config file. Overrides ECOBEE_EXPORTER_CONFIG.
    #[arg(long, short = 'c', env = "ECOBEE_EXPORTER_CONFIG")]
    config: Option<PathBuf>,

    /// Force demo mode regardless of config. Equivalent to `demo = true`.
    #[arg(long)]
    demo: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let cli = Cli::parse();
    let mut cfg = Config::load(cli.config.as_deref()).context("loading config")?;
    if cli.demo {
        cfg.demo = true;
    }

    tracing::info!(
        listen = %cfg.listen_addr,
        poll_interval = ?cfg.poll_interval,
        demo = cfg.demo,
        provider = ?cfg.provider,
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
                tracing::info!(
                    pairing_file = %cfg.homekit.pairing_file.display(),
                    "using native HomeKit provider"
                );
                Arc::new(
                    HomeKitProvider::new(&cfg.homekit).context("initializing HomeKit provider")?,
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
    let listener = TcpListener::bind(cfg.listen_addr)
        .await
        .with_context(|| format!("binding {}", cfg.listen_addr))?;
    let bound = listener.local_addr().unwrap_or(cfg.listen_addr);
    tracing::info!(addr = %bound, "metrics server listening");

    tokio::select! {
        res = axum::serve(listener, app).into_future() => {
            res.context("HTTP server crashed")?;
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
