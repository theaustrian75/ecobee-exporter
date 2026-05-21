//! Thin reqwest wrapper that posts GraphQL operations and parses the
//! standard `{ "data": ..., "errors": ... }` envelope.
//!
//! The Beehive endpoint URL, expected User-Agent, and any required custom
//! headers are all configurable — they're things you'll learn from your own
//! mitmproxy capture rather than from any public docs.

use reqwest::{Client, header::HeaderMap};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{config::BeehiveConfig, provider::ProviderError};

/// Standard GraphQL request body.
#[derive(Debug, Serialize)]
pub struct GraphQlRequest<'a, V: Serialize> {
    pub query: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_name: Option<&'a str>,
    pub variables: V,
}

/// Standard GraphQL response envelope.
#[derive(Debug, Deserialize)]
pub struct GraphQlResponse<T> {
    pub data: Option<T>,
    #[serde(default)]
    pub errors: Vec<GraphQlError>,
}

#[derive(Debug, Deserialize)]
pub struct GraphQlError {
    pub message: String,
    #[serde(default)]
    pub path: Option<Vec<Value>>,
    #[serde(default)]
    pub extensions: Option<Value>,
}

pub struct BeehiveClient {
    http: Client,
    endpoint: Option<String>,
}

impl BeehiveClient {
    pub fn new(cfg: &BeehiveConfig) -> Result<Self, ProviderError> {
        let mut headers = HeaderMap::new();
        for (k, v) in &cfg.extra_headers {
            let name: reqwest::header::HeaderName = k
                .parse()
                .map_err(|e| ProviderError::Auth(format!("bad header name {k}: {e}")))?;
            let value: reqwest::header::HeaderValue = v
                .parse()
                .map_err(|e| ProviderError::Auth(format!("bad header value for {k}: {e}")))?;
            headers.insert(name, value);
        }

        let ua = cfg
            .user_agent
            .clone()
            .unwrap_or_else(|| concat!("ecobee-exporter/", env!("CARGO_PKG_VERSION")).to_string());

        let http = Client::builder()
            .user_agent(ua)
            .default_headers(headers)
            .gzip(true)
            .build()?;

        Ok(Self { http, endpoint: cfg.endpoint.clone() })
    }

    pub fn http(&self) -> &Client {
        &self.http
    }

    /// Post a GraphQL operation with a bearer token. The response is
    /// returned with the standard envelope so callers can decide how to
    /// handle partial `data` + `errors`.
    pub async fn post<V, T>(
        &self,
        bearer: &str,
        operation_name: Option<&str>,
        query: &str,
        variables: V,
    ) -> Result<GraphQlResponse<T>, ProviderError>
    where
        V: Serialize,
        T: for<'de> Deserialize<'de>,
    {
        let endpoint = self.endpoint.as_deref().ok_or_else(|| {
            ProviderError::Auth(
                "beehive.endpoint not configured — capture the URL from your mobile app first"
                    .into(),
            )
        })?;

        let body = GraphQlRequest { query, operation_name, variables };

        let resp = self
            .http
            .post(endpoint)
            .bearer_auth(bearer)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<GraphQlResponse<T>>()
            .await?;

        Ok(resp)
    }
}
