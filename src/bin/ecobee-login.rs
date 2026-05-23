//! `ecobee-login` — interactive PKCE bootstrap.
//!
//! Run this once. It opens an Auth0 login URL in your desktop browser,
//! you log in (and complete MFA) as normal, the browser lands on a
//! `…/callback?code=…&state=…` URL, you paste that URL back here, and
//! the helper exchanges the code for a refresh token and writes it to
//! the state file. After that, run `ecobee-exporter` normally.

use std::{
    io::{BufRead, Write, stdin, stdout},
    path::PathBuf,
    process::Command,
    time::Duration,
};

use anyhow::{Context, anyhow, bail};
use clap::Parser;
use ecobee_exporter::{
    auth0::{self, PkcePair, REDIRECT_URI, build_authorize_url, exchange_code},
    state::PersistedState,
};
use url::Url;

#[derive(Debug, Parser)]
#[command(
    name = "ecobee-login",
    version,
    about = "One-time interactive login that mints a refresh token for ecobee-exporter"
)]
struct Cli {
    /// Where to write the refresh token. Must match the exporter's
    /// `state_file` setting.
    #[arg(long, env = "ECOBEE_STATE_FILE", default_value = "ecobee-exporter.state.json")]
    state_file: PathBuf,

    /// Try to spawn the system browser via `xdg-open` / `open`. Off by
    /// default; some environments make this awkward.
    #[arg(long)]
    open_browser: bool,
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
    let pkce = PkcePair::generate().context("generating PKCE pair")?;
    let url = build_authorize_url(&pkce).context("building authorize URL")?;

    println!("\nopen this URL in your browser, log in, and complete MFA:\n");
    println!("  {url}\n");
    println!("after the final redirect, the address bar will show a page");
    println!("at `{REDIRECT_URI}?code=…&state=…`");
    println!("(it may look blank or show an error — that is expected on desktop,");
    println!(" since the URL is meant to open the Android app).");
    println!("\ncopy the FULL URL out of the address bar and paste it below.\n");

    if cli.open_browser {
        try_open_browser(url.as_str());
    }

    let callback = read_callback_line()?;
    let parsed = Url::parse(callback.trim()).context("the pasted text is not a URL")?;

    let mut code = None;
    let mut returned_state = None;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => returned_state = Some(v.into_owned()),
            "error" => {
                bail!("Auth0 returned error in callback: {v}")
            }
            _ => {}
        }
    }
    let code = code.ok_or_else(|| anyhow!("no `code` query parameter in the pasted URL"))?;
    let returned_state =
        returned_state.ok_or_else(|| anyhow!("no `state` query parameter in the pasted URL"))?;
    if returned_state != pkce.state {
        bail!(
            "state mismatch — got {returned_state:?}, expected {:?}. \
             This means the callback URL is from a different login attempt. \
             Re-run ecobee-login and use the brand-new URL.",
            pkce.state
        );
    }

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("ecobee-login/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("building HTTP client")?;
    let tokens = exchange_code(&http, &code, &pkce.verifier)
        .await
        .map_err(|e| match e {
            auth0::Auth0Error::TokenError { status, body } => {
                anyhow!("Auth0 /oauth/token returned {status}: {body}")
            }
            other => anyhow::Error::from(other),
        })
        .context("exchanging code at Auth0 /oauth/token")?;

    let refresh = tokens.refresh_token.clone().ok_or_else(|| {
        anyhow!(
            "Auth0 didn't return a refresh_token. \
             Make sure your account allows `offline_access` scope."
        )
    })?;

    let mut state = PersistedState::load(&cli.state_file).unwrap_or_default();
    state.refresh_token = Some(refresh);
    state.access_token = Some(tokens.access_token);
    state.access_expires_at = Some(now_unix().saturating_add(tokens.expires_in));
    state
        .save(&cli.state_file)
        .with_context(|| format!("writing {}", cli.state_file.display()))?;

    println!("\nrefresh token saved to {}", cli.state_file.display());
    println!("(file mode forced to 0600 on Unix; treat this file as a password)");
    println!("\nyou can now run `ecobee-exporter` against your account.");
    Ok(())
}

fn read_callback_line() -> anyhow::Result<String> {
    print!("callback URL > ");
    stdout().flush().ok();
    let mut line = String::new();
    stdin()
        .lock()
        .read_line(&mut line)
        .context("reading callback URL from stdin")?;
    if line.trim().is_empty() {
        bail!("no input received");
    }
    Ok(line)
}

fn try_open_browser(url: &str) {
    let openers: &[&str] = if cfg!(target_os = "macos") {
        &["open"]
    } else if cfg!(target_os = "windows") {
        &["cmd", "/c", "start"]
    } else {
        &["xdg-open"]
    };
    if let Some((cmd, args)) = openers.split_first()
        && Command::new(cmd).args(args).arg(url).spawn().is_ok()
    {
        eprintln!("(attempted to open browser via `{cmd}`)");
    }
}

fn now_unix() -> i64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs().cast_signed())
}
