//! HTTP probe and scrape semantics when upstream fetch succeeds or fails.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use ecobee_exporter::{
    config::HealthProbeMode,
    metrics::Metrics,
    model::Thermostat,
    provider::{FakeProvider, ProviderError, ThermostatProvider},
    server::{AppState, probe_upstream, router},
};
use tower::ServiceExt;

struct FailProvider {
    message: String,
}

#[async_trait]
impl ThermostatProvider for FailProvider {
    async fn fetch(&self) -> Result<Vec<Thermostat>, ProviderError> {
        Err(ProviderError::Upstream(self.message.clone()))
    }
}

struct SlowProvider {
    delay: Duration,
}

#[async_trait]
impl ThermostatProvider for SlowProvider {
    async fn fetch(&self) -> Result<Vec<Thermostat>, ProviderError> {
        tokio::time::sleep(self.delay).await;
        Ok(vec![])
    }
}

fn app_state(
    metrics: Arc<Metrics>,
    provider: Arc<dyn ThermostatProvider>,
    mode: HealthProbeMode,
    timeout: Duration,
) -> AppState {
    AppState {
        metrics,
        provider,
        health_probe_mode: mode,
        health_check_timeout: timeout,
    }
}

fn cached_state(metrics: Arc<Metrics>) -> AppState {
    app_state(
        metrics,
        Arc::new(FakeProvider::demo()),
        HealthProbeMode::Cached,
        Duration::from_secs(1),
    )
}

#[tokio::test]
async fn liveness_always_ok() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    let app = router(cached_state(metrics));

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
async fn cached_readiness_fails_until_first_successful_fetch() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    let app = router(cached_state(Arc::clone(&metrics)));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/readiness")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");
    assert_eq!(
        response.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "/readiness should be unavailable before first fetch"
    );
}

#[tokio::test]
async fn healthz_live_probes_upstream_even_before_first_fetch() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    let app = router(cached_state(metrics));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "/healthz should live-probe upstream successfully"
    );
}

#[tokio::test]
async fn healthz_fails_when_upstream_unreachable_in_cached_mode() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    metrics.record_snapshot(&[], 0.1);

    let app = router(app_state(
        metrics,
        Arc::new(FailProvider {
            message: "connection refused".into(),
        }),
        HealthProbeMode::Cached,
        Duration::from_secs(1),
    ));

    let readiness = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/readiness")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");
    assert_eq!(readiness.status(), StatusCode::OK);

    let healthz = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");
    assert_eq!(healthz.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn metrics_returns_503_when_upstream_unhealthy() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    let app = router(cached_state(metrics));

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
async fn cached_successful_fetch_enables_probes_and_metrics() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    metrics.record_snapshot(&[], 0.42);

    let app = router(cached_state(Arc::clone(&metrics)));

    for path in ["/readiness", "/metrics"] {
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

    let healthz = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");
    assert_eq!(healthz.status(), StatusCode::OK);
}

#[tokio::test]
async fn cached_fetch_failure_marks_upstream_unhealthy_again() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    metrics.record_snapshot(&[], 0.1);
    metrics.record_fetch_failure("authentication failed: token expired");

    let app = router(cached_state(metrics));

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
async fn live_readiness_succeeds_even_when_cached_status_is_stale() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    metrics.record_fetch_failure("last collector poll failed");

    let app = router(app_state(
        metrics,
        Arc::new(FakeProvider::demo()),
        HealthProbeMode::Live,
        Duration::from_secs(1),
    ));

    for path in ["/readiness"] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .expect("response");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{path} should live-probe upstream successfully"
        );
    }
}

#[tokio::test]
async fn healthz_fails_when_upstream_fetch_fails() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    metrics.record_snapshot(&[], 0.1);

    let app = router(app_state(
        metrics,
        Arc::new(FailProvider {
            message: "connection refused".into(),
        }),
        HealthProbeMode::Live,
        Duration::from_secs(1),
    ));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn healthz_times_out_on_slow_upstream() {
    let metrics = Arc::new(Metrics::new().expect("registry"));

    let app = router(app_state(
        metrics,
        Arc::new(SlowProvider {
            delay: Duration::from_millis(200),
        }),
        HealthProbeMode::Live,
        Duration::from_millis(50),
    ));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn live_readiness_times_out_on_slow_upstream() {
    let metrics = Arc::new(Metrics::new().expect("registry"));

    let app = router(app_state(
        metrics,
        Arc::new(SlowProvider {
            delay: Duration::from_millis(200),
        }),
        HealthProbeMode::Live,
        Duration::from_millis(50),
    ));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/readiness")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn probe_upstream_reports_timeout() {
    let provider = SlowProvider {
        delay: Duration::from_millis(200),
    };

    let err = probe_upstream(&provider, Duration::from_millis(50))
        .await
        .expect_err("probe should time out");

    assert!(err.contains("timed out"));
}

#[tokio::test]
async fn index_documents_live_probe_mode() {
    let metrics = Arc::new(Metrics::new().expect("registry"));
    let app = router(app_state(
        metrics,
        Arc::new(FakeProvider::demo()),
        HealthProbeMode::Live,
        Duration::from_secs(1),
    ));

    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let text = String::from_utf8(body.to_vec()).expect("utf8");
    assert!(text.contains("live upstream connectivity probe"));
    assert!(text.contains("live upstream fetch"));
}
