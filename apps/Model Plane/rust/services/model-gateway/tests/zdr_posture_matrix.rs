//! The posture matrix, run through the gateway's own decision and normalisation.
//!
//! `fixtures/zdr-posture-matrix.json` is a byte-identical copy of the canonical matrix, pinned
//! beside it and checked by `scripts/tests/zdr-contract-sync.test.sh`. Every enforcement row here is
//! built the way a request reaches the gateway, with no hand-made decision:
//!
//! * each bearer is a real `Claims` deserialised from the token payload shape Auth Core mints (the
//!   serde path the middleware uses, so an absent claim is absent, not defaulted);
//! * the request is an `InvokeRequest` parsed from the wire body the row names;
//! * the route is `zdr_posture::Route::named`, exactly as the stream and unary entry points build it;
//! * the decision is `zdr_posture::decide`, the call both entry points make first; and for a turn that
//!   is not refused, the request must also normalise through `normalize::normalize_stream` (the stream
//!   entry) and `normalize::normalize` (the unary entry).
//!
//! The row's expected decision and code must be what those calls return. A row that the posture
//! refuses must never reach normalisation, so it is not checked there.

use model_gateway::auth::Claims;
use model_gateway::http_routes::InvokeRequest;
use model_gateway::normalize::{self, AgenticSubscriptionPolicy};
use model_gateway::sse_events::RetentionClass;
use model_gateway::zdr_posture::{decide, Route, TurnPosture};
use mp_contracts::retention::WAIVER_CONTRACT_VERSION;
use serde_json::{json, Value};

const MATRIX: &str = include_str!("fixtures/zdr-posture-matrix.json");

/// The number of enforcement rows the canonical matrix carries when this test was written. A row
/// added or removed without this test being updated is a change nobody reviewed here.
const ENFORCEMENT_ROWS: usize = 27;

fn matrix() -> Value {
    serde_json::from_str(MATRIX).expect("the vendored matrix is JSON")
}

/// The claims of one bearer, as the token payload Auth Core mints carries them.
fn bearer_claims(bearer: &Value) -> Claims {
    let mut payload = json!({
        "sub": "user-1",
        "iss": "https://auth.test/model",
        "aud": "model-gateway",
        "exp": 4_102_444_800_i64,
        "nbf": 1_700_000_000_i64,
        "org_id": "org-1",
        "user_id": "user-1",
        "principal_type": "user",
        "zdr": bearer["zdr"],
    });
    if !bearer["org_zdr"].is_null() {
        payload["org_zdr"] = bearer["org_zdr"].clone();
    }
    if !bearer["waiver"].is_null() {
        payload["zdr_waiver"] = bearer["waiver"].clone();
    }
    serde_json::from_value(payload).expect("a minted token's payload is valid Claims")
}

/// The request body a row's route names, as a client sends it.
fn request_body(route: &Value) -> Value {
    let mut body = json!({"content": "hello", "model": "gpt-5.6-terra"});
    let provider = route["provider"].as_str().unwrap_or("");
    if !provider.is_empty() {
        body["provider"] = json!(provider);
    }
    if let Some(connection) = route["connection"].as_str() {
        body["subscription_connection_id"] = json!(connection);
    }
    if route["explicit_zdr"].as_bool() == Some(true) {
        body["zdr"] = json!(true);
    }
    body
}

/// What the posture decides for a row, in the matrix's own words: `(decision, code)`.
fn decided(row: &Value) -> (String, String) {
    let claims: Vec<Claims> = row["inputs"]["bearers"]
        .as_array()
        .expect("bearers are an array")
        .iter()
        .map(bearer_claims)
        .collect();
    let refs: Vec<&Claims> = claims.iter().collect();
    let request: InvokeRequest =
        serde_json::from_value(request_body(&row["inputs"]["route"])).expect("the body parses");
    let route = Route::named(
        request.provider.as_deref(),
        request.subscription_connection_id.as_deref(),
        request.zdr,
    );
    match decide(&refs, &route) {
        Err(refusal) => ("refuse".to_owned(), refusal.code().to_owned()),
        Ok(TurnPosture::Zdr { reason }) => ("zdr".to_owned(), reason.as_str().to_owned()),
        Ok(TurnPosture::Persistent) => ("persistent".to_owned(), "org_zdr_off".to_owned()),
        Ok(TurnPosture::Waived { reason, .. }) => ("waived".to_owned(), reason.as_str().to_owned()),
    }
}

#[test]
fn the_vendored_matrix_has_the_rows_this_test_was_written_for() {
    let matrix = matrix();
    assert_eq!(matrix["contract"], "zdr.posture-matrix");
    assert_eq!(
        matrix["waiver_contract_version"].as_u64(),
        Some(u64::from(WAIVER_CONTRACT_VERSION)),
        "the gateway's waiver contract version is the matrix's"
    );
    assert_eq!(
        matrix["enforcement_rows"].as_array().map(Vec::len),
        Some(ENFORCEMENT_ROWS),
        "a changed matrix must be reviewed here"
    );
}

