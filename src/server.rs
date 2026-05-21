//! HTTP surface: `/metrics` (Prometheus text format) and `/healthz`.

use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};

use crate::metrics::Metrics;

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
