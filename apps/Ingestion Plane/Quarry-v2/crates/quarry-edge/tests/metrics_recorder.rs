//! The Prometheus recorder, the internal listener and the HTTP layer.
//!
//! The recorder is process-global, so these tests share one install and live
//! in their own test binary; each asserts only on labels it created.

use std::{
    net::SocketAddr,
    sync::{Arc, OnceLock},
    time::Duration,
};

use metrics_exporter_prometheus::PrometheusHandle;
use quarry_edge::metrics;
use quarry_runtime::{fetch::StaticDriver, lease_pool::RuntimeLeasePool, EventSink};
use tokio::sync::mpsc;

mod support;

const TOKEN: &str = "metrics-test-token-0123456789";

fn handle() -> PrometheusHandle {
    static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();
    HANDLE
        .get_or_init(|| metrics::install().expect("install the recorder"))
        .clone()
}

/// The real public router with the metrics layer on, as `main` builds it.
async fn spawn_public() -> SocketAddr {
    handle();
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    let state = support::base_state(driver, EventSink::new(tx));
    let app = quarry_edge::routes::router(state)
        .layer(axum::middleware::from_fn(metrics::http_middleware));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn spawn_metrics(token: Option<&str>) -> SocketAddr {
    let router = metrics::router(handle(), token);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    addr
}

async fn get(addr: SocketAddr, path: &str, bearer: Option<&str>) -> (u16, String) {
    let mut request = reqwest::Client::new().get(format!("http://{addr}{path}"));
    if let Some(token) = bearer {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.unwrap();
    (response.status().as_u16(), response.text().await.unwrap())
}

#[tokio::test]
async fn metrics_endpoint_requires_the_bearer_token() {
    let addr = spawn_metrics(Some(TOKEN)).await;
    assert_eq!(get(addr, "/metrics", None).await.0, 401);
    assert_eq!(
        get(addr, "/metrics", Some("wrong-token-0123456789"))
            .await
            .0,
        401
    );
    assert_eq!(get(addr, "/metrics", Some("")).await.0, 401);
    let (status, body) = get(addr, "/metrics", Some(TOKEN)).await;
    assert_eq!(status, 200);
    assert!(body.contains("quarry_build_info"), "{body}");
    // The health probe never needs the token.
    assert_eq!(get(addr, "/healthz", None).await, (200, "ok".to_owned()));
}

#[tokio::test]
async fn an_untokened_listener_is_open_for_development_only() {
    let addr = spawn_metrics(None).await;
    assert_eq!(get(addr, "/metrics", None).await.0, 200);
}

#[tokio::test]
async fn public_listener_does_not_serve_metrics() {
    let public = spawn_public().await;
    for path in ["/metrics", "/metrics/", "/healthz-metrics"] {
        assert_eq!(get(public, path, Some("anything")).await.0, 404, "{path}");
    }
}

#[tokio::test]
async fn http_requests_are_counted_by_route_class_not_by_path() {
    let public = spawn_public().await;
    let metrics_addr = spawn_metrics(Some(TOKEN)).await;

    assert_eq!(get(public, "/health", None).await.0, 200);
    // Probes with a host, an org-like id and an id-bearing path.
    get(public, "/v1/probe-host.example.com/secret-path", None).await;
    get(public, "/v1/agent/runs/org_secret_12345/step", None).await;
    get(public, "/wp-admin/../../etc/passwd", None).await;

    let (_, body) = get(metrics_addr, "/metrics", Some(TOKEN)).await;
    assert!(
        body.lines()
            .any(|line| line.starts_with("quarry_http_requests_total{")
                && line.contains(r#"route="health""#)
                && line.contains(r#"status_class="2xx""#)),
        "{body}"
    );
    assert!(
        body.contains("quarry_http_request_duration_seconds_bucket"),
        "{body}"
    );
    assert!(
        body.contains(r#"route="v1_other""#),
        "an unknown /v1 path is one class: {body}"
    );
    assert!(body.contains(r#"route="agent""#), "{body}");
    assert!(body.contains(r#"route="other""#), "{body}");
    for secret in [
        "probe-host",
        "example.com",
        "org_secret_12345",
        "secret-path",
        "passwd",
    ] {
        assert!(
            !body.contains(secret),
            "{secret} leaked into the exposition"
        );
    }
}

#[tokio::test]
async fn metrics_labels_are_closed_sets() {
    let public = spawn_public().await;
    let metrics_addr = spawn_metrics(Some(TOKEN)).await;
    for index in 0..25 {
        get(public, &format!("/v1/unique-probe-{index}/x"), None).await;
        get(public, &format!("/unique-root-{index}"), None).await;
    }
    let (_, body) = get(metrics_addr, "/metrics", Some(TOKEN)).await;
    assert!(
        !body.contains("unique-probe"),
        "a path segment became a label"
    );
    assert!(
        !body.contains("unique-root"),
        "a path segment became a label"
    );
    let routes: std::collections::BTreeSet<&str> = body
        .lines()
        .filter(|line| line.starts_with("quarry_http_requests_total{"))
        .filter_map(|line| line.split(r#"route=""#).nth(1)?.split('"').next())
        .collect();
    assert!(
        routes.len() <= 10,
        "route label set grew with the traffic: {routes:?}"
    );
}

#[tokio::test]
async fn sampler_exports_lease_pool_gauges() {
    // A value recorded before a recorder exists is dropped, so install first.
    handle();
    let pool = RuntimeLeasePool::new(3);
    metrics::register_pool("sampler_test_pool", pool.clone());
    let _held = pool.try_acquire("affinity-a").expect("lease");
    metrics::sample_pools();

    let addr = spawn_metrics(Some(TOKEN)).await;
    let (_, body) = get(addr, "/metrics", Some(TOKEN)).await;
    let line = |name: &str| {
        body.lines()
            .find(|line| line.starts_with(&format!(r#"{name}{{pool="sampler_test_pool"}}"#)))
            .unwrap_or_else(|| panic!("{name} missing in {body}"))
            .to_owned()
    };
    assert!(line("quarry_lease_pool_capacity").ends_with(" 3"));
    assert!(line("quarry_lease_pool_in_flight").ends_with(" 1"));
    assert!(line("quarry_lease_pool_acquired_total").ends_with(" 1"));
}

#[tokio::test]
async fn scrape_outcomes_are_counted_by_closed_outcome() {
    handle();
    metrics::record_scrape("ok");
    metrics::record_scrape(&metrics::error_label(
        quarry_core::error::ErrorCode::UnsupportedMedia,
    ));
    let addr = spawn_metrics(Some(TOKEN)).await;
    let (_, body) = get(addr, "/metrics", Some(TOKEN)).await;
    assert!(
        body.contains(r#"quarry_scrape_outcomes_total{outcome="ok"}"#),
        "{body}"
    );
    assert!(
        body.contains(r#"quarry_scrape_outcomes_total{outcome="unsupported_media"}"#),
        "{body}"
    );
}
