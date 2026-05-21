//! Token management for the Beehive client.
//!
//! Refreshes are real (against Auth0 `/oauth/token` with the captured
//! `client_id`); the initial interactive login lives in the
//! `ecobee-login` binary, not here, because it requires a human at a
//! browser to handle Auth0's MFA prompt.
//!
//! Bootstrap order, from the exporter's perspective:
//!   1. User runs `ecobee-login` once. That writes a refresh token to
//!      `state_file`.
//!   2. The exporter starts, reads `state_file`, mints an access token
//!      via `/oauth/token` (refresh-grant), then uses it against the
//!      data API.
//!   3. Access tokens get refreshed ~30s before expiry; refresh
//!      tokens are rotated by Auth0 and persisted on every successful
//!      refresh so the file stays usable across restarts.

use std::{path::PathBuf, time::SystemTime};

use crate::{
    auth0,
    config::BeehiveConfig,
    provider::ProviderError,
    state::PersistedState,
};

use super::client::BeehiveClient;

pub struct AuthState {
    state_file: PathBuf,
    persisted: PersistedState,
}

impl AuthState {
    pub fn load(cfg: &BeehiveConfig, state_file: PathBuf) -> Result<Self, ProviderError> {
        let mut persisted = PersistedState::load(&state_file).map_err(|e| {
            ProviderError::Auth(format!("loading {}: {e}", state_file.display()))
        })?;
        if let Some(rt) = cfg.refresh_token.clone()
            && persisted.refresh_token.as_ref() != Some(&rt)
        {
            tracing::info!("seeding refresh token from config");
            persisted.refresh_token = Some(rt);
            persisted.access_token = None;
            persisted.access_expires_at = None;
        }
        Ok(Self { state_file, persisted })
    }

    /// Return a usable bearer token. Refreshes from Auth0 if the cached
    /// one is missing or near expiry. Persists rotated refresh tokens
    /// back to disk on each successful exchange.
    pub async fn access_token(&mut self, client: &BeehiveClient) -> Result<String, ProviderError> {
        if let (Some(tok), Some(exp)) = (&self.persisted.access_token, self.persisted.access_expires_at)
            && exp.saturating_sub(now_unix()) > 30
        {
            return Ok(tok.clone());
        }

        let rt = self.persisted.refresh_token.clone().ok_or_else(|| {
            ProviderError::Auth(
                "no refresh token on disk — run `ecobee-login` once to bootstrap".into(),
            )
        })?;

        let tokens = auth0::refresh_token(client.http(), &rt)
            .await
            .map_err(|e| ProviderError::Auth(format!("Auth0 refresh failed: {e}")))?;

        self.persisted.access_token = Some(tokens.access_token.clone());
        self.persisted.access_expires_at = Some(now_unix().saturating_add(tokens.expires_in));
        if let Some(new_rt) = tokens.refresh_token {
            if Some(&new_rt) != self.persisted.refresh_token.as_ref() {
                tracing::info!("refresh token rotated by Auth0");
            }
            self.persisted.refresh_token = Some(new_rt);
        }
        if let Err(e) = self.persisted.save(&self.state_file) {
            tracing::warn!(error = %e, "could not persist refreshed state");
        }
        Ok(tokens.access_token)
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs().cast_signed())
}
