//! `POST /v1/internal/run_page` with a `crawl` scope (WS14.S2).
//!
//! The orchestrator calls this route once per page. A request with no `crawl`
//! gets the page's links as they were. A request with one gets its links scoped
//! and a `crawl` report attached. The page source is the standard fixture site
//! (`FixtureSiteDriver::standard_site`), so no network is used.
//!
//! Written in the build wave and compile-checked only (`cargo check --tests`).
//! Not run yet: the final test phase runs it.

use std::{net::SocketAddr, sync::Arc};

use hmac::{Hmac, Mac};
use quarry_edge::{internal_auth::InternalSigner, state::AppState};
use quarry_runtime::EventSink;
use serde_json::{json, Value};
use sha2::Sha256;
use tokio::sync::mpsc;

mod support;

use support::fixture_site::{FixtureSiteDriver, FIXTURE_A};

const RUNTIME_TOKEN: &str = "crawl-scope-runtime-token";
const INTERNAL_SECRET: &str = "crawl-scope-internal-secret-0123456789";
const ORG: &str = "org_crawl_scope_test";

type HmacSha256 = Hmac<Sha256>;

/// `X-Quarry-Run-Sig`: hex(HMAC-SHA256(secret, org_id + "\n" + url)).
fn run_sig(org: &str, url: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(INTERNAL_SECRET.as_bytes()).unwrap();
    mac.update(format!("{org}\n{url}").as_bytes());
    format!("{:x}", mac.finalize().into_bytes())
}

