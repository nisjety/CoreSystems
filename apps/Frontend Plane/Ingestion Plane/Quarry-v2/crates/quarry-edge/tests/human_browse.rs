//! Human browsing on the open web through `POST /v1/agent/runs` (WS8.B, plan D8-1 to
//! D8-5). A person's `human_browse` grant is admitted only with `QUARRY_EDGE__HUMAN_BROWSE`
//! on, from a person, for its owner, with an open-web policy; the run takes no hosts from
//! the caller. Built and compile-checked, NOT run (wave 2 rule).
//!
//! The switch is process-wide, so every test in this file turns it on first; the refusal
//! with the switch off is pinned by the pure unit test `human_browse_refused_when_flag_off`.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use std::collections::HashMap;
use std::sync::Arc;

use agent_harness::*;
use async_trait::async_trait;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::QuarryResult;
use quarry_edge::run_limits::RunLimits;
use quarry_runtime::{GrantValidation, GrantValidator};
use serde_json::{json, Value};

/// The broker, from memory: it knows the grants it was given and nothing else.
struct FakeBroker(HashMap<String, GrantValidation>);

#[async_trait]
impl GrantValidator for FakeBroker {
    async fn validate(&self, grant_id: &str) -> QuarryResult<GrantValidation> {
        self.0
            .get(grant_id)
            .cloned()
            .ok_or_else(|| QuarryError::new(ErrorCode::NotFound, "unknown browser grant"))
    }
}

fn switch_on() {
    std::env::set_var("QUARRY_EDGE__HUMAN_BROWSE", "1");
}

async fn edge_with(grants: Vec<GrantValidation>, require: bool) -> Edge {
    let broker = FakeBroker(
        grants
            .into_iter()
            .map(|grant| (grant.grant_id.clone(), grant))
            .collect(),
    );
    Edge::start_with(RunLimits::default(), move |state| {
        let mut state = state;
        state.grant_validator = Arc::new(broker);
        state.require_browser_grants = require;
        state
    })
    .await
}

/// A person's grant as the broker would answer it, for `owner`, in `policy_mode`.
fn human_grant(owner: &str, policy_mode: &str, denied: &[&str], zdr: bool) -> GrantValidation {
    serde_json::from_value(json!({
        "grant_id": "grant-human",
        "active": true,
        "kind": "human_browse",
        "owner_id": owner,
        "policy_mode": policy_mode,
        "denied_domains": denied,
        "zdr": zdr,
    }))
    .expect("a grant validation")
}

/// A start that names the grant and, as a client would, its own host list. The host list
/// must be ignored for a human-browse run.
fn start_body() -> Value {
    json!({
        "grant_id": "grant-human",
        "constraints": { "max_steps": 0, "allowed_domains": ["example.com"] },
    })
}

async fn run_domains(edge: &Edge, run: &str) -> Vec<String> {
    let handle = edge.live(run).expect("the run is live");
    let entry = handle.entry.lock().await;
    entry.constraints.allowed_domains.clone()
}

/// A person starts an open-web run; the caller's host list is not the run's authority.
#[tokio::test]
async fn human_browse_grant_sets_open_web_policy() {
    switch_on();
    let edge = edge_with(
        vec![human_grant("user-golden", "open_web", &["bank.example"], false)],
        true,
    )
    .await;
    let run = edge.start_run(&member_token(), start_body()).await;
    assert!(run_domains(&edge, &run).await.is_empty());
    assert!(edge.driver.acquired().len() == 1);
}

/// A service principal may not start a human-browse run: only a person may.
#[tokio::test]
async fn human_browse_service_principal_is_refused() {
    switch_on();
    let edge = edge_with(
        vec![human_grant("user-golden", "open_web", &[], false)],
        true,
    )
    .await;
    let refused = edge.post(&service_token(), "/v1/agent/runs", start_body()).await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    assert_eq!(refused.message(), "human_browse_service_refused");
    assert!(edge.driver.acquired().is_empty());
}

/// A grant is refused to anyone but its owner.
#[tokio::test]
async fn human_browse_service_owner_mismatch_is_refused() {
    switch_on();
    let edge = edge_with(
        vec![human_grant("user-someone-else", "open_web", &[], false)],
        true,
    )
    .await;
    let refused = edge.post(&member_token(), "/v1/agent/runs", start_body()).await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    assert_eq!(refused.message(), "human_browse_owner_mismatch");
    assert!(edge.driver.acquired().is_empty());
}

/// An allow-list policy on a human-browse grant is refused: a person's run is open web or
/// nothing, and an unknown mode fails closed.
#[tokio::test]
async fn human_browse_old_policy_mode_unknown_fails_closed() {
    switch_on();
    let edge = edge_with(
        vec![human_grant("user-golden", "allowlist", &[], false)],
        true,
    )
    .await;
    let refused = edge.post(&member_token(), "/v1/agent/runs", start_body()).await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    assert_eq!(refused.message(), "human_browse_policy_unknown");
    assert!(edge.driver.acquired().is_empty());
}

/// A ZDR human grant cannot back a non-ZDR run. The generic ZDR-grant check runs before the
/// human-browse gate, so its message is the generic one; the refusal and the empty browser
/// pool are what this pins.
#[tokio::test]
async fn zdr_human_grant_cannot_back_a_non_zdr_run() {
    switch_on();
    let edge = edge_with(
        vec![human_grant("user-golden", "open_web", &[], true)],
        true,
    )
    .await;
    let refused = edge.post(&member_token(), "/v1/agent/runs", start_body()).await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    assert!(edge.driver.acquired().is_empty());
}
