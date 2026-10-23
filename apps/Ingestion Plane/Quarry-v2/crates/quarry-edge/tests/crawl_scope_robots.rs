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
const INTERNAL_SECRET: &str = "crawl-scope-internal-secret";
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
