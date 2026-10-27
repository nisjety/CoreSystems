//! WS7.D: the agent path's web policy, through the real router and real token
//! verification (the harness is in `support/agent_harness.rs`).
//!
//! A denial has to leave nothing behind. These tests pin:
//!
//! - a capability the run's policy disables is a typed 403 `policy_denied`, with
//!   `capability` and `decision_source`, on REST `step` and on the browser WebSocket;
//! - that denial consumes no step and writes no receipt, and the run still runs a typed
//!   action straight after, so the zeros are not a dead counter;
//! - a persistent profile the policy disables is refused at start, with ZDR off;
//! - a host the policy denies is refused even where the run's own allow list permits it
//!   (the intersection denies; it never falls back to allow-all);
//! - an operator baseline refuses a host with no ask at all, and names itself as the
//!   deciding layer;
//! - raw script stays `raw_script_denied` with the policy in place.
//!
//! Not written here (test phase, see `.plan-work/quarry-w1w2/W2-U29-progress.md`): the upload
//! and download refusals (the harness driver does not advertise `uploads_from_artifacts` or
//! `downloads_to_artifacts`, so the capability gate answers first), the resumed-run refusal
//! (needs a checkpoint of a persisted profile) and the broker-grant equality case (needs a
//! grant validator double).

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use agent_harness::*;
use quarry_edge::policy_baseline::PolicyBaseline;
use quarry_edge::run_limits::RunLimits;
use quarry_edge::state::AppState;
use serde_json::{json, Value};

fn click_action() -> Value {
    json!({ "type": "click", "selector": "#go" })
}

fn navigate_action(url: &str) -> Value {
    json!({ "type": "navigate", "url": url })
}

/// How many steps the run has taken, read from the live entry.
async fn steps_taken(edge: &Edge, run: &str) -> u32 {
    let handle = edge.live(run).expect("the run is live");
    let entry = handle.entry.lock().await;
    entry.summary.step
}

/// A run whose policy disables interaction (clicks, pointer input).
async fn start_without_interaction(edge: &Edge, bearer: &str) -> String {
    edge.start_run(
        bearer,
        json!({
            "constraints": { "max_steps": 10 },
            "policy": { "disable": ["browser.act.interact"] },
        }),
    )
    .await
}

#[tokio::test]
async fn persistent_profile_denied_when_policy_disables_it_even_without_zdr() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();

    let reply = edge
        .post(
            &person,
            "/v1/agent/runs",
            json!({
                "persist_profile": true,
                "policy": { "disable": ["browser.profile.persistent"] },
            }),
        )
        .await;
    assert_eq!(reply.status, 403, "{}", reply.body);
    assert_eq!(reply.code(), "FORBIDDEN");
    assert_eq!(reply.details()["reason"], "policy_denied");
    assert_eq!(reply.details()["capability"], "browser.profile.persistent");
    assert_eq!(reply.details()["decision_source"], "request");
    assert_eq!(
        edge.state.agent_runs.stats().live,
        0,
        "a refused start left a live run"
    );
}

#[tokio::test]
async fn persistent_profile_is_allowed_when_nothing_disables_it() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();

    let reply = edge
        .post(&person, "/v1/agent/runs", json!({ "persist_profile": true }))
        .await;
    assert_eq!(reply.status, 200, "{}", reply.body);
}

#[tokio::test]
async fn a_disabled_capability_is_refused_on_rest_step_and_consumes_nothing() {
    let edge = Edge::start(RunLimits::default()).await;
    // `browser.act.interact` applies to an autonomous principal only (the catalogue's
    // `applies_to`, WS7.B): a person clicking is not an agent clicking, so the same ask does
    // not refuse a person's token (`human_principal_skips_autonomous_only_keys`). The
    // autonomous caller here is the service principal that drives its own runs.
    let person = service_token();
    let run = start_without_interaction(&edge, &person).await;
    let before = steps_taken(&edge, &run).await;

    let reply = edge.step(&person, &run, click_action(), None).await;
    assert_eq!(reply.status, 403, "{}", reply.body);
    assert_eq!(reply.code(), "FORBIDDEN");
    assert_eq!(reply.details()["reason"], "policy_denied");
    assert_eq!(reply.details()["capability"], "browser.act.interact");
    assert_eq!(reply.details()["denied_action"], "click");
    assert_eq!(
        steps_taken(&edge, &run).await,
        before,
        "a policy denial consumed a step"
    );
    assert!(
        edge.receipts(&person, &run).await.is_empty(),
        "a policy denial left a receipt"
    );

    // The run is not poisoned: a typed action that the policy allows still runs, and the
    // counters move by exactly that one action.
    let ran = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(ran.status, 200, "{}", ran.body);
    assert_eq!(steps_taken(&edge, &run).await, before + 1);
    assert_eq!(edge.receipts(&person, &run).await.len(), 1);
}

