//! INNH-7: `max_age` on `POST /v1/scrape`, through the real router, with the
//! switch ON.
//!
//! `QUARRY_EDGE__SCRAPE_MAX_AGE` is read once per process, so this binary turns it
//! on before the first request. `tests/scrape_max_age_off.rs` runs with the
//! committed default (off), and `tests/cache_policy.rs` (switch off) pins that a
//! request without `max_age` behaves as before.
//!
//! Most cases need a reachable Redis at `REDIS_URL` (default `redis://127.0.0.1:6379`)
//! and skip without one; with `REQUIRE_CONTRACT=1` an unreachable Redis is a failure,
//! as in `cache_policy.rs`. The case for a stack with no cache at all needs no Redis.
//!
//! The origin is a fake static driver that serves one page and counts its fetches,
//! so "served without a fetch" is a count that did not move. Every URL carries a
//! nonce, so cases never share a cache key, and an entry is planted straight into
//! Redis (with a chosen `fetched_at`) where a case needs one of a given age.
//!
//! This binary runs on the auth dev bypass: every caller is the stub org `DEV_ORG`.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Once};
use std::time::Duration;

use async_trait::async_trait;
use quarry_core::output::{DriverKind, NormalizedOutput};
use quarry_core::scrape::{ScrapeLimits, ScrapeOptions};
use quarry_core::QuarryResult;
use quarry_edge::cache::{CachedScrape, PageCache};
use quarry_edge::state::AppState;
use quarry_runtime::driver::Driver;
use quarry_runtime::fetch::FetchResponse;
use quarry_runtime::EventSink;
use redis::AsyncCommands;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use url::Url;

mod support;

const TEST_BEARER: &str = "scrape-max-age-dev-token";
const DEV_ORG: &str = "org_placeholder";

fn init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::env::set_var("QUARRY_EDGE__SCRAPE_MAX_AGE", "on");
        std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    });
}

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

fn unique_nonce() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    format!("{}-{nanos}", std::process::id())
}

fn page(nonce: &str) -> String {
    format!(
        "<html><head><title>Max age {nonce}</title></head><body><nav>menu</nav><main><p>Max age page {nonce} is long enough to read as page content.</p></main><footer>foot</footer></body></html>"
    )
}

/// A static driver that serves one page with one status and counts its fetches.
struct PageDriver {
    status: u16,
    body: String,
    fetches: Arc<AtomicUsize>,
}

#[async_trait]
impl Driver for PageDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        Ok(FetchResponse {
            status: self.status,
            final_url: url.clone(),
            headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
            body: self.body.clone().into_bytes(),
            duration_ms: 2,
            served_by: DriverKind::Static,
            captures: None,
        })
    }
}

struct Edge {
    addr: SocketAddr,
    fetches: Arc<AtomicUsize>,
    nonce: String,
    /// A URL no other case uses.
    url: String,
    conn: Option<redis::aio::ConnectionManager>,
}

impl Edge {
    fn fetches(&self) -> usize {
        self.fetches.load(Ordering::SeqCst)
    }
}

/// `redis: true` wires the page cache to Redis and returns `None` when none is
/// reachable (the case skips); `redis: false` is a stack with no cache.
async fn spawn_edge(redis: bool, status: u16) -> Option<Edge> {
    spawn_edge_serving(redis, status, None).await
}

