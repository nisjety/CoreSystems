//! Integration tests for CachePolicy honouring in the edge scrape route.
//!
//! These tests require a reachable Redis at `REDIS_URL` (default
//! `redis://127.0.0.1:6379`). When Redis is unavailable the tests print a
//! skip notice and return success so CI without Redis still passes.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use quarry_edge::cache::PageCache;
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};
use redis::AsyncCommands;
use tokio::sync::mpsc;
use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

mod support;

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
    let state = AppState {
        redis: Some(conn.clone()),
        cache: Some(PageCache::new(conn.clone(), Duration::from_secs(3600))),
        ..support::base_state(static_driver, event_sink)
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

/// With `QUARRY_EDGE_AUTH_DEV_BYPASS` every caller is the stub org below, so
/// page-cache entries written by these tests live under that org's key.
const DEV_ORG: &str = "org_placeholder";

async fn purge_key(conn: &mut redis::aio::ConnectionManager, fp: &str) {
    purge_key_for(conn, DEV_ORG, fp).await;
}

async fn purge_key_for(conn: &mut redis::aio::ConnectionManager, org: &str, fp: &str) {
    let key = format!("quarry:page:{org}:{fp}");
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

/// A cached output with a recognisable title, as an org would have stored it.
fn planted_output(url: &str, fingerprint: &str, title: &str) -> quarry_core::output::NormalizedOutput {
    serde_json::from_value(serde_json::json!({
        "run_id": quarry_core::ids::kinds::RunKind::new(),
        "url": { "requested": url, "final_url": url },
        "status": 200,
        "fetched_at": "2026-01-01T00:00:00Z",
        "fingerprint": fingerprint,
        "formats": {},
        "change": { "status": "new" },
        "metadata": { "title": title },
        "driver": { "kind": "static", "duration_ms": 1 }
    }))
    .expect("a minimal NormalizedOutput")
}

/// Tenant isolation: a fingerprint is a hash of the raw page body, so anyone
/// can compute it for a public page. It must never address another org's
/// cached output (final URL, title, artifact refs) through `prev_fingerprint`.
///
/// Entries are planted straight into the cache so the test needs no origin
/// (the SSRF guard refuses a loopback mock origin): a hit returns without
/// fetching, and the miss case targets a refused loopback URL so it never
/// leaves the process.
#[tokio::test]
async fn page_cache_is_not_readable_across_orgs() {
    let Some((state, mut conn)) = build_state().await else {
        eprintln!("skip: REDIS_URL unreachable");
        return;
    };
    let addr = spawn_app(state).await;
    let client = reqwest::Client::new();
    let cache = PageCache::new(conn.clone(), Duration::from_secs(3600));

    let url = "http://127.0.0.1:1/page";
    let own_fp = "fp-own-org-isolation-test";
    let other_fp = "fp-other-org-isolation-test";
    cache
        .put(DEV_ORG, own_fp, &planted_output(url, own_fp, "OWN ORG TITLE"))
        .await
        .expect("plant the dev org entry");
    cache
        .put("org_other", other_fp, &planted_output(url, other_fp, "OTHER ORG TITLE"))
        .await
        .expect("plant the other org entry");

    // The dev org reading its own fingerprint is served from the cache.
    let own = authed(
        client
            .post(format!("http://{addr}/v1/scrape"))
            .json(&serde_json::json!({
                "url": url,
                "prev_fingerprint": own_fp,
                "cache": { "mode": "read_write" }
            })),
    )
    .send()
    .await
    .unwrap();
    assert!(own.status().is_success(), "own-org fingerprint is a hit");
    assert!(own.text().await.unwrap().contains("OWN ORG TITLE"));

    // The dev org presenting the OTHER org's fingerprint must not be served
    // that org's output: the key includes the org, so it is a miss.
    let other = authed(
        client
            .post(format!("http://{addr}/v1/scrape"))
            .json(&serde_json::json!({
                "url": url,
                "prev_fingerprint": other_fp,
                "cache": { "mode": "read_write" }
            })),
    )
    .send()
    .await
    .unwrap();
    let body = other.text().await.unwrap();
    assert!(
        !body.contains("OTHER ORG TITLE"),
        "another org's cached output leaked through prev_fingerprint: {body}"
    );

    // ...and the cache itself agrees at the key level.
    assert!(cache.get("org_other", other_fp).await.is_some());
    assert!(cache.get(DEV_ORG, other_fp).await.is_none());
    assert!(cache.get("org_other", own_fp).await.is_none());

    purge_key(&mut conn, own_fp).await;
    purge_key_for(&mut conn, "org_other", other_fp).await;
}
