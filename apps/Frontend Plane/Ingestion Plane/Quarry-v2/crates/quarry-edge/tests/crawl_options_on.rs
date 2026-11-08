//! `scrape_options` and `sitemap` on `/v1/crawl` and `/v1/internal/run_page` (INNH-6), flags on.
//!
//! Both flags are set by this binary (`QUARRY_EDGE__CRAWL_SITEMAP`,
//! `QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS`). `crawl_options_off.rs` covers the default.
//!
//! - The handoff validates the options, stamps their canonical form into the job
//!   params and echoes them in `effective`; nothing is stamped when they are absent.
//! - `run_page` cleans a page with the options (the stored markdown shows it), echoes
//!   `scrape_applied`, and leaves a page without options exactly as it was.
//!
//! Not covered here: that a page cleaned with non-default options neither reads nor
//! writes the Redis page cache. That decision is `CrawlScrapeOptions::cache_admissible`
//! (unit tests in `crawl_scrape.rs`); an end-to-end check needs a Redis, as
//! `cache_policy.rs` does.

use std::{
    collections::HashSet,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{extract::State, routing::post, Json, Router};
use hmac::{Hmac, Mac};
use quarry_core::pagination::ListFilter;
use quarry_edge::{internal_auth::InternalSigner, state::AppState};
use quarry_runtime::{
    artifact_store::{ArtifactStore, InMemoryStore},
    fetch::StaticDriver,
    EventSink,
};
use serde_json::{json, Value};
use sha2::Sha256;
use tokio::sync::mpsc;

mod support;

use support::fixture_site::{page, FixtureReply, FixtureSiteDriver, FIXTURE_A};

const RUNTIME_TOKEN: &str = "crawl-options-runtime-token";
const INTERNAL_SECRET: &str = "crawl-options-internal-secret-0123456789";
const ORG: &str = "org_crawl_options_test";
const TEST_BEARER: &str = "crawl-options-dev-token";

type HmacSha256 = Hmac<Sha256>;

fn set_flags() {
    std::env::set_var("QUARRY_EDGE__RUNTIME_AUTH_TOKEN", RUNTIME_TOKEN);
    std::env::set_var("QUARRY_EDGE__CRAWL_SITEMAP", "on");
    std::env::set_var("QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS", "on");
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
}

// -- the handoff -------------------------------------------------------------

type Recorded = Arc<Mutex<Vec<Value>>>;

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

async fn spawn_handoff_edge(control_url: String) -> SocketAddr {
    set_flags();
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let static_driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    let state = AppState {
        control_base_url: control_url,
        ..support::base_state(static_driver, EventSink::new(tx))
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

#[tokio::test]
async fn the_handoff_stamps_the_checked_options_into_the_job_and_echoes_them() {
    let (control, recorded) = spawn_control().await;
    let addr = spawn_handoff_edge(control).await;
    let (status, body) = crawl(
        addr,
        json!({
            "url": "https://example.com/",
            "sitemap": "include",
            "scrapeOptions": { "formats": ["markdown", "markdown"], "excludeTags": ["nav", "footer"], "timeoutMs": 20000 },
        }),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let jobs = recorded.lock().unwrap();
    assert_eq!(jobs.len(), 1);
    let params = &jobs[0]["params"];
    assert_eq!(params["sitemap"], "include");
    assert_eq!(
        params["scrape_options"],
        json!({
            "formats": ["markdown"],
            "only_main_content": true,
            "include_tags": [],
            "exclude_tags": ["nav", "footer"],
            "remove_base64_images": false,
            "extractor": "readability",
            "timeout_ms": 20000,
        }),
        "the canonical, resolved form is what the orchestrator receives"
    );
    let effective = &body["data"]["effective"];
    assert_eq!(effective["sitemap"], "include");
    assert_eq!(
        effective["scrape_options"]["exclude_tags"],
        json!(["nav", "footer"])
    );
}

#[tokio::test]
async fn a_job_without_the_options_is_stamped_exactly_as_before() {
    let (control, recorded) = spawn_control().await;
    let addr = spawn_handoff_edge(control).await;
    for body in [
        json!({ "url": "https://example.com/" }),
        json!({ "url": "https://example.com/", "sitemap": "skip" }),
        json!({ "url": "https://example.com/", "scrape_options": null }),
    ] {
        let (status, response) = crawl(addr, body).await;
        assert_eq!(status, 200, "{response}");
        assert!(response["data"]["effective"].get("sitemap").is_none());
        assert!(response["data"]["effective"]
            .get("scrape_options")
            .is_none());
    }
    let jobs = recorded.lock().unwrap();
    assert_eq!(jobs.len(), 3);
    for job in jobs.iter() {
        assert!(job["params"].get("sitemap").is_none(), "{job}");
        assert!(job["params"].get("scrape_options").is_none(), "{job}");
    }
}

#[tokio::test]
async fn bad_options_are_a_400_with_prefixed_paths_and_no_job() {
    let (control, recorded) = spawn_control().await;
    let addr = spawn_handoff_edge(control).await;
    let (status, body) = crawl(
        addr,
        json!({
            "url": "https://example.com/",
            "sitemap": "both",
            "scrape_options": { "actions": [], "include_tags": ["ok", " "], "formats": ["pdf"] },
        }),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    let mut found: Vec<(String, String)> = body["error"]["details"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["path"].as_str().unwrap().to_owned(),
                f["code"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    found.sort();
    assert_eq!(
        found,
        vec![
            (
                "scrape_options.actions".to_owned(),
                "unsupported_option".to_owned()
            ),
            (
                "scrape_options.formats[0]".to_owned(),
                "unsupported_option".to_owned()
            ),
            (
                "scrape_options.include_tags[1]".to_owned(),
                "empty".to_owned()
            ),
            ("sitemap".to_owned(), "invalid_option".to_owned()),
        ]
    );
    assert!(
        recorded.lock().unwrap().is_empty(),
        "nothing was handed off"
    );
}

// -- run_page ----------------------------------------------------------------

const MAIN_TEXT: &str = "ARTICLEBODYUNIQUE is the text of the page. It is long enough, and has enough words, to be \
the main content of the page for any extractor that looks for one. A second sentence keeps going, and a third, so the \
paragraph is not mistaken for a caption.";

fn options_page() -> String {
    page(
        "Options",
        &format!(
            "<nav><a href=\"/other\">NAVTEXTUNIQUE</a></nav><article><h1>Heading</h1><p>{MAIN_TEXT}</p></article>\
             <footer>FOOTERTEXTUNIQUE</footer>"
        ),
    )
}

fn run_sig(org: &str, url: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(INTERNAL_SECRET.as_bytes()).unwrap();
    mac.update(format!("{org}\n{url}").as_bytes());
    format!("{:x}", mac.finalize().into_bytes())
}

async fn spawn_page_edge() -> (SocketAddr, Arc<InMemoryStore>) {
    set_flags();
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let mut site = FixtureSiteDriver::new();
    for path in ["default", "same", "clean", "pruned", "bad", "timeout"] {
        site = site.serve(
            &format!("{FIXTURE_A}/o/{path}"),
            FixtureReply::html(options_page()),
        );
    }
    let store = Arc::new(InMemoryStore::new());
    let state = AppState {
        internal_signer: Some(Arc::new(InternalSigner::new(INTERNAL_SECRET).unwrap())),
        artifacts: store.clone(),
        ..support::base_state(Arc::new(site), EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (addr, store)
}

async fn run_page(addr: SocketAddr, url: &str, scrape: Option<Value>) -> (u16, Value) {
    let mut body = json!({ "url": url, "org_id": ORG });
    if let Some(scrape) = scrape {
        body["scrape"] = scrape;
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

/// The markdown artifacts the store holds, as (id, text).
async fn markdown_artifacts(store: &InMemoryStore) -> Vec<(String, String)> {
    let page = store.list(ORG, &ListFilter::default()).await.unwrap();
    let mut out = Vec::new();
    for summary in page.items.into_iter().filter(|s| s.kind == "markdown") {
        let bytes = store.get(ORG, &summary.artifact_id).await.unwrap();
        out.push((
            summary.artifact_id.to_string(),
            String::from_utf8_lossy(&bytes).into_owned(),
        ));
    }
    out
}

/// Run a page and return the markdown it stored (the one artifact the call added).
async fn stored_markdown(
    addr: SocketAddr,
    store: &InMemoryStore,
    url: &str,
    scrape: Option<Value>,
) -> (Value, String) {
    let before: HashSet<String> = markdown_artifacts(store)
        .await
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let (status, body) = run_page(addr, url, scrape).await;
    assert_eq!(status, 200, "{body}");
    let added: Vec<String> = markdown_artifacts(store)
        .await
        .into_iter()
        .filter(|(id, _)| !before.contains(id))
        .map(|(_, text)| text)
        .collect();
    assert_eq!(
        added.len(),
        1,
        "one markdown artifact per page run: {added:?}"
    );
    (body, added.into_iter().next().unwrap())
}

#[tokio::test]
async fn options_change_the_stored_markdown_and_are_echoed() {
    let (addr, store) = spawn_page_edge().await;

    // The whole body, chrome included.
    let whole = json!({ "only_main_content": false, "extractor": "clean_full" });
    let (body, clean) = stored_markdown(
        addr,
        &store,
        &format!("{FIXTURE_A}/o/clean"),
        Some(whole.clone()),
    )
    .await;
    assert_eq!(body["scrape_applied"], true);
    assert!(clean.contains("ARTICLEBODYUNIQUE"), "{clean}");
    assert!(
        clean.contains("NAVTEXTUNIQUE") && clean.contains("FOOTERTEXTUNIQUE"),
        "{clean}"
    );

    // The same with the selectors excluded: the chrome is gone from the stored artifact.
    let mut pruned = whole;
    pruned["exclude_tags"] = json!(["nav", "footer"]);
    let (body, text) =
        stored_markdown(addr, &store, &format!("{FIXTURE_A}/o/pruned"), Some(pruned)).await;
    assert_eq!(body["scrape_applied"], true);
    assert!(text.contains("ARTICLEBODYUNIQUE"), "{text}");
    assert!(
        !text.contains("NAVTEXTUNIQUE") && !text.contains("FOOTERTEXTUNIQUE"),
        "{text}"
    );
}

#[tokio::test]
async fn default_options_and_no_options_store_the_same_markdown() {
    let (addr, store) = spawn_page_edge().await;
    let (plain, without) =
        stored_markdown(addr, &store, &format!("{FIXTURE_A}/o/default"), None).await;
    assert!(
        plain.get("scrape_applied").is_none(),
        "no options, no echo: {plain}"
    );
    // `formats` and `timeout_ms` shape delivery only; the cleaning is the default one.
    let (echoed, with) = stored_markdown(
        addr,
        &store,
        &format!("{FIXTURE_A}/o/same"),
        Some(json!({ "formats": ["markdown", "links"], "timeout_ms": 30000 })),
    )
    .await;
    assert_eq!(echoed["scrape_applied"], true);
    assert_eq!(
        without, with,
        "default cleaning is byte-identical with and without the options"
    );
    assert_eq!(plain["links"], echoed["links"]);
}

#[tokio::test]
async fn options_the_edge_cannot_apply_per_page_are_a_400_naming_the_field() {
    let (addr, _store) = spawn_page_edge().await;
    let (status, body) = run_page(
        addr,
        &format!("{FIXTURE_A}/o/bad"),
        Some(json!({ "actions": [], "timeout_ms": 5 })),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    let paths: Vec<&str> = body["error"]["details"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"scrape.actions"), "{paths:?}");
    // timeout_ms is only checked once the other errors are gone (the validators run in order).
    let (status, body) = run_page(
        addr,
        &format!("{FIXTURE_A}/o/timeout"),
        Some(json!({ "timeout_ms": 5 })),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(
        body["error"]["details"]["fields"][0]["path"],
        "scrape.timeout_ms"
    );
}

#[tokio::test]
async fn a_null_scrape_is_no_scrape() {
    let (addr, _store) = spawn_page_edge().await;
    let (status, body) = run_page(addr, &format!("{FIXTURE_A}/o/default"), Some(Value::Null)).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.get("scrape_applied").is_none());
}
