//! HTTP surface: `/metrics` (Prometheus text format) and `/healthz`.

use std::{net::SocketAddr, sync::Arc};

use anyhow::Context;
use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};

use crate::{config::TlsConfig, metrics::Metrics};

#[derive(Clone)]
pub struct AppState {
    pub metrics: Arc<Metrics>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .route("/metrics", get(metrics_handler))
        .with_state(state)
}

/// Bind and serve until the HTTP(S) server exits.
pub async fn run(app: Router, addr: SocketAddr, tls: Option<&TlsConfig>) -> anyhow::Result<()> {
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
        tracing::info!(addr = %addr, "metrics server listening (HTTPS)");
        axum_server::bind_rustls(addr, config)
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
            .await
            .context("HTTP server exited")?;
    }
    Ok(())
}

async fn index() -> &'static str {
    "ecobee-exporter\n\n  /metrics  Prometheus text format\n  /healthz  liveness probe\n"
}

async fn healthz() -> &'static str {
    "ok"
}

async fn metrics_handler(State(state): State<AppState>) -> Response {
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
