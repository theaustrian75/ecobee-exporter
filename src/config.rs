//! Configuration loading.
//!
//! Resolution order, lowest-to-highest precedence:
//!   1. built-in defaults
//!   2. `ecobee-exporter.toml` in the current directory (if present)
//!   3. file at `$ECOBEE_EXPORTER_CONFIG` (if set)
//!   4. environment variables prefixed `ECOBEE_` (e.g. `ECOBEE_LISTEN_ADDR`)
//!   5. CLI flags on `ecobee-exporter` (each mirrors its `ECOBEE_*` env var)
//!
//! Sensitive values (refresh token, access token, MFA seeds) should live in
//! the config file with `chmod 600`, not env vars, so they don't leak into
//! process listings or systemd journal output.

use std::{net::SocketAddr, path::PathBuf, str::FromStr, time::Duration};

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
    Homeassistant,
}

impl FromStr for ProviderKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "beehive" => Ok(Self::Beehive),
            "homekit" => Ok(Self::Homekit),
            "homeassistant" | "ha" => Ok(Self::Homeassistant),
            other => Err(format!(
                "invalid provider {other:?}; expected `beehive`, `homekit`, or `homeassistant`"
            )),
        }
    }
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

    /// Settings for Home Assistant REST access. Used when `provider = "homeassistant"`.
    #[serde(default)]
    pub homeassistant: HomeAssistantConfig,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeAssistantConfig {
    /// Home Assistant base URL, e.g. `http://homeassistant.local:8123`.
    #[serde(default)]
    pub url: String,

    /// Long-lived access token (Profile → Security → Long-Lived Access Tokens).
    #[serde(default)]
    pub token: String,

    /// Explicit climate entity IDs to export. When empty, every `climate.*` entity is used.
    #[serde(default)]
    pub climate_entities: Vec<String>,

    /// Explicit `weather.*` entity IDs. When empty, weather is auto-linked by entity-id
    /// stem (e.g. `weather.ecobee` for all ecobees, or the sole `weather.*` on the instance).
    #[serde(default)]
    pub weather_entities: Vec<String>,
}

/// CLI / env overrides applied after [`Config::load`]. Each field mirrors an
/// `ECOBEE_*` environment variable exposed as a flag on `ecobee-exporter`.
#[derive(Debug, Default, Clone)]
pub struct CliOverrides {
    pub demo: bool,
    pub provider: Option<ProviderKind>,
    pub listen_addr: Option<SocketAddr>,
    pub poll_interval: Option<Duration>,
    pub state_file: Option<PathBuf>,
    pub beehive_endpoint: Option<String>,
    pub beehive_user_agent: Option<String>,
    pub beehive_refresh_token: Option<String>,
    pub beehive_headers: Vec<(String, String)>,
    pub homekit_pairing_file: Option<PathBuf>,
    pub homeassistant_url: Option<String>,
    pub homeassistant_token: Option<String>,
    pub homeassistant_climate_entities: Vec<String>,
    pub homeassistant_weather_entities: Vec<String>,
}

/// Parse a duration string such as `3m` or `90s` (same syntax as config TOML).
pub fn parse_poll_interval(s: &str) -> Result<Duration, String> {
    humantime::parse_duration(s).map_err(|e| e.to_string())
}

