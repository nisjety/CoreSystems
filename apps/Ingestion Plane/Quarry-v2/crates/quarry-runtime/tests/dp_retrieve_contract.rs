//! Quarry's wire contract with the Data Plane retrieve route (`retrieve-v1`).
//!
//! The goldens under `tests/fixtures/dp-contracts/retrieve-v1/` are vendored byte
//! for byte from the Data Plane, and its `SHA256SUMS` pins them on that side too.
//! These tests drive the real `DataPlaneVectorIndex` against a mock Data Plane
//! that answers only the golden request, and assert what leaves Quarry and what
//! comes back.

use std::time::{Duration, Instant};

use quarry_core::error::ErrorCode;
use quarry_core::zdr::ZdrMode;
use quarry_runtime::vector_index::{
    DataPlaneZdrMode, SkipReason, VectorIndex, VectorOutcome, VectorQuery,
};
use quarry_runtime::DataPlaneVectorIndex;
use sha2::{Digest, Sha256};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const REQUEST_GOLDEN: &str = include_str!("fixtures/dp-contracts/retrieve-v1/request.golden.json");
const RESPONSE_GOLDEN: &str =
    include_str!("fixtures/dp-contracts/retrieve-v1/response.golden.json");
const ZDR_MODES_GOLDEN: &str =
    include_str!("fixtures/dp-contracts/retrieve-v1/zdr-modes.golden.json");
const SHA256SUMS: &str = include_str!("fixtures/dp-contracts/retrieve-v1/SHA256SUMS");

/// The org and query the goldens were written for.
const GOLDEN_ORG: &str = "org-golden";
const GOLDEN_QUERY: &str = "how often is the conveyor belt cleaned";

fn golden_query(zdr: ZdrMode) -> VectorQuery<'static> {
    VectorQuery {
        org_id: GOLDEN_ORG,
        query: GOLDEN_QUERY,
        zdr,
        limit: 10,
    }
}

fn golden_request() -> serde_json::Value {
    serde_json::from_str(REQUEST_GOLDEN).unwrap()
}

/// A Data Plane that answers the golden request with the golden response, and
/// answers nothing else.
async fn golden_data_plane(expected_calls: u64) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/retrieve"))
        .and(body_json(golden_request()))
        .respond_with(ResponseTemplate::new(200).set_body_raw(RESPONSE_GOLDEN, "application/json"))
        .expect(expected_calls)
        .mount(&server)
        .await;
    server
}

/// A Data Plane that must receive no call at all.
async fn silent_data_plane() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(RESPONSE_GOLDEN, "application/json"))
        .expect(0)
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn the_request_is_the_vendored_request_golden() {
    let server = golden_data_plane(1).await;
    let index = DataPlaneVectorIndex::new(server.uri()).expect("a vector index");
    let outcome = index
        .retrieve(&golden_query(ZdrMode::Off))
        .await
        .expect("the golden request is answered");
    assert!(matches!(outcome, VectorOutcome::Hits(_)));
}

#[tokio::test]
async fn the_response_golden_maps_to_the_one_quarry_page_under_its_real_url() {
    let server = golden_data_plane(1).await;
    let index = DataPlaneVectorIndex::new(server.uri()).expect("a vector index");
    let VectorOutcome::Hits(hits) = index.retrieve(&golden_query(ZdrMode::Off)).await.unwrap()
    else {
        panic!("the golden answer is hits");
    };
    assert_eq!(
        hits.len(),
        1,
        "only the quarry document carries a page address"
    );
    assert_eq!(hits[0].url, "https://docs.example.test/pricing?plan=pro");
    assert_eq!(hits[0].title.as_deref(), Some("Pricing"));
    assert_eq!(hits[0].document_id.as_deref(), Some("doc-quarry-pricing"));
}

#[tokio::test]
async fn zero_retention_sends_nothing() {
    let server = silent_data_plane().await;
    let index = DataPlaneVectorIndex::new(server.uri()).expect("a vector index");
    let outcome = index.retrieve(&golden_query(ZdrMode::On)).await.unwrap();
    assert_eq!(outcome, VectorOutcome::Skipped(SkipReason::Zdr));
}

#[tokio::test]
async fn a_sovereign_deployment_sends_nothing() {
    let server = silent_data_plane().await;
    let index = DataPlaneVectorIndex::new(server.uri())
        .expect("a vector index")
        .with_sovereign_posture(true);
    let outcome = index.retrieve(&golden_query(ZdrMode::Off)).await.unwrap();
    assert_eq!(outcome, VectorOutcome::Skipped(SkipReason::Sovereign));
}

