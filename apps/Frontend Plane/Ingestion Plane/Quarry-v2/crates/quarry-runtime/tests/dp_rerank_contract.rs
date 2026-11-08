//! Quarry's wire contract with the Data Plane rerank route (`rerank-v1`, `POST /v1/rerank`).
//!
//! The goldens under `tests/fixtures/dp-contracts/rerank-v1/` are vendored byte for byte from the
//! Data Plane, whose `SHA256SUMS` pins them on that side too. These tests drive the real
//! `DataPlaneSearchReranker` through its public API against a mock Data Plane that answers with the
//! golden bodies (the request id echoed, as the real route echoes it), and a mock Auth Core that
//! mints tokens through the real `AuthCoreServiceTokenProvider`. Nothing is stubbed on the Quarry
//! side: the token request, the headers, the body and the reading of the answer are all the code
//! that ships.

use std::sync::Arc;
use std::time::{Duration, Instant};

use quarry_runtime::rerank::{RerankContext, RerankStatus};
use quarry_runtime::serp::SearchResult;
use quarry_runtime::service_tokens::{
    AuthCoreServiceTokenProvider, ServiceTokenAudience, ServiceTokenRequest,
    SharedServiceTokenProvider,
};
use quarry_runtime::{DataPlaneSearchReranker, SearchReranker};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const OK_GOLDEN: &str = include_str!("fixtures/dp-contracts/rerank-v1/ok.golden.json");
const SKIPPED_ZDR_GOLDEN: &str =
    include_str!("fixtures/dp-contracts/rerank-v1/skipped-zdr.golden.json");
const DEGRADED_TIMEOUT_GOLDEN: &str =
    include_str!("fixtures/dp-contracts/rerank-v1/degraded-timeout.golden.json");
const SHA256SUMS: &str = include_str!("fixtures/dp-contracts/rerank-v1/SHA256SUMS");

const ROUTE: &str = "/v1/rerank";
const MINT_ROUTE: &str = "/api/data-plane/internal-token";
const MINTED: &str = "minted.service.token";
const ORG: &str = "org-acme";

/// The Data Plane's own pin is over the bytes without carriage returns.
fn normalised(text: &str) -> String {
    text.replace('\r', "")
}

fn golden(text: &str) -> Value {
    serde_json::from_str(text).expect("a golden is JSON")
}

/// Answers like the real route: the golden body, with its request id replaced by the one the
/// caller sent. The goldens carry fixed ids, and the real route echoes the caller's.
struct Golden {
    body: Value,
    delay: Option<Duration>,
}

impl Respond for Golden {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let sent: Value = serde_json::from_slice(&request.body).expect("the request is JSON");
        let mut body = self.body.clone();
        body["request_id"] = sent["request_id"].clone();
        let mut response = ResponseTemplate::new(200).set_body_json(body);
        if let Some(delay) = self.delay {
            response = response.set_delay(delay);
        }
        response
    }
}

fn answer(text: &str) -> Golden {
    Golden {
        body: golden(text),
        delay: None,
    }
}

/// An Auth Core that mints for every request and keeps what it was asked.
async fn auth_core() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(MINT_ROUTE))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "token": MINTED,
            "expiresInSeconds": 300
        })))
        .mount(&server)
        .await;
    server
}

fn tokens(auth: &MockServer) -> SharedServiceTokenProvider {
    Arc::new(
        AuthCoreServiceTokenProvider::new(auth.uri(), "credential-long-enough")
            .expect("the provider accepts a long enough credential"),
    )
}

async fn mints(auth: &MockServer) -> Vec<Value> {
    auth.received_requests()
        .await
        .expect("request recording is on")
        .into_iter()
        .filter(|request| request.url.path() == MINT_ROUTE)
        .map(|request| serde_json::from_slice(&request.body).expect("a mint body is JSON"))
        .collect()
}

/// A Data Plane that answers `POST /v1/rerank` with `answer`, and that must receive exactly
/// `expected_calls` calls.
async fn data_plane(answer: impl Respond + 'static, expected_calls: u64) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(ROUTE))
        .respond_with(answer)
        .expect(expected_calls)
        .mount(&server)
        .await;
    server
}

/// A Data Plane that must receive no call at all.
async fn silent_data_plane() -> MockServer {
    data_plane(answer(OK_GOLDEN), 0).await
}

fn reranker(data_plane: &MockServer, auth: &MockServer) -> DataPlaneSearchReranker {
    DataPlaneSearchReranker::new(data_plane.uri(), tokens(auth)).expect("a reranker")
}

fn context<'a>(org: &'a str, request_id: &'a str, top_n: usize) -> RerankContext<'a> {
    RerankContext {
        org_id: org,
        zdr: false,
        sovereign: false,
        request_id,
        deadline: Duration::from_secs(2),
        top_n,
    }
}

