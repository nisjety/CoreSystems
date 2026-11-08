//! F-t2-068 route tests: `POST /v1/profiles/:id/clear-data` and the flag that gates it.
//!
//! The handlers are called directly with hand-built claims, as `profile_owner.rs` does. The
//! refusal of a profile a live run holds (409 `profile_in_use`) is tested beside the run registry
//! in `agent_routes/tests.rs`, where a run can be registered.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use quarry_browser::session::{
    BrowsingDataCounts, Cookie, ProfileMetadata, ProfileScope, SaveMeta, SessionSnapshot,
};
use quarry_core::envelope::Envelope;
use quarry_core::ids::kinds::ProfileKind;
use quarry_edge::auth::Claims;
use quarry_edge::profile_guard::OwnerEnforce;
use quarry_edge::profile_routes::{self, ClearDataRequest, ClearDataResponse};
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};
use tokio::sync::mpsc;

mod support;

type Refusal = (StatusCode, Json<Envelope<()>>);

const ORG: &str = "org_clear";
const ALICE: &str = "user_alice";
const BOB: &str = "user_bob";

fn claims(user: &str) -> Claims {
    Claims {
        web_policy: None,
        sub: user.to_owned(),
        iss: "test".to_owned(),
        exp: i64::MAX,
        org_id: ORG.to_owned(),
        user_id: user.to_owned(),
        principal_type: Some("user".to_owned()),
        service_id: None,
        nbf: None,
        aud: None,
        scopes: Vec::new(),
        zdr: None,
        key_id: None,
        acting_user_id: None,
    }
}

fn service_claims() -> Claims {
    Claims {
        user_id: String::new(),
        principal_type: Some("service".to_owned()),
        service_id: Some("model-gateway".to_owned()),
        ..claims(ALICE)
    }
}

/// An edge state with an in-memory profile store, in the given owner-check mode and flag.
fn state(mode: OwnerEnforce, clear_enabled: bool) -> AppState {
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(10), "quarry-test").unwrap());
    let mut state = support::base_state(driver, EventSink::new(tx));
    state.profile_owner_enforce = mode;
    state.profile_clear_data = clear_enabled;
    state
}

fn stored_snapshot() -> SessionSnapshot {
    SessionSnapshot {
        cookies: vec![Cookie {
            name: "sid".into(),
            value: "SECRET-COOKIE".into(),
            ..Default::default()
        }],
        local_storage: vec![("token".into(), "SECRET-LS".into())],
        session_storage: vec![("tab".into(), "SECRET-SS".into())],
        user_agent: Some("Quarry/2.0 (test)".into()),
        locale: Some("nb-NO".into()),
        ..Default::default()
    }
}

/// A stored profile named "Work", owned by `owner`, holding one cookie and two storage entries.
async fn stored_profile(state: &AppState, owner: &str, scope: ProfileScope) -> ProfileKind {
    let id: ProfileKind = quarry_core::ids::Id::new();
    state
        .profiles
        .save_metadata(
            ORG,
            &id,
            &ProfileMetadata {
                name: Some("Work".into()),
                scope,
                owner: Some(owner.into()),
            },
        )
        .await
        .unwrap();
    state
        .profiles
        .save(
            ORG,
            &id,
            &stored_snapshot(),
            &SaveMeta::now(Some(owner.into())),
        )
        .await
        .unwrap();
    id
}

async fn clear_as(
    state: &AppState,
    who: Claims,
    id: &ProfileKind,
    dry_run: bool,
) -> Result<Json<Envelope<ClearDataResponse>>, Refusal> {
    profile_routes::clear_profile_data(
        State(state.clone()),
        Extension(who),
        Path(id.to_string()),
        Json(ClearDataRequest { dry_run }),
    )
    .await
}

async fn stored_counts(state: &AppState, id: &ProfileKind) -> BrowsingDataCounts {
    state
        .profiles
        .load(ORG, id)
        .await
        .unwrap()
        .expect("the snapshot is stored")
        .browsing_data_counts()
}

#[tokio::test]
async fn clear_answers_501_and_changes_nothing_while_the_flag_is_off() {
    let state = state(OwnerEnforce::Observe, false);
    let id = stored_profile(&state, ALICE, ProfileScope::UserPrivate).await;

    let (status, body) = clear_as(&state, claims(ALICE), &id, false)
        .await
        .expect_err("the flag is off by default");
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    let details = body
        .0
        .error
        .expect("an error envelope")
        .details
        .expect("details");
    assert_eq!(details["reason"], "profile_clear_data_disabled");
    assert_eq!(details["env"], "QUARRY_EDGE__PROFILE_CLEAR_DATA");
    assert_eq!(stored_counts(&state, &id).await.cookies, 1);
}

