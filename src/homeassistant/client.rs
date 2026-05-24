//! Minimal Home Assistant REST client.

use std::collections::{HashMap, HashSet};

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

/// Maps HA entities to devices and parent/child device relationships.
///
/// Built from the Template API (`device_id()`, `device_attr()`) so HomeKit
/// ecobee room sensors (separate accessories linked via `via_device_id`) attach
/// to the correct thermostat without substring false positives (Roku, etc.).
#[derive(Debug, Clone, Default)]
pub struct DeviceGraph {
    entity_to_device: HashMap<String, String>,
    /// child device id -> parent device id (`via_device_id`)
    via_device: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct EntityDeviceRow {
    entity_id: String,
    device_id: String,
}

#[derive(Debug, Deserialize)]
struct DeviceRow {
    id: String,
    #[serde(default)]
    via_device_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeviceGraphPayload {
    entities: Vec<EntityDeviceRow>,
    devices: Vec<DeviceRow>,
}

impl DeviceGraph {
    pub fn parse(entity_rows_json: &str, device_rows_json: &str) -> Result<Self, ProviderError> {
        let entity_rows: Vec<EntityDeviceRow> = serde_json::from_str(entity_rows_json.trim())
            .map_err(|e| ProviderError::Upstream(format!("device graph entity JSON: {e}")))?;
        let device_rows: Vec<DeviceRow> = serde_json::from_str(device_rows_json.trim())
            .map_err(|e| ProviderError::Upstream(format!("device graph device JSON: {e}")))?;

        Ok(Self::from_rows(entity_rows, device_rows))
    }

    pub fn parse_payload(json: &str) -> Result<Self, ProviderError> {
        let payload: DeviceGraphPayload = serde_json::from_str(json.trim())
            .map_err(|e| ProviderError::Upstream(format!("device graph JSON: {e}")))?;
        Ok(Self::from_rows(payload.entities, payload.devices))
    }

    fn from_rows(entity_rows: Vec<EntityDeviceRow>, device_rows: Vec<DeviceRow>) -> Self {
        let entity_to_device = entity_rows
            .into_iter()
            .map(|row| (row.entity_id, row.device_id))
            .collect();

        let via_device = device_rows
            .into_iter()
            .filter_map(|row| row.via_device_id.map(|parent| (row.id, parent)))
            .collect();

        Self {
            entity_to_device,
            via_device,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entity_to_device.is_empty()
    }

    pub fn related_device_ids(&self, climate_entity_id: &str) -> HashSet<String> {
        let Some(root) = self.entity_to_device.get(climate_entity_id) else {
            return HashSet::new();
        };

        let mut related = HashSet::new();
        related.insert(root.clone());
        let mut queue = vec![root.clone()];
        while let Some(device_id) = queue.pop() {
            for (child, parent) in &self.via_device {
                if parent == &device_id && related.insert(child.clone()) {
                    queue.push(child.clone());
                }
            }
        }
        related
    }

    pub fn entity_on_devices(&self, entity_id: &str, device_ids: &HashSet<String>) -> bool {
        self.entity_to_device
            .get(entity_id)
            .is_some_and(|id| device_ids.contains(id))
    }
}

pub struct HaClient {
    http: reqwest::Client,
    base_url: String,
}

const DEVICE_GRAPH_TEMPLATE: &str = r"{%- set ents = namespace(items=[]) -%}
{%- set devs = namespace(items=[], seen=[]) -%}
{%- for s in states -%}
{%- if s.entity_id.startswith('sensor.') or s.entity_id.startswith('binary_sensor.') or s.entity_id.startswith('climate.') -%}
{%- set did = device_id(s.entity_id) -%}
{%- if did -%}
{%- set ents.items = ents.items + [{'entity_id': s.entity_id, 'device_id': did}] -%}
{%- if did not in devs.seen -%}
{%- set devs.seen = devs.seen + [did] -%}
{%- set devs.items = devs.items + [{'id': did, 'via_device_id': device_attr(did, 'via_device_id')}] -%}
{%- endif -%}
{%- endif -%}
{%- endif -%}
{%- endfor -%}
{{ {'entities': ents.items, 'devices': devs.items} | tojson }}";

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

    pub async fn fetch_device_graph(&self) -> Result<DeviceGraph, ProviderError> {
        let payload = self.render_template(DEVICE_GRAPH_TEMPLATE).await?;
        DeviceGraph::parse_payload(&payload)
    }

    async fn render_template(&self, template: &str) -> Result<String, ProviderError> {
        let url = format!("{}/api/template", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(&serde_json::json!({ "template": template }))
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::Auth(
                "Home Assistant rejected the access token for /api/template (401); an admin token is required".into(),
            ));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let detail = body.trim();
            let detail = if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
            };
            return Err(ProviderError::Upstream(format!(
                "POST /api/template returned HTTP {status}{detail}"
            )));
        }
        Ok(resp.text().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_graph_collects_child_devices() {
        let graph = DeviceGraph::parse(
            r#"[
              {"entity_id":"climate.living_room","device_id":"therm"},
              {"entity_id":"sensor.bedroom_temperature","device_id":"remote1"},
              {"entity_id":"sensor.living_room_roku_active","device_id":"roku1"}
            ]"#,
            r#"[
              {"id":"therm","via_device_id":null},
              {"id":"remote1","via_device_id":"therm"},
              {"id":"roku1","via_device_id":null}
            ]"#,
        )
        .expect("parse");

        let related = graph.related_device_ids("climate.living_room");
        assert!(related.contains("therm"));
        assert!(related.contains("remote1"));
        assert!(!related.contains("roku1"));

        assert!(graph.entity_on_devices("sensor.bedroom_temperature", &related));
        assert!(!graph.entity_on_devices("sensor.living_room_roku_active", &related));
    }

    #[test]
    fn device_graph_parse_payload() {
        let graph = DeviceGraph::parse_payload(
            r#"{"entities":[
              {"entity_id":"climate.living_room","device_id":"therm"},
              {"entity_id":"sensor.bedroom_temperature","device_id":"remote1"}
            ],"devices":[
              {"id":"therm","via_device_id":null},
              {"id":"remote1","via_device_id":"therm"}
            ]}"#,
        )
        .expect("parse");

        let related = graph.related_device_ids("climate.living_room");
        assert!(related.contains("remote1"));
    }
}
