//! WS17.P2 route tests: the owner and ACL check on `/v1/profiles*`.
//!
//! The handlers are called directly with hand-built claims. The dev bypass issues one
//! stub identity, so two users in one org cannot both go through the router. The
//! decision table is unit-tested in `profile_guard`; these tests pin what each route
//! answers to an owner, to an org member who is not the owner, and to a stranger,
//! under `enforce`, and what `observe` serves.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use quarry_browser::session::{ProfileScope, SaveMeta, SessionSnapshot};
use quarry_core::envelope::Envelope;
use quarry_core::ids::kinds::ProfileKind;
use quarry_edge::auth::Claims;
use quarry_edge::profile_guard::{self, OwnerEnforce, ProfileOp, IMPORT_SCOPE};
use quarry_edge::profile_routes::{
    self, CreateProfileRequest, LoadProfileResponse, RestoreProbeRequest, SaveProfileRequest,
    UpdateProfileRequest,
};
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};
use tokio::sync::mpsc;

mod support;

type Refusal = (StatusCode, Json<Envelope<()>>);

const ORG: &str = "org_acl";
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
        // A save (the import route) needs the import scope (WS17.P0); the owner check
        // under test sits behind it, so every caller here holds it. The scope itself is
        // tested in `profile_guard.rs`.
        scopes: vec![IMPORT_SCOPE.to_owned()],
        zdr: None,
    }
}

/// An edge state with an in-memory profile store, in the given owner-check mode.
fn state(mode: OwnerEnforce) -> AppState {
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(10), "quarry-test").unwrap());
    let mut state = support::base_state(driver, EventSink::new(tx));
    state.profile_owner_enforce = mode;
    state
}

/// The status a route answered with: 200 for any success, the refusal's status otherwise.
fn status_of<T>(result: Result<T, Refusal>) -> StatusCode {
    match result {
        Ok(_) => StatusCode::OK,
        Err((status, _)) => status,
    }
}

async fn create_as(state: &AppState, user: &str, scope: ProfileScope) -> String {
    let created = profile_routes::create_profile(
        State(state.clone()),
        Extension(claims(user)),
        Json(CreateProfileRequest {
            name: Some("Work".to_owned()),
            scope,
        }),
    )
    .await
    .unwrap_or_else(|_| panic!("create_profile refused for {user}"));
    created.0.data.expect("created row").profile_id
}

async fn load_as(
    state: &AppState,
    user: &str,
    id: &str,
) -> Result<Json<Envelope<LoadProfileResponse>>, Refusal> {
    profile_routes::load_profile(
        State(state.clone()),
        Extension(claims(user)),
        Path(id.to_owned()),
    )
    .await
}

async fn update_as(state: &AppState, user: &str, id: &str) -> StatusCode {
    status_of(
        profile_routes::update_profile(
            State(state.clone()),
            Extension(claims(user)),
            Path(id.to_owned()),
            Json(UpdateProfileRequest {
                name: Some("Renamed".to_owned()),
                scope: None,
            }),
        )
        .await,
    )
}

async fn probe_as(state: &AppState, user: &str, id: &str) -> StatusCode {
    status_of(
        profile_routes::restore_probe(
            State(state.clone()),
            Extension(claims(user)),
            Path(id.to_owned()),
            Json(RestoreProbeRequest {
                url: "https://example.com/".to_owned(),
            }),
        )
        .await,
    )
}

async fn delete_as(state: &AppState, user: &str, id: &str) -> StatusCode {
    match profile_routes::delete_profile(
        State(state.clone()),
        Extension(claims(user)),
        Path(id.to_owned()),
    )
    .await
    {
        Ok(_) => StatusCode::NO_CONTENT,
        Err((status, _)) => status,
    }
}

async fn save_as(state: &AppState, user: &str, id: Option<&str>) -> StatusCode {
    status_of(
        profile_routes::save_profile(
            State(state.clone()),
            Extension(claims(user)),
            Json(SaveProfileRequest {
                profile_id: id.map(str::to_owned),
                snapshot: SessionSnapshot::default(),
            }),
        )
        .await,
    )
}

async fn listed_ids(state: &AppState, user: &str) -> Vec<String> {
    profile_routes::list_profiles(State(state.clone()), Extension(claims(user)))
        .await
        .unwrap_or_else(|_| panic!("list_profiles refused for {user}"))
        .0
        .data
        .expect("listing")
        .profiles
        .into_iter()
        .map(|row| row.profile_id)
        .collect()
}

