//! X-12: the agent-run budget, as ratified on 2026-10-28, through the real router and real
//! token verification.
//!
//! The numbers: an agent run that asks for nothing gets 60 steps, 900 s and 1 USD; what it
//! asks for is clamped to 300 steps, 3600 s and 20 USD; the browser second is priced at
//! 0.00003 USD as an estimate, not a price. A person's own steps are never capped, and the
//! steps a person takes while holding an agent-class run's browser (takeover) do not count.
//!
//! The edge guard (decision 3.3): with enforcement `on`, the typed gate owns the bound of an
//! agent-class run and a person's session has none, so a model-proposed step inside a
//! person's session would be left with nothing. It keeps the request's own step cap (the
//! gateway sends 12), refused with the legacy 429, while the person's own steps stay uncapped.
//!
//! `on` is process-wide and read once, so the tests in this file run with it `on` (set before
//! the first read). `shadow` and `off` are proven in a child process of this same test binary
//! (`child_*`, run by `shadow_and_off_change_nothing_a_caller_can_see` and
//! `the_defaults_are_env_tunable_through_the_real_path` with their own environment), because a
//! second mode cannot be read in one process.
//!
//! Every number here is a plain request or the shipped default; no test sets the flag in a
//! deployment. The flag itself stays `shadow` until the stack proof
//! (`scripts/x12-budget-stack-proof.mjs`) has run.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use std::process::Command;
use std::sync::{Once, OnceLock};
use std::time::Duration;

use agent_harness::*;
use metrics_exporter_prometheus::PrometheusHandle;
use quarry_core::event::EventType;
use quarry_edge::budget::{BudgetLimits, ENV_MODE};
use quarry_edge::metrics;
use quarry_edge::run_limits::RunLimits;
use serde_json::{json, Value};
use serial_test::serial;

const CHILD_ENV: &str = "AGENT_RUN_BUDGET_CHILD";

/// Budget enforcement `on` for this process (read once, before anything reads it).
fn setup_on() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::env::set_var(ENV_MODE, "on");
        for name in [
            "QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS",
            "QUARRY_EDGE__AGENT_DEFAULT_MAX_RUNTIME_S",
            "QUARRY_EDGE__AGENT_DEFAULT_MAX_COST_USD",
            "QUARRY_EDGE__AGENT_CEILING_MAX_STEPS",
            "QUARRY_EDGE__AGENT_CEILING_MAX_RUNTIME_S",
            "QUARRY_EDGE__AGENT_CEILING_MAX_COST_USD",
            "QUARRY_EDGE__GRANT_BUDGET_CLAIM",
        ] {
            std::env::remove_var(name);
        }
    });
    set_takeover_enforcement(None);
}

fn recorder() -> &'static PrometheusHandle {
    static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();
    HANDLE.get_or_init(|| metrics::install().expect("install the recorder"))
}

/// The exposition lines of the budget-exhausted counter that carry every given fragment.
fn exhausted_series(fragments: &[&str]) -> Vec<String> {
    recorder()
        .render()
        .lines()
        .filter(|line| line.starts_with("quarry_agent_budget_exhausted_total{"))
        .filter(|line| fragments.iter().all(|fragment| line.contains(fragment)))
        .map(str::to_owned)
        .collect()
}

async fn edge() -> Edge {
    // No cap on how many runs there are and no idle sweeping in the way, so only the
    // budget is under test.
    let limits = RunLimits {
        agent_idle_ttl_s: 600,
        interactive_idle_ttl_s: 900,
        max_concurrent_runs: 0,
        max_runs_per_org: 0,
        max_interactive_runs_per_org: 0,
        ..RunLimits::default()
    };
    Edge::start(limits).await
}

/// A start body for an agent-class run owned by the person (the `agent` hint). `constraints`
/// are the limits asked for: an empty object asks for nothing, and `max_steps` is sent as 0
/// ("not asked") when the caller names only another limit, because the wire type needs it.
fn agent_body(constraints: Value) -> Value {
    let mut asked = constraints.as_object().cloned().unwrap_or_default();
    if asked.is_empty() {
        return json!({ "initiated_by": "agent" });
    }
    asked.entry("max_steps").or_insert(json!(0));
    json!({ "initiated_by": "agent", "constraints": asked })
}

