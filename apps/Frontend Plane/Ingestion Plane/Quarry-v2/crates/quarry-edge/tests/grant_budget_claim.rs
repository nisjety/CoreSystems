//! The grant-carried budget claim (WS6.E), through the real router and real token
//! verification, with a fake grant validator in place of the broker.
//!
//! A broker grant may carry an upper bound on what a run under it may spend. When
//! `QUARRY_EDGE__GRANT_BUDGET_CLAIM` is `enforce`, `start_run` applies it once:
//!
//! - it only tightens: a run never gets more than its request, the default and the
//!   ceiling give, whatever a grant says;
//! - it is reported: `clamped[]` names `by: "grant"` and a refusal's `limit_source`
//!   is `grant`;
//! - it is snapshotted: per-action grant revalidation, and a resume under a wider
//!   grant, cannot widen the run;
//! - with the switch off (the default) it changes nothing at all.
//!
//! Budget enforcement itself is `on` here (it is process-wide and read once); the
//! claim switch is read on each start, so every test is `#[serial]` and sets it.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Once};

use agent_harness::*;
use async_trait::async_trait;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::QuarryResult;
use quarry_edge::agent_routes::sweep_once;
use quarry_edge::budget::{ClaimMode, ENV_CLAIM_MODE};
use quarry_edge::run_limits::RunLimits;
use quarry_runtime::{GrantValidation, GrantValidator};
use serde_json::{json, Value};
use serial_test::serial;

/// The broker, from memory. A test can change what it answers between calls, which
/// is how a revalidation that disagrees with the start is staged.
#[derive(Clone, Default)]
struct FakeBroker(Arc<Mutex<HashMap<String, GrantValidation>>>);

impl FakeBroker {
    fn set(&self, grant: GrantValidation) {
        self.0.lock().unwrap().insert(grant.grant_id.clone(), grant);
    }
}

#[async_trait]
impl GrantValidator for FakeBroker {
    async fn validate(&self, grant_id: &str) -> QuarryResult<GrantValidation> {
        self.0
            .lock()
            .unwrap()
            .get(grant_id)
            .cloned()
            .ok_or_else(|| QuarryError::new(ErrorCode::NotFound, "unknown browser grant"))
    }
}

/// A usable grant of `kind` for `owner`, bounded to `steps` (0 = no claim).
fn grant(id: &str, kind: &str, owner: &str, claim: Option<Value>) -> GrantValidation {
    let mut fields = json!({
        "grant_id": id,
        "active": true,
        "kind": kind,
        "owner_id": owner,
    });
    // No allowed domains: the driver double has no page, so a step's observation has
    // an empty URL and a domain-restricted run would answer 403 after the step ran.
    if let Some(claim) = claim {
        fields["budget"] = claim;
    }
    serde_json::from_value(fields).expect("a grant validation")
}

fn steps_claim(max_steps: u32) -> Option<Value> {
    Some(json!({ "max_steps": max_steps, "source": "request", "minted_at_unix": 1_760_000_000 }))
}

/// Budget enforcement `on` for this process (read once), and the claim switch.
fn setup(claim_mode: Option<&str>) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| std::env::set_var("QUARRY_EDGE__BUDGET_ENFORCEMENT", "on"));
    match claim_mode {
        Some(value) => std::env::set_var(ENV_CLAIM_MODE, value),
        None => std::env::remove_var(ENV_CLAIM_MODE),
    }
}

async fn edge_with(broker: FakeBroker) -> Edge {
    // Agent runs idle out after 60 s (the sweeper is driven by hand), and no cap on
    // how many runs there are, so only the budget is under test.
    let limits = RunLimits {
        agent_idle_ttl_s: 60,
        interactive_idle_ttl_s: 900,
        max_concurrent_runs: 0,
        max_runs_per_org: 0,
        max_interactive_runs_per_org: 0,
        ..RunLimits::default()
    };
    Edge::start_with(limits, move |state| {
        let mut state = state;
        state.grant_validator = Arc::new(broker);
        state
    })
    .await
}

fn start_body(grant_id: &str, max_steps: u32) -> Value {
    json!({
        "grant_id": grant_id,
        "constraints": { "max_steps": max_steps },
    })
}

fn grant_clamps(reply: &Reply) -> Vec<Value> {
    reply.data()["clamped"]
        .as_array()
        .map(|all| all.iter().filter(|c| c["by"] == "grant").cloned().collect())
        .unwrap_or_default()
}

async fn org_of(edge: &Edge, run: &str) -> String {
    let handle = edge.live(run).expect("the run is live");
    let entry = handle.entry.lock().await;
    entry.org_id.clone()
}

