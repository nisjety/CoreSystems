//! Integration tests for CachePolicy honouring in the edge scrape route.
//!
//! These tests require a reachable Redis at `REDIS_URL` (default
//! `redis://127.0.0.1:6379`). When Redis is unavailable the tests print a
//! skip notice and return success, so a job without Redis still passes. A job
//! that provides a Redis sets `REQUIRE_CONTRACT=1`; then an unreachable Redis is
//! a failure, as in `zdr_claim_floor.rs`. `quarry-v2-ci.yml` runs these cases with
//! a Redis service and `REQUIRE_CONTRACT=1` in its "Page-cache and ZDR cache gates"
//! step. Its `Test workspace` step has no Redis at the default address, so there they
//! skip.
//!
//! The origin is a loopback wiremock server. The production static driver refuses
//! loopback targets, so these tests fetch through `LoopbackDriver` instead.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use quarry_edge::cache::PageCache;
use quarry_edge::state::AppState;
use quarry_runtime::driver::Driver;
use quarry_runtime::fetch::FetchResponse;
use quarry_runtime::EventSink;
use redis::AsyncCommands;
use tokio::sync::mpsc;
use url::Url;
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

/// The origin in these tests is a loopback wiremock server, and the production
/// static driver refuses loopback targets (its address guard, `resolve_public_url`
/// in `quarry-runtime/src/dns_guard.rs`, which `fetch.rs` calls). This driver fetches
/// the origin over plain HTTP without that guard, so what these tests exercise is the
/// page cache at the edge. The origin still records every request it receives.
#[derive(Default)]
struct LoopbackDriver;

#[async_trait]
impl Driver for LoopbackDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        let started = std::time::Instant::now();
        let response = reqwest::get(url.clone()).await.map_err(|error| {
            quarry_core::error::QuarryError::new(
                quarry_core::error::ErrorCode::DriverFailed,
                format!("loopback fetch: {}", error.without_url()),
            )
        })?;
        let status = response.status().as_u16();
        let final_url = response.url().clone();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.to_string(), value.to_owned()))
            })
            .collect();
        let body = response
            .bytes()
            .await
            .map_err(|error| {
                quarry_core::error::QuarryError::new(
                    quarry_core::error::ErrorCode::DriverFailed,
                    format!("loopback body: {}", error.without_url()),
                )
            })?
            .to_vec();
        Ok(FetchResponse {
            status,
            final_url,
            headers,
            body,
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            served_by: DriverKind::Static,
            captures: None,
        })
    }
}

