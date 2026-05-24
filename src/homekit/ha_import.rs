//! Import HomeKit pairing keys from Home Assistant `core.config_entries`.

use housekey::PairedDevice;
use serde::Deserialize;
use thiserror::Error;

const HA_DOMAIN: &str = "homekit_controller";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HaImportError {
    #[error("failed to read HA config: {0}")]
    Io(String),
    #[error("failed to parse HA config: {0}")]
    Parse(String),
    #[error("no HomeKit Controller entries with pairing data found")]
    NoMatches,
    #[error("entry {entry_id} is missing required field {field}")]
    MissingField {
        entry_id: String,
        field: &'static str,
    },
    #[error("entry {entry_id}: {field} is not valid hex: {reason}")]
    InvalidHex {
        entry_id: String,
        field: &'static str,
        reason: String,
    },
    #[error(
        "multiple entries match; re-run with --entry-id (ids: {})",
        ids.join(", ")
    )]
    Ambiguous { ids: Vec<String> },
    #[error("entry {entry_id} not found")]
    EntryNotFound { entry_id: String },
}

/// A Home Assistant `homekit_controller` config entry eligible for import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HaHomekitEntry {
    pub entry_id: String,
    pub title: String,
    pub unique_id: Option<String>,
    pub data: HaPairingData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HaPairingData {
    pub accessory_id: String,
    pub accessory_ltpk: String,
    pub controller_pairing_id: String,
    pub controller_ltsk: String,
    pub controller_ltpk: String,
    pub host: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct HaConfigEntriesFile {
    data: HaConfigEntriesData,
}

#[derive(Debug, Deserialize)]
struct HaConfigEntriesData {
    entries: Vec<HaConfigEntry>,
}

#[derive(Debug, Deserialize)]
struct HaConfigEntry {
    entry_id: String,
    domain: String,
    title: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    unique_id: Option<String>,
    #[serde(default)]
    data: serde_json::Map<String, serde_json::Value>,
}

/// Parse HA `.storage/core.config_entries` and return importable HomeKit entries.
pub fn parse_ha_config_entries(content: &str) -> Result<Vec<HaHomekitEntry>, HaImportError> {
    let file: HaConfigEntriesFile =
        serde_json::from_str(content).map_err(|e| HaImportError::Parse(e.to_string()))?;

    let mut out = Vec::new();
    for entry in file.data.entries {
        if entry.domain != HA_DOMAIN {
            continue;
        }
        if entry.source.as_deref() == Some("ignore") {
            continue;
        }
        if entry.data.is_empty() {
            continue;
        }
        if !is_ip_connection(&entry.data) {
            continue;
        }
        let Some(data) = pairing_data_from_entry(&entry)? else {
            continue;
        };
        out.push(HaHomekitEntry {
            entry_id: entry.entry_id,
            title: entry.title,
            unique_id: entry.unique_id,
            data,
        });
    }

    if out.is_empty() {
        return Err(HaImportError::NoMatches);
    }
    Ok(out)
}

/// Filter entries that look like ecobee thermostats using title heuristics.
pub fn filter_ecobee_entries(entries: &[HaHomekitEntry]) -> Vec<HaHomekitEntry> {
    entries
        .iter()
        .filter(|entry| looks_like_ecobee(entry))
        .cloned()
        .collect()
}

fn looks_like_ecobee(entry: &HaHomekitEntry) -> bool {
    entry.title.to_ascii_lowercase().contains("ecobee")
}

fn is_ip_connection(data: &serde_json::Map<String, serde_json::Value>) -> bool {
    match data.get("Connection").and_then(|v| v.as_str()) {
        None | Some("IP") => true,
        Some(_) => false,
    }
}

fn pairing_data_from_entry(entry: &HaConfigEntry) -> Result<Option<HaPairingData>, HaImportError> {
    let accessory_id = required_string(&entry.data, "AccessoryPairingID", &entry.entry_id)?;
    let accessory_ltpk = normalize_hex32(
        &required_string(&entry.data, "AccessoryLTPK", &entry.entry_id)?,
        "AccessoryLTPK",
        &entry.entry_id,
    )?;
    let controller_pairing_id = required_string_any(
        &entry.data,
        &["iOSDevicePairingID", "iOSPairingId", "iOSPairingID"],
        &entry.entry_id,
    )?;
    let controller_ltsk = normalize_hex32(
        &required_string(&entry.data, "iOSDeviceLTSK", &entry.entry_id)?,
        "iOSDeviceLTSK",
        &entry.entry_id,
    )?;
    let controller_ltpk_hex = normalize_hex32(
        &required_string(&entry.data, "iOSDeviceLTPK", &entry.entry_id)?,
        "iOSDeviceLTPK",
        &entry.entry_id,
    )?;

    let host = optional_string_any(&entry.data, &["AccessoryIP", "AccessoryAddress"]);
    let port = entry
        .data
        .get("AccessoryPort")
        .and_then(serde_json::Value::as_u64)
        .and_then(|p| u16::try_from(p).ok());

    Ok(Some(HaPairingData {
        accessory_id,
        accessory_ltpk,
        controller_pairing_id,
        controller_ltsk,
        controller_ltpk: controller_ltpk_hex,
        host,
        port,
    }))
}

pub fn alias_from_title(title: &str) -> String {
    let mut alias = String::new();
    let mut last_dash = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            alias.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !alias.is_empty() {
            alias.push('-');
            last_dash = true;
        }
    }
    alias.trim_matches('-').to_string()
}