#[tokio::test]
async fn owner_can_use_user_private() {
    let state = state(OwnerEnforce::Enforce);
    let id = create_as(&state, ALICE, ProfileScope::UserPrivate).await;

    let loaded = load_as(&state, ALICE, &id).await.expect("owner loads");
    assert!(loaded.0.data.expect("row").owner_is_caller);
    assert_eq!(update_as(&state, ALICE, &id).await, StatusCode::OK);
    assert_eq!(probe_as(&state, ALICE, &id).await, StatusCode::OK);
    assert_eq!(save_as(&state, ALICE, Some(&id)).await, StatusCode::OK);
    assert_eq!(delete_as(&state, ALICE, &id).await, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn non_owner_gets_404_on_every_route() {
    let state = state(OwnerEnforce::Enforce);
    let id = create_as(&state, ALICE, ProfileScope::UserPrivate).await;

    assert_eq!(status_of(load_as(&state, BOB, &id).await), StatusCode::NOT_FOUND);
    assert_eq!(update_as(&state, BOB, &id).await, StatusCode::NOT_FOUND);
    assert_eq!(probe_as(&state, BOB, &id).await, StatusCode::NOT_FOUND);
    assert_eq!(save_as(&state, BOB, Some(&id)).await, StatusCode::NOT_FOUND);
    assert_eq!(delete_as(&state, BOB, &id).await, StatusCode::NOT_FOUND);

    // The refused delete changed nothing: the owner still loads the profile.
    assert!(load_as(&state, ALICE, &id).await.is_ok());
}

#[tokio::test]
async fn non_owner_list_hides_user_private_but_shows_org_shared() {
    let state = state(OwnerEnforce::Enforce);
    let private = create_as(&state, ALICE, ProfileScope::UserPrivate).await;
    let shared = create_as(&state, ALICE, ProfileScope::OrgShared).await;

    let for_bob = listed_ids(&state, BOB).await;
    assert!(for_bob.contains(&shared), "org-shared row hidden from a member");
    assert!(!for_bob.contains(&private), "user-private row listed to a non-owner");

    let for_alice = listed_ids(&state, ALICE).await;
    assert!(for_alice.contains(&private) && for_alice.contains(&shared));
}

#[tokio::test]
async fn org_shared_member_can_attach_but_not_rename_or_delete() {
    let state = state(OwnerEnforce::Enforce);
    let id = create_as(&state, ALICE, ProfileScope::OrgShared).await;

    assert!(load_as(&state, BOB, &id).await.is_ok());
    assert_eq!(probe_as(&state, BOB, &id).await, StatusCode::OK);
    assert_eq!(save_as(&state, BOB, Some(&id)).await, StatusCode::OK);
    assert_eq!(update_as(&state, BOB, &id).await, StatusCode::NOT_FOUND);
    assert_eq!(delete_as(&state, BOB, &id).await, StatusCode::NOT_FOUND);

    let parsed: ProfileKind = id.parse().expect("profile id");
    assert!(profile_guard::authorize(&state, &claims(BOB), &parsed, ProfileOp::Attach)
        .await
        .is_ok());
}

#[tokio::test]
async fn legacy_ownerless_user_private_is_hidden_in_enforce_and_allowed_in_observe() {
    let enforce = state(OwnerEnforce::Enforce);
    let mut observe = enforce.clone();
    observe.profile_owner_enforce = OwnerEnforce::Observe;

    // A row written with no owner, as a pre-WS17.P1 save leaves it.
    let legacy: ProfileKind = quarry_core::ids::Id::new();
    enforce
        .profiles
        .save(ORG, &legacy, &SessionSnapshot::default(), &SaveMeta::now(None))
        .await
        .expect("legacy save");
    let id = legacy.to_string();

    assert_eq!(status_of(load_as(&enforce, ALICE, &id).await), StatusCode::NOT_FOUND);
    assert!(!listed_ids(&enforce, ALICE).await.contains(&id));

    assert!(load_as(&observe, ALICE, &id).await.is_ok());
    assert!(listed_ids(&observe, ALICE).await.contains(&id));
}

#[tokio::test]
async fn observe_mode_serves_a_non_owner() {
    let mut state = state(OwnerEnforce::Enforce);
    let id = create_as(&state, ALICE, ProfileScope::UserPrivate).await;
    state.profile_owner_enforce = OwnerEnforce::Observe;

    assert!(load_as(&state, BOB, &id).await.is_ok());
    assert_eq!(delete_as(&state, BOB, &id).await, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn attach_of_unknown_profile_id_creates_it_owned_by_caller() {
    let state = state(OwnerEnforce::Enforce);
    let fresh: ProfileKind = quarry_core::ids::Id::new();
    let id = fresh.to_string();

    assert_eq!(save_as(&state, ALICE, Some(&id)).await, StatusCode::OK);
    let loaded = load_as(&state, ALICE, &id).await.expect("owner loads");
    assert!(loaded.0.data.expect("row").owner_is_caller);
    assert_eq!(status_of(load_as(&state, BOB, &id).await), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn second_user_cannot_claim_existing_profile_id_by_saving() {
    let state = state(OwnerEnforce::Enforce);
    let id = create_as(&state, ALICE, ProfileScope::UserPrivate).await;

    assert_eq!(save_as(&state, BOB, Some(&id)).await, StatusCode::NOT_FOUND);
    assert!(load_as(&state, ALICE, &id).await.is_ok());
}
