//! Integration tests for CachePolicy honouring in the edge scrape route.
//!
//! These tests require a reachable Redis at `REDIS_URL` (default
//! `redis://127.0.0.1:6379`). When Redis is unavailable the tests print a
//! skip notice and return success so CI without Redis still passes.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use quarry_core::output::DriverKind;
use quarry_edge::cache::PageCache;
use quarry_edge::state::AppState;
use quarry_runtime::driver_registry::DriverRegistry;
use quarry_runtime::{artifact_store::InMemoryStore, fetch::StaticDriver, EventSink};
use quarry_security::preflight::DefaultEngine;
use redis::AsyncCommands;
use tokio::sync::mpsc;
use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

const TEST_BEARER: &str = "cache-policy-dev-token";

fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string())
}

async fn try_redis() -> Option<redis::aio::ConnectionManager> {
    let client = redis::Client::open(redis_url()).ok()?;
    tokio::time::timeout(Duration::from_secs(2), client.get_connection_manager())
        .await
        .ok()?
        .ok()
}

async fn build_state() -> Option<(AppState, redis::aio::ConnectionManager)> {
    let conn = try_redis().await?;
    let (tx, mut rx) = mpsc::channel(1024);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let event_sink = EventSink::new(tx);
    let static_driver =
        Arc::new(StaticDriver::new(Duration::from_secs(10), "quarry-test").unwrap());
    let mut drivers = DriverRegistry::new(DriverKind::Static);
    drivers.register(static_driver.clone());
    let state = AppState {
        driver: static_driver,
        drivers,
        http3: None,
        security: Arc::new(DefaultEngine::new().with_allow_private_hosts(true)),
        artifacts: Arc::new(InMemoryStore::new()),
        control_base_url: String::new(),
        redis: Some(conn.clone()),
        cache: Some(PageCache::new(conn.clone(), Duration::from_secs(3600))),
        event_sink,
        ingest: None,
        profiles: Arc::new(quarry_browser::session::InMemoryProfileStore::new()),
        search: None,
        vector_index: None,
        searxng_url: None,
        model_plane_url: None,
        model_plane_token: None,
        answer_pipeline: None,
        local_index: None,
        policy: quarry_runtime::RunPolicy::default(),
        scheduler: None,
        internal_signer: None,
        #[cfg(feature = "postgres-queue")]
        event_history: None,
        usage: std::sync::Arc::new(quarry_runtime::NoopUsageMeter),
        #[cfg(feature = "browser-agent")]
        agent_driver: Arc::new(quarry_browser::chromiumoxide::ChromiumoxideDriver::new()),
        #[cfg(feature = "browser-agent")]
        agent_runs: quarry_edge::agent_routes::new_runs(),
    };
    Some((state, conn))
}

async fn spawn_app(state: AppState) -> SocketAddr {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");

    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

fn authed(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    request.bearer_auth(TEST_BEARER)
}

/// Extract `data.fingerprint` from an envelope `{ data: { fingerprint: "..." } }`.
fn extract_fingerprint(body: &serde_json::Value) -> String {
    body.get("data")
        .and_then(|d| d.get("fingerprint"))
        .and_then(|f| f.as_str())
        .unwrap_or_else(|| panic!("no fingerprint in body: {body}"))
        .to_string()
}

async fn purge_key(conn: &mut redis::aio::ConnectionManager, fp: &str) {
    let key = format!("quarry:page:{fp}");
    let _: redis::RedisResult<i64> = conn.del(&key).await;
}

#[tokio::test]
async fn cache_read_write_serves_from_cache_on_second_call() {
    let Some((state, mut conn)) = build_state().await else {
        eprintln!("skip: REDIS_URL unreachable");
        return;
    };

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("<html><body><h1>hello</h1></body></html>"),
        )
        .mount(&server)
        .await;

    let addr = spawn_app(state).await;
    let client = reqwest::Client::new();

    // First call: populates the cache (cache write keyed by out.fingerprint).
    let resp1 = authed(
        client
            .post(format!("http://{addr}/v1/scrape"))
            .json(&serde_json::json!({
                "url": server.uri(),
                "cache": { "mode": "read_write" }
            })),
    )
    .send()
    .await
    .unwrap();
    assert!(resp1.status().is_success());
    let body1: serde_json::Value = resp1.json().await.unwrap();
    let fp = extract_fingerprint(&body1);

    // Second call: passes the fingerprint as prev_fingerprint to trigger a cache hit.
    let resp2 = authed(
        client
            .post(format!("http://{addr}/v1/scrape"))
            .json(&serde_json::json!({
                "url": server.uri(),
                "prev_fingerprint": fp,
                "cache": { "mode": "read_write" }
            })),
    )
    .send()
    .await
    .unwrap();
    assert!(resp2.status().is_success());

    // Origin should have been hit exactly once: the second request was served from cache.
    let received = server.received_requests().await.unwrap_or_default();
    assert_eq!(
        received.len(),
        1,
        "expected origin hit once, got {} hits",
        received.len()
    );

    purge_key(&mut conn, &fp).await;
}

