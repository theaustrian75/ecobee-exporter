//! One-time HomeKit pairing bootstrap for `provider = "homekit"`.

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use housekey::Controller;

#[derive(Debug, Parser)]
#[command(
    name = "ecobee-homekit-pair",
    version,
    about = "Discover and pair an ecobee thermostat over HomeKit"
)]
struct Cli {
    /// Alias stored in the pairing file (default: ecobee).
    #[arg(long, default_value = "ecobee")]
    alias: String,

    /// 8-digit HomeKit setup code from ecobee Settings → HomeKit.
    #[arg(long)]
    code: String,

    /// Accessory id from `discover` (required unless --discover-only).
    #[arg(long)]
    device_id: Option<String>,

    /// Pairing store path. Must match the exporter's `homekit.pairing_file`.
    #[arg(
        long,
        env = "ECOBEE_HOMEKIT_PAIRING_FILE",
        default_value = "./homekit-pairings.json"
    )]
    pairing_file: PathBuf,

    /// Scan the LAN for `_hap._tcp` accessories and exit.
    #[arg(long)]
    discover_only: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    let mut controller = Controller::new(cli.pairing_file.clone());
    controller.load().context("loading existing pairings")?;

    let discovered = controller.discover().await.context("mDNS discover")?;

    if cli.discover_only {
        if discovered.is_empty() {
            println!("No HomeKit accessories found on the LAN.");
        }
        for item in discovered {
            println!(
                "{}  id={}  {}:{}  category={:?}  model={}",
                item.name, item.id, item.addr, item.port, item.category, item.model
            );
        }
        return Ok(());
    }

    let device_id = cli
        .device_id
        .context("--device-id is required (run with --discover-only first)")?;
    let accessory = discovered
        .into_iter()
        .find(|d| d.id.eq_ignore_ascii_case(&device_id))
        .with_context(|| format!("accessory {device_id} not found on the LAN"))?;

    controller
        .pair(&accessory, &cli.alias, &cli.code)
        .await
        .context("pairing failed")?;

    println!(
        "Paired {} as {:?}; keys saved to {}",
        accessory.name,
        cli.alias,
        cli.pairing_file.display()
    );
    Ok(())
}