/// Every enforcement row, through the decision the entry points make, then normalisation.
#[test]
fn every_enforcement_row_is_decided_and_normalised_as_the_matrix_says() {
    let matrix = matrix();
    let rows = matrix["enforcement_rows"]
        .as_array()
        .expect("rows are an array");
    let policy = AgenticSubscriptionPolicy::from_env();
    let mut failures = Vec::new();
    for row in rows {
        let id = row["id"].as_str().unwrap_or("?");
        let expect_decision = row["expect"]["decision"].as_str().unwrap_or("");
        let expect_code = row["expect"]["code"].as_str().unwrap_or("");
        let (decision, code) = decided(row);
        if decision != expect_decision || code != expect_code {
            failures.push(format!(
                "{id}: decided {decision}/{code}, the matrix says {expect_decision}/{expect_code}"
            ));
            continue;
        }
        if decision == "refuse" {
            continue;
        }
        // A turn the posture serves must also normalise on both entry points.
        let request: InvokeRequest =
            serde_json::from_value(request_body(&row["inputs"]["route"])).expect("the body parses");
        if let Err((status, body)) = normalize::normalize_stream(&request, policy) {
            failures.push(format!(
                "{id}: the stream entry refused it: {status} {}",
                body.0
            ));
        }
        if let Err((status, body)) = normalize::normalize(&request) {
            failures.push(format!(
                "{id}: the unary entry refused it: {status} {}",
                body.0
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The usage frame states a turn's class from the same decision: `zdr` for a ZDR turn,
/// `zdr_waived` (with its reason) for a waived one, and nothing for a persistent turn, whose frames
/// have never carried the key.
#[test]
fn the_usage_frame_states_the_decided_class_and_nothing_for_a_persistent_turn() {
    let matrix = matrix();
    let rows = matrix["enforcement_rows"]
        .as_array()
        .expect("rows are an array");
    for row in rows {
        let id = row["id"].as_str().unwrap_or("?");
        let claims: Vec<Claims> = row["inputs"]["bearers"]
            .as_array()
            .expect("bearers are an array")
            .iter()
            .map(bearer_claims)
            .collect();
        let refs: Vec<&Claims> = claims.iter().collect();
        let request: InvokeRequest =
            serde_json::from_value(request_body(&row["inputs"]["route"])).expect("the body parses");
        let route = Route::named(
            request.provider.as_deref(),
            request.subscription_connection_id.as_deref(),
            request.zdr,
        );
        let Ok(posture) = decide(&refs, &route) else {
            continue;
        };
        let class = posture.retention_class();
        match row["expect"]["decision"].as_str() {
            Some("zdr") => assert_eq!(class, Some(RetentionClass::Zdr), "{id}"),
            Some("waived") => assert_eq!(
                class,
                Some(RetentionClass::ZdrWaived {
                    reason: "connection_not_zdr_capable",
                }),
                "{id}"
            ),
            Some("persistent") => assert_eq!(class, None, "{id}"),
            other => panic!("{id}: unexpected decision {other:?}"),
        }
        // A waived turn is persistent for its connection and filters restricted documents.
        if posture.is_waived() {
            assert!(!posture.zdr(), "{id}: a waived turn is not ZDR downstream");
            assert_eq!(posture.data_plane_mode().as_deref(), Some("reject"), "{id}");
        }
    }
}

/// The rows the matrix tags as the properties this gateway enforces. Each has at least one row, so
/// the test cannot pass by having none of them.
#[test]
fn the_safety_tagged_rows_are_present_and_decided_as_tagged() {
    let matrix = matrix();
    let rows = matrix["enforcement_rows"]
        .as_array()
        .expect("rows are an array");
    for tag in [
        "silent_reroute",
        "no_unauthorized_persistent",
        "non_zdr_unchanged",
        "mixed_zdr_token_set",
    ] {
        assert!(
            rows.iter().any(|row| row["tags"]
                .as_array()
                .is_some_and(|tags| tags.iter().any(|t| t == tag))),
            "the matrix has no row tagged {tag}"
        );
    }
    for row in rows {
        let tags = row["tags"].as_array().cloned().unwrap_or_default();
        if tags
            .iter()
            .any(|t| t == "silent_reroute" || t == "no_unauthorized_persistent")
        {
            let (decision, _) = decided(row);
            assert_eq!(
                decision, "refuse",
                "{}: a ZDR turn or an unwaived persistent ZDR-org turn must never be served",
                row["id"]
            );
        }
    }
}