fn constraints_of(reply: &Reply) -> &Value {
    &reply.data()["constraints"]
}

fn clamp_for<'a>(reply: &'a Reply, field: &str) -> Option<&'a Value> {
    reply.data()["clamped"]
        .as_array()?
        .iter()
        .find(|clamp| clamp["field"] == field)
}

async fn ledger_steps(edge: &Edge, run: &str) -> u32 {
    let handle = edge.live(run).expect("the run is live");
    let entry = handle.entry.lock().await;
    entry.budget.as_ref().expect("a budget").ledger.steps
}

async fn take_over(edge: &Edge, person: &str, run: &str, mode: &str) {
    let reply = edge.control(person, run, json!({ "mode": mode })).await;
    assert_eq!(reply.status, 200, "{mode}: {}", reply.body);
}

fn assert_budget_refusal(reply: &Reply, dimension: &str, limit: f64) {
    assert_eq!(reply.status, 402, "{}", reply.body);
    assert_eq!(reply.code(), "BUDGET_EXHAUSTED", "{}", reply.body);
    assert_eq!(reply.body["error"]["retryable"], false, "{}", reply.body);
    assert_eq!(reply.details()["dimension"], dimension, "{}", reply.body);
    assert_eq!(reply.details()["limit"], limit, "{}", reply.body);
    assert_eq!(reply.details()["limit_source"], "run_budget", "{}", reply.body);
    assert_eq!(reply.details()["resumable"], false, "{}", reply.body);
}

// ── the numbers ─────────────────────────────────────────────────────────────

#[test]
fn the_ratified_numbers_are_the_shipped_defaults_and_each_is_env_tunable() {
    let limits = BudgetLimits::default();
    assert_eq!(limits.default_max_steps, 60);
    assert_eq!(limits.default_max_runtime_s, 900);
    assert_eq!(limits.default_max_cost_usd, 1.0);
    assert_eq!(limits.ceiling_max_steps, 300);
    assert_eq!(limits.ceiling_max_runtime_s, 3600);
    assert_eq!(limits.ceiling_max_cost_usd, 20.0);
    assert_eq!(limits.browser_second_usd, 0.00003, "an estimate, not a price");

    // An empty environment is the defaults.
    assert_eq!(BudgetLimits::from_env(|_| None).unwrap(), limits);

    // Every number is a variable that changes it and only it.
    let tuned = |name: &'static str, value: &'static str| {
        BudgetLimits::from_env(move |asked| (asked == name).then(|| value.to_owned()))
    };
    assert_eq!(tuned("QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS", "10").unwrap().default_max_steps, 10);
    assert_eq!(tuned("QUARRY_EDGE__AGENT_DEFAULT_MAX_RUNTIME_S", "30").unwrap().default_max_runtime_s, 30);
    assert_eq!(tuned("QUARRY_EDGE__AGENT_DEFAULT_MAX_COST_USD", "0.5").unwrap().default_max_cost_usd, 0.5);
    assert_eq!(tuned("QUARRY_EDGE__AGENT_CEILING_MAX_STEPS", "400").unwrap().ceiling_max_steps, 400);
    assert_eq!(tuned("QUARRY_EDGE__AGENT_CEILING_MAX_RUNTIME_S", "7200").unwrap().ceiling_max_runtime_s, 7200);
    assert_eq!(tuned("QUARRY_EDGE__AGENT_CEILING_MAX_COST_USD", "50").unwrap().ceiling_max_cost_usd, 50.0);
    assert_eq!(tuned("QUARRY_EDGE__BUDGET_BROWSER_SECOND_USD", "0.0001").unwrap().browser_second_usd, 0.0001);

    // A bad value is refused at boot and names its variable; it never silently falls back.
    for (name, value) in [
        ("QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS", "0"),
        ("QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS", "many"),
        ("QUARRY_EDGE__AGENT_DEFAULT_MAX_COST_USD", "-1"),
        ("QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS", "301"),
        ("QUARRY_EDGE__AGENT_DEFAULT_MAX_RUNTIME_S", "3601"),
        ("QUARRY_EDGE__AGENT_DEFAULT_MAX_COST_USD", "21"),
    ] {
        let error = tuned(name, value).expect_err(&format!("{name}={value} must be refused"));
        assert!(error.contains("QUARRY_EDGE__"), "{error}");
    }
}