pub fn to_paired_device(entry: &HaHomekitEntry, alias: &str) -> PairedDevice {
    PairedDevice {
        alias: alias.to_string(),
        accessory_id: entry.data.accessory_id.clone(),
        accessory_ltpk: entry.data.accessory_ltpk.clone(),
        controller_pairing_id: entry.data.controller_pairing_id.clone(),
        controller_ltsk: entry.data.controller_ltsk.clone(),
        controller_ltpk: entry.data.controller_ltpk.clone(),
        host: entry.data.host.clone(),
        port: entry.data.port,
    }
}

pub fn select_entries<'a>(
    entries: &'a [HaHomekitEntry],
    entry_id: Option<&str>,
    alias: Option<&str>,
) -> Result<Vec<(&'a HaHomekitEntry, String)>, HaImportError> {
    let pool: Vec<&HaHomekitEntry> = if let Some(id) = entry_id {
        let entry = entries.iter().find(|e| e.entry_id == id).ok_or_else(|| {
            HaImportError::EntryNotFound {
                entry_id: id.to_string(),
            }
        })?;
        vec![entry]
    } else {
        entries.iter().collect()
    };

    if pool.len() == 1 {
        let entry = pool[0];
        let alias = alias
            .map(str::to_string)
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| alias_from_title(&entry.title));
        return Ok(vec![(entry, alias)]);
    }

    if alias.as_ref().is_some_and(|a| !a.is_empty()) {
        return Err(HaImportError::Ambiguous {
            ids: pool.iter().map(|e| e.entry_id.clone()).collect(),
        });
    }

    Ok(pool
        .into_iter()
        .map(|entry| (entry, alias_from_title(&entry.title)))
        .collect())
}

fn required_string(
    data: &serde_json::Map<String, serde_json::Value>,
    field: &'static str,
    entry_id: &str,
) -> Result<String, HaImportError> {
    data.get(field)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or(HaImportError::MissingField {
            entry_id: entry_id.to_string(),
            field,
        })
}

fn required_string_any(
    data: &serde_json::Map<String, serde_json::Value>,
    fields: &[&str],
    entry_id: &str,
) -> Result<String, HaImportError> {
    for field in fields {
        if let Some(value) = data
            .get(*field)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Ok(value.to_string());
        }
    }
    Err(HaImportError::MissingField {
        entry_id: entry_id.to_string(),
        field: "iOSDevicePairingID",
    })
}