async fn caps_max_steps(edge: &Edge, run: &str) -> u32 {
    let handle = edge.live(run).expect("the run is live");
    let entry = handle.entry.lock().await;
    entry
        .budget
        .as_ref()
        .and_then(|budget| budget.caps.as_ref())
        .map(|caps| caps.max_steps)
        .expect("an agent run has caps")
}

/// Plan name (WS6.E): the claim tightens a request and cannot loosen it.
#[tokio::test]
#[serial]
async fn grant_claim_tightens_but_never_loosens_the_request() {
    setup(Some("enforce"));
    let broker = FakeBroker::default();
    broker.set(grant("g-tight", "run", "svc", steps_claim(3)));
    broker.set(grant("g-wide", "run", "svc", steps_claim(500)));
    let edge = edge_with(broker).await;
    let agent = service_token();

    // A request for 50 steps under a 3-step claim: 3, reported as the grant's.
    let started = edge
        .post(&agent, "/v1/agent/runs", start_body("g-tight", 50))
        .await;
    assert_eq!(started.status, 200, "{}", started.body);
    assert_eq!(started.data()["constraints"]["max_steps"], 3, "{}", started.body);
    assert_eq!(started.data()["constraints"]["class"], "agent");
    let clamps = grant_clamps(&started);
    assert_eq!(clamps.len(), 1, "{}", started.body);
    assert_eq!(clamps[0]["field"], "max_steps");
    assert_eq!(clamps[0]["requested"], 50.0);
    assert_eq!(clamps[0]["effective"], 3.0);
    let run = started.data()["run_id"].as_str().unwrap().to_owned();

    for n in 1..=3 {
        let step = edge.step(&agent, &run, wait_action(), None).await;
        assert_eq!(step.status, 200, "step {n}: {}", step.body);
    }
    let refused = edge.step(&agent, &run, wait_action(), None).await;
    assert_eq!(refused.status, 402, "{}", refused.body);
    assert_eq!(refused.code(), "BUDGET_EXHAUSTED");
    assert_eq!(refused.details()["dimension"], "steps");
    assert_eq!(refused.details()["limit"], 3.0);
    assert_eq!(refused.details()["limit_source"], "grant", "{}", refused.body);

    // A claim above the request does not raise it: the request's 2 stands, and nothing
    // is reported as the grant's.
    let wide = edge
        .post(&agent, "/v1/agent/runs", start_body("g-wide", 2))
        .await;
    assert_eq!(wide.status, 200, "{}", wide.body);
    assert_eq!(wide.data()["constraints"]["max_steps"], 2, "{}", wide.body);
    assert!(grant_clamps(&wide).is_empty(), "{}", wide.body);

    // And a claim above the ceiling does not lift the ceiling (300 steps).
    let ceiling = edge
        .post(&agent, "/v1/agent/runs", start_body("g-wide", 1_000))
        .await;
    assert_eq!(ceiling.data()["constraints"]["max_steps"], 300, "{}", ceiling.body);
    assert!(grant_clamps(&ceiling).is_empty(), "{}", ceiling.body);
}

/// Plan name (WS6.E): a grant with no claim, and a run with no grant, change nothing.
#[tokio::test]
#[serial]
async fn absent_claim_changes_nothing() {
    setup(Some("enforce"));
    let broker = FakeBroker::default();
    broker.set(grant("g-plain", "run", "svc", None));
    broker.set(grant("g-empty", "run", "svc", Some(json!({ "source": "request" }))));
    let edge = edge_with(broker).await;
    let agent = service_token();

    for body in [
        start_body("g-plain", 7),
        start_body("g-empty", 7),
        json!({ "constraints": { "max_steps": 7 } }),
    ] {
        let started = edge.post(&agent, "/v1/agent/runs", body).await;
        assert_eq!(started.status, 200, "{}", started.body);
        assert_eq!(started.data()["constraints"]["max_steps"], 7, "{}", started.body);
        assert!(grant_clamps(&started).is_empty(), "{}", started.body);
    }
}

/// The switch is off by default: the claim is read and carried, and binds nothing,
/// and a run grant does not make a person's run an agent run.
#[tokio::test]
#[serial]
async fn the_claim_is_ignored_unless_enforced() {
    let broker = FakeBroker::default();
    broker.set(grant("g-tight", "run", "svc", steps_claim(3)));
    broker.set(grant("g-user", "run", "user-golden", steps_claim(3)));
    for mode in [None, Some("ignore")] {
        setup(mode);
        assert_eq!(quarry_edge::budget::current_claim_mode(), ClaimMode::Ignore);
        let edge = edge_with(broker.clone()).await;

        let started = edge
            .post(&service_token(), "/v1/agent/runs", start_body("g-tight", 50))
            .await;
        assert_eq!(started.status, 200, "{}", started.body);
        assert_eq!(
            started.data()["constraints"]["max_steps"], 50,
            "mode {mode:?}: {}",
            started.body
        );
        assert!(grant_clamps(&started).is_empty(), "mode {mode:?}: {}", started.body);

        // A person on a run grant stays interactive: never capped.
        let person = edge
            .post(&member_token(), "/v1/agent/runs", start_body("g-user", 50))
            .await;
        assert_eq!(person.status, 200, "{}", person.body);
        assert!(
            person.data()["constraints"].is_null()
                || person.data()["constraints"]["class"] == "interactive",
            "mode {mode:?}: {}",
            person.body
        );
        let run = person.data()["run_id"].as_str().unwrap().to_owned();
        for _ in 0..6 {
            let step = edge.step(&member_token(), &run, wait_action(), None).await;
            assert_eq!(step.status, 200, "{}", step.body);
        }
    }
}

