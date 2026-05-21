//! GraphQL queries against Beehive, plus their response types.
//!
//! # TODO(capture)
//!
//! The Beehive schema has not been published anywhere. The query strings
//! below are placeholders showing the *shape* of what we need; the real
//! field names will come from your capture.
//!
//! From a capture of a single "pull-to-refresh" on the home screen of the
//! mobile app you should be able to extract one or more queries that
//! together cover:
//!
//!   - thermostat identifier, display name, connection state
//!   - runtime block: actualTemperature, desiredHeat, desiredCool,
//!     actualHumidity (units may be tenths-of-a-degree like the REST API,
//!     or full degrees like HomeKit; check)
//!   - settings block: hvacMode
//!   - remoteSensors[]: id, name, type, inUse, capabilities[]{type, value}
//!
//! Once you have the captured query, drop the body into `LIST_THERMOSTATS`
//! and the response type into `ListThermostatsResponse`, then implement
//! `translate` to map the response into `Vec<Thermostat>`. A test in
//! `tests/parse_sample.rs` verifies the round-trip against a saved JSON
//! fixture in `samples/`.

use serde::Deserialize;

use crate::model::Thermostat;

/// Placeholder query body. Replace with the actual GraphQL operation from
/// your capture before this module does anything useful.
pub const LIST_THERMOSTATS: &str = r"
# TODO(capture): replace with the actual query the mobile app sends when
# loading the thermostat list. Likely something like:
#
#   query ListThermostats {
#     thermostats {
#       identifier
#       name
#       runtime { actualTemperature desiredHeat desiredCool actualHumidity }
#       settings { hvacMode }
#       remoteSensors { id name type inUse capabilities { type value } }
#     }
#   }
";

#[derive(Debug, Deserialize)]
pub struct ListThermostatsResponse {
    // TODO(capture): replace with the real response shape. The current
    // empty struct only exists so the module compiles.
}

/// Translate Beehive's raw response into our domain model.
///
/// Kept as a free function (rather than a method) so it can be unit-tested
/// against captured JSON without touching the network.
pub fn translate(_raw: &ListThermostatsResponse) -> Vec<Thermostat> {
    // TODO(capture): map response fields onto Thermostat / Runtime /
    // Settings / RemoteSensor / SensorCapability.
    Vec::new()
}
