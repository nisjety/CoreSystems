//! WS7.E: web policy on the remaining routes (search, extract, map, crawl, batch and
//! profile create), decided without a server. Written in the build phase; run in the
//! wave's test phase (`cargo test -p quarry-edge --test web_policy_routes`).
//!
//! The decisions each route takes are asserted here. The route-level cases the plan also
//! names (an orchestrator double that records zero handoff calls, the 403 body through
//! the router, the `web_policy_downgraded_total` counter read through a recorder) need the
//! real-token harness of `zdr_claim_floor.rs` and are written in the test phase.

use quarry_core::error::ErrorCode;
use quarry_core::web_policy::{zdr_forces_disabled, CapKey};
use quarry_edge::auth::Claims;
use quarry_edge::policy_baseline::{PolicyBaseline, BASELINE_ENV};
use quarry_edge::web_policy_edge::{decide, gate_url, gate_urls, paid_provider};

fn claims(org_id: &str) -> Claims {
    Claims {
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
        web_policy: None,
    }
}

/// An operator baseline from its wire document.
fn baseline(json: &str) -> PolicyBaseline {
    PolicyBaseline::from_lookup(|name| (name == BASELINE_ENV).then(|| json.to_string()))
        .expect("the test baseline is a valid layer")
}

#[test]
fn default_policy_changes_nothing_on_the_remaining_routes() {
    let policy = PolicyBaseline::default();
    let decision = decide(&policy, &claims("org_alpha"), false, None).expect("default decides");
    let paid = paid_provider(&decision, true, false);
    assert!(paid.allowed, "a paid-provider request is honoured under the default policy");
    assert_eq!(paid.downgraded_by, None);
    gate_url(&decision, "https://example.org/seed").expect("the default admits the seed");
    gate_urls(&decision, ["https://example.org/a", "https://example.net/b"])
        .expect("the default admits every URL");
    decision
        .policy()
        .require(CapKey::ProfilePersistent, "profile.create")
        .expect("the default allows a persistent profile");
}

