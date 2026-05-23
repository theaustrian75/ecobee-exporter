//! Client for ecobee's mobile-app data API.
//!
//! Auth0-issued JWT bearer tokens are exchanged via [`auth`]; thermostat
//! data is fetched from the Selection-based REST endpoint in [`queries`].
//! Use `--demo` (or `[demo = true]` in the config) to exercise metrics and
//! the HTTP server without credentials.

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