/// R-8, with claims enforced: a grant of kind `run` is minted for an AI run, so a
/// person's token holding one runs under agent bounds. A session grant does not
/// make a person's run an agent run, and the claim on it binds nothing.
#[tokio::test]
#[serial]
async fn a_run_grant_makes_a_users_run_an_agent_run_when_claims_are_enforced() {
    setup(Some("enforce"));
    let broker = FakeBroker::default();
    broker.set(grant("g-run", "run", "user-golden", steps_claim(2)));
    broker.set(grant("g-session", "session", "user-golden", steps_claim(1)));
    let edge = edge_with(broker).await;
    let person = member_token();

    let on_run_grant = edge
        .post(&person, "/v1/agent/runs", start_body("g-run", 50))
        .await;
    assert_eq!(on_run_grant.status, 200, "{}", on_run_grant.body);
    assert_eq!(on_run_grant.data()["constraints"]["class"], "agent", "{}", on_run_grant.body);
    assert_eq!(on_run_grant.data()["constraints"]["max_steps"], 2);
    let run = on_run_grant.data()["run_id"].as_str().unwrap().to_owned();
    for _ in 0..2 {
        assert_eq!(edge.step(&person, &run, wait_action(), None).await.status, 200);
    }
    let refused = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(refused.status, 402, "{}", refused.body);
    assert_eq!(refused.details()["limit_source"], "grant");

    let on_session = edge
        .post(&person, "/v1/agent/runs", start_body("g-session", 50))
        .await;
    assert_eq!(on_session.status, 200, "{}", on_session.body);
    assert!(
        on_session.data()["constraints"].is_null()
            || on_session.data()["constraints"]["class"] == "interactive",
        "{}",
        on_session.body
    );
    let run = on_session.data()["run_id"].as_str().unwrap().to_owned();
    for _ in 0..5 {
        assert_eq!(edge.step(&person, &run, wait_action(), None).await.status, 200);
    }
}

/// Plan name (WS6.E): the claim is read at start and never again. Every step
/// revalidates the grant; a revalidation that answers a wider claim, or none, does
/// not widen the run.
#[tokio::test]
#[serial]
async fn claim_is_not_rewidened_when_the_grant_is_revalidated() {
    setup(Some("enforce"));
    let broker = FakeBroker::default();
    broker.set(grant("g-1", "run", "svc", steps_claim(3)));
    let edge = edge_with(broker.clone()).await;
    let agent = service_token();

    let run = edge.start_run(&agent, start_body("g-1", 50)).await;
    assert_eq!(caps_max_steps(&edge, &run).await, 3);
    assert_eq!(edge.step(&agent, &run, wait_action(), None).await.status, 200);

    // The broker now says 50 steps (or nothing at all) for the same grant.
    broker.set(grant("g-1", "run", "svc", steps_claim(50)));
    assert_eq!(edge.step(&agent, &run, wait_action(), None).await.status, 200);
    broker.set(grant("g-1", "run", "svc", None));
    assert_eq!(edge.step(&agent, &run, wait_action(), None).await.status, 200);
    assert_eq!(caps_max_steps(&edge, &run).await, 3, "a revalidation re-read nothing");

    let refused = edge.step(&agent, &run, wait_action(), None).await;
    assert_eq!(refused.status, 402, "{}", refused.body);
    assert_eq!(refused.details()["limit"], 3.0);
}

