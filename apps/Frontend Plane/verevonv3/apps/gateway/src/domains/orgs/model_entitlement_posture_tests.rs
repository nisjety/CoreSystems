//! ZDR-Z-03c at the model-entitlement guard: `org_zdr_enabled` reads the shared live reader, so a
//! record without `retention_posture` (switch on) is unverified (`None`), which the guard answers
//! as unavailable rather than letting a restricting policy through.

use serde_json::{json, Value};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

use super::org_zdr_enabled;
use crate::{
    cache::ResultCache,
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
        auth_role: Some("owner".to_owned()),
        active_org_id: Some("org-1".to_owned()),
        authorized_membership: Some(AuthorizedMembership {
            organization_id: "org-1".to_owned(),
            role: "owner".to_owned(),
        }),
    }
}

async fn state_serving(record: Value, effective_read: bool) -> (MockServer, AppState) {
    let org_core = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/organizations/org-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(record))
        .mount(&org_core)
        .await;
    let state = AppState {
        org_core_url: org_core.uri(),
        cache: ResultCache::in_memory(),
        zdr_effective_posture_read: effective_read,
        ..AppState::for_tests()
    };
    (org_core, state)
}

#[tokio::test]
async fn a_record_without_retention_posture_is_unverified_with_the_switch_on() {
    let record = json!({ "data": { "metadata": { "interactiveRetention": { "zdr": false } } } });
    let (_org_core, state) = state_serving(record.clone(), true).await;
    assert_eq!(org_zdr_enabled(&state, &user(), "org-1").await, None);

    // Switch off: the intent-only rule answers "not ZDR".
    let (_org_core, state) = state_serving(record, false).await;
    assert_eq!(org_zdr_enabled(&state, &user(), "org-1").await, Some(false));
}

#[tokio::test]
async fn the_effective_field_decides_with_the_switch_on() {
    for (effective, expected) in [(true, Some(true)), (false, Some(false))] {
        let record = json!({ "data": {
            "metadata": { "interactiveRetention": { "zdr": !effective } },
            "retention_posture": { "effective_zdr": effective },
        }});
        let (_org_core, state) = state_serving(record, true).await;
        assert_eq!(
            org_zdr_enabled(&state, &user(), "org-1").await,
            expected,
            "effective_zdr={effective}"
        );
    }
}

#[tokio::test]
async fn an_org_core_outage_is_unverified() {
    let org_core = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/organizations/org-1"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&org_core)
        .await;
    for effective_read in [false, true] {
        let state = AppState {
            org_core_url: org_core.uri(),
            zdr_effective_posture_read: effective_read,
            ..AppState::for_tests()
        };
        assert_eq!(org_zdr_enabled(&state, &user(), "org-1").await, None);
    }
}
