//! WS17.P2 / D2-045: starting a run that names a profile attaches it, and the owner and
//! ACL check of `/v1/profiles*` applies to that attach too.
//!
//! Through the real router with the agent harness. The profile owner check ships in
//! `observe` (D16 Q2), so the test that needs a refusal sets `enforce` explicitly.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use agent_harness::*;
use quarry_browser::session::{ProfileMetadata, ProfileScope};
use quarry_core::ids::kinds::ProfileKind;
use quarry_edge::profile_guard::OwnerEnforce;
use quarry_edge::run_limits::RunLimits;
use serde_json::json;

/// The org and the owner of `member_token()`.
const ORG: &str = "org-standard-golden";
const OWNER: &str = "user-golden";

async fn edge(mode: OwnerEnforce) -> Edge {
    Edge::start_with(RunLimits::default(), move |mut state| {
        state.profile_owner_enforce = mode;
        state
    })
    .await
}

/// A profile the owner already created, with the scope given (as the create route stores it).
async fn saved_profile(edge: &Edge, scope: ProfileScope) -> String {
    let id: ProfileKind = quarry_core::ids::Id::new();
    let metadata = ProfileMetadata {
        name: Some("Work".to_owned()),
        scope,
        owner: Some(OWNER.to_owned()),
    };
    edge.state
        .profiles
        .save_metadata(ORG, &id, &metadata)
        .await
        .expect("the owner's profile is stored");
    id.to_string()
}

#[tokio::test]
async fn a_non_owner_start_run_naming_a_private_profile_is_a_404_under_enforce() {
    let edge = edge(OwnerEnforce::Enforce).await;
    let id = saved_profile(&edge, ProfileScope::UserPrivate).await;

    // Another member of the same org: the same 404 a missing profile gives.
    let other = other_member_token();
    let reply = edge
        .post(&other, "/v1/agent/runs", json!({ "profile_id": id }))
        .await;
    assert_eq!(reply.status, 404, "{}", reply.body);
    assert!(reply.message().contains("profile not found"), "{}", reply.body);

    // The owner attaches it.
    let owner = member_token();
    let started = edge
        .post(&owner, "/v1/agent/runs", json!({ "profile_id": id }))
        .await;
    assert_eq!(started.status, 200, "{}", started.body);
}

#[tokio::test]
async fn an_org_shared_profile_attaches_for_any_member_and_an_unknown_id_is_created() {
    let edge = edge(OwnerEnforce::Enforce).await;
    let shared = saved_profile(&edge, ProfileScope::OrgShared).await;
    let other = other_member_token();

    let reply = edge
        .post(&other, "/v1/agent/runs", json!({ "profile_id": shared }))
        .await;
    assert_eq!(reply.status, 200, "{}", reply.body);

    let fresh: ProfileKind = quarry_core::ids::Id::new();
    let reply = edge
        .post(&other, "/v1/agent/runs", json!({ "profile_id": fresh.to_string() }))
        .await;
    assert_eq!(reply.status, 200, "{}", reply.body);
}

#[tokio::test]
async fn observe_mode_serves_a_non_owner_start_run_as_before() {
    let edge = edge(OwnerEnforce::Observe).await;
    let id = saved_profile(&edge, ProfileScope::UserPrivate).await;

    let other = other_member_token();
    let reply = edge
        .post(&other, "/v1/agent/runs", json!({ "profile_id": id }))
        .await;
    assert_eq!(reply.status, 200, "{}", reply.body);
}

// ── D16 Q2: the owner is stamped at run start, for a person's run only ──────

/// The profile id a started run reports.
fn started_profile(reply: &Reply) -> ProfileKind {
    reply.data()["profile_id"]
        .as_str()
        .expect("the start reply names the profile")
        .parse()
        .expect("a profile id")
}

