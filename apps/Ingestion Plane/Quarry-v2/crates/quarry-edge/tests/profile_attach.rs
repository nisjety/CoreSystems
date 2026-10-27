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
