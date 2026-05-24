//! Home Assistant REST provider — reads ecobee entities HA already exposes.

pub mod client;
pub mod translate;

use async_trait::async_trait;

use crate::{
    config::HomeAssistantConfig,
    model::Thermostat,
    provider::{ProviderError, ThermostatProvider},
};

use client::HaClient;

/// Pulls thermostat snapshots from Home Assistant's `/api/states` endpoint.
pub struct HomeAssistantProvider {
    client: HaClient,
    climate_entities: Vec<String>,
    weather_entities: Vec<String>,
}

impl HomeAssistantProvider {
    pub fn new(cfg: &HomeAssistantConfig) -> Result<Self, ProviderError> {
        if cfg.url.trim().is_empty() {
            return Err(ProviderError::Auth(
                "homeassistant.url is required when provider = \"homeassistant\"".into(),
            ));
        }
        if cfg.token.trim().is_empty() {
            return Err(ProviderError::Auth(
                "homeassistant.token is required when provider = \"homeassistant\"".into(),
            ));
        }
        Ok(Self {
            client: HaClient::new(&cfg.url, &cfg.token)?,
            climate_entities: cfg.climate_entities.clone(),
            weather_entities: cfg.weather_entities.clone(),
        })
    }
}

#[async_trait]
impl ThermostatProvider for HomeAssistantProvider {
    async fn fetch(&self) -> Result<Vec<Thermostat>, ProviderError> {
        let states = self.client.fetch_states().await?;
        Ok(translate::translate_states(
            &states,
            &self.climate_entities,
            &self.weather_entities,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_requires_url_and_token() {
        let cfg = HomeAssistantConfig {
            url: String::new(),
            token: "token".into(),
            climate_entities: vec![],
            weather_entities: vec![],
        };
        assert!(matches!(
            HomeAssistantProvider::new(&cfg),
            Err(ProviderError::Auth(_))
        ));

        let cfg = HomeAssistantConfig {
            url: "http://localhost:8123".into(),
            token: String::new(),
            climate_entities: vec![],
            weather_entities: vec![],
        };
        assert!(matches!(
            HomeAssistantProvider::new(&cfg),
            Err(ProviderError::Auth(_))
        ));
    }
}