#[tokio::test]
async fn cache_bypass_always_hits_origin() {
    let Some((state, mut conn)) = build_state().await else {
        eprintln!("skip: REDIS_URL unreachable");
        return;
    };

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("<html><body><p>bypass</p></body></html>"),
        )
        .mount(&server)
        .await;

    let addr = spawn_app(state).await;
    let client = reqwest::Client::new();

    let resp1 = authed(
        client
            .post(format!("http://{addr}/v1/scrape"))
            .json(&serde_json::json!({
                "url": server.uri(),
                "cache": { "mode": "bypass" }
            })),
    )
    .send()
    .await
    .unwrap();
    assert!(resp1.status().is_success());
    let body1: serde_json::Value = resp1.json().await.unwrap();
    let fp = extract_fingerprint(&body1);

    let resp2 = authed(
        client
            .post(format!("http://{addr}/v1/scrape"))
            .json(&serde_json::json!({
                "url": server.uri(),
                "prev_fingerprint": fp,
                "cache": { "mode": "bypass" }
            })),
    )
    .send()
    .await
    .unwrap();
    assert!(resp2.status().is_success());

    let received = server.received_requests().await.unwrap_or_default();
    assert_eq!(
        received.len(),
        2,
        "bypass mode must always hit origin, got {} hits",
        received.len()
    );

    // Bypass should not write either: key must be absent.
    let key = format!("quarry:page:{fp}");
    let exists: bool = conn.exists(&key).await.unwrap_or(false);
    assert!(!exists, "bypass mode must not write to redis");
}

#[tokio::test]
async fn cache_write_only_skips_read_but_populates_cache() {
    let Some((state, mut conn)) = build_state().await else {
        eprintln!("skip: REDIS_URL unreachable");
        return;
    };

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("<html><body><span>write</span></body></html>"),
        )
        .mount(&server)
        .await;

    let addr = spawn_app(state).await;
    let client = reqwest::Client::new();

    let resp1 = authed(
        client
            .post(format!("http://{addr}/v1/scrape"))
            .json(&serde_json::json!({
                "url": server.uri(),
                "cache": { "mode": "write_only" }
            })),
    )
    .send()
    .await
    .unwrap();
    assert!(resp1.status().is_success());
    let body1: serde_json::Value = resp1.json().await.unwrap();
    let fp = extract_fingerprint(&body1);

    // Key should now exist in Redis (write happened).
    let key = format!("quarry:page:{fp}");
    let exists: bool = conn.exists(&key).await.unwrap_or(false);
    assert!(exists, "write_only mode must populate redis");

    // Second call passes prev_fingerprint but write_only must not read from cache.
    let resp2 = authed(
        client
            .post(format!("http://{addr}/v1/scrape"))
            .json(&serde_json::json!({
                "url": server.uri(),
                "prev_fingerprint": fp,
                "cache": { "mode": "write_only" }
            })),
    )
    .send()
    .await
    .unwrap();
    assert!(resp2.status().is_success());

    let received = server.received_requests().await.unwrap_or_default();
    assert_eq!(
        received.len(),
        2,
        "write_only must not serve from cache, got {} hits",
        received.len()
    );

    purge_key(&mut conn, &fp).await;
}

#[tokio::test]
async fn cache_max_age_zero_uses_default_ttl() {
    // Sanity: a max_age_s of 0 should fall back to DEFAULT_TTL_SECS (3600),
    // i.e. the key remains alive long enough to be readable on a follow-up.
    let Some((state, mut conn)) = build_state().await else {
        eprintln!("skip: REDIS_URL unreachable");
        return;
    };

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html><body>ttl</body></html>"))
        .mount(&server)
        .await;

    let addr = spawn_app(state).await;
    let client = reqwest::Client::new();

    let resp1 = authed(
        client
            .post(format!("http://{addr}/v1/scrape"))
            .json(&serde_json::json!({
                "url": server.uri(),
                "cache": { "mode": "read_write", "max_age_s": 0 }
            })),
    )
    .send()
    .await
    .unwrap();
    assert!(resp1.status().is_success());
    let body1: serde_json::Value = resp1.json().await.unwrap();
    let fp = extract_fingerprint(&body1);

    let key = format!("quarry:page:{fp}");
    let ttl: i64 = conn.ttl(&key).await.unwrap_or(-2);
    assert!(
        ttl > 60,
        "expected default ttl ~3600s when max_age_s=0, got {ttl}"
    );

    purge_key(&mut conn, &fp).await;
}