#[tokio::test]
#[serial]
async fn an_agent_run_that_asks_for_nothing_gets_the_defaults_and_one_that_asks_for_more_is_clamped() {
    setup_on();
    let edge = edge().await;
    let person = member_token();

    // Nothing asked: 60 steps, 900 s, 1 USD, every one reported as the default.
    let plain = edge.post(&person, "/v1/agent/runs", agent_body(json!({}))).await;
    assert_eq!(plain.status, 200, "{}", plain.body);
    let effective = constraints_of(&plain);
    assert_eq!(effective["class"], "agent", "{}", plain.body);
    assert_eq!(effective["enforcement"], "on", "{}", plain.body);
    assert_eq!(effective["max_steps"], 60, "{}", plain.body);
    assert_eq!(effective["max_runtime_s"], 900, "{}", plain.body);
    assert_eq!(effective["max_cost_usd"], 1.0, "{}", plain.body);
    assert_eq!(effective["estimated"], true, "{}", plain.body);
    for field in ["max_steps", "max_runtime_s", "max_cost_usd"] {
        assert_eq!(clamp_for(&plain, field).expect(field)["by"], "default", "{field}: {}", plain.body);
    }

    // A zero is not a way to ask for unlimited: it is "nothing asked".
    let zero = edge
        .post(&person, "/v1/agent/runs", agent_body(json!({ "max_steps": 0, "max_runtime_s": 0 })))
        .await;
    assert_eq!(constraints_of(&zero)["max_steps"], 60, "{}", zero.body);
    assert_eq!(constraints_of(&zero)["max_runtime_s"], 900, "{}", zero.body);

    // Asking for more than the ceiling: 300 steps, 3600 s, 20 USD, with what was asked kept.
    let greedy = edge
        .post(
            &person,
            "/v1/agent/runs",
            agent_body(json!({ "max_steps": 1000, "max_runtime_s": 100000, "max_cost_usd": 100.0 })),
        )
        .await;
    assert_eq!(greedy.status, 200, "{}", greedy.body);
    let effective = constraints_of(&greedy);
    assert_eq!(effective["max_steps"], 300, "{}", greedy.body);
    assert_eq!(effective["max_runtime_s"], 3600, "{}", greedy.body);
    assert_eq!(effective["max_cost_usd"], 20.0, "{}", greedy.body);
    for (field, asked) in [("max_steps", 1000.0), ("max_runtime_s", 100000.0), ("max_cost_usd", 100.0)] {
        let clamp = clamp_for(&greedy, field).expect(field);
        assert_eq!(clamp["by"], "ceiling", "{field}: {}", greedy.body);
        assert_eq!(clamp["requested"], asked, "{field}: {}", greedy.body);
    }

    // Asking for less is honoured, and nothing is reported.
    let modest = edge
        .post(
            &person,
            "/v1/agent/runs",
            agent_body(json!({ "max_steps": 5, "max_runtime_s": 100, "max_cost_usd": 0.5 })),
        )
        .await;
    assert_eq!(constraints_of(&modest)["max_steps"], 5, "{}", modest.body);
    assert_eq!(constraints_of(&modest)["max_runtime_s"], 100, "{}", modest.body);
    assert_eq!(constraints_of(&modest)["max_cost_usd"], 0.5, "{}", modest.body);
    assert!(
        modest.data()["clamped"].as_array().is_none_or(|all| all.is_empty()),
        "{}",
        modest.body
    );

    // A service principal is an agent whatever the body says.
    let service = edge.post(&service_token(), "/v1/agent/runs", json!({})).await;
    assert_eq!(service.status, 200, "{}", service.body);
    assert_eq!(constraints_of(&service)["class"], "agent", "{}", service.body);
    assert_eq!(constraints_of(&service)["max_steps"], 60, "{}", service.body);
}

