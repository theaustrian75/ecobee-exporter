//! Minimal Home Assistant REST client.

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::Value;

use crate::provider::ProviderError;

#[derive(Debug, Clone, Deserialize)]
pub struct HaState {
    pub entity_id: String,
    pub state: String,
    #[serde(default)]
    pub attributes: Value,
}

pub struct HaClient {
    http: reqwest::Client,
    base_url: String,
}

impl HaClient {
    pub fn new(base_url: &str, token: &str) -> Result<Self, ProviderError> {
        let base_url = base_url.trim_end_matches('/').to_string();
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|e| ProviderError::Auth(e.to_string()))?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;
        Ok(Self { http, base_url })
    }

    pub async fn fetch_states(&self) -> Result<Vec<HaState>, ProviderError> {
        let url = format!("{}/api/states", self.base_url);
        let resp = self.http.get(&url).send().await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::Auth(
                "Home Assistant rejected the access token (401)".into(),
            ));
        }
        if !resp.status().is_success() {
            return Err(ProviderError::Upstream(format!(
                "GET /api/states returned HTTP {}",
                resp.status()
            )));
        }
        resp.json().await.map_err(ProviderError::from)
    }
}