/// Synthetic public candidates, in input order.
fn candidates(count: usize) -> Vec<SearchResult> {
    (0..count)
        .map(|i| SearchResult {
            url: format!("https://docs.example.test/page-{i}"),
            title: Some(format!("Page {i}")),
            snippet: Some(format!("Solar panels should be cleaned every {i} weeks.")),
            rank: i as u32 + 1,
            provider: "test".into(),
            ..Default::default()
        })
        .collect()
}

fn urls(results: &[SearchResult]) -> Vec<&str> {
    results.iter().map(|r| r.url.as_str()).collect()
}

#[test]
fn the_vendored_goldens_match_the_data_plane_pins() {
    let mut pinned = 0;
    for line in SHA256SUMS.lines().filter(|line| !line.trim().is_empty()) {
        let (digest, name) = line.split_once("  ").expect("sha256sum format");
        let content = match name {
            "ok.golden.json" => OK_GOLDEN,
            "skipped-zdr.golden.json" => SKIPPED_ZDR_GOLDEN,
            "degraded-timeout.golden.json" => DEGRADED_TIMEOUT_GOLDEN,
            other => panic!("the pins name an unvendored file: {other}"),
        };
        assert_eq!(
            format!("{:x}", Sha256::digest(normalised(content).as_bytes())),
            digest,
            "{name} differs from the Data Plane pin"
        );
        pinned += 1;
    }
    assert_eq!(pinned, 3, "every vendored golden is pinned");
}

#[test]
fn the_web_rerank_row_is_a_data_plane_request_for_data_rerank_only() {
    let request = ServiceTokenRequest::from_manifest("web-rerank").expect("the row exists");
    assert_eq!(request.audience(), ServiceTokenAudience::DataPlane);
    assert_eq!(request.scopes(), ["data:rerank".to_owned()]);
}

#[test]
fn no_rerank_code_path_asks_for_a_model_plane_token() {
    // The rerank path is these two files. A Model Plane audience, mint route or scope named in
    // either would be a rerank that bypasses the Data Plane's route table.
    let sources = [
        include_str!("../src/rerank.rs"),
        include_str!("../src/data_plane_http.rs"),
    ];
    for source in sources {
        for forbidden in [
            "ModelPlane",
            "model_plane(",
            "model-gateway",
            "models:invoke",
            "/api/model-plane",
        ] {
            assert!(
                !source.contains(forbidden),
                "the rerank path names {forbidden}"
            );
        }
    }
}

#[tokio::test]
async fn an_ok_answer_orders_the_head_by_the_data_plane_and_keeps_the_tail() {
    let auth = auth_core().await;
    let dp = data_plane(answer(OK_GOLDEN), 1).await;
    let input = candidates(5);
    let outcome = reranker(&dp, &auth)
        .rerank(
            "solar panel cleaning",
            input.clone(),
            &context(ORG, "qrr-okgolden1", 3),
        )
        .await;
    assert_eq!(outcome.report.status, RerankStatus::Applied);
    assert_eq!(outcome.report.profile.as_deref(), Some("web-v1"));
    // The golden ranks head indexes 0, 1, 2 with 1.0, 0.99, 0.98: the head keeps that order.
    assert_eq!(urls(&outcome.results), urls(&input));
    let scores: Vec<f32> = outcome.results.iter().filter_map(|r| r.score).collect();
    assert_eq!(scores.len(), 3, "exactly the head is scored");
    for (got, want) in scores.iter().zip([1.0_f32, 0.99, 0.98]) {
        assert!((got - want).abs() < 1e-6, "{got} != {want}");
    }
    assert!(
        outcome.results[3..].iter().all(|r| r.score.is_none()),
        "the tail carries no score"
    );
    let ranks: Vec<u32> = outcome.results.iter().map(|r| r.rank).collect();
    assert_eq!(ranks, vec![1, 2, 3, 4, 5]);
}

#[tokio::test]
async fn a_skipped_answer_keeps_the_order_and_says_why() {
    let auth = auth_core().await;
    let dp = data_plane(answer(SKIPPED_ZDR_GOLDEN), 1).await;
    let input = candidates(3);
    let outcome = reranker(&dp, &auth)
        .rerank(
            "solar panel cleaning",
            input.clone(),
            &context(ORG, "qrr-skipgold1", 3),
        )
        .await;
    assert_eq!(outcome.report.status, RerankStatus::Skipped);
    assert_eq!(outcome.report.reason.as_deref(), Some("zdr"));
    assert_eq!(urls(&outcome.results), urls(&input));
    assert!(outcome.results.iter().all(|r| r.score.is_none()));
}