// ── each bound, refused with the typed 402 ──────────────────────────────────

#[tokio::test]
#[serial]
async fn an_agent_run_stops_at_the_default_60_steps_with_a_typed_402_that_latches() {
    setup_on();
    recorder();
    let edge = edge().await;
    let person = member_token();
    // No limits asked: the default 60 applies.
    let run = edge.start_run(&person, agent_body(json!({}))).await;

    for n in 1..=60 {
        let step = edge.step(&person, &run, wait_action(), None).await;
        assert_eq!(step.status, 200, "step {n}: {}", step.body);
    }
    let refused = edge.step(&person, &run, wait_action(), None).await;
    assert_budget_refusal(&refused, "steps", 60.0);
    assert_eq!(refused.details()["spent"], 60.0, "{}", refused.body);

    // It stays refused: calling again cannot refill it, and the run keeps what it spent.
    let again = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(again.status, 402, "{}", again.body);
    assert_eq!(ledger_steps(&edge, &run).await, 60, "a refusal runs nothing and counts nothing");

    // Announced once, as the run's own event, and counted as a refusal (not a would-be one).
    let announced = edge
        .durable_events()
        .await
        .into_iter()
        .filter(|(kind, _)| *kind == EventType::AgentBudgetExhausted)
        .collect::<Vec<_>>();
    assert_eq!(announced.len(), 1, "{announced:?}");
    assert_eq!(announced[0].1["dimension"], "steps");
    assert!(
        !exhausted_series(&["dimension=\"steps\"", "class=\"agent\"", "mode=\"on\""]).is_empty(),
        "{}",
        recorder().render()
    );
}

/// X-12 stack row "SSE shows exactly one `agent.budget_exhausted`": the run's event stream
/// carries the announcement once however many calls are refused, under that name, with the
/// refusal's figures only (no URL, no page, no prompt).
#[tokio::test]
#[serial]
async fn the_event_stream_carries_one_budget_exhausted_per_refused_segment() {
    setup_on();
    let edge = edge().await;
    let person = member_token();
    let run = edge
        .start_run(&person, agent_body(json!({ "max_steps": 2 })))
        .await;
    let stream = edge
        .open_stream(&person, &format!("/v1/agent/runs/{run}/events/stream"))
        .await;

    for n in 1..=2 {
        let step = edge.step(&person, &run, wait_action(), None).await;
        assert_eq!(step.status, 200, "step {n}: {}", step.body);
    }
    // Three refused calls, as on the stack: one announcement.
    for _ in 0..3 {
        let refused = edge.step(&person, &run, wait_action(), None).await;
        assert_budget_refusal(&refused, "steps", 2.0);
    }
    let closed = edge.delete(&person, &format!("/v1/agent/runs/{run}")).await;
    assert_eq!(closed.status, 200, "{}", closed.body);

    let body = stream.text().await.expect("the stream ends after done");
    let announced = body
        .split("\n\n")
        .filter(|block| block.lines().any(|line| line == "event: agent.budget_exhausted"))
        .collect::<Vec<_>>();
    assert_eq!(announced.len(), 1, "{body}");
    let data = announced[0]
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .expect("the event has data");
    let event: Value = serde_json::from_str(data).expect("the data is the Event envelope");
    assert_eq!(event["type"], "agent_budget_exhausted", "{data}");
    let payload = event["payload"].as_object().expect("a payload object");
    let mut keys = payload.keys().map(String::as_str).collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["dimension", "estimated", "limit", "limit_source", "spent", "unit"],
        "{data}"
    );
    assert_eq!(payload["dimension"], "steps", "{data}");
    assert_eq!(payload["limit"], 2.0, "{data}");
    assert_eq!(payload["spent"], 2.0, "{data}");
    // Before the terminal event, and the stream still ends with `done`.
    let announced_at = body.find("event: agent.budget_exhausted").expect("announced");
    let completed_at = body.find("event: agent.completed").expect("agent.completed sent");
    assert!(announced_at < completed_at, "{body}");
    assert!(body.contains("data: run_closed"), "{body}");
}

