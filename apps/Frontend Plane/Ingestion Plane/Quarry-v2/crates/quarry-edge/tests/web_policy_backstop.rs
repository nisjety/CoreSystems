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
//!
//! WS7.G 6b (the second half of this file, from `SNAPSHOT_*` down): the policy snapshot a
//! durable job carries. The crawl and batch handoffs stamp it on the job's params, the
//! orchestrator sends it with every page, `/v1/internal/run_page` verifies its hash inside the
//! signed run-binding string and, with `QUARRY_EDGE__WEB_POLICY_SNAPSHOT=enforce`, meets it with
//! the page's own policy. The two-sided golden test of the signature is in
//! `internal_auth.rs` (unit) and `activities_test.go`.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{extract::State, routing::post, Json, Router};
use hmac::{Hmac, Mac};
use quarry_core::event::Event;
use quarry_edge::internal_auth::{snapshot_sha256_hex, InternalSigner};
use quarry_edge::policy_baseline::{
    PolicyBaseline, BASELINE_ENV, FINAL_URL_CHECK_ENV, SNAPSHOT_ENV,
};
use quarry_edge::state::AppState;
use quarry_runtime::EventSink;
use quarry_security::{Decision, SecurityEngine};
use serde_json::{json, Value};
use sha2::Sha256;
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

// ===========================================================================================
// WS7.G 6b: the durable-job policy snapshot.
// ===========================================================================================

type HmacSha256 = Hmac<Sha256>;

const SNAPSHOT_RUNTIME_TOKEN: &str = "web-policy-snapshot-runtime-token";
const SNAPSHOT_SECRET: &str = "web-policy-snapshot-internal-secret-0123456789";
const SNAPSHOT_ORG: &str = "org_snapshot_backstop";
/// A host the edge's own baseline denies, for the handoff tests.
const HANDOFF_DENIED_HOST: &str = "handoff-denied.backstop.test";
const SNAP_OK: &str = "http://snap-ok.backstop.test/page";
const SNAP_DENIED: &str = "http://snap-denied.backstop.test/page";

/// A snapshot (as `ResolvedPolicy::snapshot_json` writes it) that denies `SNAP_DENIED`'s host.
fn snapshot_denying_snap_host() -> Value {
    json!({
        "v": 1,
        "modes": {},
        "robots_max": "respect",
        "proxy_max": "none",
        "allow": [],
        "deny": ["snap-denied.backstop.test"],
    })
}

fn snapshot_site() -> FixtureSiteDriver {
    FixtureSiteDriver::new()
        .serve(SNAP_OK, FixtureReply::html(support::fixture_site::page("Ok", "<p>fine</p>")))
        .serve(SNAP_DENIED, FixtureReply::html(support::fixture_site::page("Denied", "<p>kept out</p>")))
        .serve(
            &format!("http://{HANDOFF_DENIED_HOST}/page"),
            FixtureReply::html(support::fixture_site::page("Handoff", "<p>kept out</p>")),
        )
}

/// `X-Quarry-Run-Sig` written independently of the edge's own signer: the canonical string is
/// `org\nurl`, plus `\npolicy=<sha256 hex of the canonical snapshot>` when a snapshot is signed.
fn snapshot_run_sig(org: &str, url: &str, signed_snapshot: Option<&Value>) -> String {
    let mut canonical = format!("{org}\n{url}");
    if let Some(snapshot) = signed_snapshot {
        canonical.push_str(&format!("\npolicy={}", snapshot_sha256_hex(snapshot)));
    }
    let mut mac = HmacSha256::new_from_slice(SNAPSHOT_SECRET.as_bytes()).unwrap();
    mac.update(canonical.as_bytes());
    format!("{:x}", mac.finalize().into_bytes())
}