#[tokio::test]
async fn the_browser_socket_refuses_the_same_capability_through_the_same_gate() {
    let edge = Edge::start(RunLimits::default()).await;
    // `browser.act.interact` applies to an autonomous principal only (the catalogue's
    // `applies_to`, WS7.B): a person clicking is not an agent clicking, so the same ask does
    // not refuse a person's token (`human_principal_skips_autonomous_only_keys`). The
    // autonomous caller here is the service principal that drives its own runs.
    let person = service_token();
    let run = start_without_interaction(&edge, &person).await;
    let before = steps_taken(&edge, &run).await;

    let mut socket =
        TestSocket::connect(&edge, &person, &format!("/v1/agent/runs/{run}/frames/ws")).await;
    socket
        .send_text(&json!({ "type": "action", "action": click_action() }).to_string())
        .await;
    let refused = socket
        .next_matching(|message| message["type"] == "error")
        .await;
    let text = refused["message"].as_str().unwrap_or_default().to_owned();
    assert!(text.starts_with("Forbidden:"), "{refused}");
    assert_eq!(
        steps_taken(&edge, &run).await,
        before,
        "a socket denial consumed a step"
    );

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

#[tokio::test]
async fn raw_script_is_still_refused_as_raw_script_with_the_policy_in_place() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = start_without_interaction(&edge, &person).await;
    let before = steps_taken(&edge, &run).await;

    let reply = edge
        .step(
            &person,
            &run,
            json!({ "type": "evaluate", "script": "document.title = 'x'" }),
            None,
        )
        .await;
    assert_eq!(reply.status, 403, "{}", reply.body);
    assert_eq!(reply.details()["reason"], "raw_script_denied");
    assert_eq!(steps_taken(&edge, &run).await, before);
    assert!(edge.receipts(&person, &run).await.is_empty());
}

#[tokio::test]
async fn a_host_the_policy_denies_is_refused_even_when_the_run_allows_it() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    // The run's own allow list permits example.com; the request denies it. The
    // intersection is empty, so nothing is left: it denies, it does not fall back to
    // allow-all.
    let run = edge
        .start_run(
            &person,
            json!({
                "constraints": { "max_steps": 10, "allowed_domains": ["example.com"] },
                "policy": { "deny_domains": ["example.com"] },
            }),
        )
        .await;
    let before = steps_taken(&edge, &run).await;

    let reply = edge
        .step(&person, &run, navigate_action("https://example.com/"), None)
        .await;
    assert_eq!(reply.status, 403, "{}", reply.body);
    assert_eq!(reply.details()["reason"], "domain_denied");
    assert_eq!(reply.details()["capability"], "domains.deny");
    assert_eq!(reply.details()["decision_source"], "request");
    assert_eq!(steps_taken(&edge, &run).await, before);
    assert!(edge.receipts(&person, &run).await.is_empty());
}

#[tokio::test]
async fn the_operator_baseline_refuses_a_host_with_no_ask_and_names_itself() {
    let baseline = PolicyBaseline::from_lookup(|name| {
        (name == "QUARRY_EDGE__WEB_POLICY_BASELINE_JSON")
            .then(|| r#"{"domains":{"deny":["example.net"]}}"#.to_string())
    })
    .expect("the baseline parses");
    let edge = Edge::start_with(RunLimits::default(), |state| AppState {
        policy: baseline,
        ..state
    })
    .await;
    let person = member_token();
    let run = edge
        .start_run(
            &person,
            json!({
                "constraints": { "max_steps": 10, "allowed_domains": ["example.net"] },
            }),
        )
        .await;

    let reply = edge
        .step(&person, &run, navigate_action("https://example.net/"), None)
        .await;
    assert_eq!(reply.status, 403, "{}", reply.body);
    assert_eq!(reply.details()["reason"], "domain_denied");
    assert_eq!(reply.details()["decision_source"], "baseline");
    assert!(edge.receipts(&person, &run).await.is_empty());
}
