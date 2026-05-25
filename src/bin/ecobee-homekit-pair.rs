//! One-time HomeKit pairing bootstrap for `provider = "homekit"`.

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Parser;
use housekey::{Controller, DISCOVER_TIMEOUT_SECS, DiscoveredAccessory};

#[derive(Debug, Parser)]
#[command(
    name = "ecobee-homekit-pair",
    version,
    about = "Discover and pair an ecobee thermostat over HomeKit"
)]
#[allow(clippy::struct_excessive_bools, reason = "clap CLI flags")]
struct Cli {
    /// Alias stored in the pairing file (default: ecobee).
    #[arg(long, default_value = "ecobee")]
    alias: String,

    /// 8-digit HomeKit setup code from ecobee Settings → HomeKit.
    #[arg(long)]
    code: Option<String>,

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

    /// Verify existing pairings via mDNS + HAP handshake (no new pairing).
    #[arg(long)]
    verify: bool,

    /// List every HomeKit accessory, not just ecobee thermostats.
    #[arg(long)]
    all: bool,

    /// Enable info-level progress on stderr and from housekey.
    #[arg(long, short = 'v')]
    verbose: bool,

    /// Enable debug-level logging from housekey (HTTP wire details).
    #[arg(long, short = 'd')]
    debug: bool,
}

fn init_logging(verbose: bool, debug: bool) {
    let default_filter = if debug {
        "ecobee_homekit_pair=debug,housekey=debug"
    } else if verbose {
        "ecobee_homekit_pair=info,housekey=info"
    } else {
        "warn"
    };
    let timer = ecobee_exporter::tracing_init::local_timer()
        .expect("local timezone unavailable; set TZ and install tzdata (see README)");
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_filter)),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .with_timer(timer)
        .init();
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose, cli.debug);

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?
        .block_on(run(cli))
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    let mut controller = Controller::new(cli.pairing_file.clone());
    controller.load().context("loading existing pairings")?;

    if cli.verify {
        return run_verify(&mut controller, &cli.pairing_file).await;
    }

    if cli.verbose {
        eprintln!(
            "Browsing _hap._tcp.local for {DISCOVER_TIMEOUT_SECS}s (use RUST_LOG=… to override logging)…"
        );
    }

    let discovered = controller.discover().await.context("mDNS discover")?;

    if cli.verbose {
        let ecobee = discovered.iter().filter(|d| d.is_ecobee()).count();
        eprintln!(
            "Found {} HomeKit accessory(ies), {ecobee} ecobee thermostat(s)",
            discovered.len()
        );
    }

    if cli.discover_only {
        print_discovered(&discovered, cli.all);
        return Ok(());
    }

    let device_id = cli
        .device_id
        .context("--device-id is required (run with --discover-only first)")?;
    let code = cli
        .code
        .context("--code is required when pairing (ecobee Settings → HomeKit)")?;
    let accessory = discovered
        .into_iter()
        .find(|d| d.id.eq_ignore_ascii_case(&device_id))
        .with_context(|| format!("accessory {device_id} not found on the LAN"))?;

    if !accessory.is_ecobee() {
        anyhow::bail!(
            "{} (model={}) is not an ecobee thermostat",
            accessory.display_name(),
            accessory.model
        );
    }

    controller
        .pair(&accessory, &cli.alias, &code)
        .await
        .context("pairing failed")?;

    println!(
        "Paired {} as {:?}; keys saved to {}",
        accessory.display_name(),
        cli.alias,
        cli.pairing_file.display()
    );
    Ok(())
}

async fn run_verify(controller: &mut Controller, pairing_file: &Path) -> anyhow::Result<()> {
    let devices: Vec<_> = controller
        .paired_devices()
        .map(|d| {
            (
                d.alias.clone(),
                d.accessory_id.clone(),
                d.host.clone(),
                d.port,
                d.controller_pairing_id.clone(),
            )
        })
        .collect();
    if devices.is_empty() {
        anyhow::bail!("no pairings in {}", pairing_file.display());
    }
    eprintln!(
        "Verifying {} pairing(s) from {}:",
        devices.len(),
        pairing_file.display()
    );
    for (alias, accessory_id, host, port, controller_id) in &devices {
        let endpoint = match (host, port) {
            (Some(h), Some(p)) => format!("{h}:{p}"),
            (Some(h), None) => format!("{h}:51826 (default port)"),
            _ => "<unresolved; will use mDNS>".to_string(),
        };
        eprintln!(
            "  alias={alias} accessory_id={accessory_id} endpoint={endpoint} controller_id={controller_id}"
        );
    }
    eprintln!(
        "Each verification: mDNS lookup → TCP connect → pair-verify M1/M2/M3/M4 → encrypted GET /accessories."
    );

    match tokio::time::timeout(
        std::time::Duration::from_mins(2),
        controller.read_all_accessories(),
    )
    .await
    {
        Ok(Ok(results)) => {
            for (alias, accessories) in results {
                eprintln!("  {alias}: OK ({} accessory tree(s))", accessories.len());
            }
            eprintln!("HomeKit reads succeeded.");
        }
        Ok(Err(e)) => {
            eprintln!("FAIL: {e}");
            eprintln!(
                "If errors mention pair-verify timeouts, Home Assistant may still be \
                 polling these thermostats with the same HomeKit keys. Remove the ecobee \
                 from HA's HomeKit Device integration, wait ~60s, and retry. Alternatively \
                 pair natively with this tool (not HA import) after resetting HomeKit on the ecobee."
            );
        }
        Err(_) => {
            eprintln!("FAIL: timed out after 120s");
            eprintln!(
                "Likely blocked on HAP pair-verify while Home Assistant holds an active session."
            );
        }
    }
    Ok(())
}

fn print_discovered(discovered: &[DiscoveredAccessory], show_all: bool) {
    let mut items: Vec<&DiscoveredAccessory> = discovered
        .iter()
        .filter(|item| show_all || item.is_ecobee())
        .collect();

    if items.is_empty() {
        if show_all {
            println!("No HomeKit accessories found on the LAN.");
        } else {
            println!("No ecobee thermostats found on the LAN.");
        }
        return;
    }

    items.sort_by(|a, b| a.display_name().cmp(b.display_name()));

    let headers = ["NAME", "ID", "ADDRESS", "CATEGORY", "MODEL"];
    let rows: Vec<[String; 5]> = items
        .iter()
        .map(|item| {
            [
                item.display_name().to_string(),
                item.id.clone(),
                item.socket_addr(),
                item.category.to_string(),
                item.model.clone(),
            ]
        })
        .collect();

    let mut widths = headers.map(str::len);
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row.iter()) {
            *width = (*width).max(cell.len());
        }
    }

    print_discover_row(&headers, &widths);
    for row in &rows {
        print_discover_row(row, &widths);
    }
}

fn print_discover_row<S: AsRef<str>>(cells: &[S; 5], widths: &[usize; 5]) {
    println!(
        "{:<name_w$}  {:>id_w$}  {:>addr_w$}  {:<cat_w$}  {:<model_w$}",
        cells[0].as_ref(),
        cells[1].as_ref(),
        cells[2].as_ref(),
        cells[3].as_ref(),
        cells[4].as_ref(),
        name_w = widths[0],
        id_w = widths[1],
        addr_w = widths[2],
        cat_w = widths[3],
        model_w = widths[4],
    );
}
