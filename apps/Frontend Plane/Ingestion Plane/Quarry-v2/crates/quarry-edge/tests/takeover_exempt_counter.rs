//! TAKEOVER-BUDGET: the steps a person takes while holding an agent-class run's browser do
//! not count against the run's budget (7d78ab2e8, plan R-8 and WS8.F). They are metered
//! apart so the X-12 shadow review can see takeover use: `quarry_agent_takeover_exempt_steps_total`
//! and the `_sum` of `quarry_agent_takeover_exempt_step_duration_seconds`.
//!
//! Through the real router: the counter moves only for the step the takeover rule spared
//! (a person's step, during a takeover, on a run whose class is the agent's), on both the
//! REST step and the browser socket. It does not move for the same person's step on the same
//! run after the browser is handed back (that step counts against the budget), for a person
//! on their own run (exempt anyway), or for an agent's step.
//!
//! One test, one process-global Prometheus recorder: the counter is read as a delta around
//! each call, so nothing else in the binary can disturb it.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use std::sync::OnceLock;

use agent_harness::*;
use metrics_exporter_prometheus::PrometheusHandle;
use quarry_edge::metrics;
use quarry_edge::run_limits::RunLimits;
use serde_json::json;

fn recorder() -> &'static PrometheusHandle {
    static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();
    HANDLE.get_or_init(|| metrics::install().expect("install the recorder"))
}

/// The value of an unlabelled series in the exposition, 0 before it exists.
fn series(name: &str) -> f64 {
    recorder()
        .render()
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(' ')?;
            (key == name).then(|| value.trim().parse::<f64>().expect("a number"))
        })
        .unwrap_or(0.0)
}

fn steps() -> f64 {
    series("quarry_agent_takeover_exempt_steps_total")
}

fn seconds() -> f64 {
    series("quarry_agent_takeover_exempt_step_duration_seconds_sum")
}

fn observations() -> f64 {
    series("quarry_agent_takeover_exempt_step_duration_seconds_count")
}

async fn take_over(edge: &Edge, person: &str, run: &str, mode: &str) {
    let reply = edge.control(person, run, json!({ "mode": mode })).await;
    assert_eq!(reply.status, 200, "{mode}: {}", reply.body);
}

#[tokio::test]
async fn a_takeover_step_the_budget_does_not_count_is_metered_apart() {
    recorder();
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    // An agent-class run (budgeted as the agent's), owned by the person.
    let run = edge.start_run(&person, json!({ "initiated_by": "agent" })).await;

    // Outside a takeover a person's step on an agent-class run counts against the budget,
    // so it is not takeover use.
    let (steps0, seconds0, count0) = (steps(), seconds(), observations());
    let reply = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(steps(), steps0, "a step that counts is not takeover use");

    // The person takes over: each of their steps is metered, on REST and on the socket.
    take_over(&edge, &person, &run, "human_takeover").await;
    for _ in 0..2 {
        let reply = edge.step(&person, &run, wait_action(), None).await;
        assert_eq!(reply.status, 200, "{}", reply.body);
    }
    assert_eq!(steps(), steps0 + 2.0, "two REST takeover steps");

    let mut socket =
        TestSocket::connect(&edge, &person, &format!("/v1/agent/runs/{run}/frames/ws")).await;
    socket
        .send_text(&json!({ "type": "action", "action": wait_action() }).to_string())
        .await;
    let observed = socket
        .next_matching(|message| message["type"] == "observation" || message["type"] == "error")
        .await;
    assert_eq!(observed["type"], "observation", "{observed}");
    drop(socket);
    assert_eq!(steps(), steps0 + 3.0, "the socket step is metered too");
    assert_eq!(
        observations(),
        count0 + 3.0,
        "the duration histogram has one observation per metered step"
    );
    assert!(seconds() >= seconds0, "seconds never go down");

    // Handed back: the same person's step counts again and is not metered.
    take_over(&edge, &person, &run, "agent_control").await;
    let after = steps();
    let reply = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(steps(), after, "after the hand-back a person's step counts and is not metered");

    // A person's own run: their steps are exempt anyway, so the takeover changed nothing.
    let own = edge.start_run(&person, json!({})).await;
    take_over(&edge, &person, &own, "human_takeover").await;
    let reply = edge.step(&person, &own, wait_action(), None).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(steps(), after, "a step the budget never counted is not takeover use");

    // An agent's step is never spared by a takeover (it is refused or counted, not metered).
    take_over(&edge, &person, &run, "human_takeover").await;
    let before_agent = steps();
    let _ = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(steps(), before_agent, "an agent's step is not takeover use");
}