/// Parse a `KEY=VALUE` pair for `--beehive-header`.
pub fn parse_header_pair(s: &str) -> Result<(String, String), String> {
    let (key, value) = s
        .split_once('=')
        .ok_or_else(|| format!("expected KEY=VALUE, got {s:?}"))?;
    let key = key.trim();
    if key.is_empty() {
        return Err(format!("header key must not be empty in {s:?}"));
    }
    Ok((key.to_string(), value.to_string()))
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
        cfg.clamp_poll_interval();
        Ok(cfg)
    }

    /// Apply CLI flags (and their mirrored `ECOBEE_*` env vars) on top of
    /// layered config from [`Self::load`].
    pub fn apply_cli_overrides(&mut self, cli: &CliOverrides) {
        if cli.demo {
            self.demo = true;
        }
        if let Some(provider) = cli.provider {
            self.provider = provider;
        }
        if let Some(listen_addr) = cli.listen_addr {
            self.listen_addr = listen_addr;
        }
        if let Some(poll_interval) = cli.poll_interval {
            self.poll_interval = poll_interval;
        }
        if let Some(state_file) = &cli.state_file {
            self.state_file.clone_from(state_file);
        }
        if let Some(endpoint) = &cli.beehive_endpoint {
            self.beehive.endpoint = Some(endpoint.clone());
        }
        if let Some(user_agent) = &cli.beehive_user_agent {
            self.beehive.user_agent = Some(user_agent.clone());
        }
        if let Some(refresh_token) = &cli.beehive_refresh_token {
            self.beehive.refresh_token = Some(refresh_token.clone());
        }
        if !cli.beehive_headers.is_empty() {
            self.beehive.extra_headers.clone_from(&cli.beehive_headers);
        }
        if let Some(pairing_file) = &cli.homekit_pairing_file {
            self.homekit.pairing_file.clone_from(pairing_file);
        }
        if let Some(url) = &cli.homeassistant_url {
            self.homeassistant.url.clone_from(url);
        }
        if let Some(token) = &cli.homeassistant_token {
            self.homeassistant.token.clone_from(token);
        }
        if !cli.homeassistant_climate_entities.is_empty() {
            self.homeassistant
                .climate_entities
                .clone_from(&cli.homeassistant_climate_entities);
        }
        if !cli.homeassistant_weather_entities.is_empty() {
            self.homeassistant
                .weather_entities
                .clone_from(&cli.homeassistant_weather_entities);
        }
        self.clamp_poll_interval();
    }

    fn clamp_poll_interval(&mut self) {
        let floor = Duration::from_mins(1);
        if self.poll_interval < floor {
            tracing::warn!(
                requested = ?self.poll_interval,
                "poll_interval below 60s clamped to 60s; ecobee only refreshes data every ~3 minutes"
            );
            self.poll_interval = floor;
        }
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
            homeassistant: HomeAssistantConfig::default(),
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
    fn provider_parses_from_str() {
        assert_eq!(
            "beehive".parse::<ProviderKind>().unwrap(),
            ProviderKind::Beehive
        );
        assert_eq!(
            "homekit".parse::<ProviderKind>().unwrap(),
            ProviderKind::Homekit
        );
        assert_eq!(
            "homeassistant".parse::<ProviderKind>().unwrap(),
            ProviderKind::Homeassistant
        );
        assert_eq!(
            "ha".parse::<ProviderKind>().unwrap(),
            ProviderKind::Homeassistant
        );
        assert!("cloud".parse::<ProviderKind>().is_err());
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

    #[test]
    fn apply_cli_overrides_takes_precedence() {
        let mut cfg = Config::default();
        cfg.apply_cli_overrides(&CliOverrides {
            listen_addr: Some("127.0.0.1:9099".parse().unwrap()),
            poll_interval: Some(Duration::from_mins(5)),
            provider: Some(ProviderKind::Homekit),
            homekit_pairing_file: Some(PathBuf::from("/tmp/pairings.json")),
            beehive_endpoint: Some("https://example.test/1".to_string()),
            beehive_headers: vec![("x-test".into(), "1".into())],
            ..CliOverrides::default()
        });
        assert_eq!(cfg.listen_addr, "127.0.0.1:9099".parse().unwrap());
        assert_eq!(cfg.poll_interval, Duration::from_mins(5));
        assert_eq!(cfg.provider, ProviderKind::Homekit);
        assert_eq!(
            cfg.homekit.pairing_file,
            PathBuf::from("/tmp/pairings.json")
        );
        assert_eq!(
            cfg.beehive.endpoint.as_deref(),
            Some("https://example.test/1")
        );
        assert_eq!(
            cfg.beehive.extra_headers,
            vec![("x-test".into(), "1".into())]
        );
    }

    #[test]
    fn parse_header_pair_splits_on_first_equals() {
        assert_eq!(
            parse_header_pair("x-ecobee-app-version=4.0.0").unwrap(),
            ("x-ecobee-app-version".into(), "4.0.0".into())
        );
        assert!(parse_header_pair("no-equals").is_err());
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
