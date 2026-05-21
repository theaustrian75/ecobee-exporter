//! Prometheus exporter for ecobee thermostats.
//!
//! Data is sourced from ecobee's internal Beehive mobile-app GraphQL API. The
//! official developer REST API is not used here because new developer
//! registrations have been closed since March 28, 2024 and pre-existing keys
//! are not assumed.
//!
//! See `README.md` for the ToS caveat and `CAPTURE.md` for how to bootstrap
//! credentials by capturing your own mobile-app traffic.

pub mod auth0;
pub mod beehive;
pub mod collector;
pub mod config;
pub mod metrics;
pub mod model;
pub mod provider;
pub mod server;
pub mod state;