#[tokio::test]
#[serial]
async fn an_agent_run_stops_at_its_runtime_with_a_typed_402() {
    setup_on();
    let edge = edge().await;
    let person = member_token();
    let run = edge
        .start_run(&person, agent_body(json!({ "max_runtime_s": 1 })))
        .await;
    let first = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(first.status, 200, "{}", first.body);

    tokio::time::sleep(Duration::from_millis(1300)).await;
    let refused = edge.step(&person, &run, wait_action(), None).await;
    assert_budget_refusal(&refused, "runtime", 1.0);
}

#[tokio::test]
#[serial]
async fn an_agent_run_stops_before_it_overshoots_its_cost_with_a_typed_402() {
    setup_on();
    let edge = edge().await;
    let person = member_token();
    // A wait is estimated at 0.0001 USD, so 0.00025 pays for two and refuses the third
    // before it runs (the refusal is made on the projection, not after the overshoot).
    let run = edge
        .start_run(&person, agent_body(json!({ "max_cost_usd": 0.00025 })))
        .await;
    for n in 1..=2 {
        let step = edge.step(&person, &run, wait_action(), None).await;
        assert_eq!(step.status, 200, "step {n}: {}", step.body);
    }
    let refused = edge.step(&person, &run, wait_action(), None).await;
    assert_budget_refusal(&refused, "cost", 0.00025);
    assert_eq!(refused.details()["estimated"], true, "cost is labelled an estimate: {}", refused.body);
    let spent = refused.details()["spent"].as_f64().expect("spent");
    assert!((0.0002..0.00025).contains(&spent), "spent {spent}");
}

// ── people are uncapped, and the guard ──────────────────────────────────────

#[tokio::test]
#[serial]
async fn a_persons_own_steps_are_never_capped() {
    setup_on();
    let edge = edge().await;
    let person = member_token();
    // The gateway asks for 12 steps for a person's browser session; the person is not held to it.
    let run = edge.start_run(&person, json!({ "constraints": { "max_steps": 12 } })).await;
    for n in 1..=70 {
        let step = edge.step(&person, &run, wait_action(), None).await;
        assert_eq!(step.status, 200, "the person's step {n} (past 12 and past the agent's 60): {}", step.body);
    }
    // Their steps were not counted either.
    assert_eq!(ledger_steps(&edge, &run).await, 0);

    // A person-class run has no typed caps to report.
    let start = edge.post(&person, "/v1/agent/runs", json!({})).await;
    assert_eq!(constraints_of(&start)["class"], "interactive", "{}", start.body);
    assert!(constraints_of(&start).get("max_steps").is_none(), "{}", start.body);
}

#[tokio::test]
#[serial]
async fn an_agent_step_inside_a_persons_session_keeps_the_request_cap_while_the_person_stays_uncapped() {
    setup_on();
    let edge = edge().await;
    let person = member_token();
    // A person's session as the gateway opens it: a request cap of 3 for the model's steps.
    let run = edge.start_run(&person, json!({ "constraints": { "max_steps": 3 } })).await;

    for n in 1..=3 {
        let step = edge.step(&person, &run, wait_action(), Some("agent")).await;
        assert_eq!(step.status, 200, "the model's step {n}: {}", step.body);
    }
    // The fourth is bounded: the legacy 429, not a typed 402 (this run has no typed caps).
    let refused = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(refused.status, 429, "{}", refused.body);
    assert_ne!(refused.code(), "BUDGET_EXHAUSTED", "{}", refused.body);

    // The person on the same run, past that cap, is not held to it.
    for n in 1..=5 {
        let step = edge.step(&person, &run, wait_action(), None).await;
        assert_eq!(step.status, 200, "the person's step {n} after the cap: {}", step.body);
    }
    // And the model is still refused afterwards: the person's steps do not reopen the cap.
    let still = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(still.status, 429, "{}", still.body);
}

