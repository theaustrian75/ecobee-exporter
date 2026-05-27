//! HTTP probe and scrape semantics when upstream fetch succeeds or fails.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use ecobee_exporter::{
    metrics::Metrics,
    server::{AppState, router},
};
use tower::ServiceExt;

#[tokio::test]
async fn liveness_always_ok() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    let app = router(AppState { metrics });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/liveness")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn readiness_and_healthz_fail_until_first_successful_fetch() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    let app = router(AppState {
        metrics: Arc::clone(&metrics),
    });

    for path in ["/healthz", "/readiness"] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .expect("response");
        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "{path} should be unavailable before first fetch"
        );
    }
}

#[tokio::test]
async fn metrics_returns_503_when_upstream_unhealthy() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    let app = router(AppState { metrics });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn successful_fetch_enables_probes_and_metrics() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    metrics.record_snapshot(&[], 0.42);

    let app = router(AppState {
        metrics: Arc::clone(&metrics),
    });

    for path in ["/healthz", "/readiness", "/metrics"] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .expect("response");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{path} should succeed after fetch"
        );
    }
}

#[tokio::test]
async fn fetch_failure_marks_upstream_unhealthy_again() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    metrics.record_snapshot(&[], 0.1);
    metrics.record_fetch_failure("authentication failed: token expired");

    let app = router(AppState { metrics });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
