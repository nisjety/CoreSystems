//! WS7.G (6a): the backstops. A redirect to a host the web policy denies ends a scrape
//! with `SecurityBlocked` before anything is stored; the engine the edge builds keeps the
//! wrapped engine's private-host rules; the final-URL recheck is a flag. Written in the
//! build phase; run in the wave's test phase
//! (`cargo test -p quarry-edge --test web_policy_backstop`).
//!
//! The browser half (a denied sub-resource of an allowed page, shown in the egress
//! receipts) is `egress_deny_blocks_a_subresource_on_an_allowed_page` in
//! `quarry-browser/src/chromiumoxide.rs` (`#[ignore]`, needs Chromium) and the deny-list
//! unit tests in `quarry-browser/tests/egress_deny_list.rs`. Not written here: the 6b
//! snapshot tests (they wait for WS14 and WS15 to be deployed).

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use quarry_core::event::Event;
use quarry_edge::policy_baseline::{PolicyBaseline, BASELINE_ENV, FINAL_URL_CHECK_ENV};
use quarry_edge::state::AppState;
use quarry_runtime::EventSink;
use quarry_security::{Decision, SecurityEngine};
use serde_json::{json, Value};
use support::fixture_site::{FixtureReply, FixtureSiteDriver};
use tokio::sync::mpsc;

mod support;

const TEST_BEARER: &str = "web-policy-backstop-dev-token";
const START: &str = "http://start.backstop.test/old";
const DENIED_TARGET: &str = "http://denied.backstop.test/new";
const ALLOWED_TARGET: &str = "http://www.start.backstop.test/new";

fn baseline(final_url_check: &str) -> PolicyBaseline {
    PolicyBaseline::from_lookup(|name| match name {
        BASELINE_ENV => Some(r#"{"domains":{"deny":["denied.backstop.test"]}}"#.to_owned()),
        FINAL_URL_CHECK_ENV => Some(final_url_check.to_owned()),
        _ => None,
    })
    .expect("the test baseline is valid")
}

fn site() -> FixtureSiteDriver {
    FixtureSiteDriver::new()
        .serve(START, FixtureReply::redirect(302, DENIED_TARGET))
        .serve(
            DENIED_TARGET,
            FixtureReply::html(support::fixture_site::page("Denied", "<p>kept out</p>")),
        )
        .serve(
            "http://start.backstop.test/ok",
            FixtureReply::redirect(302, ALLOWED_TARGET),
        )
        .serve(
            ALLOWED_TARGET,
            FixtureReply::html(support::fixture_site::page("Allowed", "<p>fine</p>")),
        )
}

/// An edge over `site()` whose events are collected, with `final_url_check` as the flag.
async fn spawn(final_url_check: &str) -> (SocketAddr, Arc<Mutex<Vec<Event>>>) {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    let (tx, mut rx) = mpsc::channel(64);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            sink.lock().unwrap().push(event);
        }
    });
    let state = AppState {
        policy: baseline(final_url_check),
        ..support::base_state(Arc::new(site()), EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, seen)
}

async fn scrape(addr: SocketAddr, url: &str) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v1/scrape"))
        .bearer_auth(TEST_BEARER)
        .json(&json!({ "url": url }))
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(Value::Null))
}

fn event_names(seen: &Arc<Mutex<Vec<Event>>>) -> Vec<String> {
    seen.lock()
        .unwrap()
        .iter()
        .map(|event| format!("{:?}", event.event_type))
        .collect()
}

#[tokio::test]
async fn redirect_to_a_denied_host_returns_security_blocked_and_persists_nothing() {
    let (addr, seen) = spawn("1").await;
    let (status, body) = scrape(addr, START).await;
    assert_eq!(status, 403, "a redirect to a denied host is a 403: {body}");
    assert_eq!(body["error"]["code"], "SECURITY_BLOCKED", "{body}");
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(message.starts_with("policy_denied:"), "{message}");
    assert!(!message.contains("backstop.test"), "the reason names no host: {message}");

    // Give the event drain a moment, then check nothing was fetched or stored.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let names = event_names(&seen);
    assert!(
        !names.iter().any(|name| name.contains("PageFetched") || name.contains("ArtifactWritten")),
        "nothing is kept for a refused redirect: {names:?}"
    );
}

#[tokio::test]
async fn a_redirect_within_the_allowed_hosts_still_succeeds() {
    let (addr, _seen) = spawn("1").await;
    let (status, body) = scrape(addr, "http://start.backstop.test/ok").await;
    assert_eq!(status, 200, "an allowed redirect is not refused: {body}");
}

#[tokio::test]
async fn with_the_flag_off_the_redirect_is_not_rechecked() {
    // The documented staging state: the host rules still gate the requested URL, the
    // final-URL recheck is the flag.
    let (addr, _seen) = spawn("0").await;
    let (status, body) = scrape(addr, START).await;
    assert_eq!(status, 200, "the flag is off, so the redirect target is not rechecked: {body}");
}

#[tokio::test]
async fn a_denied_requested_url_is_still_refused_at_the_edge_whatever_the_flag() {
    let (addr, _seen) = spawn("0").await;
    let (status, body) = scrape(addr, DENIED_TARGET).await;
    assert_eq!(status, 403, "{body}");
}

#[tokio::test]
async fn policy_security_engine_delegates_private_host_rules() {
    // The engine the edge builds answers the private-host question exactly as the shared
    // engine does, and its recheck flag is the baseline's.
    let decision = baseline("1")
        .decide(quarry_core::web_policy::Principal::Human, false, None)
        .expect("decides");
    let private_ok: Arc<dyn SecurityEngine> =
        Arc::new(quarry_security::preflight::DefaultEngine::new().with_allow_private_hosts(true));
    let private_refused: Arc<dyn SecurityEngine> =
        Arc::new(quarry_security::preflight::DefaultEngine::new());

    let open = quarry_edge::web_policy_edge::security_engine(&private_ok, &baseline("1"), &decision);
    assert!(open.allow_private_hosts());
    assert!(open.recheck_final_url());

    let strict =
        quarry_edge::web_policy_edge::security_engine(&private_refused, &baseline("0"), &decision);
    assert!(!strict.allow_private_hosts());
    assert!(!strict.recheck_final_url());
    let verdict = strict
        .preflight(&url::Url::parse("http://127.0.0.1/").unwrap())
        .await;
    assert_eq!(verdict.decision, Decision::Block, "the shared engine's rule still holds");
}
