//! Client for ecobee's internal Beehive GraphQL API.
//!
//! # Status: scaffolding only
//!
//! As of mid-2026 there is **no public reverse-engineering of Beehive** — no
//! published endpoint URL, no documented query shape, no auth flow writeup.
//! Everything below is the structural shell; the actual request/response
//! shapes are marked `TODO(capture)` and must be filled in from your own
//! mitmproxy capture (see `CAPTURE.md`).
//!
//! Until that happens, the binary should be run with `--demo` (or
//! `[demo = true]` in the config) so the rest of the stack — metrics,
//! HTTP server, polling cadence — can be verified independently.

pub mod auth;
pub mod client;
pub mod queries;

use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::{
    config::BeehiveConfig,
    model::Thermostat,
    provider::{ProviderError, ThermostatProvider},
};

use auth::AuthState;
use client::BeehiveClient;

pub struct BeehiveProvider {
    client: BeehiveClient,
    auth: Arc<Mutex<AuthState>>,
}

impl BeehiveProvider {
    pub fn new(cfg: &BeehiveConfig, state_file: PathBuf) -> Result<Self, ProviderError> {
        let client = BeehiveClient::new(cfg)?;
        let auth = Arc::new(Mutex::new(AuthState::load(cfg, state_file)?));
        Ok(Self { client, auth })
    }
}

#[async_trait]
impl ThermostatProvider for BeehiveProvider {
    async fn fetch(&self) -> Result<Vec<Thermostat>, ProviderError> {
        let token = {
            let mut state = self.auth.lock().await;
            state.access_token(&self.client).await?
        };
        let resp = queries::list_thermostats(&self.client, &token).await?;
        Ok(queries::translate(&resp))
    }
}
