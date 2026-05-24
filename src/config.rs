//! Configuration loading.
//!
//! Resolution order, lowest-to-highest precedence:
//!   1. built-in defaults
//!   2. `ecobee-exporter.toml` in the current directory (if present)
//!   3. file at `$ECOBEE_EXPORTER_CONFIG` (if set)
//!   4. environment variables prefixed `ECOBEE_` (e.g. `ECOBEE_LISTEN_ADDR`)
//!
//! Sensitive values (refresh token, access token, MFA seeds) should live in
//! the config file with `chmod 600`, not env vars, so they don't leak into
//! process listings or systemd journal output.

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use figment::{
    Figment,
    providers::{Env, Format, Toml},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    #[default]
    Beehive,
    Homekit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Where the `/metrics` HTTP server listens.
    #[serde(default = "Config::default_listen_addr")]
    pub listen_addr: SocketAddr,

    /// How often the collector polls upstream.
    ///
    /// Ecobee thermostats only report new data every ~3 minutes, so polling
    /// more frequently than that just burns API quota for no benefit. The
    /// minimum enforced at startup is 60 seconds.
    #[serde(default = "Config::default_poll_interval", with = "humantime_serde")]
    pub poll_interval: Duration,

    /// Where to persist refresh tokens / session state between restarts.
    #[serde(default = "Config::default_state_file")]
    pub state_file: PathBuf,

    /// Skip the real upstream and serve demo data. Useful for verifying the
    /// metrics layer end-to-end without credentials.
    #[serde(default)]
    pub demo: bool,

    /// Data source: `beehive` (cloud Auth0 API, default) or `homekit` (local
    /// HomeKit HAP). Ignored when `demo = true`.
    #[serde(default)]
    pub provider: ProviderKind,

    /// Credentials for the Beehive API. Used when `provider = "beehive"`.
    #[serde(default)]
    pub beehive: BeehiveConfig,

    /// Settings for native HomeKit access. Used when `provider = "homekit"`.
    #[serde(default)]
    pub homekit: HomeKitConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeehiveConfig {
    /// Data API base URL. Defaults to `https://api.ecobee.com/1` when unset.
    #[serde(default)]
    pub endpoint: Option<String>,

    /// `User-Agent` to send. Some upstream APIs reject vanilla `reqwest/x.y.z`.
    #[serde(default)]
    pub user_agent: Option<String>,

    /// Extra headers the mobile app sends that the server checks for
    /// (e.g. `x-ecobee-app-version`, region headers).
    #[serde(default)]
    pub extra_headers: Vec<(String, String)>,

    /// Pre-minted refresh token. Normally this lives in the state file
    /// (written by the `ecobee-login` helper). Setting it here forces a
    /// re-seed on next startup and is mainly useful for testing or for
    /// transplanting a token between machines.
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeKitConfig {
    /// JSON file storing HomeKit pairing keys (see `ecobee-homekit-pair`).
    #[serde(default = "HomeKitConfig::default_pairing_file")]
    pub pairing_file: PathBuf,
}

impl HomeKitConfig {
    fn default_pairing_file() -> PathBuf {
        PathBuf::from("./homekit-pairings.json")
    }
}

impl Default for HomeKitConfig {
    fn default() -> Self {
        Self {
            pairing_file: Self::default_pairing_file(),
        }
    }
}

impl Config {
    fn default_listen_addr() -> SocketAddr {
        "0.0.0.0:9098"
            .parse()
            .expect("default listen addr is valid")
    }

    fn default_poll_interval() -> Duration {
        Duration::from_mins(3)
    }

    fn default_state_file() -> PathBuf {
        PathBuf::from("ecobee-exporter.state.json")
    }

    /// Build a `Config` from the layered sources described in the module docs.
    pub fn load(config_path: Option<&std::path::Path>) -> Result<Self, Box<figment::Error>> {
        let mut fig = Figment::new().merge(Toml::file("ecobee-exporter.toml"));
        if let Some(p) = config_path {
            fig = fig.merge(Toml::file(p));
        } else if let Ok(p) = std::env::var("ECOBEE_EXPORTER_CONFIG") {
            fig = fig.merge(Toml::file(p));
        }
        fig = fig.merge(Env::prefixed("ECOBEE_").split("__"));
        let mut cfg: Config = fig.extract().map_err(Box::new)?;

        let floor = Duration::from_mins(1);
        if cfg.poll_interval < floor {
            tracing::warn!(
                requested = ?cfg.poll_interval,
                "poll_interval below 60s clamped to 60s; ecobee only refreshes data every ~3 minutes"
            );
            cfg.poll_interval = floor;
        }

        Ok(cfg)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen_addr: Self::default_listen_addr(),
            poll_interval: Self::default_poll_interval(),
            state_file: Self::default_state_file(),
            demo: false,
            provider: ProviderKind::Beehive,
            beehive: BeehiveConfig::default(),
            homekit: HomeKitConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use figment::{Figment, providers::Toml};

    #[test]
    fn homekit_config_defaults() {
        let cfg = HomeKitConfig::default();
        assert_eq!(cfg.pairing_file, PathBuf::from("./homekit-pairings.json"));
    }

    #[test]
    fn deserializes_homekit_provider_and_pairing_file() {
        let cfg: Config = Figment::new()
            .merge(Toml::string(
                r#"
provider = "homekit"
[homekit]
pairing_file = "/var/lib/ecobee/pairings.json"
"#,
            ))
            .extract()
            .expect("figment extract");

        assert_eq!(cfg.provider, ProviderKind::Homekit);
        assert_eq!(
            cfg.homekit.pairing_file,
            PathBuf::from("/var/lib/ecobee/pairings.json")
        );
    }

    #[test]
    fn clamps_poll_interval_below_one_minute() {
        let dir = tempdir();
        let path = dir.join("cfg.toml");
        std::fs::write(&path, r#"poll_interval = "15s""#).unwrap();
        let cfg = Config::load(Some(&path)).expect("load config");
        assert_eq!(cfg.poll_interval, Duration::from_mins(1));
    }

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "ecobee-config-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0u128, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