fn optional_string_any(
    data: &serde_json::Map<String, serde_json::Value>,
    fields: &[&str],
) -> Option<String> {
    fields.iter().find_map(|field| {
        data.get(*field)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

fn normalize_hex32(
    value: &str,
    field: &'static str,
    entry_id: &str,
) -> Result<String, HaImportError> {
    let trimmed = value.trim();
    if trimmed.len() != 64 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(HaImportError::InvalidHex {
            entry_id: entry_id.to_string(),
            field,
            reason: format!("expected 64 hex chars, got {}", trimmed.len()),
        });
    }
    Ok(trimmed.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ecobee_fixture_entry() {
        let entries = parse_ha_config_entries(include_str!(
            "../../tests/fixtures/ha_core.config_entries.json"
        ))
        .expect("fixture parses");
        assert_eq!(entries.len(), 2);

        let ecobee = entries
            .iter()
            .find(|e| e.title == "Main Floor")
            .expect("main floor entry");
        assert_eq!(ecobee.data.accessory_id, "30:D8:FA:9D:78:A4");
        assert_eq!(
            ecobee.data.accessory_ltpk,
            "83560f758a9261dad018499f4ca30d2059d1f8ed5247b485afca14c7b2decbf7"
        );
        assert_eq!(
            ecobee.data.controller_pairing_id,
            "d5926769-7c9f-4428-a25b-1fbd6ef7d3ab"
        );
        assert_eq!(ecobee.data.host.as_deref(), Some("192.168.1.42"));
        assert_eq!(ecobee.data.port, Some(39725));
    }

    #[test]
    fn ecobee_filter_uses_title_heuristic() {
        let entries = parse_ha_config_entries(include_str!(
            "../../tests/fixtures/ha_core.config_entries.json"
        ))
        .unwrap();
        let filtered = filter_ecobee_entries(&entries);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "ecobee Upstairs");
    }

    #[test]
    fn alias_from_title_slugifies() {
        assert_eq!(alias_from_title("Main Floor"), "main-floor");
        assert_eq!(alias_from_title("ecobee Upstairs"), "ecobee-upstairs");
    }

    #[test]
    fn maps_to_paired_device() {
        let entries = parse_ha_config_entries(include_str!(
            "../../tests/fixtures/ha_core.config_entries.json"
        ))
        .unwrap();
        let entry = &entries[0];
        let device = to_paired_device(entry, "ecobee");
        assert_eq!(device.alias, "ecobee");
        assert_eq!(device.accessory_id, entry.data.accessory_id);
        assert_eq!(device.controller_ltpk, entry.data.controller_ltpk);
    }

    #[test]
    fn rejects_non_ip_connection() {
        let json = r#"{
  "data": {
    "entries": [{
      "entry_id": "ble-1",
      "domain": "homekit_controller",
      "title": "BLE Device",
      "data": {
        "AccessoryPairingID": "AA:BB:CC:DD:EE:FF",
        "AccessoryLTPK": "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
        "iOSPairingId": "00112233-4455-6677-8899-AABBCCDDEEFF",
        "iOSDeviceLTSK": "2122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f40",
        "iOSDeviceLTPK": "4142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f60",
        "Connection": "BLE"
      }
    }]
  }
}"#;
        assert!(matches!(
            parse_ha_config_entries(json),
            Err(HaImportError::NoMatches)
        ));
    }

    #[test]
    fn select_entries_requires_entry_id_when_multiple() {
        let entries = parse_ha_config_entries(include_str!(
            "../../tests/fixtures/ha_core.config_entries.json"
        ))
        .unwrap();
        assert!(matches!(
            select_entries(&entries, None, Some("ecobee")),
            Err(HaImportError::Ambiguous { .. })
        ));
    }
}
