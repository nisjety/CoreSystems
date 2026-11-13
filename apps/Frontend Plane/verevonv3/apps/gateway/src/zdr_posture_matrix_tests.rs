//! ZDR-Z-03c: the BFF's one org-posture reader, driven by the vendored posture matrix
//! (`tests/fixtures/zdr-posture-matrix.json`, Control's `org_posture_rows`).
//!
//! With `GATEWAY_ZDR_EFFECTIVE_POSTURE_ENABLED` on, every row is built into the organization record
//! org-core would serve (its computed `retention_posture.effective_zdr` follows the stored intent;
//! a lookup the row marks unavailable is an HTTP 500) and read through [`org_record_posture`] and
//! the live and cached readers. The expected state is the row's own `expect.state`.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

use super::*;
use crate::{cache::ResultCache, middleware::AuthorizedMembership};

const MATRIX: &str = include_str!("../tests/fixtures/zdr-posture-matrix.json");
const MATRIX_SHA256: &str = include_str!("../tests/fixtures/zdr-posture-matrix.json.sha256");

fn matrix() -> Value {
    serde_json::from_str(MATRIX).expect("the vendored matrix is JSON")
}

fn user() -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: "user-1".to_owned(),
        user_email: "user@example.test".to_owned(),
        user_name: "User One".to_owned(),
        user_image: None,
        email_verified: true,
        auth_role: Some("member".to_owned()),
        active_org_id: Some("org-1".to_owned()),
        authorized_membership: Some(AuthorizedMembership {
            organization_id: "org-1".to_owned(),
            role: "member".to_owned(),
        }),
    }
}

fn expected_posture(row: &Value) -> OrgPosture {
    match row["expect"]["state"].as_str().expect("expect.state") {
        "zdr" => OrgPosture::Zdr,
        "not_zdr" => OrgPosture::NotZdr,
        "unknown" => OrgPosture::Unknown,
        other => panic!("unknown matrix state {other}"),
    }
}

/// The record org-core serves for a row whose lookup succeeds: the stored intent under
/// `metadata.interactiveRetention.zdr` and org-core's computed `retention_posture` (ZDR-Z-03a: the
/// effective posture is the intent; entitlement only marks a lapse).
fn org_record(row: &Value) -> Value {
    let intent = row["inputs"]["intent"].as_bool().expect("intent");
    let entitled = row["inputs"]["entitled"].as_bool().expect("entitled");
    json!({
        "data": {
            "id": "org-1",
            "metadata": { "interactiveRetention": { "zdr": intent } },
            "retention_posture": {
                "effective_zdr": intent,
                "lapsed": intent && !entitled,
            },
        }
    })
}

fn lookup_ok(row: &Value) -> bool {
    match row["inputs"]["lookup"].as_str().expect("lookup") {
        "ok" => true,
        "unavailable" => false,
        other => panic!("unknown lookup {other}"),
    }
}

async fn org_core_serving(row: &Value) -> MockServer {
    let org_core = MockServer::start().await;
    let response = if lookup_ok(row) {
        ResponseTemplate::new(200).set_body_json(org_record(row))
    } else {
        ResponseTemplate::new(500).set_body_json(json!({"error": {"code": "internal"}}))
    };
    Mock::given(method("GET"))
        .and(path("/api/v1/organizations/org-1"))
        .respond_with(response)
        .mount(&org_core)
        .await;
    org_core
}

fn state_on(org_core: &MockServer) -> AppState {
    AppState {
        org_core_url: org_core.uri(),
        cache: ResultCache::in_memory(),
        zdr_effective_posture_read: true,
        ..AppState::for_tests()
    }
}

#[test]
fn the_vendored_matrix_is_the_byte_copy_its_digest_names() {
    let digest = hex::encode(Sha256::digest(MATRIX.as_bytes()));
    let pinned = MATRIX_SHA256
        .split_whitespace()
        .next()
        .expect("a digest in the .sha256 file");
    assert_eq!(
        digest, pinned,
        "zdr-posture-matrix.json differs from its .sha256"
    );
}

