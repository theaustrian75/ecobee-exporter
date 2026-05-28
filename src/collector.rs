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
        Self {
            provider,
            metrics,
            poll_interval,
        }
    }

    /// Run the polling loop until `shutdown` completes.
    ///
    /// Call [`Self::poll_once`] before spawning if `/metrics` should be warm on
    /// the first scrape (startup probe or demo bootstrap).
    pub async fn run(self, mut shutdown: tokio::sync::broadcast::Receiver<()>) {
        let mut ticker = interval(self.poll_interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = shutdown.recv() => break,
                _ = ticker.tick() => {
                    let _ = self.poll_once().await;
                }
            }
        }
    }

    /// Fetch once and update metrics. Returns `true` on success.
    pub async fn poll_once(&self) -> bool {
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
                true
            }
            Err(e) => {
                let message = e.to_string();
                tracing::error!(error = %e, "fetch failed");
                self.metrics.record_fetch_failure(&message);
                false
            }
        }
    }

    pub fn upstream_status(&self) -> Result<(), String> {
        self.metrics.upstream_status()
    }
}