/// A resume reads the grant again at start. It keeps the claim the run started under:
/// a wider grant (or none) presented with the resume does not widen it, and what the
/// run already spent is not refilled.
#[tokio::test]
#[serial]
async fn a_resume_under_a_wider_grant_keeps_the_claim_and_the_spend() {
    setup(Some("enforce"));
    let broker = FakeBroker::default();
    broker.set(grant("g-1", "run", "svc", steps_claim(3)));
    let edge = edge_with(broker.clone()).await;
    let agent = service_token();

    let run = edge.start_run(&agent, start_body("g-1", 50)).await;
    let org = org_of(&edge, &run).await;
    assert_eq!(edge.step(&agent, &run, wait_action(), None).await.status, 200);
    let report = sweep_once(&edge.state, later(10 * 3_600_000)).await;
    assert_eq!(report.expired, 1, "the run is released and checkpointed");

    // The checkpoint carries the claim the run was bounded by.
    let saved = edge
        .state
        .receipts
        .load_run_checkpoint(&org, &kind(&run))
        .await
        .expect("a checkpoint read")
        .expect("a checkpoint");
    let stored = saved.budget.expect("the ledger is stored").claim.expect("the claim is stored");
    assert_eq!(stored.max_steps, 3);

    // The broker now hands the same run a much wider claim, and a resume names it.
    broker.set(grant("g-wide", "run", "svc", steps_claim(500)));
    let resumed = edge
        .post(
            &agent,
            "/v1/agent/runs",
            json!({ "resume_run_id": run, "grant_id": "g-wide" }),
        )
        .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    assert_eq!(caps_max_steps(&edge, &run).await, 3, "the resume did not widen the claim");

    // One step was spent before the expiry: two remain, not three.
    for n in 1..=2 {
        let step = edge.step(&agent, &run, wait_action(), None).await;
        assert_eq!(step.status, 200, "resumed step {n}: {}", step.body);
    }
    let refused = edge.step(&agent, &run, wait_action(), None).await;
    assert_eq!(refused.status, 402, "the spend carried over: {}", refused.body);
    assert_eq!(refused.details()["limit_source"], "grant");
}

/// A ZDR run is still tightened by its claim, and nothing about it is persisted:
/// no checkpoint exists to hold the claim, in memory or in the store.
#[tokio::test]
#[serial]
async fn a_zdr_run_is_tightened_by_its_claim_and_persists_nothing() {
    setup(Some("enforce"));
    let broker = FakeBroker::default();
    broker.set(grant("g-1", "run", "svc", steps_claim(2)));
    let edge = edge_with(broker).await;
    let agent = service_token();

    let mut body = start_body("g-1", 50);
    body["zdr"] = json!(true);
    let started = edge.post(&agent, "/v1/agent/runs", body).await;
    assert_eq!(started.status, 200, "{}", started.body);
    assert_eq!(started.data()["constraints"]["max_steps"], 2, "{}", started.body);
    let run = started.data()["run_id"].as_str().unwrap().to_owned();

    let org = org_of(&edge, &run).await;
    let saved = edge
        .state
        .receipts
        .load_run_checkpoint(&org, &kind(&run))
        .await
        .expect("a checkpoint read");
    assert!(saved.is_none(), "a ZDR run writes no checkpoint, so no claim is stored");
    for _ in 0..2 {
        assert_eq!(edge.step(&agent, &run, wait_action(), None).await.status, 200);
    }
    assert_eq!(edge.step(&agent, &run, wait_action(), None).await.status, 402);
}

/// What the claim is, stated so it cannot drift: a cap on one Quarry run, not a
/// budget for the grant. Quarry keeps no spend state per grant (a grant is checked
/// at each start and the claim is snapshotted into that run's own ledger), so a
/// second `start_run` under the same grant gets a full ledger again, and only a
/// resume of a named run continues one. The broker mints a run grant for one model
/// run and the gateway presents it to one dispatch; a caller that starts several
/// Quarry runs under one run grant is bounded per run, by the concurrency and
/// per-organization run caps, and by the grant's own expiry, not by a shared total.
/// If this ever changes to a per-grant total, this test is the one to change on
/// purpose.
#[tokio::test]
#[serial]
async fn the_claim_caps_each_run_not_the_grant() {
    setup(Some("enforce"));
    let broker = FakeBroker::default();
    broker.set(grant("g-1", "run", "svc", steps_claim(2)));
    let edge = edge_with(broker).await;
    let agent = service_token();

    let first = edge.start_run(&agent, start_body("g-1", 50)).await;
    for _ in 0..2 {
        assert_eq!(edge.step(&agent, &first, wait_action(), None).await.status, 200);
    }
    let spent = edge.step(&agent, &first, wait_action(), None).await;
    assert_eq!(spent.status, 402, "the first run spent the whole claim: {}", spent.body);

    // The same grant, a new run: a fresh ledger with the whole claim again.
    let second = edge.start_run(&agent, start_body("g-1", 50)).await;
    assert_ne!(first, second);
    assert_eq!(caps_max_steps(&edge, &second).await, 2);
    for n in 1..=2 {
        let step = edge.step(&agent, &second, wait_action(), None).await;
        assert_eq!(step.status, 200, "second run step {n}: {}", step.body);
    }
    assert_eq!(edge.step(&agent, &second, wait_action(), None).await.status, 402);
}