/// Like [`spawn_edge`], serving `body` (with the nonce in a comment so the page stays
/// unique) instead of the ordinary page when one is given.
async fn spawn_edge_serving(redis: bool, status: u16, body: Option<&'static str>) -> Option<Edge> {
    init();
    let conn = if redis {
        let Some(conn) = try_redis().await else {
            assert!(
                std::env::var_os("REQUIRE_CONTRACT").is_none(),
                "REQUIRE_CONTRACT is set but REDIS_URL is unreachable: the max_age guards cannot be skipped"
            );
            eprintln!("skip: REDIS_URL unreachable");
            return None;
        };
        Some(conn)
    } else {
        None
    };
    let (tx, mut rx) = mpsc::channel(256);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let nonce = unique_nonce();
    let fetches = Arc::new(AtomicUsize::new(0));
    let driver = Arc::new(PageDriver {
        status,
        body: match body {
            Some(fixed) => format!("{fixed}<!-- {nonce} -->"),
            None => page(&nonce),
        },
        fetches: fetches.clone(),
    });
    let state = AppState {
        redis: conn.clone(),
        cache: conn
            .clone()
            .map(|conn| PageCache::new(conn, Duration::from_secs(3600))),
        #[cfg(feature = "browser-agent")]
        agent_driver: support::governed_agent_driver(),
        ..support::base_state(driver, EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Some(Edge {
        url: format!("http://127.0.0.1:1/max-age/{nonce}"),
        addr,
        fetches,
        nonce,
        conn,
    })
}

async fn post(edge: &Edge, body: Value) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("http://{}/v1/scrape", edge.addr))
        .bearer_auth(TEST_BEARER)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let text = response.text().await.unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

/// The options key a request with these options resolves to (what the edge keys on).
fn options_key(options: Value, zdr: bool) -> String {
    let limits = ScrapeLimits {
        max_age_enabled: true,
        ..ScrapeLimits::default()
    };
    ScrapeOptions::from_value(&options)
        .expect("options parse")
        .resolve(zdr, &limits)
        .expect("options resolve")
        .canonical_options_key()
}

fn key_for(org: &str, url: &str, options: Value) -> String {
    PageCache::result_key(org, url, &options_key(options, false)).expect("a key")
}

/// An entry as the cache would hold it: a minimal output with a recognisable title,
/// fetched `ago_ms` ago, with no artifacts.
fn planted(url: &str, title: &str, ago_ms: i64) -> CachedScrape {
    let fetched_at = chrono::Utc::now() - chrono::Duration::milliseconds(ago_ms);
    let output: NormalizedOutput = serde_json::from_value(json!({
        "run_id": quarry_core::ids::kinds::RunKind::new(),
        "url": { "requested": url, "final_url": url },
        "status": 200,
        "fetched_at": fetched_at.to_rfc3339(),
        "fingerprint": format!("fp-planted-{title}"),
        "formats": {},
        "change": { "status": "new" },
        "metadata": { "title": title },
        "driver": { "kind": "static", "duration_ms": 1 }
    }))
    .expect("a minimal NormalizedOutput");
    CachedScrape { output, warnings: Vec::new() }
}

async fn plant(edge: &Edge, key: &str, entry: &CachedScrape) {
    let cache = PageCache::new(edge.conn.clone().expect("redis"), Duration::from_secs(3600));
    cache.put_result(key, entry, 600).await.expect("plant the entry");
}

async fn stored(edge: &Edge, key: &str) -> Option<CachedScrape> {
    let mut conn = edge.conn.clone().expect("redis");
    let raw: Option<String> = conn.get(key).await.ok()?;
    serde_json::from_str(&raw?).ok()
}

async fn exists(edge: &Edge, key: &str) -> bool {
    let mut conn = edge.conn.clone().expect("redis");
    conn.exists(key).await.unwrap_or(false)
}

async fn purge(edge: &Edge, keys: &[&str]) {
    let mut conn = edge.conn.clone().expect("redis");
    for key in keys {
        let _: redis::RedisResult<i64> = conn.del(*key).await;
    }
}

fn state_of(body: &Value) -> &str {
    body["data"]["effective_options"]["cache_state"].as_str().unwrap_or("")
}

fn warning_codes(body: &Value) -> Vec<String> {
    body["data"]["warnings"]
        .as_array()
        .map(|items| items.iter().filter_map(|w| w["code"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

// ---- a request within the window is served without a fetch ----------------

#[tokio::test]
async fn a_second_request_within_max_age_is_served_without_a_fetch() {
    let Some(edge) = spawn_edge(true, 200).await else { return };
    let request = json!({
        "url": edge.url,
        "formats": ["markdown", "html", "rawHtml"],
        "max_age": 60_000
    });
    let key = key_for(DEV_ORG, &edge.url, json!({ "formats": ["markdown", "html", "rawHtml"] }));

    let (status, first) = post(&edge, request.clone()).await;
    assert_eq!(status, 200, "{first}");
    assert_eq!(state_of(&first), "miss", "{first}");
    assert_eq!(first["data"]["effective_options"]["max_age"], 60_000);
    assert!(first["data"]["effective_options"].get("cached_at").is_none());
    assert_eq!(edge.fetches(), 1);
    assert!(exists(&edge, &key).await, "a cacheable success is written under the URL key");

    let (status, second) = post(&edge, request).await;
    assert_eq!(status, 200, "{second}");
    assert_eq!(state_of(&second), "hit", "{second}");
    assert_eq!(edge.fetches(), 1, "the second request did not fetch");

    // The hit says where it came from and how old it is...
    let options = &second["data"]["effective_options"];
    let age = options["cache_age_ms"].as_u64().expect("cache_age_ms");
    assert!(age < 60_000, "{age}");
    // ...and it is the ORIGINAL fetch time, not the time of the hit.
    assert_eq!(second["data"]["fetched_at"], first["data"]["fetched_at"]);
    let cached_at = chrono::DateTime::parse_from_rfc3339(options["cached_at"].as_str().expect("cached_at")).unwrap();
    let fetched_at = chrono::DateTime::parse_from_rfc3339(first["data"]["fetched_at"].as_str().unwrap()).unwrap();
    assert_eq!(cached_at, fetched_at);
    // A hit makes no new record.
    assert!(first["data"]["scrape_id"].is_string());
    assert!(second["data"].get("scrape_id").is_none());

    // The inline text is rebuilt from the stored artifacts and matches the first answer.
    assert_eq!(second["data"]["markdown"], first["data"]["markdown"]);
    assert_eq!(second["data"]["html"], first["data"]["html"]);
    assert_eq!(second["data"]["raw_html"], first["data"]["raw_html"]);
    assert!(second["data"]["markdown"].as_str().unwrap().contains(&edge.nonce));
    assert!(second["data"]["raw_html"].as_str().unwrap().contains("<footer>foot</footer>"));
    assert_eq!(second["data"]["fingerprint"], first["data"]["fingerprint"]);

    purge(&edge, &[&key]).await;
}

#[tokio::test]
async fn max_age_can_ride_the_camel_case_spelling_and_a_cache_policy_without_a_mode() {
    let Some(edge) = spawn_edge(true, 200).await else { return };
    let key = key_for(DEV_ORG, &edge.url, json!({}));
    // `cache` with only max_age_s used to be a 400 (the mode was required); it is now
    // read_write, and max_age_s is the TTL of the entry this request writes.
    let (status, first) = post(&edge, json!({ "url": edge.url, "maxAge": 60_000, "cache": { "max_age_s": 120 } })).await;
    assert_eq!(status, 200, "{first}");
    let mut conn = edge.conn.clone().unwrap();
    let ttl: i64 = conn.ttl(&key).await.unwrap_or(-2);
    assert!((60..=120).contains(&ttl), "the entry lives for cache.max_age_s, got {ttl}");
    let (_, second) = post(&edge, json!({ "url": edge.url, "maxAge": 60_000 })).await;
    assert_eq!(state_of(&second), "hit", "{second}");
    assert_eq!(edge.fetches(), 1);
    purge(&edge, &[&key]).await;
}

// ---- freshness ------------------------------------------------------------

#[tokio::test]
async fn an_entry_older_than_max_age_is_refetched_and_replaced() {
    let Some(edge) = spawn_edge(true, 200).await else { return };
    let key = key_for(DEV_ORG, &edge.url, json!({}));
    plant(&edge, &key, &planted(&edge.url, "PLANTED STALE TITLE", 10_000)).await;

    let (status, body) = post(&edge, json!({ "url": edge.url, "max_age": 1_000 })).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(state_of(&body), "miss", "{body}");
    assert!(!body.to_string().contains("PLANTED STALE TITLE"));
    assert_eq!(edge.fetches(), 1, "a stale entry is refetched");

    let replaced = stored(&edge, &key).await.expect("an entry");
    assert!(replaced.output.metadata.title.unwrap_or_default().contains(&edge.nonce));

    // The same planted age is fresh under a wider window.
    plant(&edge, &key, &planted(&edge.url, "PLANTED FRESH TITLE", 10_000)).await;
    let (_, body) = post(&edge, json!({ "url": edge.url, "max_age": 30_000 })).await;
    assert_eq!(state_of(&body), "hit", "{body}");
    assert!(body.to_string().contains("PLANTED FRESH TITLE"));
    assert_eq!(edge.fetches(), 1, "a fresh planted entry is served");
    let age = body["data"]["effective_options"]["cache_age_ms"].as_u64().unwrap();
    assert!((10_000..30_000).contains(&age), "{age}");

    purge(&edge, &[&key]).await;
}

#[tokio::test]
async fn max_age_zero_never_reads_but_still_writes() {
    let Some(edge) = spawn_edge(true, 200).await else { return };
    let key = key_for(DEV_ORG, &edge.url, json!({}));
    plant(&edge, &key, &planted(&edge.url, "PLANTED ZERO TITLE", 100)).await;

    let (status, body) = post(&edge, json!({ "url": edge.url, "max_age": 0 })).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(state_of(&body), "bypass", "{body}");
    assert!(!body.to_string().contains("PLANTED ZERO TITLE"));
    assert_eq!(edge.fetches(), 1);
    let replaced = stored(&edge, &key).await.expect("an entry");
    assert!(replaced.output.metadata.title.unwrap_or_default().contains(&edge.nonce), "it still wrote");

    purge(&edge, &[&key]).await;
}

#[tokio::test]
async fn a_request_without_max_age_never_reads_the_url_cache() {
    let Some(edge) = spawn_edge(true, 200).await else { return };
    let key = key_for(DEV_ORG, &edge.url, json!({}));
    plant(&edge, &key, &planted(&edge.url, "PLANTED ABSENT TITLE", 100)).await;

    let (status, body) = post(&edge, json!({ "url": edge.url })).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(state_of(&body), "bypass", "the response is what it was before max_age existed: {body}");
    assert!(!body.to_string().contains("PLANTED ABSENT TITLE"));
    assert!(body["data"]["effective_options"].get("max_age").is_none());
    assert_eq!(edge.fetches(), 1);
    assert!(warning_codes(&body).is_empty());

    purge(&edge, &[&key]).await;
}

// ---- keys: options, orgs --------------------------------------------------

#[tokio::test]
async fn different_options_are_different_entries() {
    let Some(edge) = spawn_edge(true, 200).await else { return };
    let main = json!({ "url": edge.url, "formats": ["markdown"], "include_tags": ["main"], "max_age": 60_000 });
    let para = json!({ "url": edge.url, "formats": ["markdown"], "include_tags": ["p"], "max_age": 60_000 });
    let raw = json!({ "url": edge.url, "formats": ["markdown"], "extractor": "raw", "only_main_content": false, "max_age": 60_000 });

    assert_eq!(state_of(&post(&edge, main.clone()).await.1), "miss");
    assert_eq!(state_of(&post(&edge, para.clone()).await.1), "miss", "another selector is another entry");
    assert_eq!(state_of(&post(&edge, raw.clone()).await.1), "miss", "another extractor is another entry");
    assert_eq!(edge.fetches(), 3);
    assert_eq!(state_of(&post(&edge, main).await.1), "hit");
    assert_eq!(state_of(&post(&edge, para).await.1), "hit");
    assert_eq!(state_of(&post(&edge, raw).await.1), "hit");
    assert_eq!(edge.fetches(), 3, "each option set hit its own entry");

    let keys = [
        key_for(DEV_ORG, &edge.url, json!({ "formats": ["markdown"], "include_tags": ["main"] })),
        key_for(DEV_ORG, &edge.url, json!({ "formats": ["markdown"], "include_tags": ["p"] })),
        key_for(DEV_ORG, &edge.url, json!({ "formats": ["markdown"], "extractor": "raw", "only_main_content": false })),
    ];
    purge(&edge, &[&keys[0], &keys[1], &keys[2]]).await;
}

#[tokio::test]
async fn another_orgs_entry_is_never_served_and_never_overwritten() {
    let Some(edge) = spawn_edge(true, 200).await else { return };
    let own = key_for(DEV_ORG, &edge.url, json!({}));
    let other = key_for("org_other", &edge.url, json!({}));
    assert_ne!(own, other);
    plant(&edge, &other, &planted(&edge.url, "OTHER ORG TITLE", 100)).await;

    let (status, body) = post(&edge, json!({ "url": edge.url, "max_age": 60_000 })).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(state_of(&body), "miss", "{body}");
    assert!(!body.to_string().contains("OTHER ORG TITLE"), "org B's entry reached org A");
    assert_eq!(edge.fetches(), 1);
    // The caller's own entry was written under its own key; the other org's is untouched.
    assert!(exists(&edge, &own).await);
    let theirs = stored(&edge, &other).await.expect("the other org's entry");
    assert_eq!(theirs.output.metadata.title.as_deref(), Some("OTHER ORG TITLE"));

    purge(&edge, &[&own, &other]).await;
}

// ---- rehydrate ------------------------------------------------------------

#[tokio::test]
async fn an_entry_whose_artifacts_are_gone_is_a_miss_not_an_error() {
    let Some(edge) = spawn_edge(true, 200).await else { return };
    let options = json!({ "formats": ["markdown"] });
    let key = key_for(DEV_ORG, &edge.url, options);
    // An entry that points at artifacts the store does not hold (expired or deleted).
    let mut entry = planted(&edge.url, "PLANTED GONE TITLE", 100);
    entry.output.formats.markdown = Some(quarry_core::output::FormatRef {
        artifact_id: quarry_core::ids::kinds::ArtifactKind::new(),
        bytes: 10,
    });
    plant(&edge, &key, &entry).await;

    let (status, body) = post(&edge, json!({ "url": edge.url, "formats": ["markdown"], "max_age": 60_000 })).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(state_of(&body), "miss", "{body}");
    assert_eq!(edge.fetches(), 1, "the page is fetched again");
    assert!(body["data"]["markdown"].as_str().unwrap().contains(&edge.nonce));
    assert!(!body.to_string().contains("PLANTED GONE TITLE"));
    // ...and the entry now points at artifacts that exist, so the next request is a hit.
    let (_, again) = post(&edge, json!({ "url": edge.url, "formats": ["markdown"], "max_age": 60_000 })).await;
    assert_eq!(state_of(&again), "hit", "{again}");
    assert_eq!(edge.fetches(), 1);

    purge(&edge, &[&key]).await;
}

// ---- what is never cached ---------------------------------------------------

#[tokio::test]
async fn zdr_never_reads_or_writes_the_url_cache_and_says_so() {
    let Some(edge) = spawn_edge(true, 200).await else { return };
    // Plant under the key a ZDR request would compute (its options resolve to inline
    // markdown), and under the plain key: a ZDR read of either would be served.
    let zdr_key = PageCache::result_key(DEV_ORG, &edge.url, &options_key(json!({}), true)).unwrap();
    let plain_key = key_for(DEV_ORG, &edge.url, json!({}));
    plant(&edge, &zdr_key, &planted(&edge.url, "PLANTED ZDR TITLE", 100)).await;
    plant(&edge, &plain_key, &planted(&edge.url, "PLANTED PLAIN TITLE", 100)).await;

    let (status, body) = post(&edge, json!({ "url": edge.url, "zdr": true, "max_age": 60_000 })).await;
    assert_eq!(status, 200, "{body}");
    let text = body.to_string();
    assert!(!text.contains("PLANTED ZDR TITLE") && !text.contains("PLANTED PLAIN TITLE"), "{text}");
    assert_eq!(state_of(&body), "bypass");
    assert!(warning_codes(&body).contains(&"max_age_ignored_zdr".to_owned()), "{body}");
    assert_eq!(edge.fetches(), 1);
    assert!(body["data"].get("scrape_id").is_none());
    // Neither planted entry was touched by the ZDR result.
    for key in [&zdr_key, &plain_key] {
        let held = stored(&edge, key).await.expect("the planted entry");
        assert!(held.output.metadata.title.unwrap_or_default().starts_with("PLANTED"));
    }

    // A ZDR request for a URL nobody planted leaves no key behind, not even a hashed one.
    let fresh_url = format!("{}/zdr-fresh", edge.url);
    let fresh_zdr = PageCache::result_key(DEV_ORG, &fresh_url, &options_key(json!({}), true)).unwrap();
    let fresh_plain = key_for(DEV_ORG, &fresh_url, json!({}));
    let (status, body) = post(&edge, json!({ "url": fresh_url, "zdr": true, "max_age": 60_000 })).await;
    assert_eq!(status, 200, "{body}");
    assert!(!exists(&edge, &fresh_zdr).await && !exists(&edge, &fresh_plain).await);

    purge(&edge, &[&zdr_key, &plain_key]).await;
}

#[tokio::test]
async fn a_result_that_is_not_2xx_is_never_written() {
    let Some(edge) = spawn_edge(true, 404).await else { return };
    let key = key_for(DEV_ORG, &edge.url, json!({}));
    let _ = post(&edge, json!({ "url": edge.url, "max_age": 60_000 })).await;
    assert!(edge.fetches() >= 1);
    assert!(!exists(&edge, &key).await, "a 404 must not be cached");
    purge(&edge, &[&key]).await;
}

// A 200 bot wall and a 200 "page not found" body. `QUARRY_CHALLENGE_ENFORCE` is NOT set
// in this binary, so the run is under the shipped default (`log`): the pipeline returns
// an Ok output for both. They must still never be written, and the next `max_age` call
// must fetch again instead of serving them as a fresh hit.
const TURNSTILE_WALL: &str = r#"<html><head><title>Turnstile Test</title></head><body><h1>Demo</h1><div class="cf-turnstile" data-sitekey="1x00000000000000000000AA"></div><script src="https://challenges.cloudflare.com/turnstile/v0/api.js" async defer></script></body></html>"#;
const WAF_INTERSTITIAL_200: &str = r#"<!DOCTYPE html><html lang="en-US"><head><title>Just a moment...</title></head><body class="no-js"><h2 id="challenge-running">Checking if the site connection is secure</h2><div id="challenge-stage"></div><script>window._cf_chl_opt={cvId:'3'};var a=document.createElement('script');a.src='/cdn-cgi/challenge-platform/h/b/orchestrate/chl_page/v1?ray=8abc';</script><script src="https://challenges.cloudflare.com/turnstile/v0/g/abc/api.js"></script></body></html>"#;
const SOFT_404_PAGE: &str = "<html><head><title>Oops</title></head><body><h1>Page not found</h1><p>Sorry.</p></body></html>";

#[tokio::test]
async fn a_bot_wall_or_a_soft_404_is_never_written_under_the_default_log_mode() {
    assert!(
        std::env::var_os("QUARRY_CHALLENGE_ENFORCE").is_none(),
        "this guard is about the shipped default; unset QUARRY_CHALLENGE_ENFORCE"
    );
    for (name, body) in [
        ("a captcha widget page", TURNSTILE_WALL),
        ("a WAF interstitial served as 200", WAF_INTERSTITIAL_200),
        ("a soft-404", SOFT_404_PAGE),
    ] {
        let Some(edge) = spawn_edge_serving(true, 200, Some(body)).await else { return };
        let key = key_for(DEV_ORG, &edge.url, json!({}));
        let request = json!({ "url": edge.url, "max_age": 60_000 });

        let (status, first) = post(&edge, request.clone()).await;
        assert_eq!(status, 200, "{name}: log mode returns the page, {first}");
        assert_eq!(edge.fetches(), 1, "{name}");
        assert!(!exists(&edge, &key).await, "{name}: it must not be cached");

        // The next call inside the window fetches again; it is not a hit.
        let (status, second) = post(&edge, request).await;
        assert_eq!(status, 200, "{name}: {second}");
        assert_eq!(state_of(&second), "miss", "{name}: {second}");
        assert_eq!(edge.fetches(), 2, "{name}: refetched, not served from the cache");
        assert!(!exists(&edge, &key).await, "{name}");
        purge(&edge, &[&key]).await;
    }
}

#[tokio::test]
async fn a_browser_plan_is_never_cached_and_max_age_says_it_was_ignored() {
    let Some(edge) = spawn_edge(true, 200).await else { return };
    let key = key_for(DEV_ORG, &edge.url, json!({}));
    plant(&edge, &key, &planted(&edge.url, "PLANTED BROWSER TITLE", 100)).await;
    let signals = json!({
        "actions": [], "screenshot": false, "pdf": false,
        "prior_block_signals": 0, "profile_required": true, "url_type": "Default"
    });

    let (status, body) = post(&edge, json!({ "url": edge.url, "max_age": 60_000, "signals": signals })).await;
    assert_eq!(status, 200, "{body}");
    assert!(!body.to_string().contains("PLANTED BROWSER TITLE"), "{body}");
    assert_eq!(state_of(&body), "bypass");
    assert!(warning_codes(&body).contains(&"max_age_ignored".to_owned()), "{body}");
    assert_eq!(edge.fetches(), 1);
    let held = stored(&edge, &key).await.expect("the planted entry");
    assert_eq!(held.output.metadata.title.as_deref(), Some("PLANTED BROWSER TITLE"), "it was not overwritten");

    purge(&edge, &[&key]).await;
}

#[tokio::test]
async fn cache_modes_gate_the_read_and_the_write() {
    let Some(edge) = spawn_edge(true, 200).await else { return };
    let key = key_for(DEV_ORG, &edge.url, json!({}));

    // read_only: nothing is written.
    let (_, body) = post(&edge, json!({ "url": edge.url, "max_age": 60_000, "cache": { "mode": "read_only" } })).await;
    assert_eq!(state_of(&body), "miss");
    assert!(!exists(&edge, &key).await, "read_only must not write");

    // write_only: written, but a planted fresh entry is not read.
    plant(&edge, &key, &planted(&edge.url, "PLANTED MODE TITLE", 100)).await;
    let (_, body) = post(&edge, json!({ "url": edge.url, "max_age": 60_000, "cache": { "mode": "write_only" } })).await;
    assert_eq!(state_of(&body), "bypass");
    assert!(!body.to_string().contains("PLANTED MODE TITLE"));
    assert!(stored(&edge, &key).await.unwrap().output.metadata.title.unwrap().contains(&edge.nonce));

    // bypass: neither.
    plant(&edge, &key, &planted(&edge.url, "PLANTED BYPASS TITLE", 100)).await;
    let (_, body) = post(&edge, json!({ "url": edge.url, "max_age": 60_000, "cache": { "mode": "bypass" } })).await;
    assert_eq!(state_of(&body), "bypass");
    assert_eq!(
        stored(&edge, &key).await.unwrap().output.metadata.title.as_deref(),
        Some("PLANTED BYPASS TITLE")
    );

    purge(&edge, &[&key]).await;
}

// ---- no cache backend (no Redis needed) -------------------------------------

#[tokio::test]
async fn without_a_cache_backend_max_age_is_ignored_with_a_warning_not_an_error() {
    let edge = spawn_edge(false, 200).await.expect("an edge with no cache");
    let (status, body) = post(&edge, json!({ "url": edge.url, "max_age": 60_000 })).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(state_of(&body), "bypass");
    assert!(warning_codes(&body).contains(&"cache_unavailable".to_owned()), "{body}");
    let (_, again) = post(&edge, json!({ "url": edge.url, "max_age": 60_000 })).await;
    assert_eq!(state_of(&again), "bypass");
    assert_eq!(edge.fetches(), 2, "every request fetches");
}

#[tokio::test]
async fn max_age_is_validated_with_field_errors() {
    let edge = spawn_edge(false, 200).await.expect("an edge with no cache");
    for (value, code) in [
        (json!(2_592_000_001u64), "out_of_range"),
        (json!("60000"), "invalid_type"),
        (json!(-5), "invalid_type"),
        (json!(1.5), "invalid_type"),
    ] {
        let (status, body) = post(&edge, json!({ "url": edge.url, "max_age": value })).await;
        assert_eq!(status, 400, "{value}: {body}");
        let field = &body["error"]["details"]["fields"][0];
        assert_eq!(field["path"], "max_age", "{value}: {body}");
        assert_eq!(field["code"], code, "{value}: {body}");
    }
    assert_eq!(edge.fetches(), 0, "a refused request fetches nothing");
    // The ceiling itself and zero are fine.
    for value in [0u64, 2_592_000_000] {
        let (status, body) = post(&edge, json!({ "url": edge.url, "max_age": value })).await;
        assert_eq!(status, 200, "{value}: {body}");
    }
}