// ── takeover is exempt ──────────────────────────────────────────────────────

#[tokio::test]
#[serial]
async fn a_persons_takeover_steps_on_an_agent_run_do_not_count_and_are_not_refused() {
    setup_on();
    let edge = edge().await;
    let person = member_token();
    let run = edge
        .start_run(&person, agent_body(json!({ "max_steps": 2 })))
        .await;

    // One step before the takeover counts: a person's step on an agent-class run is the
    // agent's budget (R-8).
    let counted = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(counted.status, 200, "{}", counted.body);
    assert_eq!(ledger_steps(&edge, &run).await, 1);

    // During the takeover the person goes well past the 2-step bound, and nothing is spent.
    take_over(&edge, &person, &run, "human_takeover").await;
    for n in 1..=6 {
        let step = edge.step(&person, &run, wait_action(), None).await;
        assert_eq!(step.status, 200, "takeover step {n}: {}", step.body);
    }
    assert_eq!(ledger_steps(&edge, &run).await, 1, "takeover steps are exempt");

    // Handed back: the same person's step counts again, and the bound is where it was left.
    take_over(&edge, &person, &run, "agent_control").await;
    let second = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(second.status, 200, "{}", second.body);
    assert_eq!(ledger_steps(&edge, &run).await, 2);
    let refused = edge.step(&person, &run, wait_action(), None).await;
    assert_budget_refusal(&refused, "steps", 2.0);

    // Taking over a run that has hit its bound is not refused either, and spends nothing.
    take_over(&edge, &person, &run, "human_takeover").await;
    let during = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(during.status, 200, "{}", during.body);
    assert_eq!(ledger_steps(&edge, &run).await, 2);
}

// ── on against shadow and off, each in its own process ──────────────────────