#[tokio::test]
async fn a_call_without_a_tenant_sends_nothing() {
    let server = silent_data_plane().await;
    let index = DataPlaneVectorIndex::new(server.uri()).expect("a vector index");
    let query = VectorQuery {
        org_id: "  ",
        ..golden_query(ZdrMode::Off)
    };
    let outcome = index.retrieve(&query).await.unwrap();
    assert_eq!(outcome, VectorOutcome::Skipped(SkipReason::NoOrg));
}

#[tokio::test]
async fn the_hourly_budget_stops_calls_without_a_request() {
    let server = golden_data_plane(1).await;
    let index = DataPlaneVectorIndex::new(server.uri())
        .expect("a vector index")
        .with_hourly_budget(1);
    let first = index.retrieve(&golden_query(ZdrMode::Off)).await.unwrap();
    assert!(matches!(first, VectorOutcome::Hits(_)));
    let second = index.retrieve(&golden_query(ZdrMode::Off)).await.unwrap();
    assert_eq!(second, VectorOutcome::Skipped(SkipReason::Budget));
}

#[tokio::test]
async fn a_refused_credential_is_an_error_and_not_an_empty_answer() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let index = DataPlaneVectorIndex::new(server.uri()).expect("a vector index");
    let error = index
        .retrieve(&golden_query(ZdrMode::Off))
        .await
        .expect_err("a refusal must not look like a search with no results");
    assert_eq!(error.code, ErrorCode::UpstreamBlocked);
    assert!(error.message.contains("403"), "{}", error.message);
}

#[tokio::test]
async fn a_rejected_request_is_an_error_and_not_an_empty_answer() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(422))
        .mount(&server)
        .await;
    let index = DataPlaneVectorIndex::new(server.uri()).expect("a vector index");
    let error = index
        .retrieve(&golden_query(ZdrMode::Off))
        .await
        .expect_err("a contract rejection must not look like a search with no results");
    assert_eq!(error.code, ErrorCode::UpstreamBlocked);
    assert!(error.message.contains("422"), "{}", error.message);
}

#[tokio::test]
async fn an_answer_that_breaks_the_contract_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"candidates":[{"text":"no document id"}],"sources":[]}"#,
            "application/json",
        ))
        .mount(&server)
        .await;
    let index = DataPlaneVectorIndex::new(server.uri()).expect("a vector index");
    assert!(index.retrieve(&golden_query(ZdrMode::Off)).await.is_err());
}

#[tokio::test]
async fn a_slow_data_plane_is_cut_at_the_deadline() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(RESPONSE_GOLDEN, "application/json")
                .set_delay(Duration::from_secs(3)),
        )
        .mount(&server)
        .await;
    let index = DataPlaneVectorIndex::new(server.uri())
        .expect("a vector index")
        .with_timeout(Duration::from_millis(200))
        .expect("a client with a short deadline");
    let started = Instant::now();
    let error = index
        .retrieve(&golden_query(ZdrMode::Off))
        .await
        .expect_err("a call past its deadline fails");
    assert_eq!(error.code, ErrorCode::Timeout);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the deadline bounds the call: {:?}",
        started.elapsed()
    );
}

#[test]
fn the_vendored_copies_match_the_data_plane_pins() {
    let mut pinned = 0;
    for line in SHA256SUMS.lines().filter(|line| !line.trim().is_empty()) {
        let (digest, name) = line.split_once("  ").expect("sha256sum format");
        let content = match name {
            "request.golden.json" => REQUEST_GOLDEN,
            "response.golden.json" => RESPONSE_GOLDEN,
            "zdr-modes.golden.json" => ZDR_MODES_GOLDEN,
            other => panic!("the pins name an unvendored file: {other}"),
        };
        assert_eq!(
            format!("{:x}", Sha256::digest(content.as_bytes())),
            digest,
            "{name} differs from the Data Plane pin"
        );
        pinned += 1;
    }
    assert_eq!(pinned, 3, "every vendored golden is pinned");
}

#[test]
fn the_zdr_mapping_agrees_with_the_vendored_golden() {
    let golden: serde_json::Value = serde_json::from_str(ZDR_MODES_GOLDEN).unwrap();
    let accepted: Vec<&str> = golden["accepted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["value"].as_str().unwrap())
        .collect();
    let refused: Vec<&str> = golden["refused_examples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(accepted, vec!["disabled", "reject", "ephemeral"]);
    assert!(
        refused.contains(&"off"),
        "the literal Quarry used to send is on the refused list"
    );
    for zdr in [ZdrMode::Off, ZdrMode::On] {
        let wire = DataPlaneZdrMode::from(zdr).as_wire();
        assert!(
            accepted.contains(&wire),
            "{wire} is not an accepted zdr_mode"
        );
        assert!(!refused.contains(&wire), "{wire} is a refused zdr_mode");
    }
    assert_eq!(
        golden["default_when_absent"].as_str(),
        Some(DataPlaneZdrMode::from(ZdrMode::Off).as_wire())
    );
}