#[tokio::test]
async fn a_persistent_person_run_creates_its_profile_owned_by_the_person_user_private() {
    for mode in [OwnerEnforce::Observe, OwnerEnforce::Enforce] {
        let edge = edge(mode).await;
        let owner = member_token();
        let reply = edge
            .post(&owner, "/v1/agent/runs", json!({ "persist_profile": true }))
            .await;
        assert_eq!(reply.status, 200, "{}", reply.body);
        let id = started_profile(&reply);

        // The row exists before the run saves anything, with the person as owner. The run's
        // `ephemeral` default scope is stored as user_private, never as `ephemeral`.
        assert!(edge.state.profiles.exists(ORG, &id).await.unwrap());
        let stored = edge
            .state
            .profiles
            .load_metadata(ORG, &id)
            .await
            .unwrap()
            .expect("metadata");
        assert_eq!(stored.owner.as_deref(), Some(OWNER), "{mode:?}");
        assert_eq!(stored.scope, ProfileScope::UserPrivate, "{mode:?}");
        assert!(stored.name.is_none());

        // So under enforce the creator keeps their profile and another member does not get it.
        let attach = edge
            .post(
                &owner,
                "/v1/agent/runs",
                json!({ "profile_id": id.to_string() }),
            )
            .await;
        assert_eq!(attach.status, 200, "{}", attach.body);
        let other = edge
            .post(
                &other_member_token(),
                "/v1/agent/runs",
                json!({ "profile_id": id.to_string() }),
            )
            .await;
        let expected = if mode == OwnerEnforce::Enforce { 404 } else { 200 };
        assert_eq!(other.status, expected, "{mode:?} {}", other.body);
    }
}

#[tokio::test]
async fn a_named_new_profile_and_an_org_shared_scope_are_stamped_as_asked() {
    let edge = edge(OwnerEnforce::Enforce).await;
    let id: ProfileKind = quarry_core::ids::Id::new();
    let reply = edge
        .post(
            &member_token(),
            "/v1/agent/runs",
            json!({ "profile_id": id.to_string(), "profile_scope": "org_shared" }),
        )
        .await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    let stored = edge
        .state
        .profiles
        .load_metadata(ORG, &id)
        .await
        .unwrap()
        .expect("metadata");
    assert_eq!(stored.owner.as_deref(), Some(OWNER));
    assert_eq!(stored.scope, ProfileScope::OrgShared);
}

#[tokio::test]
async fn a_run_that_keeps_no_profile_creates_no_row() {
    let edge = edge(OwnerEnforce::Observe).await;
    let reply = edge.post(&member_token(), "/v1/agent/runs", json!({})).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    let id = started_profile(&reply);
    assert!(!edge.state.profiles.exists(ORG, &id).await.unwrap());
}

#[tokio::test]
async fn a_service_principal_run_never_creates_an_owned_profile() {
    // A model-driven run: `actor_id()` is the shared service id, so stamping it would make the
    // service the owner of those cookies and hide them from the person (D16 Q2).
    let edge = edge(OwnerEnforce::Enforce).await;
    let reply = edge
        .post(&service_token(), "/v1/agent/runs", json!({ "persist_profile": true }))
        .await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    let id = started_profile(&reply);
    let owner = match edge.state.profiles.load_metadata(ORG, &id).await.unwrap() {
        None => None,
        Some(stored) => stored.owner,
    };
    assert_eq!(owner, None, "a service run was stamped as an owner");
}

#[tokio::test]
async fn the_stamp_is_off_when_the_owner_check_is_off() {
    let edge = edge(OwnerEnforce::Off).await;
    let reply = edge
        .post(&member_token(), "/v1/agent/runs", json!({ "persist_profile": true }))
        .await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    let id = started_profile(&reply);
    assert!(!edge.state.profiles.exists(ORG, &id).await.unwrap());
}

#[tokio::test]
async fn a_zdr_run_that_names_a_profile_is_refused_and_creates_no_row() {
    let edge = edge(OwnerEnforce::Observe).await;
    let id: ProfileKind = quarry_core::ids::Id::new();
    let reply = edge
        .post(
            &zdr_member_token(),
            "/v1/agent/runs",
            json!({ "profile_id": id.to_string() }),
        )
        .await;
    assert_eq!(reply.status, 400, "{}", reply.body);
    assert!(
        !edge
            .state
            .profiles
            .exists("org-zdr-golden", &id)
            .await
            .unwrap(),
        "a refused ZDR start left a profile row"
    );
}

#[tokio::test]
async fn the_stamp_never_changes_a_profile_that_already_exists() {
    let edge = edge(OwnerEnforce::Observe).await;
    let id = saved_profile(&edge, ProfileScope::OrgShared).await;
    let parsed: ProfileKind = id.parse().unwrap();
    // Another member of the org starts a persistent run on the owner's shared profile.
    let reply = edge
        .post(
            &other_member_token(),
            "/v1/agent/runs",
            json!({ "profile_id": id, "persist_profile": true, "profile_scope": "user_private" }),
        )
        .await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    let stored = edge
        .state
        .profiles
        .load_metadata(ORG, &parsed)
        .await
        .unwrap()
        .expect("metadata");
    assert_eq!(stored.owner.as_deref(), Some(OWNER));
    assert_eq!(stored.scope, ProfileScope::OrgShared);
    assert_eq!(stored.name.as_deref(), Some("Work"));
}
