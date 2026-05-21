//! Beehive authentication state.
//!
//! # TODO(capture)
//!
//! The exact login + refresh flow is *not* publicly documented. The 2020
//! ecobee co-op blog post mentions MFA, which suggests the mobile app does
//! something like:
//!
//!   1. POST email/password to an auth endpoint, receive a session token
//!      plus an MFA challenge.
//!   2. POST the MFA code to a verify endpoint, receive an access token
//!      and a refresh token.
//!   3. Refresh: POST the refresh token to a refresh endpoint, receive a
//!      new access token (and possibly a rotated refresh token).
//!
//! Common shapes that turn up in mobile-app GraphQL stacks are:
//!   - AWS Cognito user pools (`InitiateAuth` / `RespondToAuthChallenge`)
//!   - Auth0 ROPG (`/oauth/token` with `grant_type=password`)
//!   - Custom OAuth-like REST on the same host as Beehive itself
//!
//! Capture a login + a refresh from your phone (see `CAPTURE.md`) to learn
//! which one ecobee uses, then implement the matching flow here. Until
//! that's done, this module supports the "I have a refresh token already,
//! just use it" path by reading `beehive.refresh_token` from the config —
//! that's enough to validate the rest of the pipeline if you can extract
//! one manually.

use std::time::{Duration, Instant};

use crate::{config::BeehiveConfig, provider::ProviderError};

use super::client::BeehiveClient;

#[derive(Debug, Clone)]
pub struct AuthState {
    refresh_token: Option<String>,
    access_token: Option<String>,
    /// When the cached access token stops being usable. We refresh ~30s
    /// before this to avoid races near the boundary.
    access_expires_at: Option<Instant>,
    email: Option<String>,
    password: Option<String>,
}

impl AuthState {
    pub fn from_config(cfg: &BeehiveConfig) -> Self {
        Self {
            refresh_token: cfg.refresh_token.clone(),
            access_token: None,
            access_expires_at: None,
            email: cfg.email.clone(),
            password: cfg.password.clone(),
        }
    }

    /// Return a usable bearer token, refreshing or logging in as needed.
    pub async fn access_token(&mut self, client: &BeehiveClient) -> Result<String, ProviderError> {
        if let (Some(tok), Some(exp)) = (&self.access_token, self.access_expires_at)
            && exp.saturating_duration_since(Instant::now()) > Duration::from_secs(30)
        {
            return Ok(tok.clone());
        }

        if self.refresh_token.is_some() {
            self.refresh(client).await
        } else if self.email.is_some() && self.password.is_some() {
            self.login(client).await
        } else {
            Err(ProviderError::Auth(
                "no refresh_token and no email/password configured".into(),
            ))
        }
    }

    async fn refresh(&mut self, _client: &BeehiveClient) -> Result<String, ProviderError> {
        // TODO(capture): replace this stub with a real call. From your
        // mitmproxy flow, you need:
        //   - the refresh endpoint URL (it may or may not be the same host
        //     as Beehive itself)
        //   - the request body shape (form-urlencoded? JSON?)
        //   - the response body shape (look for `access_token`, `expires_in`,
        //     possibly a rotated `refresh_token`)
        //   - any required headers beyond what BeehiveClient adds
        //
        // Once filled in, populate `self.access_token`, `self.access_expires_at`,
        // and (if rotated) `self.refresh_token`, then return the new token.
        Err(ProviderError::NotImplemented(
            "refresh-token flow not yet captured — see src/beehive/auth.rs",
        ))
    }

    async fn login(&mut self, _client: &BeehiveClient) -> Result<String, ProviderError> {
        // TODO(capture): same drill as refresh(), but for the initial
        // username+password login. Watch for an MFA step — the co-op blog
        // post explicitly mentions MFA features in Beehive, so this is
        // likely a two-call sequence (initiate auth -> respond to
        // challenge) rather than a single round-trip.
        Err(ProviderError::NotImplemented(
            "username/password login flow not yet captured — see src/beehive/auth.rs",
        ))
    }
}
