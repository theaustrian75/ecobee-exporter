use serde::{Deserialize, Serialize};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AccessoryError {
    #[error("characteristic not found: {0}")]
    CharacteristicNotFound(String),
    #[error("service not found: {0}")]
    ServiceNotFound(String),
    #[error("operation not permitted on this characteristic")]
    NotPermitted,
    #[error("accessory returned HAP status {0}")]
    HapStatus(i32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Accessory {
    pub aid: u64,
    pub services: Vec<Service>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub iid: u64,
    #[serde(rename = "type")]
    pub service_type: String,
    pub characteristics: Vec<Characteristic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Characteristic {
    pub iid: u64,
    #[serde(rename = "type")]
    pub char_type: String,
    pub perms: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(rename = "minValue", skip_serializing_if = "Option::is_none")]
    pub min_value: Option<f64>,
    #[serde(rename = "maxValue", skip_serializing_if = "Option::is_none")]
    pub max_value: Option<f64>,
    #[serde(rename = "minStep", skip_serializing_if = "Option::is_none")]
    pub min_step: Option<f64>,
}

impl Characteristic {
    pub fn is_readable(&self) -> bool {
        self.perms.iter().any(|p| p == "pr")
    }

    pub fn is_writable(&self) -> bool {
        self.perms.iter().any(|p| p == "pw")
    }

    pub fn supports_events(&self) -> bool {
        self.perms.iter().any(|p| p == "ev")
    }
}
