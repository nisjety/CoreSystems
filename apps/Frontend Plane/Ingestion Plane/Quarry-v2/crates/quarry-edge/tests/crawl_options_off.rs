//! The two INNH-6 features with their flags off, the default.
//!
//! `QUARRY_EDGE__CRAWL_SITEMAP` and `QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS` are removed from
//! the environment by every test here. With them off:
//!
//! - the handoff answers the 400 `unsupported_option` it always did, and hands nothing on;
//! - `POST /v1/internal/crawl/sitemap` is a 501;
//! - `run_page` ignores a `scrape` field: the page is cleaned as before and the response
//!   has no `scrape_applied`, which is how an orchestrator learns the options were not applied.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{extract::State, routing::post, Json, Router};
use hmac::{Hmac, Mac};
use quarry_edge::{internal_auth::InternalSigner, state::AppState};
use quarry_runtime::EventSink;
use serde_json::{json, Value};
use sha2::Sha256;
use tokio::sync::mpsc;

mod support;

use support::fixture_site::{FixtureSiteDriver, FIXTURE_A};

const RUNTIME_TOKEN: &str = "crawl-options-off-runtime-token";
const INTERNAL_SECRET: &str = "crawl-options-off-internal-secret-0123456789";
const ORG: &str = "org_crawl_options_off";
const TEST_BEARER: &str = "crawl-options-off-dev-token";

type HmacSha256 = Hmac<Sha256>;
type Recorded = Arc<Mutex<Vec<Value>>>;

fn clear_flags() {
    std::env::remove_var("QUARRY_EDGE__CRAWL_SITEMAP");
    std::env::remove_var("QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS");
    std::env::set_var("QUARRY_EDGE__RUNTIME_AUTH_TOKEN", RUNTIME_TOKEN);
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
}

async fn spawn_control() -> (String, Recorded) {
    async fn create_job(State(recorded): State<Recorded>, Json(body): Json<Value>) -> Json<Value> {
        recorded.lock().unwrap().push(body);
        Json(json!({ "data": { "id": "job_stub", "run_id": null } }))
    }
    let recorded: Recorded = Arc::default();
    let app = Router::new()
        .route("/v1/jobs", post(create_job))
        .with_state(recorded.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), recorded)
}

async fn spawn_edge(control_url: String) -> SocketAddr {
    clear_flags();
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(FixtureSiteDriver::standard_site());
    let state = AppState {
        control_base_url: control_url,
        internal_signer: Some(Arc::new(InternalSigner::new(INTERNAL_SECRET).unwrap())),
        ..support::base_state(driver, EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn crawl(addr: SocketAddr, body: Value) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v1/crawl"))
        .bearer_auth(TEST_BEARER)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap())
}

fn run_sig(org: &str, url: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(INTERNAL_SECRET.as_bytes()).unwrap();
    mac.update(format!("{org}\n{url}").as_bytes());
    format!("{:x}", mac.finalize().into_bytes())
}

async fn internal(addr: SocketAddr, path: &str, url: &str, mut body: Value) -> (u16, Value) {
    body["url"] = json!(url);
    body["org_id"] = json!(ORG);
    let response = reqwest::Client::new()
        .post(format!("http://{addr}{path}"))
        .bearer_auth(RUNTIME_TOKEN)
        .header("x-quarry-run-sig", run_sig(ORG, url))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap())
}

#[tokio::test]
async fn the_handoff_keeps_its_400_for_both_options() {
    let (control, recorded) = spawn_control().await;
    let addr = spawn_edge(control).await;
    for (key, value) in [
        ("sitemap", json!("include")),
        ("sitemap", json!("only")),
        ("scrape_options", json!({ "exclude_tags": ["nav"] })),
        ("scrapeOptions", json!({})),
    ] {
        let mut body = json!({ "url": "https://example.com/" });
        body[key] = value;
        let (status, response) = crawl(addr, body).await;
        assert_eq!(status, 400, "{key}: {response}");
        let fields = response["error"]["details"]["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 1, "{response}");
        assert_eq!(fields[0]["path"], key);
        assert_eq!(fields[0]["code"], "unsupported_option");
    }
    // The values that always worked still work.
    let (status, response) = crawl(
        addr,
        json!({ "url": "https://example.com/", "sitemap": "skip" }),
    )
    .await;
    assert_eq!(status, 200, "{response}");
    assert!(
        recorded.lock().unwrap().len() == 1,
        "only the accepted request was handed off"
    );
}

#[tokio::test]
async fn the_sitemap_route_is_a_501() {
    let (control, _recorded) = spawn_control().await;
    let addr = spawn_edge(control).await;
    let (status, body) = internal(
        addr,
        "/v1/internal/crawl/sitemap",
        &format!("{FIXTURE_A}/"),
        json!({ "crawl": { "v": 1, "scope_mode": "enforce" } }),
    )
    .await;
    assert_eq!(status, 501, "{body}");
    assert_eq!(body["error"]["code"], "UNSUPPORTED");
}

#[tokio::test]
async fn run_page_ignores_scrape_and_does_not_echo_it() {
    let (control, _recorded) = spawn_control().await;
    let addr = spawn_edge(control).await;
    let url = format!("{FIXTURE_A}/docs/a");
    let (status, plain) = internal(addr, "/v1/internal/run_page", &url, json!({})).await;
    assert_eq!(status, 200, "{plain}");
    let (status, with_scrape) = internal(
        addr,
        "/v1/internal/run_page",
        &url,
        json!({ "scrape": { "exclude_tags": ["a"], "only_main_content": false, "extractor": "clean_full" } }),
    )
    .await;
    assert_eq!(status, 200, "{with_scrape}");
    assert!(
        with_scrape.get("scrape_applied").is_none(),
        "the options were not applied: {with_scrape}"
    );
    assert_eq!(plain["fingerprint"], with_scrape["fingerprint"]);
    assert_eq!(
        plain["excerpt"], with_scrape["excerpt"],
        "the page was cleaned as before"
    );
    // Even an invalid scrape is ignored, not a 400: with the flag off the field does not exist.
    let (status, body) = internal(
        addr,
        "/v1/internal/run_page",
        &url,
        json!({ "scrape": { "actions": [] } }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
}