#[tokio::test]
async fn a_degraded_answer_keeps_the_order_and_is_reported() {
    let auth = auth_core().await;
    let dp = data_plane(answer(DEGRADED_TIMEOUT_GOLDEN), 1).await;
    let input = candidates(3);
    let outcome = reranker(&dp, &auth)
        .rerank(
            "solar panel cleaning",
            input.clone(),
            &context(ORG, "qrr-degrgold1", 3),
        )
        .await;
    assert_eq!(outcome.report.status, RerankStatus::Degraded);
    assert_eq!(outcome.report.reason.as_deref(), Some("timeout"));
    assert_eq!(urls(&outcome.results), urls(&input));
    assert!(outcome.results.iter().all(|r| r.score.is_none()));
}

#[tokio::test]
async fn a_zdr_run_makes_no_data_plane_call_and_mints_no_token() {
    let auth = auth_core().await;
    let dp = silent_data_plane().await;
    let input = candidates(3);
    let mut ctx = context(ORG, "qrr-zdrrun001", 3);
    ctx.zdr = true;
    let outcome = reranker(&dp, &auth).rerank("q", input.clone(), &ctx).await;
    assert_eq!(outcome.report.status, RerankStatus::Skipped);
    assert_eq!(outcome.report.reason.as_deref(), Some("zdr"));
    assert_eq!(urls(&outcome.results), urls(&input));
    assert!(
        mints(&auth).await.is_empty(),
        "no token is minted for a ZDR run"
    );
}

#[tokio::test]
async fn a_sovereign_deployment_makes_no_data_plane_call() {
    let auth = auth_core().await;
    let dp = silent_data_plane().await;
    let mut ctx = context(ORG, "qrr-sovereign1", 3);
    ctx.sovereign = true;
    let outcome = reranker(&dp, &auth).rerank("q", candidates(3), &ctx).await;
    assert_eq!(outcome.report.reason.as_deref(), Some("sovereign"));
}

#[tokio::test]
async fn a_non_tenant_org_is_never_sent() {
    let auth = auth_core().await;
    let dp = silent_data_plane().await;
    for org in ["onboarding", "   "] {
        let outcome = reranker(&dp, &auth)
            .rerank("q", candidates(3), &context(org, "qrr-tenant0001", 3))
            .await;
        assert_eq!(
            outcome.report.reason.as_deref(),
            Some("input_not_tenant"),
            "{org:?}"
        );
    }
    assert!(mints(&auth).await.is_empty());
}

#[tokio::test]
async fn the_request_carries_the_rerank_v1_fields_and_the_web_rerank_token() {
    let auth = auth_core().await;
    let dp = data_plane(answer(OK_GOLDEN), 2).await;
    let rerank = reranker(&dp, &auth);
    let first = rerank
        .rerank(
            "  solar  panels ",
            candidates(4),
            &context(ORG, "qrr-fieldsone1", 3),
        )
        .await;
    let second = rerank
        .rerank(
            "solar panels",
            candidates(4),
            &context(ORG, "qrr-fieldstwo2", 3),
        )
        .await;
    assert_eq!(first.report.status, RerankStatus::Applied);
    assert_eq!(second.report.status, RerankStatus::Applied);

    // The token: one mint, for the web-rerank row, with the org and nothing else.
    let minted = mints(&auth).await;
    assert_eq!(
        minted,
        vec![
            json!({"orgId": ORG, "scopes": ["data:rerank"], "reason": "rank web search candidates"})
        ],
        "one mint from the row; the cached token serves the second call"
    );
    let received = dp
        .received_requests()
        .await
        .expect("request recording is on");
    assert_eq!(received.len(), 2);
    let body: Value = serde_json::from_slice(&received[0].body).unwrap();
    let mut keys: Vec<&str> = body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "deadline_ms",
            "documents",
            "profile",
            "query",
            "request_id",
            "sovereign_required",
            "top_n",
            "zdr_mode",
        ],
        "rerank-v1 refuses unknown fields, so exactly these are sent"
    );
    assert_eq!(body["profile"], "web-v1");
    assert_eq!(body["zdr_mode"], "disabled");
    assert_eq!(
        body["sovereign_required"], false,
        "the flag is always explicit"
    );
    assert_eq!(body["query"], "solar  panels", "trimmed, never reshaped");
    assert_eq!(body["top_n"], 3);
    assert_eq!(body["deadline_ms"], 2000);
    // Four candidates were offered and top_n is 3: only the head is sent. The tail is never reordered,
    // so it is never sent (it would be paid for and scored, for no result).
    let documents = body["documents"].as_array().expect("documents is an array");
    assert_eq!(documents.len(), 3, "the head only");
    assert_eq!(documents[0]["id"], "0");
    assert_eq!(documents[2]["id"], "2");
    assert_eq!(
        documents[2]["text"], "Page 2\nSolar panels should be cleaned every 2 weeks.",
        "a candidate's text is its title, a line break, then its snippet"
    );
    assert!(
        body["documents"][3].is_null(),
        "candidate 3 lies outside the head and is not sent"
    );
    assert_eq!(
        received[0]
            .headers
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap(),
        format!("Bearer {MINTED}")
    );
    assert_eq!(
        received[0]
            .headers
            .get("x-org-id")
            .unwrap()
            .to_str()
            .unwrap(),
        ORG
    );
    let again: Value = serde_json::from_slice(&received[1].body).unwrap();
    assert_ne!(
        body["request_id"], again["request_id"],
        "each call has its own request id"
    );
}