async fn build_state() -> Option<(AppState, redis::aio::ConnectionManager)> {
    let Some(conn) = try_redis().await else {
        // A guard that quietly skips is not a guard (see the module comment).
        assert!(
            std::env::var_os("REQUIRE_CONTRACT").is_none(),
            "REQUIRE_CONTRACT is set but REDIS_URL is unreachable: the page cache guards \
             cannot be skipped"
        );
        return None;
    };
    let (tx, mut rx) = mpsc::channel(1024);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let event_sink = EventSink::new(tx);
    let driver = Arc::new(LoopbackDriver);
    let state = AppState {
        redis: Some(conn.clone()),
        cache: Some(PageCache::new(conn.clone(), Duration::from_secs(3600))),
        ..support::base_state(driver, event_sink)
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
    let key = page_key(DEV_ORG, &fp);
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
    let key = page_key(DEV_ORG, &fp);
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

    let key = page_key(DEV_ORG, &fp);
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

/// `quarry:page:{org}:{fingerprint}`, the key `PageCache` writes.
fn page_key(org: &str, fingerprint: &str) -> String {
    format!("quarry:page:{org}:{fingerprint}")
}

/// A value unique to this run, so the pages these tests serve never share a
/// fingerprint with another test or an earlier run.
fn unique_nonce() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    format!("{}-{nanos}", std::process::id())
}

/// A page whose title and body carry `nonce`.
fn nonce_page(nonce: &str) -> String {
    format!(
        "<html><head><title>Cache options {nonce}</title></head><body><main><p>Cache policy page {nonce} is long enough to read as page content.</p></main></body></html>"
    )
}

/// A scrape with options other than the defaults must neither read the page cache
/// through `prev_fingerprint` nor write to it: a cached entry is a default-options
/// result. The planted entry is never served, the origin is always fetched, and
/// nothing is written under the fingerprint of the new result.
#[tokio::test]
async fn non_default_options_neither_read_nor_write_the_page_cache() {
    let Some((state, mut conn)) = build_state().await else {
        eprintln!("skip: REDIS_URL unreachable");
        return;
    };
    let addr = spawn_app(state).await;
    let client = reqwest::Client::new();
    let cache = PageCache::new(conn.clone(), Duration::from_secs(3600));

    // An entry a default request would serve, with a title no real page has.
    let planted_fp = format!("fp-non-default-planted-{}", unique_nonce());
    let url = "http://127.0.0.1:1/planted";
    cache
        .put(DEV_ORG, &planted_fp, &planted_output(url, &planted_fp, "PLANTED TITLE"))
        .await
        .expect("plant the entry");

    let cases = [
        ("formats", serde_json::json!({ "formats": ["markdown"] })),
        ("include_tags", serde_json::json!({ "include_tags": ["main"] })),
        ("exclude_tags", serde_json::json!({ "exclude_tags": [".no-such-element"] })),
        ("extractor", serde_json::json!({ "extractor": "raw" })),
        ("only_main_content", serde_json::json!({ "only_main_content": false })),
        ("remove_base64_images", serde_json::json!({ "remove_base64_images": true })),
    ];
    for (label, options) in cases {
        let nonce = unique_nonce();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(nonce_page(&nonce)))
            .mount(&server)
            .await;

        let mut body = serde_json::json!({
            "url": server.uri(),
            "prev_fingerprint": planted_fp,
            "cache": { "mode": "read_write" }
        });
        body.as_object_mut()
            .unwrap()
            .extend(options.as_object().unwrap().clone());
        let response = authed(client.post(format!("http://{addr}/v1/scrape")).json(&body))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{label}");
        let text = response.text().await.unwrap();
        assert!(!text.contains("PLANTED TITLE"), "{label}: the planted entry was read: {text}");
        let reply: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(reply["data"]["effective_options"]["cache_state"], "bypass", "{label}");

        let received = server.received_requests().await.unwrap_or_default();
        assert_eq!(received.len(), 1, "{label}: the origin is fetched, got {} hits", received.len());

        let fp = extract_fingerprint(&reply);
        let written: bool = conn.exists(page_key(DEV_ORG, &fp)).await.unwrap_or(false);
        assert!(!written, "{label}: a non-default result must not be written to the cache");
    }

    purge_key(&mut conn, &planted_fp).await;
}

/// A `signals` object with every field the contract requires (`DriverSignals` has no
/// serde defaults), with `overrides` applied.
fn driver_signals(overrides: serde_json::Value) -> serde_json::Value {
    let mut signals = serde_json::json!({
        "actions": [],
        "screenshot": false,
        "pdf": false,
        "prior_block_signals": 0,
        "profile_required": false,
        "url_type": "Default"
    });
    signals
        .as_object_mut()
        .expect("signals is an object")
        .extend(overrides.as_object().expect("overrides is an object").clone());
    signals
}

