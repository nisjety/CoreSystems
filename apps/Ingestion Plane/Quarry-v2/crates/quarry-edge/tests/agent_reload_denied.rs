//! WS9.B / D2-012 (MP-GATEWAY-AUTH): a page reload proposed by an agent is refused by
//! Quarry itself, on every action surface of a live run, through the real router and real
//! token verification (the harness is in `support/agent_harness.rs`).
//!
//! Before this the reload rule lived only in the verevonv3 gateway. The Model Plane drives
//! Quarry directly with a service token, so the gateway never saw it, and Quarry accepted a
//! `reload` from an agent step. The decision (no Model Plane credential to the gateway;
//! Quarry decides who the agent is from the verified token) needs the rule here.
//!
//! The agent is whoever `derive_initiator` says: a service principal (whatever it states),
//! or a person's token labelled `agent`. For each way an action can arrive (`POST .../step`,
//! its alias `POST .../interact`, and an `action` message on the browser WebSocket) these
//! pin that the refusal is `403 FORBIDDEN` with `details.reason == "agent_reload_denied"`
//! and that it leaves nothing behind: the driver is never asked to reload, and no receipt,
//! timeline item, budget step or attempt, spent approval or step counter moves. A person's
//! own reload still runs, and the same counters then move by exactly one, so the zeros are
//! not a dead counter.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use agent_harness::*;
use quarry_edge::run_limits::RunLimits;
use serde_json::{json, Value};

fn reload_action() -> Value {
    json!({ "type": "reload", "ignore_cache": true })
}

/// Everything a run can have consumed: the registry's own counters for it.
#[derive(Debug, PartialEq, Eq)]
struct Consumed {
    ctx_step: u32,
    budget_steps: Option<u32>,
    budget_attempts: Option<u32>,
    spent_approvals: usize,
}

async fn consumed(edge: &Edge, run: &str) -> Consumed {
    let handle = edge.live(run).expect("the run is live");
    let entry = handle.entry.lock().await;
    Consumed {
        ctx_step: entry.summary.step,
        budget_steps: entry.budget.as_ref().map(|budget| budget.ledger.steps),
        budget_attempts: entry.budget.as_ref().map(|budget| budget.ledger.attempts),
        spent_approvals: entry.used_sensitive_approval_grants.len(),
    }
}

/// The refusal every surface must give.
fn assert_denied_reply(reply: &Reply, surface: &str) {
    assert_eq!(reply.status, 403, "{surface}: {}", reply.body);
    assert_eq!(reply.code(), "FORBIDDEN", "{surface}: {}", reply.body);
    assert_eq!(
        reply.details()["reason"], "agent_reload_denied",
        "{surface}: {}",
        reply.body
    );
    assert_eq!(
        reply.details()["denied_action"], "reload",
        "{surface}: {}",
        reply.body
    );
}

/// Nothing was consumed by the denial, and the driver was never asked to reload.
async fn assert_nothing_consumed(
    edge: &Edge,
    bearer: &str,
    run: &str,
    before: &Consumed,
    surface: &str,
) {
    assert_eq!(
        &consumed(edge, run).await,
        before,
        "{surface}: a denial consumed a step, an attempt or an approval"
    );
    assert_eq!(
        edge.driver.reload_calls(),
        0,
        "{surface}: the driver was asked to reload"
    );
    assert!(
        edge.receipts(bearer, run).await.is_empty(),
        "{surface}: a denial left a receipt"
    );
}

/// A typed action right after still runs and moves the counters by exactly one: the run is
/// not poisoned by the refusal, and the zeros above are not a dead counter.
async fn assert_the_run_still_works(edge: &Edge, bearer: &str, run: &str, before: &Consumed, surface: &str) {
    let ran = edge.step(bearer, run, wait_action(), None).await;
    assert_eq!(ran.status, 200, "{surface}: {}", ran.body);
    let after = consumed(edge, run).await;
    assert_eq!(after.ctx_step, before.ctx_step + 1, "{surface}");
    assert_eq!(
        after.budget_attempts,
        before.budget_attempts.map(|attempts| attempts + 1),
        "{surface}: the budget counter did not move for a typed action"
    );
    assert_eq!(edge.receipts(bearer, run).await.len(), 1, "{surface}");
}