#[tokio::test]
async fn a_refused_credential_degrades_and_never_reorders() {
    let auth = auth_core().await;
    let dp = data_plane(ResponseTemplate::new(403), 1).await;
    let input = candidates(3);
    let outcome = reranker(&dp, &auth)
        .rerank("q", input.clone(), &context(ORG, "qrr-refused01", 3))
        .await;
    assert_eq!(outcome.report.status, RerankStatus::Degraded);
    assert_eq!(outcome.report.reason.as_deref(), Some("refused"));
    assert_eq!(urls(&outcome.results), urls(&input));
}

#[tokio::test]
async fn a_request_the_contract_rejects_degrades_as_a_contract_break() {
    let auth = auth_core().await;
    let dp = data_plane(ResponseTemplate::new(400), 1).await;
    let outcome = reranker(&dp, &auth)
        .rerank("q", candidates(3), &context(ORG, "qrr-contract1", 3))
        .await;
    assert_eq!(outcome.report.reason.as_deref(), Some("contract"));
}

#[tokio::test]
async fn an_answer_to_another_request_never_reorders_these_results() {
    let auth = auth_core().await;
    // The unmodified golden names its own request id, which is not the one this call sent.
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(ROUTE))
        .respond_with(ResponseTemplate::new(200).set_body_raw(OK_GOLDEN, "application/json"))
        .expect(1)
        .mount(&dp)
        .await;
    let input = candidates(3);
    let outcome = reranker(&dp, &auth)
        .rerank("q", input.clone(), &context(ORG, "qrr-wrongid01", 3))
        .await;
    assert_eq!(outcome.report.reason.as_deref(), Some("contract"));
    assert_eq!(urls(&outcome.results), urls(&input));
    assert!(outcome.results.iter().all(|r| r.score.is_none()));
}

#[tokio::test]
async fn a_slow_data_plane_is_cut_at_the_deadline() {
    let auth = auth_core().await;
    let dp = data_plane(
        Golden {
            body: golden(OK_GOLDEN),
            delay: Some(Duration::from_secs(3)),
        },
        1,
    )
    .await;
    let mut ctx = context(ORG, "qrr-slowcall1", 3);
    ctx.deadline = Duration::from_millis(300);
    let started = Instant::now();
    let outcome = reranker(&dp, &auth).rerank("q", candidates(3), &ctx).await;
    assert_eq!(outcome.report.reason.as_deref(), Some("timeout"));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the deadline bounds the call: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_401_refreshes_the_token_once_and_the_retry_is_answered() {
    let auth = auth_core().await;
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(ROUTE))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&dp)
        .await;
    Mock::given(method("POST"))
        .and(path(ROUTE))
        .respond_with(answer(OK_GOLDEN))
        .expect(1)
        .mount(&dp)
        .await;
    let outcome = reranker(&dp, &auth)
        .rerank("q", candidates(3), &context(ORG, "qrr-retry0001", 3))
        .await;
    assert_eq!(outcome.report.status, RerankStatus::Applied);
    assert_eq!(
        mints(&auth).await.len(),
        2,
        "one forced refresh after the 401"
    );
}

#[tokio::test]
async fn the_circuit_opens_after_five_failures_and_stops_calling_the_data_plane() {
    let auth = auth_core().await;
    // Exactly five calls reach the Data Plane. The sixth is answered by the open breaker.
    let dp = data_plane(ResponseTemplate::new(500), 5).await;
    let rerank = reranker(&dp, &auth);
    for call in 0..5 {
        let outcome = rerank
            .rerank(
                "q",
                candidates(3),
                &context(ORG, &format!("qrr-breaker{call:02}"), 3),
            )
            .await;
        assert_eq!(outcome.report.status, RerankStatus::Degraded, "call {call}");
    }
    let sixth = rerank
        .rerank("q", candidates(3), &context(ORG, "qrr-breaker06", 3))
        .await;
    assert_eq!(sixth.report.status, RerankStatus::Skipped);
    assert_eq!(sixth.report.reason.as_deref(), Some("circuit_open"));
}
