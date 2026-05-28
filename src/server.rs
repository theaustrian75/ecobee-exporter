//! HTTP surface: `/metrics` (Prometheus text format), health probes, and graceful shutdown.

use std::{future::Future, net::SocketAddr, sync::Arc, time::Duration};

use anyhow::Context;
use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use axum_server::Handle;

use crate::{
    config::{HealthProbeMode, TlsConfig},
    metrics::Metrics,
    provider::ThermostatProvider,
};

const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct AppState {
    pub metrics: Arc<Metrics>,
    pub provider: Arc<dyn ThermostatProvider>,
    pub health_probe_mode: HealthProbeMode,
    pub health_check_timeout: Duration,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .route("/readiness", get(readiness))
        .route("/liveness", get(liveness))
        .route("/metrics", get(metrics_handler))
        .with_state(state)
}

/// Probe upstream with a timeout. Used by live health checks.
pub async fn probe_upstream(
    provider: &dyn ThermostatProvider,
    timeout: Duration,
) -> Result<(), String> {
    match tokio::time::timeout(timeout, provider.fetch()).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err(format!(
            "upstream probe timed out after {}s",
            timeout.as_secs()
        )),
    }
}

/// Bind and serve until `shutdown` completes or the HTTP(S) server exits.
pub async fn run(
    app: Router,
    addr: SocketAddr,
    tls: Option<&TlsConfig>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    if let Some(tls) = tls {
        tls.validate().map_err(anyhow::Error::msg)?;
        let config =
            axum_server::tls_rustls::RustlsConfig::from_pem_file(&tls.cert_file, &tls.key_file)
                .await
                .with_context(|| {
                    format!(
                        "loading TLS certificate {} and key {}",
                        tls.cert_file.display(),
                        tls.key_file.display()
                    )
                })?;
        let handle = Handle::new();
        let server_handle = handle.clone();
        tokio::spawn(async move {
            shutdown.await;
            tracing::info!("stopping HTTPS server");
            handle.graceful_shutdown(Some(GRACEFUL_SHUTDOWN_TIMEOUT));
        });
        tracing::info!(addr = %addr, "metrics server listening (HTTPS)");
        axum_server::bind_rustls(addr, config)
            .handle(server_handle)
            .serve(app.into_make_service())
            .await
            .context("HTTPS server exited")?;
    } else {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("binding {addr}"))?;
        let bound = listener.local_addr().unwrap_or(addr);
        tracing::info!(addr = %bound, "metrics server listening");
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown)
            .await
            .context("HTTP server exited")?;
    }
    Ok(())
}

pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate =
            signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
        tokio::select! {
            () = ctrl_c => {},
            _ = terminate.recv() => {},
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await;
    }

    tracing::info!("shutdown signal received");
}

async fn index(State(state): State<AppState>) -> String {
    let probe_mode = match state.health_probe_mode {
        HealthProbeMode::Cached => "cached collector status",
        HealthProbeMode::Live => "live upstream fetch",
    };
    format!(
        "ecobee-exporter\n\n\
           /metrics    Prometheus text format (503 when upstream fetch failed)\n\
           /healthz    live upstream connectivity probe\n\
           /readiness  upstream readiness probe ({probe_mode})\n\
           /liveness   process liveness probe (always ok)\n"
    )
}

async fn liveness() -> &'static str {
    "ok"
}

async fn healthz(State(state): State<AppState>) -> Response {
    upstream_health_response(
        probe_upstream(state.provider.as_ref(), state.health_check_timeout).await,
        "connectivity",
    )
}

async fn readiness(State(state): State<AppState>) -> Response {
    let result = match state.health_probe_mode {
        HealthProbeMode::Cached => state.metrics.upstream_status(),
        HealthProbeMode::Live => {
            probe_upstream(state.provider.as_ref(), state.health_check_timeout).await
        }
    };
    upstream_health_response(result, "readiness")
}

fn upstream_health_response(result: Result<(), String>, probe: &str) -> Response {
    match result {
        Ok(()) => (StatusCode::OK, "ok").into_response(),
        Err(detail) => {
            tracing::error!(probe, detail = %detail, "upstream unhealthy");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("error: upstream unavailable: {detail}"),
            )
                .into_response()
        }
    }
}

async fn metrics_handler(State(state): State<AppState>) -> Response {
    if let Err(detail) = state.metrics.upstream_status() {
        tracing::error!(detail = %detail, "refusing stale metrics scrape");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("failed to fetch upstream data: {detail}"),
        )
            .into_response();
    }

    match state.metrics.render() {
        Ok(body) => (
            StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                "text/plain; version=0.0.4; charset=utf-8",
            )],
            body,
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "failed to render metrics");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to render metrics",
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::FakeProvider;

    #[tokio::test]
    async fn probe_upstream_succeeds_for_fake_provider() {
        let provider = FakeProvider::demo();
        probe_upstream(&provider, Duration::from_secs(1))
            .await
            .expect("probe should succeed");
    }
}