/// Each signal that selects the browser driver needs a render the page cache does not
/// hold, so a request carrying one neither reads nor writes the cache: the planted
/// entry is not served, the origin is fetched, and the new result is not written.
/// Without the plan check in the `routes.rs` gate, the cached static result would
/// answer the request and its browser-only output would be absent.
/// The signals are the ones `plan_from_signals` (quarry-runtime/src/driver_plan.rs)
/// routes to the browser: a non-empty `actions` list, `screenshot`, `pdf`, and
/// `profile_required`, plus `prior_block_signals` of 2 or more only under
/// `QUARRY_BLOCK_ESCALATION=legacy` (WS3.I: under `honest` the field is ignored, the
/// plan is static, and the page cache applies like for any default scrape). Each has a
/// case below; the `prior_block_signals` case is present only when the process runs
/// in legacy mode, because the cache gate follows the plan.
#[tokio::test]
async fn browser_signals_neither_read_nor_write_the_page_cache() {
    let Some((state, mut conn)) = build_state().await else {
        eprintln!("skip: REDIS_URL unreachable");
        return;
    };
    let addr = spawn_app(state).await;
    let client = reqwest::Client::new();
    let cache = PageCache::new(conn.clone(), Duration::from_secs(3600));

    let planted_fp = format!("fp-browser-signals-planted-{}", unique_nonce());
    let url = "http://127.0.0.1:1/browser-signals";
    cache
        .put(
            DEV_ORG,
            &planted_fp,
            &planted_output(url, &planted_fp, "PLANTED SIGNALS TITLE"),
        )
        .await
        .expect("plant the entry");

    let mut cases = vec![
        ("screenshot", driver_signals(serde_json::json!({ "screenshot": true }))),
        ("pdf", driver_signals(serde_json::json!({ "pdf": true }))),
        ("actions", driver_signals(serde_json::json!({ "actions": ["click"] }))),
        (
            "profile_required",
            driver_signals(serde_json::json!({ "profile_required": true })),
        ),
    ];
    if !quarry_runtime::escalation::EscalationMode::current().is_honest() {
        cases.push((
            "prior_block_signals 2",
            driver_signals(serde_json::json!({ "prior_block_signals": 2 })),
        ));
    }
    for (label, signals) in cases {
        let nonce = unique_nonce();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(nonce_page(&nonce)))
            .mount(&server)
            .await;

        let body = serde_json::json!({
            "url": server.uri(),
            "prev_fingerprint": planted_fp,
            "cache": { "mode": "read_write" },
            "signals": signals
        });
        let response = authed(client.post(format!("http://{addr}/v1/scrape")).json(&body))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{label}");
        let text = response.text().await.unwrap();
        assert!(
            !text.contains("PLANTED SIGNALS TITLE"),
            "{label}: the planted entry was read: {text}"
        );
        let reply: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            reply["data"]["effective_options"]["cache_state"], "bypass",
            "{label}"
        );

        let received = server.received_requests().await.unwrap_or_default();
        assert_eq!(
            received.len(),
            1,
            "{label}: the origin is fetched, got {} hits",
            received.len()
        );

        let fp = extract_fingerprint(&reply);
        let written: bool = conn.exists(page_key(DEV_ORG, &fp)).await.unwrap_or(false);
        assert!(
            !written,
            "{label}: a browser-signal result must not be written to the cache"
        );
    }

    purge_key(&mut conn, &planted_fp).await;
}

/// The other half of the `prior_block_signals 2` case above, for `honest` (the
/// default since WS3.I): the field is ignored, the plan is static, and the request is
/// an ordinary default scrape for the cache. A planted entry is served on its
/// fingerprint without a fetch, and a miss fetches and writes the result. Skipped in
/// `legacy` mode, where the same request is a browser request and bypasses the cache.
#[tokio::test]
async fn honest_prior_block_signals_do_not_bypass_the_page_cache() {
    if !quarry_runtime::escalation::EscalationMode::current().is_honest() {
        eprintln!("skip: QUARRY_BLOCK_ESCALATION=legacy (the bypass is asserted above)");
        return;
    }
    let Some((state, mut conn)) = build_state().await else {
        eprintln!("skip: REDIS_URL unreachable");
        return;
    };
    let addr = spawn_app(state).await;
    let client = reqwest::Client::new();
    let cache = PageCache::new(conn.clone(), Duration::from_secs(3600));

    let planted_fp = format!("fp-honest-prior-block-planted-{}", unique_nonce());
    cache
        .put(
            DEV_ORG,
            &planted_fp,
            &planted_output(
                "http://127.0.0.1:1/honest-prior-block",
                &planted_fp,
                "PLANTED HONEST TITLE",
            ),
        )
        .await
        .expect("plant the entry");

    let nonce = unique_nonce();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(nonce_page(&nonce)))
        .mount(&server)
        .await;
    let signals = driver_signals(serde_json::json!({ "prior_block_signals": 2 }));

    let hit = authed(
        client
            .post(format!("http://{addr}/v1/scrape"))
            .json(&serde_json::json!({
                "url": server.uri(),
                "prev_fingerprint": planted_fp,
                "cache": { "mode": "read_write" },
                "signals": signals
            })),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(hit.status(), 200);
    let hit: serde_json::Value = hit.json().await.unwrap();
    assert_eq!(hit["data"]["effective_options"]["cache_state"], "hit");
    assert!(hit["data"]["metadata"]["title"]
        .as_str()
        .unwrap_or_default()
        .contains("PLANTED HONEST TITLE"));
    assert_eq!(
        server.received_requests().await.unwrap_or_default().len(),
        0,
        "served from the cache, no fetch"
    );

    let miss = authed(
        client
            .post(format!("http://{addr}/v1/scrape"))
            .json(&serde_json::json!({
                "url": server.uri(),
                "prev_fingerprint": "fp-not-in-the-cache",
                "cache": { "mode": "read_write" },
                "signals": signals
            })),
    )
    .send()
    .await
    .unwrap();
    let miss: serde_json::Value = miss.json().await.unwrap();
    assert_eq!(miss["data"]["effective_options"]["cache_state"], "miss");
    assert_eq!(
        server.received_requests().await.unwrap_or_default().len(),
        1
    );
    let fp = extract_fingerprint(&miss);
    let written: bool = conn.exists(page_key(DEV_ORG, &fp)).await.unwrap_or(false);
    assert!(
        written,
        "the result of a default-plan request is written to the cache"
    );

    purge_key(&mut conn, &planted_fp).await;
    purge_key(&mut conn, &fp).await;
}

