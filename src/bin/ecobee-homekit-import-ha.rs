//! Import HomeKit pairing keys from Home Assistant into the exporter pairing store.

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use ecobee_exporter::homekit::ha_import::{
    filter_ecobee_entries, parse_ha_config_entries, select_entries, to_paired_device,
};
use housekey::Controller;

#[derive(Debug, Parser)]
#[command(
    name = "ecobee-homekit-import-ha",
    version,
    about = "Import Home Assistant HomeKit Controller pairings for ecobee thermostats"
)]
struct Cli {
    /// Path to Home Assistant `.storage/core.config_entries`.
    #[arg(long)]
    ha_config: PathBuf,

    /// Pairing store path. Must match the exporter's `homekit.pairing_file`.
    #[arg(
        long,
        env = "ECOBEE_HOMEKIT_PAIRING_FILE",
        default_value = "./homekit-pairings.json"
    )]
    pairing_file: PathBuf,

    /// Alias stored in the pairing file. Required when multiple entries match.
    #[arg(long)]
    alias: Option<String>,

    /// Import a specific Home Assistant config entry id.
    #[arg(long)]
    entry_id: Option<String>,

    /// Import all HomeKit Controller IP entries, not just ecobee-looking titles.
    #[arg(long)]
    all: bool,

    /// Print planned imports without writing the pairing file.
    #[arg(long)]
    dry_run: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    eprintln!(
        "warning: this imports Home Assistant's HomeKit controller identity. \
         Running HA and the exporter against the same thermostat concurrently may \
         cause connection conflicts or stale reads."
    );

    let content = std::fs::read_to_string(&cli.ha_config)
        .with_context(|| format!("reading {}", cli.ha_config.display()))?;

    let mut entries = parse_ha_config_entries(&content)
        .with_context(|| format!("parsing {}", cli.ha_config.display()))?;

    if !cli.all {
        entries = filter_ecobee_entries(&entries);
        if entries.is_empty() {
            anyhow::bail!(
                "no ecobee-looking HomeKit Controller entries found \
                 (title must contain \"ecobee\"); re-run with --all to import every IP pairing"
            );
        }
    }

    let selected = select_entries(&entries, cli.entry_id.as_deref(), cli.alias.as_deref())
        .context("selecting entries to import")?;

    let mut controller = Controller::new(cli.pairing_file.clone());
    if !cli.dry_run {
        controller.load().context("loading existing pairing file")?;
    }

    for (entry, alias) in &selected {
        let device = to_paired_device(entry, alias);
        println!(
            "import {} ({}) as {:?}{}",
            entry.title,
            entry.data.accessory_id,
            alias,
            if cli.dry_run { " [dry-run]" } else { "" }
        );
        if !cli.dry_run {
            controller.insert_paired(device);
        }
    }

    if cli.dry_run {
        println!(
            "Dry run complete; {} pairing(s) would be written to {}",
            selected.len(),
            cli.pairing_file.display()
        );
        return Ok(());
    }

    controller.save().context("writing pairing file")?;

    println!(
        "Imported {} pairing(s) into {}",
        selected.len(),
        cli.pairing_file.display()
    );
    Ok(())
}
