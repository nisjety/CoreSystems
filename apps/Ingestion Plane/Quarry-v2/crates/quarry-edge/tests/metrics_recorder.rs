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
async fn route_label_is_the_matched_template_not_the_raw_path() {
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
                && line.contains(r#"route="/health""#)
                && line.contains(r#"status_class="2xx""#)),
        "{body}"
    );
    assert!(
        body.contains("quarry_http_request_duration_seconds_bucket"),
        "{body}"
    );
    assert!(
        body.contains(r#"route="unmatched""#),
        "a path no route matches is one label: {body}"
    );
    // The id in the path never becomes a label; the route's own template does.
    assert!(
        body.contains(r#"route="/v1/agent/runs/:run_id/step""#),
        "{body}"
    );
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
        routes.iter().all(|route| *route == "unmatched" || route.starts_with('/')),
        "{routes:?}"
    );
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

#[tokio::test]
async fn agent_run_and_cache_series_use_closed_label_sets() {
    handle();
    metrics::record_run_start("agent", "started");
    metrics::record_run_start("interactive", "resumed");
    // Values outside the closed sets collapse to `other`; they never become labels.
    metrics::record_run_start("org_secret_class_77", "outcome-from-a-caller");
    metrics::record_run_denied("org_run_cap");
    metrics::record_run_denied("https://probe-denied.example/x");
    metrics::record_run_expired("idle", 2);
    metrics::record_run_expired("reason-from-a-caller", 1);
    metrics::record_agent_step("click_ref", true, Duration::from_millis(12));
    metrics::record_agent_step("https://probe-step.example/y", false, Duration::from_millis(3));
    metrics::record_cache("page", "hit");
    metrics::record_cache("search", "miss");
    metrics::record_cache("probe-cache", "probe-outcome");

    let addr = spawn_metrics(Some(TOKEN)).await;
    let (_, body) = get(addr, "/metrics", Some(TOKEN)).await;
    for want in [
        r#"quarry_agent_run_starts_total{class="agent",outcome="started"}"#,
        r#"quarry_agent_run_starts_total{class="interactive",outcome="resumed"}"#,
        r#"quarry_agent_run_starts_total{class="other",outcome="other"}"#,
        r#"quarry_agent_run_denied_total{reason="org_run_cap"}"#,
        r#"quarry_agent_run_denied_total{reason="other"}"#,
        r#"quarry_agent_run_expired_total{reason="idle"}"#,
        r#"quarry_agent_run_expired_total{reason="other"}"#,
        r#"quarry_agent_steps_total{action_class="click",outcome="ok"}"#,
        r#"quarry_agent_steps_total{action_class="other",outcome="error"}"#,
        r#"quarry_agent_step_duration_seconds_bucket{action_class="click",outcome="ok""#,
        r#"quarry_cache_requests_total{cache="page",outcome="hit"}"#,
        r#"quarry_cache_requests_total{cache="search",outcome="miss"}"#,
        r#"quarry_cache_requests_total{cache="other",outcome="other"}"#,
    ] {
        assert!(body.contains(want), "{want} missing in {body}");
    }
    for secret in [
        "org_secret_class_77",
        "outcome-from-a-caller",
        "probe-denied",
        "reason-from-a-caller",
        "probe-step",
        "probe-cache",
        "probe-outcome",
    ] {
        assert!(!body.contains(secret), "{secret} leaked into the exposition");
    }
}

#[tokio::test]
async fn sampler_exports_run_gauges_the_drain_flag_and_pool_refusals() {
    handle();
    metrics::register_run_stats(|| metrics::RunStats {
        interactive: 2,
        agent: 3,
        pending: 1,
    });
    let pool = RuntimeLeasePool::new(1);
    metrics::register_pool("refusal_test_pool", pool.clone());
    let _held = pool.try_acquire("affinity-a").expect("lease");
    assert!(pool.try_acquire("affinity-b").is_err());
    assert!(pool
        .acquire_timeout("affinity-c", Duration::from_millis(20))
        .await
        .is_err());
    metrics::sample_all();

    let addr = spawn_metrics(Some(TOKEN)).await;
    let (_, body) = get(addr, "/metrics", Some(TOKEN)).await;
    let line = |prefix: &str| {
        body.lines()
            .find(|line| line.starts_with(prefix))
            .unwrap_or_else(|| panic!("{prefix} missing in {body}"))
            .to_owned()
    };
    assert!(line(r#"quarry_agent_runs_active{class="interactive"}"#).ends_with(" 2"));
    assert!(line(r#"quarry_agent_runs_active{class="agent"}"#).ends_with(" 3"));
    assert!(line("quarry_agent_runs_pending").ends_with(" 1"));
    // This test binary never begins a drain.
    assert!(line("quarry_edge_draining").ends_with(" 0"));
    assert!(
        line(r#"quarry_lease_pool_refused_total{pool="refusal_test_pool",reason="pool_full"}"#)
            .ends_with(" 1")
    );
    assert!(
        line(r#"quarry_lease_pool_refused_total{pool="refusal_test_pool",reason="wait_timeout"}"#)
            .ends_with(" 1")
    );
    assert!(line(r#"quarry_lease_pool_waits_total{pool="refusal_test_pool"}"#).ends_with(" 1"));
}

#[tokio::test]
async fn sampler_exports_each_robots_decision_under_its_mode_and_outcome() {
    use quarry_runtime::{RobotsEnforceMode, RobotsGate, RobotsGateConfig};
    use support::fixture_site::{FixtureSiteDriver, FIXTURE_A};

    handle();
    // Observe over the fixture site: one disallowed page (would_deny) and one allowed.
    let gate = Arc::new(RobotsGate::new(
        Arc::new(FixtureSiteDriver::standard_site()),
        RobotsGateConfig {
            mode: RobotsEnforceMode::Observe,
            ..RobotsGateConfig::default()
        },
    ));
    for path in ["/private/secret", "/docs/a"] {
        let url: url::Url = format!("{FIXTURE_A}{path}").parse().unwrap();
        let decision = gate.check(&url, "org_metrics", false).await;
        gate.record(RobotsEnforceMode::Observe, &decision);
    }
    metrics::register_robots(gate.clone());
    metrics::sample_all();

    let addr = spawn_metrics(Some(TOKEN)).await;
    let (_, body) = get(addr, "/metrics", Some(TOKEN)).await;
    let line = |prefix: &str| {
        body.lines()
            .find(|line| line.starts_with(prefix))
            .unwrap_or_else(|| panic!("{prefix} missing in {body}"))
            .to_owned()
    };
    assert!(line(r#"quarry_robots_decisions_total{mode="observe",outcome="would_deny"}"#).ends_with(" 1"));
    assert!(line(r#"quarry_robots_decisions_total{mode="any",outcome="allowed"}"#).ends_with(" 1"));
    assert!(line(r#"quarry_robots_decisions_total{mode="observe",outcome="would_unreachable"}"#).ends_with(" 0"));
    assert!(line(r#"quarry_robots_decisions_total{mode="enforce",outcome="denied"}"#).ends_with(" 0"));
    assert!(line("quarry_robots_cache_entries").ends_with(" 1"));
}
