//! ZDR-Z-03c at the two inbox durable-write gates: both read the org's CURRENT posture through the
//! shared reader (`zdr::org_posture_live`), so a record without `retention_posture` (switch on)
//! answers each gate's Unknown refusal, and a cached "not ZDR" answer never opens a gate.

use axum::response::Response;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

use super::{require_draft_persistence, require_support_ai_review};
use crate::{
    cache::{cache_key, ResultCache},
    config::AppState,
    middleware::{AuthenticatedUser, AuthorizedMembership},
};

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

async fn org_core_serving(record: Value) -> MockServer {
    let org_core = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/organizations/org-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(record))
        .mount(&org_core)
        .await;
    org_core
}

fn state(org_core: &MockServer, effective_read: bool) -> AppState {
    AppState {
        org_core_url: org_core.uri(),
        cache: ResultCache::in_memory(),
        zdr_effective_posture_read: effective_read,
        ..AppState::for_tests()
    }
}

async fn refusal(result: Result<(), Response>) -> (u16, String) {
    let response = result.expect_err("the gate refuses");
    let status = response.status().as_u16();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: Value = serde_json::from_slice(&bytes).expect("json body");
    let code = body["error"]["code"]
        .as_str()
        .or_else(|| body["code"].as_str())
        .unwrap_or_default()
        .to_owned();
    (status, code)
}

/// A record that predates ZDR-Z-03a: only the stored intent, which says off.
fn record_without_retention_posture() -> Value {
    json!({ "data": {
        "id": "org-1",
        "metadata": {
            "interactiveRetention": { "zdr": false },
            "supportAi": { "mode": "review" },
        },
    }})
}

#[tokio::test]
async fn the_draft_gate_answers_its_unknown_refusal_for_a_record_without_retention_posture() {
    let org_core = org_core_serving(record_without_retention_posture()).await;
    let state = state(&org_core, true);
    let (status, code) = refusal(require_draft_persistence(&state, &user()).await).await;
    assert_eq!(status, 503);
    assert_eq!(code, "retention_posture_unavailable");

    // Switch off: the same record is "not ZDR" by the intent-only rule, so the gate opens.
    let state = AppState {
        zdr_effective_posture_read: false,
        ..state
    };
    assert!(require_draft_persistence(&state, &user()).await.is_ok());
}

#[tokio::test]
async fn the_support_ai_gate_answers_its_unknown_refusal_for_a_record_without_retention_posture() {
    let org_core = org_core_serving(record_without_retention_posture()).await;
    let state = state(&org_core, true);
    let (status, code) = refusal(require_support_ai_review(&state, &user()).await).await;
    assert_eq!(status, 503);
    assert_eq!(code, "support_ai_policy_unavailable");

    let state = AppState {
        zdr_effective_posture_read: false,
        ..state
    };
    assert!(require_support_ai_review(&state, &user()).await.is_ok());
}

/// A NotZdr answer the chat path cached a moment ago must not open a durable-write gate: org-core
/// now serves a ZDR record, and the gate reads it live.
#[tokio::test]
async fn a_cached_not_zdr_never_opens_the_draft_gate() {
    for effective_read in [false, true] {
        let org_core = org_core_serving(json!({ "data": {
            "id": "org-1",
            "metadata": { "interactiveRetention": { "zdr": true } },
            "retention_posture": { "effective_zdr": true },
        }}))
        .await;
        let state = state(&org_core, effective_read);
        state
            .cache
            .store_for_secs(&cache_key("org-zdr", &["org-1"]), &Value::Bool(false), 60)
            .await;
        let (status, code) = refusal(require_draft_persistence(&state, &user()).await).await;
        assert_eq!(status, 412, "effective_read={effective_read}");
        assert_eq!(code, "zdr_draft_persistence_forbidden");
        let (status, code) = refusal(require_support_ai_review(&state, &user()).await).await;
        assert_eq!(status, 412, "effective_read={effective_read}");
        assert_eq!(code, "zdr_ai_proposal_forbidden");
    }
}