/// Default options keep the page cache: a result is written under its fingerprint,
/// a known fingerprint is served from the cache without a fetch, and an unknown one
/// is a miss that fetches.
#[tokio::test]
async fn default_options_still_read_and_write_the_page_cache() {
    let Some((state, mut conn)) = build_state().await else {
        eprintln!("skip: REDIS_URL unreachable");
        return;
    };
    let addr = spawn_app(state).await;
    let client = reqwest::Client::new();

    let nonce = unique_nonce();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(nonce_page(&nonce)))
        .mount(&server)
        .await;

    let first: serde_json::Value = authed(
        client
            .post(format!("http://{addr}/v1/scrape"))
            .json(&serde_json::json!({ "url": server.uri(), "cache": { "mode": "read_write" } })),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    let fp = extract_fingerprint(&first);
    let written: bool = conn.exists(page_key(DEV_ORG, &fp)).await.unwrap_or(false);
    assert!(written, "a default result is written under its fingerprint");

    let hit = authed(
        client
            .post(format!("http://{addr}/v1/scrape"))
            .json(&serde_json::json!({ "url": server.uri(), "prev_fingerprint": fp, "cache": { "mode": "read_write" } })),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(hit.status(), 200);
    let hit: serde_json::Value = hit.json().await.unwrap();
    assert_eq!(hit["data"]["effective_options"]["cache_state"], "hit");
    assert!(hit["data"]["metadata"]["title"].as_str().unwrap_or_default().contains(&nonce));
    assert_eq!(server.received_requests().await.unwrap_or_default().len(), 1, "served from cache");

    let unknown = authed(
        client
            .post(format!("http://{addr}/v1/scrape"))
            .json(&serde_json::json!({ "url": server.uri(), "prev_fingerprint": "fp-not-in-the-cache", "cache": { "mode": "read_write" } })),
    )
    .send()
    .await
    .unwrap();
    let unknown: serde_json::Value = unknown.json().await.unwrap();
    assert_eq!(unknown["data"]["effective_options"]["cache_state"], "miss");
    assert_eq!(server.received_requests().await.unwrap_or_default().len(), 2, "a miss fetches");

    purge_key(&mut conn, &fp).await;
}

/// ZDR never reads or writes the page cache, even for default options and a known
/// fingerprint: the planted entry is not served and the result is not stored.
#[tokio::test]
async fn zdr_never_reads_or_writes_the_page_cache() {
    let Some((state, mut conn)) = build_state().await else {
        eprintln!("skip: REDIS_URL unreachable");
        return;
    };
    let addr = spawn_app(state).await;
    let client = reqwest::Client::new();
    let cache = PageCache::new(conn.clone(), Duration::from_secs(3600));

    let planted_fp = format!("fp-zdr-planted-{}", unique_nonce());
    cache
        .put(
            DEV_ORG,
            &planted_fp,
            &planted_output("http://127.0.0.1:1/zdr", &planted_fp, "PLANTED ZDR TITLE"),
        )
        .await
        .expect("plant the entry");

    let nonce = unique_nonce();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(nonce_page(&nonce)))
        .mount(&server)
        .await;

    let response = authed(
        client
            .post(format!("http://{addr}/v1/scrape"))
            .json(&serde_json::json!({
                "url": server.uri(),
                "zdr": true,
                "prev_fingerprint": planted_fp,
                "cache": { "mode": "read_write" }
            })),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(response.status(), 200);
    let text = response.text().await.unwrap();
    assert!(!text.contains("PLANTED ZDR TITLE"), "a ZDR scrape read the cache: {text}");
    let reply: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(reply["data"]["effective_options"]["cache_state"], "bypass");
    assert_eq!(server.received_requests().await.unwrap_or_default().len(), 1);

    let fp = extract_fingerprint(&reply);
    let written: bool = conn.exists(page_key(DEV_ORG, &fp)).await.unwrap_or(false);
    assert!(!written, "a ZDR result must not be written to the cache");

    purge_key(&mut conn, &planted_fp).await;
}