#[test]
fn paid_provider_disabled_by_org_is_downgraded_and_disclosed() {
    // The operator layer stands in for the org layer here: both are layers of the same
    // lattice and both name their source in the downgrade.
    let policy = baseline(r#"{"modes":{"search.paid_provider":"disabled"}}"#);
    let decision = decide(&policy, &claims("org_alpha"), false, None).expect("decides");

    let asked = paid_provider(&decision, true, false);
    assert!(!asked.allowed, "the policy removes paid providers");
    assert_eq!(
        asked.downgraded_by,
        Some("baseline"),
        "the downgrade names the layer that removed the permission"
    );

    // A request that did not ask for paid providers has nothing to disclose.
    let not_asked = paid_provider(&decision, false, false);
    assert!(!not_asked.allowed);
    assert_eq!(not_asked.downgraded_by, None);
}

#[test]
fn paid_provider_still_off_under_zdr_whatever_the_policy_says() {
    // Under ZDR the request is never a policy downgrade: the floor was always there.
    let permissive = PolicyBaseline::default();
    let decision = decide(&permissive, &claims("org_alpha"), true, None).expect("decides");
    let paid = paid_provider(&decision, true, true);
    assert!(!paid.allowed);
    assert_eq!(paid.downgraded_by, None, "ZDR is not reported as a policy downgrade");
}

#[test]
fn extract_denied_url_is_403_without_echoing_urls() {
    let policy = baseline(r#"{"domains":{"deny":["example.net"]}}"#);
    let decision = decide(&policy, &claims("org_alpha"), false, None).expect("decides");
    let error = gate_urls(
        &decision,
        [
            "https://example.org/",
            "https://example.net/private/page?token=s3cret",
        ],
    )
    .expect_err("one URL is denied, so the whole request is refused");

    assert_eq!(error.code, ErrorCode::Forbidden);
    let details = error.details.clone().expect("the refusal carries details");
    assert_eq!(details["capability"], "domains.deny");
    assert_eq!(details["denied_count"], 1);

    // No URL, host, path or query in the message or the details.
    let shown = format!("{} {}", error.message, details);
    assert!(!shown.contains("example.net"), "host echoed: {shown}");
    assert!(!shown.contains("private"), "path echoed: {shown}");
    assert!(!shown.contains("s3cret"), "query echoed: {shown}");
}

#[test]
fn crawl_and_batch_deny_a_denied_seed_before_the_handoff() {
    // The routes gate the seed (crawl) and every URL (batch) before the handoff; the
    // orchestrator double of the test phase records zero calls. The decisions are here.
    let policy = baseline(r#"{"domains":{"deny":["example.net"]}}"#);
    let decision = decide(&policy, &claims("org_alpha"), false, None).expect("decides");

    let seed = gate_url(&decision, "https://example.net/seed").expect_err("the seed is denied");
    assert_eq!(seed.code, ErrorCode::Forbidden);

    let batch = gate_urls(&decision, ["https://example.org/a", "https://example.net/b"])
        .expect_err("one batch URL is denied");
    assert_eq!(batch.details.as_ref().expect("details")["denied_count"], 1);

    gate_urls(&decision, ["https://example.org/a"]).expect("an allowed batch passes");
}

#[test]
fn profile_create_denied_when_policy_disables_persistence() {
    let policy = baseline(r#"{"modes":{"browser.profile.persistent":"disabled"}}"#);
    let decision = decide(&policy, &claims("org_alpha"), false, None).expect("decides");
    let denial = decision
        .policy()
        .require(CapKey::ProfilePersistent, "profile.create")
        .expect_err("persistence is disabled by the baseline");
    assert_eq!(denial.code(), "policy_denied");
}

#[test]
fn zdr_implications_match_the_legacy_refusals() {
    // ZDR forces persistence and paid providers off. The legacy refusals (profile create,
    // paid search) stay in place; the policy's ZDR layer says the same thing about them.
    assert!(zdr_forces_disabled(CapKey::ProfilePersistent));
    assert!(zdr_forces_disabled(CapKey::SearchPaidProvider));

    let decision = decide(&PolicyBaseline::default(), &claims("org_alpha"), true, None)
        .expect("decides");
    assert!(!decision.policy().allows(CapKey::ProfilePersistent));
    assert!(!decision.policy().allows(CapKey::SearchPaidProvider));
}

// ---------------------------------------------------------------------------------------
// D2-018: every route that takes a URL to fetch is gated by the web policy.
//
// Two checks. (1) Through the real router, with a policy that denies one host, every
// fetching route refuses a URL on that host and no driver call, vector lookup or
// handoff happens. (2) The route table of routes.rs is read as text and every path in
// it must be classified below, so a new route cannot be added without saying whether
// it takes a URL and where the policy gate is.
// ---------------------------------------------------------------------------------------

mod support;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use quarry_runtime::driver::Driver;
use quarry_runtime::fetch::FetchResponse;
use quarry_runtime::vector_index::{VectorIndex, VectorOutcome, VectorQuery};
use quarry_runtime::EventSink;
use serde_json::{json, Value};
use tokio::sync::mpsc;

const SWEEP_BEARER: &str = "web-policy-sweep-dev-token";
const SWEEP_RUNTIME_TOKEN: &str = "web-policy-sweep-runtime-token";
const SWEEP_INTERNAL_SECRET: &str = "web-policy-sweep-internal-secret-0123456789";
const DENIED_HOST: &str = "denied.example";

/// `X-Quarry-Run-Sig`: hex(HMAC-SHA256(secret, org_id, a line feed, then url)), the org
/// binding the orchestrator sends on `/v1/internal/run_page`.
fn run_sig(org: &str, url: &str) -> String {
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(SWEEP_INTERNAL_SECRET.as_bytes()).unwrap();
    mac.update(format!("{org}{}{url}", '\u{a}').as_bytes());
    format!("{:x}", mac.finalize().into_bytes())
}

/// Every call the routes make toward the network or a store of pages.
#[derive(Default)]
struct Reach {
    fetches: AtomicUsize,
    lookups: AtomicUsize,
    handoffs: AtomicUsize,
}

struct CountingDriver(Arc<Reach>);

#[async_trait]
impl Driver for CountingDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }
    async fn fetch(&self, url: &url::Url) -> QuarryResult<FetchResponse> {
        self.0.fetches.fetch_add(1, Ordering::SeqCst);
        Ok(FetchResponse {
            status: 200,
            final_url: url.clone(),
            headers: vec![],
            body: b"<html><body><p>hello</p></body></html>".to_vec(),
            duration_ms: 1,
            served_by: DriverKind::Static,
            captures: None,
        })
    }
}

struct CountingIndex(Arc<Reach>);

#[async_trait]
impl VectorIndex for CountingIndex {
    async fn retrieve(&self, _query: &VectorQuery<'_>) -> QuarryResult<VectorOutcome> {
        self.0.lookups.fetch_add(1, Ordering::SeqCst);
        Ok(VectorOutcome::Hits(Vec::new()))
    }
}

/// A control double that counts the jobs it is handed.
async fn spawn_counting_control(reach: Arc<Reach>) -> String {
    use axum::{routing::post, Json, Router};
    let app = Router::new().route(
        "/v1/jobs",
        post(move || {
            let reach = reach.clone();
            async move {
                reach.handoffs.fetch_add(1, Ordering::SeqCst);
                Json(json!({ "data": { "id": "job_1" } }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn spawn_sweep_edge() -> (SocketAddr, Arc<Reach>) {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    std::env::set_var("QUARRY_EDGE__RUNTIME_AUTH_TOKEN", SWEEP_RUNTIME_TOKEN);
    let reach = Arc::new(Reach::default());
    let control = spawn_counting_control(reach.clone()).await;
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(CountingDriver(reach.clone()));
    let state = quarry_edge::state::AppState {
        policy: baseline(&format!(r#"{{"domains":{{"deny":["{DENIED_HOST}"]}}}}"#)),
        vector_index: Some(Arc::new(CountingIndex(reach.clone()))),
        control_base_url: control,
        internal_signer: Some(Arc::new(
            quarry_edge::internal_auth::InternalSigner::new(SWEEP_INTERNAL_SECRET).unwrap(),
        )),
        ..support::base_state(driver, EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (addr, reach)
}

async fn sweep_post(addr: SocketAddr, path: &str, body: Value) -> (u16, String) {
    let request = reqwest::Client::new()
        .post(format!("http://{addr}{path}"))
        .timeout(Duration::from_secs(20))
        .json(&body);
    let request = if path == "/v1/internal/run_page" {
        let url = body["url"].as_str().unwrap_or_default();
        let org = body["org_id"].as_str().unwrap_or_default();
        request
            .bearer_auth(SWEEP_RUNTIME_TOKEN)
            .header("x-quarry-run-sig", run_sig(org, url))
    } else {
        request.bearer_auth(SWEEP_BEARER)
    };
    let response = request.send().await.unwrap();
    let status = response.status().as_u16();
    (status, response.text().await.unwrap())
}

/// The fetching routes and a body that names a URL on the denied host.
fn fetching_routes() -> Vec<(&'static str, Value)> {
    let url = format!("https://{DENIED_HOST}/private/page");
    vec![
        ("/v1/scrape", json!({ "url": url })),
        ("/v1/scrape/stream", json!({ "url": url })),
        ("/v1/crawl", json!({ "url": url })),
        ("/v1/batch", json!({ "urls": [url] })),
        ("/v1/map", json!({ "url": url })),
        ("/v1/extract", json!({ "urls": [url] })),
        ("/v1/search/similar", json!({ "url": url })),
        (
            "/v1/internal/run_page",
            json!({ "url": url, "org_id": "org_sweep" }),
        ),
    ]
}

#[tokio::test]
async fn every_fetching_route_refuses_a_denied_host_before_it_reaches_anything() {
    let (addr, reach) = spawn_sweep_edge().await;
    for (path, body) in fetching_routes() {
        let (status, text) = sweep_post(addr, path, body).await;
        if path == "/v1/scrape/stream" {
            // The stream answers 200 and carries the refusal as its first `error` event.
            assert_eq!(status, 200, "{path}: {text}");
            assert!(text.contains("event: error"), "{path}: {text}");
            assert!(text.to_ascii_lowercase().contains("forbidden"), "{path}: {text}");
        } else {
            assert_eq!(status, 403, "{path} did not refuse the denied host: {text}");
        }
        // A single-URL refusal may name the host the caller sent (the policy's own detail);
        // the path and query never come back.
        assert!(!text.contains("private/page"), "{path} echoed the denied path: {text}");
    }
    assert_eq!(reach.fetches.load(Ordering::SeqCst), 0, "a denied URL reached a driver");
    assert_eq!(reach.lookups.load(Ordering::SeqCst), 0, "a denied seed reached the index");
    assert_eq!(reach.handoffs.load(Ordering::SeqCst), 0, "a denied URL was handed to control");
}

#[tokio::test]
async fn the_same_routes_still_serve_an_allowed_host() {
    // Not vacuous: the refusals above are the policy's, not a route that refuses everything.
    let (addr, reach) = spawn_sweep_edge().await;
    let ok = "https://allowed.example/page";
    let (status, text) = sweep_post(addr, "/v1/scrape", json!({ "url": ok })).await;
    assert_eq!(status, 200, "{text}");
    let (status, text) = sweep_post(addr, "/v1/extract", json!({ "urls": [ok] })).await;
    assert_eq!(status, 200, "{text}");
    let (status, text) = sweep_post(addr, "/v1/crawl", json!({ "url": ok })).await;
    assert_eq!(status, 200, "{text}");
    assert!(reach.fetches.load(Ordering::SeqCst) >= 2);
    assert_eq!(reach.handoffs.load(Ordering::SeqCst), 1);
}

/// How a route of the table relates to the web policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gate {
    /// Takes a URL the edge fetches or hands on, and gates it (proved by the router test above).
    Gated,
    /// Takes no URL to fetch (health, reads of stored data, lifecycle of an existing job).
    NoUrl,
    /// A URL is stored here and fetched later by the orchestrator, whose page call
    /// (`/v1/internal/run_page`, gated) decides at fetch time, when the policy is current.
    GatedAtFireTime,
    /// The URL reaches a service of another plane or a pipeline the edge does not gate;
    /// recorded in depth/D-D-B3-egress-caps.md.
    OtherPlane,
}

fn route_manifest() -> Vec<(&'static str, Gate)> {
    use Gate::*;
    vec![
        ("/health", NoUrl),
        ("/ready", NoUrl),
        ("/version", NoUrl),
        ("/graphql/schema", NoUrl),
        ("/graphql/playground", NoUrl),
        ("/graphql", NoUrl),
        ("/v1/internal/change/record", NoUrl),
        ("/v1/internal/page-images/:org/:doc/:page/:hash", NoUrl),
        ("/v1/internal/run_page", Gated),
        ("/v1/internal/schedules/preflight", NoUrl),
        ("/v1/internal/orgs/:org/profiles", NoUrl),
        ("/v1/profiles", NoUrl),
        ("/v1/profiles/create", NoUrl),
        ("/v1/profiles/:id", NoUrl),
        // Reads the stored snapshot and echoes the URL; nothing is fetched.
        ("/v1/profiles/:id/restore_probe", NoUrl),
        ("/v1/scrape", Gated),
        ("/v1/scrape/stream", Gated),
        ("/v1/scrape/:id", NoUrl),
        ("/v1/crawl", Gated),
        ("/v1/batch", Gated),
        ("/v1/crawl/ongoing", NoUrl),
        ("/v1/crawl/:id", NoUrl),
        ("/v1/crawl/:id/errors", NoUrl),
        ("/v1/crawl/:id/pause", NoUrl),
        ("/v1/crawl/:id/resume", NoUrl),
        ("/v1/crawl/:id/events", NoUrl),
        ("/v1/batch/ongoing", NoUrl),
        ("/v1/batch/:id", NoUrl),
        ("/v1/batch/:id/errors", NoUrl),
        ("/v1/batch/:id/pause", NoUrl),
        ("/v1/batch/:id/resume", NoUrl),
        ("/v1/batch/:id/events", NoUrl),
        ("/v1/jobs/:id", NoUrl),
        ("/v1/jobs/:id/errors", NoUrl),
        ("/v1/jobs/:id/events", NoUrl),
        ("/v1/runs/:id/events", NoUrl),
        // `audio_url` goes to the Model Plane, which fetches it.
        ("/v1/audio", OtherPlane),
        ("/v1/search", NoUrl),
        ("/v1/search/images", NoUrl),
        ("/v1/search/videos", NoUrl),
        ("/v1/search/similar", Gated),
        ("/v1/search/suggest", NoUrl),
        ("/v1/map", Gated),
        ("/v1/extract", Gated),
        // The pipeline fetches the pages its own search returned; no caller URL.
        ("/v1/answer", OtherPlane),
        ("/v1/answer/stream", OtherPlane),
        ("/v1/artifacts", NoUrl),
        ("/v1/artifacts/:id", NoUrl),
        ("/v1/sources", GatedAtFireTime),
        ("/v1/sources/:id", NoUrl),
        ("/v1/snapshots", NoUrl),
        ("/v1/:kind/jobs", NoUrl),
        ("/v1/request-queues", NoUrl),
        ("/v1/benchmarks", NoUrl),
        ("/v1/team/credit-usage", NoUrl),
        ("/v1/team/token-usage", NoUrl),
        ("/v1/team/concurrency", NoUrl),
        ("/v1/team/queue-status", NoUrl),
        ("/v1/team/activity", NoUrl),
        // Compares a fingerprint the caller sends; no fetch.
        ("/v1/change/check", NoUrl),
        ("/v1/change/latest", NoUrl),
        ("/v1/change/history", NoUrl),
        ("/v1/change/snapshot", NoUrl),
        // Queues a URL for a worker; gated before it is queued (the postgres-queue handler).
        ("/v1/change/refresh", Gated),
        ("/v1/schedules", GatedAtFireTime),
        ("/v1/schedules/:id", NoUrl),
        ("/v1/schedules/:id/pause", NoUrl),
        ("/v1/schedules/:id/unpause", NoUrl),
        ("/v1/schedules/:id/trigger", GatedAtFireTime),
        ("/v1/schedules/:id/backfill", GatedAtFireTime),
    ]
}

/// The paths of the `.route("...")` calls in a source text.
fn route_paths(source: &str) -> std::collections::BTreeSet<String> {
    let mut paths = std::collections::BTreeSet::new();
    let mut rest = source;
    while let Some(at) = rest.find(".route(") {
        rest = &rest[at + ".route(".len()..];
        if let Some(inner) = rest.trim_start().strip_prefix('"') {
            if let Some(end) = inner.find('"') {
                paths.insert(inner[..end].to_string());
            }
        }
    }
    paths
}

#[test]
fn every_route_of_the_edge_table_is_classified_for_the_web_policy() {
    let source = include_str!("../src/routes.rs");
    let table = route_paths(source);
    assert!(table.len() > 60, "the route scan found only {} paths", table.len());
    let classified: std::collections::BTreeSet<String> = route_manifest()
        .iter()
        .map(|(path, _)| (*path).to_string())
        .collect();
    let unclassified: Vec<&String> = table.difference(&classified).collect();
    assert!(
        unclassified.is_empty(),
        "routes with no web-policy classification (add them to route_manifest and, when they \
         take a URL to fetch, to fetching_routes): {unclassified:?}"
    );
    let stale: Vec<&String> = classified.difference(&table).collect();
    assert!(stale.is_empty(), "classified paths that are not in the route table: {stale:?}");
}

#[test]
fn every_gated_route_has_a_router_refusal_case_except_the_feature_gated_one() {
    let tested: std::collections::BTreeSet<&str> =
        fetching_routes().into_iter().map(|(path, _)| path).collect();
    let manifest = route_manifest();
    for (path, gate) in &manifest {
        if *gate == Gate::Gated && *path != "/v1/change/refresh" {
            assert!(tested.contains(path), "{path} is gated but has no refusal case");
        }
    }
    for path in &tested {
        assert!(
            manifest.iter().any(|(p, g)| p == path && *g == Gate::Gated),
            "{path} has a refusal case but is not classified Gated"
        );
    }
}