/// Run one `child_*` test of this binary in its own process with `mode` and `extra`
/// environment, and say what it printed.
fn in_child(case: &str, mode: &str, extra: &[(&str, &str)]) -> String {
    let mut command = Command::new(std::env::current_exe().expect("this test binary"));
    command
        .args(["--ignored", "--exact", case, "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, case)
        .env(ENV_MODE, mode)
        .env_remove("QUARRY_EDGE__GRANT_BUDGET_CLAIM")
        .env_remove("QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS")
        .env_remove("QUARRY_EDGE__AGENT_CEILING_MAX_STEPS");
    for (name, value) in extra {
        command.env(name, value);
    }
    let output = command.output().expect("run the child test");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success() && text.contains("1 passed"),
        "{case} ({mode}) failed:\n{text}"
    );
    text
}

#[test]
fn shadow_and_off_change_nothing_a_caller_can_see() {
    in_child("child_shadow", "shadow", &[("QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS", "3")]);
    in_child("child_off", "off", &[]);
}

#[test]
fn the_defaults_are_env_tunable_through_the_real_path() {
    in_child(
        "child_on_tuned",
        "on",
        &[
            ("QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS", "3"),
            ("QUARRY_EDGE__AGENT_CEILING_MAX_STEPS", "5"),
        ],
    );
}

fn in_this_child(case: &str) -> bool {
    std::env::var(CHILD_ENV).is_ok_and(|asked| asked == case)
}

/// `shadow`: an agent run past its (tuned, 3-step) default is not refused, and what would
/// have been refused is counted as such. The request's own step cap is unchanged for
/// everyone: a model's step in a person's session is still bounded by it (D2-014), and the
/// person's own steps are never capped.
#[tokio::test]
#[ignore = "run by shadow_and_off_change_nothing_a_caller_can_see, in its own process"]
async fn child_shadow() {
    assert!(in_this_child("child_shadow"), "run only by its parent test");
    recorder();
    let edge = edge().await;
    let person = member_token();

    let run = edge.start_run(&person, agent_body(json!({}))).await;
    for n in 1..=6 {
        let step = edge.step(&person, &run, wait_action(), None).await;
        assert_eq!(step.status, 200, "shadow refuses nothing, step {n}: {}", step.body);
    }
    let start = edge.post(&person, "/v1/agent/runs", agent_body(json!({}))).await;
    assert_eq!(constraints_of(&start)["enforcement"], "shadow", "{}", start.body);
    assert_eq!(constraints_of(&start)["max_steps"], 3, "the tuned default is reported: {}", start.body);

    assert!(
        !exhausted_series(&["dimension=\"steps\"", "class=\"agent\"", "mode=\"shadow\""]).is_empty(),
        "the would-be refusal is counted: {}",
        recorder().render()
    );
    assert!(
        exhausted_series(&["mode=\"on\""]).is_empty(),
        "nothing was refused: {}",
        recorder().render()
    );

    // The request cap of a person's session is what it always was.
    let own = edge.start_run(&person, json!({ "constraints": { "max_steps": 2 } })).await;
    for n in 1..=2 {
        let step = edge.step(&person, &own, wait_action(), Some("agent")).await;
        assert_eq!(step.status, 200, "the model's step {n}: {}", step.body);
    }
    let refused = edge.step(&person, &own, wait_action(), Some("agent")).await;
    assert_eq!(refused.status, 429, "{}", refused.body);
    for n in 1..=5 {
        let step = edge.step(&person, &own, wait_action(), None).await;
        assert_eq!(step.status, 200, "the person's step {n}: {}", step.body);
    }
}

/// `off`: no budget at all, which is the documented rollback.
#[tokio::test]
#[ignore = "run by shadow_and_off_change_nothing_a_caller_can_see, in its own process"]
async fn child_off() {
    assert!(in_this_child("child_off"), "run only by its parent test");
    recorder();
    let edge = edge().await;
    let person = member_token();
    let start = edge.post(&person, "/v1/agent/runs", agent_body(json!({}))).await;
    assert_eq!(start.status, 200, "{}", start.body);
    assert!(constraints_of(&start).is_null(), "no budget, nothing to report: {}", start.body);
    let run = start.data()["run_id"].as_str().expect("run id").to_owned();
    for n in 1..=70 {
        let step = edge.step(&person, &run, wait_action(), None).await;
        assert_eq!(step.status, 200, "off, step {n}: {}", step.body);
    }
    assert!(exhausted_series(&[]).is_empty(), "{}", recorder().render());
}

/// `on` with the defaults changed by environment: the tuned numbers are the ones enforced.
#[tokio::test]
#[ignore = "run by the_defaults_are_env_tunable_through_the_real_path, in its own process"]
async fn child_on_tuned() {
    assert!(in_this_child("child_on_tuned"), "run only by its parent test");
    let edge = edge().await;
    let person = member_token();
    let start = edge.post(&person, "/v1/agent/runs", agent_body(json!({}))).await;
    assert_eq!(constraints_of(&start)["max_steps"], 3, "{}", start.body);
    let run = start.data()["run_id"].as_str().expect("run id").to_owned();
    for n in 1..=3 {
        let step = edge.step(&person, &run, wait_action(), None).await;
        assert_eq!(step.status, 200, "step {n}: {}", step.body);
    }
    assert_budget_refusal(&edge.step(&person, &run, wait_action(), None).await, "steps", 3.0);

    let greedy = edge
        .post(&person, "/v1/agent/runs", agent_body(json!({ "max_steps": 50 })))
        .await;
    assert_eq!(constraints_of(&greedy)["max_steps"], 5, "the tuned ceiling clamps: {}", greedy.body);
    assert_eq!(clamp_for(&greedy, "max_steps").expect("clamp")["by"], "ceiling");
}
