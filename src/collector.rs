//! Polling loop: ask the provider for a fresh snapshot on a fixed cadence
//! and feed it into the metrics registry.

use std::{sync::Arc, time::Instant};

use tokio::time::{MissedTickBehavior, interval};

use crate::{metrics::Metrics, provider::ThermostatProvider};

pub struct Collector {
    provider: Arc<dyn ThermostatProvider>,
    metrics: Arc<Metrics>,
    poll_interval: std::time::Duration,
}

impl Collector {
    pub fn new(
        provider: Arc<dyn ThermostatProvider>,
        metrics: Arc<Metrics>,
        poll_interval: std::time::Duration,
    ) -> Self {
        Self { provider, metrics, poll_interval }
    }

    /// Run the polling loop until cancelled. Does one immediate fetch on
    /// start so `/metrics` is populated before the first scrape lands.
    pub async fn run(self) -> ! {
        let mut ticker = interval(self.poll_interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            ticker.tick().await;
            self.poll_once().await;
        }
    }

    pub async fn poll_once(&self) {
        let started = Instant::now();
        match self.provider.fetch().await {
            Ok(snapshot) => {
                let elapsed = started.elapsed().as_secs_f64();
                tracing::info!(
                    thermostats = snapshot.len(),
                    elapsed_s = elapsed,
                    "fetched snapshot"
                );
                self.metrics.record_snapshot(&snapshot, elapsed);
            }
            Err(e) => {
                tracing::error!(error = %e, "fetch failed");
                self.metrics.record_fetch_failure();
            }
        }
    }
}
