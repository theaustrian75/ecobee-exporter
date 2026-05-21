//! Thin reqwest wrapper used by the data-API queries.
//!
//! Holds the configured `reqwest::Client` (user-agent, extra headers,
//! gzip) and the base URL for the upstream API. The query module on top
//! does the actual REST shaping.

use reqwest::{Client, header::HeaderMap};

use crate::{config::BeehiveConfig, provider::ProviderError};

/// Default base URL for ecobee's data API.
///
/// The Auth0 JWT we mint has `aud=https://prod.ecobee.com/api/v1`, but
/// that's just a logical identifier — `prod.ecobee.com` does not
/// resolve publicly. The API is actually served from
/// `https://api.ecobee.com/1/...`, which is the host the long-documented
/// developer REST API has always used. Our Auth0 access token is
/// accepted there with `Authorization: Bearer …` exactly like the old
/// developer tokens were.
pub const DEFAULT_BASE_URL: &str = "https://api.ecobee.com/1";

pub struct BeehiveClient {
    http: Client,
    base_url: String,
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

        let base_url = cfg
            .endpoint
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let base_url = base_url.trim_end_matches('/').to_string();

        Ok(Self { http, base_url })
    }

    pub fn http(&self) -> &Client {
        &self.http
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}