#[test]
fn every_org_posture_row_with_a_record_parses_to_its_state_with_the_switch_on() {
    let matrix = matrix();
    let rows = matrix["org_posture_rows"]
        .as_array()
        .expect("org_posture_rows");
    assert!(!rows.is_empty());
    for row in rows.iter().filter(|row| lookup_ok(row)) {
        let record = org_record(row);
        assert_eq!(
            org_record_posture(&record, true),
            expected_posture(row),
            "{}",
            row["id"]
        );
        // The bare record (no `data` envelope) reads the same.
        assert_eq!(
            org_record_posture(&record["data"], true),
            expected_posture(row),
            "{} (bare record)",
            row["id"]
        );
    }
}

#[tokio::test]
async fn every_org_posture_row_reads_its_state_through_the_live_and_cached_readers() {
    let matrix = matrix();
    for row in matrix["org_posture_rows"].as_array().expect("rows") {
        let org_core = org_core_serving(row).await;
        let state = state_on(&org_core);
        assert_eq!(
            org_posture_live(&state, &user(), "org-1").await,
            expected_posture(row),
            "live {}",
            row["id"]
        );
        let state = state_on(&org_core);
        assert_eq!(
            org_posture(&state, &user()).await,
            expected_posture(row),
            "cached reader {}",
            row["id"]
        );
    }
}

/// Switch on: a record that predates ZDR-Z-03a (no `retention_posture`), or carries a non-boolean
/// `effective_zdr`, is Unknown, never "not ZDR", even when the stored intent says off.
#[test]
fn a_record_without_a_boolean_effective_zdr_is_unknown_with_the_switch_on() {
    for record in [
        json!({ "data": { "metadata": { "interactiveRetention": { "zdr": false } } } }),
        json!({ "data": { "metadata": { "interactiveRetention": { "zdr": true } } } }),
        json!({ "data": { "retention_posture": {} } }),
        json!({ "data": { "retention_posture": { "effective_zdr": "false" } } }),
        json!({ "data": { "retention_posture": { "effective_zdr": null } } }),
        json!({}),
    ] {
        assert_eq!(
            org_record_posture(&record, true),
            OrgPosture::Unknown,
            "{record}"
        );
    }
}

/// Switch off: the intent-only rule, byte-for-byte what every reader had before; the computed
/// field is not read at all.
#[test]
fn the_switch_off_rule_reads_the_stored_intent_only() {
    let intent_on_effective_off = json!({
        "data": {
            "metadata": { "interactiveRetention": { "zdr": true } },
            "retention_posture": { "effective_zdr": false },
        }
    });
    assert_eq!(
        org_record_posture(&intent_on_effective_off, false),
        OrgPosture::Zdr
    );
    let no_block = json!({ "data": { "retention_posture": { "effective_zdr": true } } });
    assert_eq!(org_record_posture(&no_block, false), OrgPosture::NotZdr);
}

/// The live reader stores a Zdr answer under the shared key (the chat path only gets stricter)
/// and never a NotZdr answer.
#[tokio::test]
async fn the_live_reader_never_stores_a_not_zdr_answer() {
    let matrix = matrix();
    let rows = matrix["org_posture_rows"].as_array().expect("rows");
    let key = cache_key("org-zdr", &["org-1"]);
    for row in rows {
        let org_core = org_core_serving(row).await;
        let state = state_on(&org_core);
        let posture = org_posture_live(&state, &user(), "org-1").await;
        let cached = state.cache.lookup_within(&key, ORG_ZDR_CACHE_SECS).await;
        match posture {
            OrgPosture::Zdr => assert_eq!(cached, Some(Value::Bool(true)), "{}", row["id"]),
            _ => assert_eq!(cached, None, "{} stored {cached:?}", row["id"]),
        }
    }
}