#[tokio::test]
async fn a_dry_run_counts_what_a_clear_would_remove_and_removes_nothing() {
    let state = state(OwnerEnforce::Observe, true);
    let id = stored_profile(&state, ALICE, ProfileScope::UserPrivate).await;

    let view = clear_as(&state, claims(ALICE), &id, true)
        .await
        .unwrap()
        .0
        .data
        .expect("data");
    assert!(view.dry_run);
    assert!(!view.cleared);
    assert_eq!(view.counts.cookies, 1);
    assert_eq!(view.counts.local_storage_entries, 1);
    assert_eq!(view.counts.session_storage_entries, 1);
    assert_eq!(
        stored_counts(&state, &id).await.cookies,
        1,
        "a dry run keeps the data"
    );
    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains("SECRET"), "{json}");
}

#[tokio::test]
async fn a_clear_removes_site_data_and_keeps_the_profile_and_its_device_settings() {
    let state = state(OwnerEnforce::Observe, true);
    let id = stored_profile(&state, ALICE, ProfileScope::UserPrivate).await;

    let done = clear_as(&state, claims(ALICE), &id, false)
        .await
        .unwrap()
        .0
        .data
        .expect("data");
    assert!(done.cleared);
    assert!(!done.dry_run);
    assert_eq!(done.counts.cookies, 1);

    let after = state
        .profiles
        .load(ORG, &id)
        .await
        .unwrap()
        .expect("the profile still exists");
    assert!(!after.has_state());
    assert_eq!(after.user_agent.as_deref(), Some("Quarry/2.0 (test)"));
    assert_eq!(after.locale.as_deref(), Some("nb-NO"));
    let meta = state
        .profiles
        .load_metadata(ORG, &id)
        .await
        .unwrap()
        .expect("metadata");
    assert_eq!(meta.name.as_deref(), Some("Work"));
    assert_eq!(meta.scope, ProfileScope::UserPrivate);
    assert_eq!(meta.owner.as_deref(), Some(ALICE));

    let again = clear_as(&state, claims(ALICE), &id, false)
        .await
        .unwrap()
        .0
        .data
        .expect("data");
    assert!(!again.cleared, "nothing is left to clear");
    assert!(again.counts.is_empty());
}

#[tokio::test]
async fn under_enforce_a_non_owner_gets_404_and_nothing_is_cleared() {
    let state = state(OwnerEnforce::Enforce, true);
    let id = stored_profile(&state, ALICE, ProfileScope::UserPrivate).await;

    let (status, _) = clear_as(&state, claims(BOB), &id, false)
        .await
        .expect_err("not the owner");
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(stored_counts(&state, &id).await.cookies, 1);
}

#[tokio::test]
async fn an_org_member_cannot_clear_a_shared_profile_they_do_not_own() {
    let state = state(OwnerEnforce::Enforce, true);
    let id = stored_profile(&state, ALICE, ProfileScope::OrgShared).await;

    let (status, _) = clear_as(&state, claims(BOB), &id, false)
        .await
        .expect_err("shared, but not the owner");
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(stored_counts(&state, &id).await.cookies, 1);

    let done = clear_as(&state, claims(ALICE), &id, false)
        .await
        .unwrap()
        .0
        .data
        .expect("data");
    assert!(done.cleared, "the owner may clear it");
}

#[tokio::test]
async fn an_unknown_profile_is_404_and_a_service_principal_is_refused() {
    let state = state(OwnerEnforce::Observe, true);
    let unknown: ProfileKind = quarry_core::ids::Id::new();
    let (status, _) = clear_as(&state, claims(ALICE), &unknown, false)
        .await
        .expect_err("no such profile");
    assert_eq!(status, StatusCode::NOT_FOUND);

    let id = stored_profile(&state, ALICE, ProfileScope::UserPrivate).await;
    let (status, _) = clear_as(&state, service_claims(), &id, false)
        .await
        .expect_err("a service principal never reaches a profile");
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(stored_counts(&state, &id).await.cookies, 1);
}

#[tokio::test]
async fn the_profile_list_says_whether_clear_is_served() {
    for enabled in [false, true] {
        let state = state(OwnerEnforce::Observe, enabled);
        let listed = profile_routes::list_profiles(State(state.clone()), Extension(claims(ALICE)))
            .await
            .unwrap();
        assert_eq!(listed.0.data.expect("data").clear_data_enabled, enabled);
    }
}
