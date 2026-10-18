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

/// Whether one bearer, by itself, is a persistent credential, written from the matrix's own rule
/// and not from the gateway's code: it is not ZDR, its org is not ZDR (an absent org claim is an
/// ordinary service token), and it carries no waiver.
fn bearer_is_persistent(bearer: &Value) -> bool {
    bearer["zdr"] == false && bearer["org_zdr"] != true && bearer["waiver"].is_null()
}

/// `POST /v1/orchestration/runs/{run_id}/continue` states no route: the request names no provider,
/// no connection and no ZDR flag, so the gateway decides it with `Route::plain(false)` from the
/// caller's single bearer. This is that decision, over every bearer of every enforcement row.
///
/// A continuation consumes an approval and resumes stored work, and a waived turn is persistent for
/// its connection's inference only. So the route is served to a persistent bearer and to nothing
/// else: a ZDR bearer is not served, a waived bearer (whose `zdr` claim is FALSE, so a check written
/// as "not ZDR" would serve it) is not served, and a bearer the resolver refuses is not served.
#[test]
fn a_continue_is_served_to_a_persistent_bearer_on_every_row_and_to_nothing_else() {
    let matrix = matrix();
    let rows = matrix["enforcement_rows"]
        .as_array()
        .expect("rows are an array");
    let (mut served, mut waived_refused, mut zdr_refused) = (0_usize, 0_usize, 0_usize);
    for row in rows {
        let id = row["id"].as_str().unwrap_or("?");
        let bearers = row["inputs"]["bearers"]
            .as_array()
            .expect("bearers are an array");
        let continue_serves = |bearer: &Value| {
            matches!(
                model_gateway::zdr_posture::decide_for(
                    &bearer_claims(bearer),
                    &Route::plain(false)
                ),
                Ok(TurnPosture::Persistent)
            )
        };
        for bearer in bearers {
            let serves = continue_serves(bearer);
            assert_eq!(
                serves,
                bearer_is_persistent(bearer),
                "{id}: /continue must serve a bearer exactly when it is persistent (bearer {bearer})"
            );
            if serves {
                served += 1;
            } else if !bearer["waiver"].is_null() {
                // The trap the matrix exists to catch: a waived bearer whose zdr claim is false
                // is "not ZDR" and still must not be served.
                if bearer["zdr"] == false {
                    waived_refused += 1;
                }
            } else if bearer["zdr"] == true {
                zdr_refused += 1;
            }
        }
        // The matrix's own words for the same turn, where /continue and the row name the same thing:
        // one bearer, and a row that does not ask for ZDR itself (a continuation cannot).
        if bearers.len() == 1 && row["inputs"]["route"]["explicit_zdr"] != true {
            let serves = continue_serves(&bearers[0]);
            match row["expect"]["decision"].as_str() {
                Some("persistent") => assert!(serves, "{id}: the matrix says persistent"),
                Some("zdr" | "waived") => assert!(!serves, "{id}: the matrix says zdr or waived"),
                _ => {}
            }
        }
    }
    assert!(
        served >= 3,
        "the matrix has persistent bearers to serve: {served}"
    );
    assert!(
        waived_refused >= 2,
        "the matrix has waived bearers with zdr false to refuse: {waived_refused}"
    );
    assert!(
        zdr_refused >= 3,
        "the matrix has ZDR bearers to refuse: {zdr_refused}"
    );
}

/// The handler's own answer to that decision is private to `sse.rs` (`continue_turn_is_persistent`,
/// `continue_persistent_gate`, `continue_preflight`; their behaviour is pinned by the unit tests
/// beside them). What this file can hold is that the private function IS the decision above and
/// that the handler passes it on, so the table cannot drift from the route.
#[test]
fn the_continue_handler_decides_its_turn_with_the_plain_route_and_serves_persistent_only() {
    let source = include_str!("../src/sse.rs").replace("\r\n", "\n");
    let code: String = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let decision = code
        .split("fn continue_turn_is_persistent(")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("sse.rs defines continue_turn_is_persistent");
    assert!(
        decision.contains("decide_for(claims, &crate::zdr_posture::Route::plain(false))"),
        "the /continue decision is decide_for over the plain route, as this file models it: {decision}"
    );
    assert!(
        decision.contains("TurnPosture::Persistent"),
        "and it serves the persistent posture"
    );
    for widened in ["Waived", "Zdr", "is_waived", "zdr()", "|"] {
        assert!(
            !decision.contains(widened),
            "continue_turn_is_persistent must serve Persistent and nothing else, but it mentions `{widened}`: {decision}"
        );
    }
    let handler = code
        .split("pub(crate) async fn continue_run(")
        .nth(1)
        .and_then(|rest| rest.split("\n#[cfg(test)]").next())
        .expect("sse.rs defines continue_run");
    assert!(
        handler.contains("continue_persistent_gate("),
        "continue_run refuses a non-persistent turn before any work"
    );
    let preflight = handler
        .split("continue_preflight(")
        .nth(1)
        .and_then(|rest| rest.split(").map_err").next())
        .expect("continue_run calls continue_preflight");
    assert!(
        preflight.contains("continue_turn_is_persistent(&claims)"),
        "continue_run hands the same decision to the preflight: {preflight}"
    );
}