/// A service principal (execution-core, the Model Plane's path) is always the agent, so its
/// reload is refused whatever it states about itself.
#[tokio::test]
async fn a_service_principals_reload_is_refused_on_step_and_interact() {
    let edge = Edge::start(RunLimits::default()).await;
    let service = service_token();
    let run = edge.start_run(&service, json!({})).await;
    let before = consumed(&edge, &run).await;
    let timeline_before = edge.timeline(&service, &run).await.len();

    for hint in [None, Some("human"), Some("agent")] {
        let step = edge.step(&service, &run, reload_action(), hint).await;
        assert_denied_reply(&step, &format!("service step {hint:?}"));
    }
    let interact = edge
        .post(
            &service,
            &format!("/v1/agent/runs/{run}/interact"),
            json!({ "action": reload_action(), "initiated_by": "human" }),
        )
        .await;
    assert_denied_reply(&interact, "service interact");

    assert_nothing_consumed(&edge, &service, &run, &before, "service").await;
    assert_eq!(
        edge.timeline(&service, &run).await.len(),
        timeline_before,
        "a denial left a timeline item"
    );
    assert_the_run_still_works(&edge, &service, &run, &before, "service").await;
}

/// A person's token labelled `agent` (the shape the verevonv3 gateway uses for a model's
/// action through the person's own session) is the agent for this call; a `human` label is
/// not a way out because only the stricter label counts.
#[tokio::test]
async fn a_persons_token_labelled_agent_cannot_reload() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({ "initiated_by": "agent" })).await;
    let before = consumed(&edge, &run).await;

    let step = edge.step(&person, &run, reload_action(), Some("agent")).await;
    assert_denied_reply(&step, "labelled step");
    let interact = edge
        .post(
            &person,
            &format!("/v1/agent/runs/{run}/interact"),
            json!({ "action": reload_action(), "initiated_by": "agent" }),
        )
        .await;
    assert_denied_reply(&interact, "labelled interact");

    assert_nothing_consumed(&edge, &person, &run, &before, "labelled").await;
    assert_the_run_still_works(&edge, &person, &run, &before, "labelled").await;
}

/// The browser socket's `action` message goes through the same admission: an error frame
/// with the REST code and reason, nothing consumed, and the socket still answers a typed
/// action afterwards.
#[tokio::test]
async fn the_browser_socket_refuses_an_agents_reload_with_an_error_frame() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let before = consumed(&edge, &run).await;

    let mut socket =
        TestSocket::connect(&edge, &person, &format!("/v1/agent/runs/{run}/frames/ws")).await;
    socket
        .send_text(
            &json!({ "type": "action", "action": reload_action(), "actor": "agent" }).to_string(),
        )
        .await;
    let refused = socket
        .next_matching(|message| message["type"] == "error" || message["type"] == "observation")
        .await;
    assert_eq!(refused["type"], "error", "{refused}");
    assert_eq!(refused["code"], "FORBIDDEN", "{refused}");
    assert_eq!(refused["details"]["reason"], "agent_reload_denied", "{refused}");
    assert_eq!(consumed(&edge, &run).await, before, "a socket denial consumed something");
    assert_eq!(edge.driver.reload_calls(), 0);

    socket
        .send_text(&json!({ "type": "action", "action": wait_action() }).to_string())
        .await;
    let ran = socket
        .next_matching(|message| message["type"] == "observation" || message["type"] == "error")
        .await;
    assert_eq!(ran["type"], "observation", "{ran}");
    drop(socket);
    assert_eq!(edge.receipts(&person, &run).await.len(), 1);
}

/// The rule is about who the agent is, not about the action: a person's own reload (no
/// label, or `human`) runs, reaches the driver once, and is receipted.
#[tokio::test]
async fn a_persons_own_reload_still_runs() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;

    for (index, hint) in [None, Some("human")].into_iter().enumerate() {
        let reply = edge.step(&person, &run, reload_action(), hint).await;
        assert_eq!(reply.status, 200, "person step {hint:?}: {}", reply.body);
        assert_eq!(edge.driver.reload_calls() as usize, index + 1, "{hint:?}");
    }
    let interact_reload = edge
        .post(
            &person,
            &format!("/v1/agent/runs/{run}/interact"),
            json!({ "action": reload_action() }),
        )
        .await;
    assert_eq!(interact_reload.status, 200, "{}", interact_reload.body);
    assert_eq!(edge.driver.reload_calls(), 3);
    assert_eq!(edge.receipts(&person, &run).await.len(), 3);
}