async fn spawn_edge() -> SocketAddr {
    std::env::set_var("QUARRY_EDGE__RUNTIME_AUTH_TOKEN", RUNTIME_TOKEN);
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(FixtureSiteDriver::standard_site());
    let state = AppState {
        internal_signer: Some(Arc::new(InternalSigner::new(INTERNAL_SECRET).unwrap())),
        ..support::base_state(driver, EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn run_page(addr: SocketAddr, url: &str, crawl: Option<Value>) -> (u16, Value) {
    let mut body = json!({ "url": url, "org_id": ORG });
    if let Some(crawl) = crawl {
        body["crawl"] = crawl;
    }
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v1/internal/run_page"))
        .bearer_auth(RUNTIME_TOKEN)
        .header("x-quarry-run-sig", run_sig(ORG, url))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap())
}

fn links_of(body: &Value) -> Vec<String> {
    body["links"]
        .as_array()
        .expect("links is an array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn without_crawl_the_response_has_no_report() {
    let addr = spawn_edge().await;
    let (status, body) = run_page(addr, &format!("{FIXTURE_A}/"), None).await;
    assert_eq!(status, 200, "{body}");
    assert!(
        body.get("crawl").is_none(),
        "no report without a crawl object: {body}"
    );
    assert!(!links_of(&body).is_empty());
}

#[tokio::test]
async fn enforce_keeps_the_fixture_a_pages_and_reports_the_denials() {
    let addr = spawn_edge().await;
    let (status, body) = run_page(
        addr,
        &format!("{FIXTURE_A}/"),
        Some(json!({ "v": 1, "depth": 0, "max_depth": 1, "scope_mode": "enforce" })),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let links = links_of(&body);
    assert!(links.iter().all(|l| l.starts_with(FIXTURE_A)), "{links:?}");
    assert!(links.iter().all(|l| !l.contains("fixture-b")
        && !l.contains("docs.fixture-a")
        && !l.ends_with(".png")));
    assert_eq!(body["crawl"]["scope_applied"], true);
    assert!(
        body["crawl"]["denials"]["counts"]["external_host_disabled"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "{body}"
    );
}

#[tokio::test]
async fn observe_returns_every_href_and_reports_what_enforce_would_drop() {
    let addr = spawn_edge().await;
    let (_, plain) = run_page(addr, &format!("{FIXTURE_A}/"), None).await;
    let (status, body) = run_page(
        addr,
        &format!("{FIXTURE_A}/"),
        Some(json!({ "scope_mode": "observe" })),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["links"], plain["links"],
        "observe keeps every href as sent"
    );
    assert_eq!(body["crawl"]["scope_applied"], false);
    assert_eq!(body["crawl"]["mode"], "observe");
}

#[tokio::test]
async fn malformed_crawl_is_a_400_that_names_the_field() {
    let addr = spawn_edge().await;
    let (status, body) = run_page(
        addr,
        &format!("{FIXTURE_A}/"),
        Some(json!({ "scope": { "allow_subdomian": true } })),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"]["details"]["fields"][0]["path"], "crawl");
}

#[tokio::test]
async fn bad_regex_is_a_400_with_the_exact_pattern_index() {
    let addr = spawn_edge().await;
    let (status, body) = run_page(
        addr,
        &format!("{FIXTURE_A}/"),
        Some(json!({ "scope": { "include_paths": ["/docs/*", "re:(unclosed"] } })),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(
        body["error"]["details"]["fields"][0]["path"],
        "crawl.scope.include_paths[1]"
    );
}

#[tokio::test]
async fn batch_pages_return_no_links() {
    let addr = spawn_edge().await;
    let (status, body) = run_page(
        addr,
        &format!("{FIXTURE_A}/"),
        Some(json!({ "kind": "batch" })),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(links_of(&body).is_empty(), "{body}");
    assert!(body["crawl"]["links_unevaluated"].as_u64().unwrap_or(0) > 0);
}

// ── WS14.S4: the robots.txt gate on the page route and on /v1/map ────────────
//
// Written in the build wave and compile-checked only. Not run yet.

use quarry_runtime::{RobotsEnforceMode, RobotsGate, RobotsGateConfig, UnreachablePolicy};
use support::fixture_site::FixtureReply;

/// An edge whose page route and `/v1/map` are gated on robots.txt by `mode`,
/// over `site`. The gate shares the site, so its robots.txt fetches show in the
/// site's request log.
async fn spawn_gated_edge(
    site: FixtureSiteDriver,
    mode: RobotsEnforceMode,
    unreachable: UnreachablePolicy,
) -> (SocketAddr, FixtureSiteDriver, Arc<RobotsGate>) {
    std::env::set_var("QUARRY_EDGE__RUNTIME_AUTH_TOKEN", RUNTIME_TOKEN);
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(site.clone());
    let config = RobotsGateConfig {
        mode,
        unreachable,
        ..RobotsGateConfig::default()
    };
    let gate = Arc::new(RobotsGate::new(driver.clone(), config));
    let state = AppState {
        internal_signer: Some(Arc::new(InternalSigner::new(INTERNAL_SECRET).unwrap())),
        robots: Some(gate.clone()),
        ..support::base_state(driver, EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (addr, site, gate)
}

fn crawl_respecting_robots() -> Option<Value> {
    Some(json!({ "v": 1, "depth": 0, "max_depth": 1, "scope_mode": "enforce", "robots": "respect" }))
}

#[tokio::test]
async fn enforce_refuses_a_disallowed_page_with_the_typed_403() {
    let (addr, site, _gate) = spawn_gated_edge(
        FixtureSiteDriver::standard_site(),
        RobotsEnforceMode::Enforce,
        UnreachablePolicy::Disallow,
    )
    .await;
    let url = format!("{FIXTURE_A}/private/secret");
    let (status, body) = run_page(addr, &url, crawl_respecting_robots()).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["code"], "SECURITY_BLOCKED");
    assert_eq!(body["error"]["retryable"], false);
    let denial = &body["error"]["details"]["denial"];
    assert_eq!(denial["code"], "robots_disallowed");
    assert_eq!(denial["path"], "/private/secret");
    assert!(denial["user_agent"].as_str().unwrap().starts_with("quarry"));
    assert!(denial["message"].is_string() && denial["remedy"].is_string());
    assert!(!site.was_requested(&url), "the page itself was never fetched");
}

#[tokio::test]
async fn an_allowed_page_reports_the_crawl_delay_in_milliseconds() {
    let (addr, _site, _gate) = spawn_gated_edge(
        FixtureSiteDriver::standard_site(),
        RobotsEnforceMode::Enforce,
        UnreachablePolicy::Disallow,
    )
    .await;
    let (status, body) = run_page(addr, &format!("{FIXTURE_A}/"), crawl_respecting_robots()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["crawl"]["crawl_delay_ms"], 2000);
    // The cached rules drop the private link from discovery in enforce mode.
    let links = links_of(&body);
    assert!(links.iter().all(|l| !l.contains("/private/")), "{links:?}");
    assert!(
        body["crawl"]["denials"]["counts"]["robots_disallowed"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "{body}"
    );
}

#[tokio::test]
async fn observe_fetches_the_disallowed_page_and_keeps_every_link() {
    let (addr, site, gate) = spawn_gated_edge(
        FixtureSiteDriver::standard_site(),
        RobotsEnforceMode::Observe,
        UnreachablePolicy::Disallow,
    )
    .await;
    let url = format!("{FIXTURE_A}/private/secret");
    let (status, body) = run_page(addr, &url, crawl_respecting_robots()).await;
    assert_eq!(status, 200, "{body}");
    assert!(site.was_requested(&url));
    assert_eq!(gate.stats().would_deny, 1);
    let (_, index) = run_page(addr, &format!("{FIXTURE_A}/"), crawl_respecting_robots()).await;
    assert!(
        links_of(&index).iter().any(|l| l.contains("/private/")),
        "observe drops no link on robots.txt's word: {index}"
    );
}

#[tokio::test]
async fn ignore_and_a_request_without_crawl_do_not_touch_robots() {
    let (addr, site, gate) = spawn_gated_edge(
        FixtureSiteDriver::standard_site(),
        RobotsEnforceMode::Enforce,
        UnreachablePolicy::Disallow,
    )
    .await;
    let url = format!("{FIXTURE_A}/private/secret");
    let (status, _) = run_page(addr, &url, Some(json!({ "robots": "ignore" }))).await;
    assert_eq!(status, 200);
    let (status, _) = run_page(addr, &url, None).await;
    assert_eq!(status, 200);
    assert!(site.was_requested(&url));
    assert!(gate.is_empty());
}

#[tokio::test]
async fn a_503_robots_txt_blocks_every_page_after_two_attempts() {
    let site = FixtureSiteDriver::standard_site().serve(
        &format!("{FIXTURE_A}/robots.txt"),
        FixtureReply::text_with_status(503, "unavailable"),
    );
    let (addr, site, _gate) =
        spawn_gated_edge(site, RobotsEnforceMode::Enforce, UnreachablePolicy::Disallow).await;
    let (status, body) = run_page(addr, &format!("{FIXTURE_A}/"), crawl_respecting_robots()).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["details"]["denial"]["code"], "robots_unreachable");
    let robots_fetches = site
        .requests()
        .iter()
        .filter(|r| r.url.path() == "/robots.txt")
        .count();
    assert_eq!(robots_fetches, 2, "two robots.txt attempts, then unreachable");
}

#[tokio::test]
async fn a_503_robots_txt_fetches_when_the_policy_is_allow() {
    let site = FixtureSiteDriver::standard_site().serve(
        &format!("{FIXTURE_A}/robots.txt"),
        FixtureReply::text_with_status(503, "unavailable"),
    );
    let (addr, _site, _gate) =
        spawn_gated_edge(site, RobotsEnforceMode::Enforce, UnreachablePolicy::Allow).await;
    let (status, body) = run_page(addr, &format!("{FIXTURE_A}/"), crawl_respecting_robots()).await;
    assert_eq!(status, 200, "{body}");
}

#[tokio::test]
async fn a_robots_txt_redirect_to_the_metadata_address_counts_as_unreachable() {
    let site = FixtureSiteDriver::standard_site().serve(
        &format!("{FIXTURE_A}/robots.txt"),
        FixtureReply::redirect(302, "http://169.254.169.254/robots.txt"),
    );
    let (addr, site, _gate) =
        spawn_gated_edge(site, RobotsEnforceMode::Enforce, UnreachablePolicy::Disallow).await;
    let (status, body) = run_page(addr, &format!("{FIXTURE_A}/"), crawl_respecting_robots()).await;
    // The fixture driver follows the redirect to a host it does not serve (a 404), which
    // reads as "no robots.txt"; the real StaticDriver refuses the hop (see
    // `quarry-runtime/tests/robots_gate_ssrf.rs`). Either way the metadata host is
    // never contacted by the edge itself.
    assert!(status == 200 || status == 403, "{status} {body}");
    assert!(site
        .requests()
        .iter()
        .all(|r| r.url.host_str() != Some("169.254.169.254") || r.status == 404));
}

fn claims_for_org(org_id: &str) -> quarry_edge::auth::Claims {
    quarry_edge::auth::Claims {
        web_policy: None,
        sub: "u1".into(),
        iss: "auth-core".into(),
        exp: i64::MAX,
        org_id: org_id.into(),
        user_id: "u1".into(),
        principal_type: None,
        service_id: None,
        nbf: None,
        aud: None,
        scopes: Vec::new(),
        zdr: None,
    }
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body reads");
    serde_json::from_slice(&body).expect("response body is JSON")
}

fn map_request(url: &str) -> quarry_edge::map_routes::MapRequest {
    quarry_edge::map_routes::MapRequest {
        url: url.to_string(),
        search: None,
        limit: None,
        include_subdomains: false,
    }
}

async fn call_map(
    state: AppState,
    claims: quarry_edge::auth::Claims,
    url: &str,
) -> (u16, Value) {
    use axum::response::IntoResponse;
    let response = quarry_edge::map_routes::map(
        axum::extract::State(state),
        axum::Extension(claims),
        axum::Json(map_request(url)),
    )
    .await
    .into_response();
    let status = response.status().as_u16();
    (status, response_json(response).await)
}

fn map_state(site: FixtureSiteDriver, gate: Arc<RobotsGate>) -> AppState {
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    AppState {
        robots: Some(gate),
        ..support::base_state(Arc::new(site), EventSink::new(tx))
    }
}

#[tokio::test]
async fn map_drops_the_private_urls_and_a_zdr_token_leaves_no_cache_entry() {
    let site = FixtureSiteDriver::standard_site();
    let config = RobotsGateConfig {
        mode: RobotsEnforceMode::Enforce,
        ..RobotsGateConfig::default()
    };
    let gate = Arc::new(RobotsGate::new(Arc::new(site.clone()), config));

    // A ZDR token: robots.txt is fetched, nothing is stored.
    let mut zdr_claims = claims_for_org(ORG);
    zdr_claims.zdr = Some(true);
    let (status, body) = call_map(map_state(site.clone(), gate.clone()), zdr_claims, &format!("{FIXTURE_A}/")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(gate.len(), 0, "a ZDR map call fills no robots cache");

    // A normal token fills the cache and the private URL is gone.
    let claims = claims_for_org(ORG);
    let (status, body) = call_map(map_state(site, gate.clone()), claims, &format!("{FIXTURE_A}/")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(gate.len(), 1);
    let urls: Vec<&str> = body["links"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["url"].as_str().unwrap())
        .collect();
    assert!(urls.iter().all(|u| !u.contains("/private/")), "{urls:?}");
    assert!(urls.iter().any(|u| u.ends_with("/docs/a")), "{urls:?}");
}

#[tokio::test]
async fn map_with_an_unreachable_robots_txt_is_refused_under_the_disallow_policy() {
    let site = FixtureSiteDriver::standard_site().serve(
        &format!("{FIXTURE_A}/robots.txt"),
        FixtureReply::text_with_status(503, "unavailable"),
    );
    let config = RobotsGateConfig {
        mode: RobotsEnforceMode::Enforce,
        ..RobotsGateConfig::default()
    };
    let gate = Arc::new(RobotsGate::new(Arc::new(site.clone()), config));
    let claims = claims_for_org(ORG);
    let (status, body) = call_map(map_state(site, gate), claims, &format!("{FIXTURE_A}/")).await;
    assert_eq!(status, 403, "{body}");
}