/// An edge with the snapshot flag at `mode`, an operator baseline of `baseline_json` (none:
/// the empty layer) and a control stub that records the jobs handed off.
async fn spawn_snapshot_edge(
    mode: &str,
    baseline_json: Option<&str>,
    control_url: String,
) -> SocketAddr {
    std::env::set_var("QUARRY_EDGE__RUNTIME_AUTH_TOKEN", SNAPSHOT_RUNTIME_TOKEN);
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let policy = PolicyBaseline::from_lookup(|name| match name {
        SNAPSHOT_ENV => Some(mode.to_owned()),
        BASELINE_ENV => baseline_json.map(str::to_owned),
        _ => None,
    })
    .expect("the test baseline is valid");
    let state = AppState {
        policy,
        control_base_url: control_url,
        internal_signer: Some(Arc::new(InternalSigner::new(SNAPSHOT_SECRET).unwrap())),
        ..support::base_state(Arc::new(snapshot_site()), EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

type Recorded = Arc<Mutex<Vec<Value>>>;

/// Stand-in for quarry-control's `POST /v1/jobs`: records the posted body.
async fn spawn_control_stub() -> (String, Recorded) {
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

/// One `run_page` call. `body_snapshot` is what the body carries, `signed_snapshot` what the
/// signature covers; they differ in the tamper tests.
async fn run_page(
    addr: SocketAddr,
    url: &str,
    body_snapshot: Option<&Value>,
    signed_snapshot: Option<&Value>,
) -> (u16, Value) {
    let mut body = json!({ "url": url, "org_id": SNAPSHOT_ORG });
    if let Some(snapshot) = body_snapshot {
        body["policy_snapshot"] = snapshot.clone();
    }
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v1/internal/run_page"))
        .bearer_auth(SNAPSHOT_RUNTIME_TOKEN)
        .header("x-quarry-run-sig", snapshot_run_sig(SNAPSHOT_ORG, url, signed_snapshot))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn enforce_meets_the_snapshot_with_the_page_policy() {
    let addr = spawn_snapshot_edge("enforce", None, String::new()).await;
    let snapshot = snapshot_denying_snap_host();

    let (status, body) = run_page(addr, SNAP_DENIED, Some(&snapshot), Some(&snapshot)).await;
    assert_eq!(status, 403, "the snapshot's deny binds the page: {body}");
    assert!(
        !body.to_string().contains("snap-denied.backstop.test") || body["error"]["details"]["host"].is_string(),
        "the refusal is the ordinary domain denial: {body}"
    );

    let (status, body) = run_page(addr, SNAP_OK, Some(&snapshot), Some(&snapshot)).await;
    assert_eq!(status, 200, "a host the snapshot does not deny still runs: {body}");

    // Without a snapshot the same page runs: the page's own policy denies nothing.
    let (status, body) = run_page(addr, SNAP_DENIED, None, None).await;
    assert_eq!(status, 200, "no snapshot, no extra deny: {body}");
}

#[tokio::test]
async fn enforce_applies_a_snapshots_allow_list_too() {
    let addr = spawn_snapshot_edge("enforce", None, String::new()).await;
    let snapshot = json!({
        "v": 1, "modes": {}, "robots_max": "respect", "proxy_max": "none",
        "allow": [["snap-ok.backstop.test"]], "deny": [],
    });
    let (status, body) = run_page(addr, SNAP_OK, Some(&snapshot), Some(&snapshot)).await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = run_page(addr, SNAP_DENIED, Some(&snapshot), Some(&snapshot)).await;
    assert_eq!(status, 403, "outside the snapshot's allow list: {body}");
}

#[tokio::test]
async fn a_snapshot_cannot_loosen_the_pages_own_baseline() {
    // The page edge denies SNAP_DENIED's host itself; a snapshot that denies nothing changes nothing.
    let addr = spawn_snapshot_edge(
        "enforce",
        Some(r#"{"domains":{"deny":["snap-denied.backstop.test"]}}"#),
        String::new(),
    )
    .await;
    let lenient = json!({
        "v": 1, "modes": {}, "robots_max": "ignore", "proxy_max": "pool", "allow": [], "deny": [],
    });
    let (status, body) = run_page(addr, SNAP_DENIED, Some(&lenient), Some(&lenient)).await;
    assert_eq!(status, 403, "the baseline still binds: {body}");
}

#[tokio::test]
async fn the_signature_covers_the_snapshot() {
    let addr = spawn_snapshot_edge("enforce", None, String::new()).await;
    let snapshot = snapshot_denying_snap_host();

    // Stripped: the signature covers a snapshot the body no longer carries.
    let (status, _) = run_page(addr, SNAP_DENIED, None, Some(&snapshot)).await;
    assert_eq!(status, 401, "a stripped snapshot is an invalid binding");

    // Added: the body carries a snapshot nobody signed.
    let (status, _) = run_page(addr, SNAP_OK, Some(&snapshot), None).await;
    assert_eq!(status, 401, "an unsigned snapshot is an invalid binding");

    // Swapped: a snapshot with its deny list emptied under the signature of the real one.
    let mut loosened = snapshot.clone();
    loosened["deny"] = json!([]);
    let (status, _) = run_page(addr, SNAP_DENIED, Some(&loosened), Some(&snapshot)).await;
    assert_eq!(status, 401, "a swapped snapshot is an invalid binding");

    // Intact: the same call with the snapshot it signed.
    let (status, _) = run_page(addr, SNAP_OK, Some(&snapshot), Some(&snapshot)).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn the_signature_is_checked_with_the_flag_off_too() {
    let addr = spawn_snapshot_edge("off", None, String::new()).await;
    let snapshot = snapshot_denying_snap_host();
    let (status, _) = run_page(addr, SNAP_OK, Some(&snapshot), None).await;
    assert_eq!(status, 401, "the binding does not depend on the flag");
}

#[tokio::test]
async fn off_ignores_a_correctly_signed_snapshot() {
    let addr = spawn_snapshot_edge("off", None, String::new()).await;
    let snapshot = snapshot_denying_snap_host();
    let (status, body) = run_page(addr, SNAP_DENIED, Some(&snapshot), Some(&snapshot)).await;
    assert_eq!(status, 200, "flag off: the snapshot is carried and ignored: {body}");
}

#[tokio::test]
async fn observe_counts_but_does_not_enforce() {
    let addr = spawn_snapshot_edge("observe", None, String::new()).await;
    let snapshot = snapshot_denying_snap_host();
    let (status, body) = run_page(addr, SNAP_DENIED, Some(&snapshot), Some(&snapshot)).await;
    assert_eq!(status, 200, "observe never refuses a page: {body}");
}

#[tokio::test]
async fn a_snapshot_the_edge_cannot_read_is_a_400_not_a_looser_run() {
    let snapshot = json!({ "v": 2, "deny": [] });
    let enforce = spawn_snapshot_edge("enforce", None, String::new()).await;
    let (status, body) = run_page(enforce, SNAP_OK, Some(&snapshot), Some(&snapshot)).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"]["details"]["reason"], "invalid_policy_snapshot", "{body}");

    let observe = spawn_snapshot_edge("observe", None, String::new()).await;
    let (status, _) = run_page(observe, SNAP_OK, Some(&snapshot), Some(&snapshot)).await;
    assert_eq!(status, 400, "a malformed snapshot is refused in observe too");

    // Off never reads it.
    let off = spawn_snapshot_edge("off", None, String::new()).await;
    let (status, _) = run_page(off, SNAP_OK, Some(&snapshot), Some(&snapshot)).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn a_null_snapshot_is_no_snapshot() {
    let addr = spawn_snapshot_edge("enforce", None, String::new()).await;
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v1/internal/run_page"))
        .bearer_auth(SNAPSHOT_RUNTIME_TOKEN)
        .header("x-quarry-run-sig", snapshot_run_sig(SNAPSHOT_ORG, SNAP_OK, None))
        .json(&json!({ "url": SNAP_OK, "org_id": SNAPSHOT_ORG, "policy_snapshot": null }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
}

const HANDOFF_BASELINE: &str = r#"{"domains":{"deny":["handoff-denied.backstop.test"]}}"#;

async fn post_json(addr: SocketAddr, path: &str, body: Value) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("http://{addr}{path}"))
        .bearer_auth("web-policy-snapshot-dev-token")
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn the_crawl_and_batch_handoffs_stamp_the_snapshot_when_the_flag_is_on() {
    for mode in ["observe", "enforce"] {
        let (control, recorded) = spawn_control_stub().await;
        let addr = spawn_snapshot_edge(mode, Some(HANDOFF_BASELINE), control).await;

        let (status, body) = post_json(addr, "/v1/crawl", json!({ "url": "https://seed.example.com/start" })).await;
        assert_eq!(status, 200, "{mode}: {body}");
        let (status, body) = post_json(addr, "/v1/batch", json!({ "urls": ["https://seed.example.com/a"] })).await;
        assert_eq!(status, 200, "{mode}: {body}");

        let jobs = recorded.lock().unwrap();
        assert_eq!(jobs.len(), 2, "{mode}");
        for job in jobs.iter() {
            let snapshot = &job["params"]["policy_snapshot"];
            assert_eq!(snapshot["v"], 1, "{mode}: {job}");
            assert!(
                snapshot["deny"].as_array().unwrap().iter().any(|host| host == HANDOFF_DENIED_HOST),
                "{mode}: the baseline's deny list is in the snapshot: {job}"
            );
            let text = snapshot.to_string();
            assert!(!text.contains("seed.example.com"), "{mode}: the snapshot carries no URL or caller host: {text}");
            assert!(!text.contains("http"), "{mode}: {text}");
        }
    }
}

#[tokio::test]
async fn with_the_flag_off_the_handoffs_stamp_nothing() {
    let (control, recorded) = spawn_control_stub().await;
    let addr = spawn_snapshot_edge("off", Some(HANDOFF_BASELINE), control).await;
    let (status, _) = post_json(addr, "/v1/crawl", json!({ "url": "https://seed.example.com/start" })).await;
    assert_eq!(status, 200);
    let (status, _) = post_json(addr, "/v1/batch", json!({ "urls": ["https://seed.example.com/a"] })).await;
    assert_eq!(status, 200);
    for job in recorded.lock().unwrap().iter() {
        assert!(job["params"].get("policy_snapshot").is_none(), "no stamp, job unchanged: {job}");
    }
}

#[tokio::test]
async fn a_default_policy_stamps_nothing_even_with_the_flag_on() {
    let (control, recorded) = spawn_control_stub().await;
    let addr = spawn_snapshot_edge("enforce", None, control).await;
    let (status, _) = post_json(addr, "/v1/crawl", json!({ "url": "https://seed.example.com/start" })).await;
    assert_eq!(status, 200);
    let (status, _) = post_json(addr, "/v1/batch", json!({ "urls": ["https://seed.example.com/a"] })).await;
    assert_eq!(status, 200);
    for job in recorded.lock().unwrap().iter() {
        assert!(
            job["params"].get("policy_snapshot").is_none(),
            "a policy equal to the catalogue default carries nothing a page lacks: {job}"
        );
    }
}

/// The whole chain: the snapshot a handoff stamps is what a page edge with no baseline of its
/// own (another replica, another environment) enforces for the page.
#[tokio::test]
async fn a_stamped_snapshot_binds_a_page_run_by_an_edge_with_no_baseline() {
    let (control, recorded) = spawn_control_stub().await;
    let handoff = spawn_snapshot_edge("enforce", Some(HANDOFF_BASELINE), control).await;
    let (status, _) = post_json(handoff, "/v1/crawl", json!({ "url": "https://seed.example.com/start" })).await;
    assert_eq!(status, 200);
    let stamped = recorded.lock().unwrap()[0]["params"]["policy_snapshot"].clone();
    assert!(stamped.is_object());

    let page_edge = spawn_snapshot_edge("enforce", None, String::new()).await;
    let url = format!("http://{HANDOFF_DENIED_HOST}/page");
    let (status, body) = run_page(page_edge, &url, Some(&stamped), Some(&stamped)).await;
    assert_eq!(status, 403, "the handoff's deny list reaches the page: {body}");
    let (status, body) = run_page(page_edge, SNAP_OK, Some(&stamped), Some(&stamped)).await;
    assert_eq!(status, 200, "{body}");

    // The same page edge with the flag off runs it: that is the staging state.
    let off_edge = spawn_snapshot_edge("off", None, String::new()).await;
    let (status, body) = run_page(off_edge, &url, Some(&stamped), Some(&stamped)).await;
    assert_eq!(status, 200, "{body}");
}
