//! D2-061 and D2-051 (W2-U25-29): what a run's checkpoint carries across a resume.
//!
//! - `trace_seq`: the seq the run's event trace mints next. A resume (after an idle expiry,
//!   a drain or an edge restart) opens its trace there, not at 1, so a client holding an old
//!   `Last-Event-ID` never sees a seq reused for a different event.
//! - `policy_ask`: the `policy` ask the run was started with. A resume re-applies it, merged
//!   with the resume request's own ask (which can only tighten it), so a restriction the
//!   caller asked for is never dropped by resuming.
//!
//! Also the WS7.D agent-path plan tests that were left unwritten: the persisted ask reaches
//! the browser egress of an unconstrained run (an in-process sub-resource deny, D2-052), and a
//! resume is refused when the run's persisted profile is now disabled. The upload and download
//! refusals and the broker-grant equality case still need a capability-enabled driver double
//! and a grant validator double (see the status file).
//!
//! The browser is the recording driver; the receipt store is the in-memory one, which a second
//! edge can share to model a restart (the second edge's registry and event trace start empty).

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use std::sync::Arc;

use agent_harness::*;
use quarry_core::web_policy::{Principal, RequestAsk};
use quarry_edge::agent_policy_gate::egress_policy;
use quarry_edge::agent_routes::sweep_once;
use quarry_edge::policy_baseline::PolicyBaseline;
use quarry_edge::run_limits::RunLimits;
use quarry_edge::state::AppState;
use quarry_runtime::step_receipts::AgentRunCheckpoint;
use quarry_runtime::InMemoryStepReceiptStore;
use serde_json::{json, Value};

fn idle_limits(idle_s: u64) -> RunLimits {
    RunLimits {
        interactive_idle_ttl_s: idle_s,
        agent_idle_ttl_s: idle_s,
        max_concurrent_runs: 0,
        max_runs_per_org: 0,
        max_interactive_runs_per_org: 0,
        ..RunLimits::default()
    }
}

/// An edge whose receipt store is `store`, so two edges over one store model a restart.
async fn edge_on(store: Arc<InMemoryStepReceiptStore>) -> Edge {
    Edge::start_with(idle_limits(60), move |state| AppState {
        receipts: store,
        ..state
    })
    .await
}

fn click() -> Value {
    json!({ "type": "click", "selector": "#go" })
}

fn type_text() -> Value {
    json!({ "type": "type", "selector": "#name", "text": "ada" })
}

/// The run's org, read while it is live.
async fn org_of(edge: &Edge, run: &str) -> String {
    let handle = edge.live(run).expect("the run is live");
    let entry = handle.entry.lock().await;
    entry.org_id.clone()
}

async fn checkpoint(edge: &Edge, org: &str, run: &str) -> AgentRunCheckpoint {
    edge.state
        .receipts
        .load_run_checkpoint(org, &kind(run))
        .await
        .expect("checkpoint read")
        .expect("a non-ZDR run keeps its checkpoint")
}

/// Every seq the run's open trace still holds, in order.
fn trace_seqs(edge: &Edge, run: &str) -> Vec<u64> {
    edge.state
        .event_sink
        .subscribe_with_replay(&kind(run), 0)
        .map(|subscription| subscription.replay.iter().map(|event| event.seq).collect())
        .unwrap_or_default()
}

fn start_body(policy: Value) -> Value {
    json!({
        "constraints": { "max_steps": 10, "allowed_domains": ["example.test"] },
        "policy": policy,
    })
}

async fn step_ok(edge: &Edge, bearer: &str, run: &str) {
    // The page the browser reports is on the run's allowed host, as in the other agent tests.
    edge.driver.set_page_url(Some("https://example.test/"));
    let reply = edge.step(bearer, run, wait_action(), None).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
}

/// The sweeper ends the idle run (its checkpoint stays `active`).
async fn expire(edge: &Edge) {
    assert_eq!(sweep_once(&edge.state, later(120_000)).await.expired, 1);
}

async fn resume(edge: &Edge, bearer: &str, run: &str, extra: Value) -> Reply {
    let mut body = json!({ "resume_run_id": run });
    if let Some(extra) = extra.as_object() {
        for (key, value) in extra {
            body[key] = value.clone();
        }
    }
    edge.post(bearer, "/v1/agent/runs", body).await
}

async fn assert_denied(edge: &Edge, bearer: &str, run: &str, action: Value, capability: &str) {
    let reply = edge.step(bearer, run, action, None).await;
    assert_eq!(reply.status, 403, "{}", reply.body);
    assert_eq!(reply.details()["reason"], "policy_denied", "{}", reply.body);
    assert_eq!(reply.details()["capability"], capability, "{}", reply.body);
}

// ── D2-061: the trace continues ──────────────────────────────────────────────

#[tokio::test]
async fn a_checkpoint_carries_the_next_trace_seq_and_it_grows_with_the_run() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = service_token();
    let run = edge.start_run(&person, start_body(json!({}))).await;
    let org = org_of(&edge, &run).await;

    let started = checkpoint(&edge, &org, &run).await;
    assert_eq!(
        started.trace_seq, 2,
        "the first save is written before AgentStarted takes seq 1, so the next is 2"
    );

    step_ok(&edge, &person, &run).await;
    step_ok(&edge, &person, &run).await;
    let stepped = checkpoint(&edge, &org, &run).await;
    assert!(
        stepped.trace_seq > started.trace_seq,
        "each step's events advance the stored seq ({} then {})",
        started.trace_seq,
        stepped.trace_seq
    );
    assert_eq!(
        Some(stepped.trace_seq),
        edge.state.event_sink.next_seq(&kind(&run)),
        "the checkpoint holds exactly the seq the open trace mints next"
    );
}

#[tokio::test]
async fn event_seq_continues_across_an_expiry_and_a_resume() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = service_token();
    let run = edge.start_run(&person, start_body(json!({}))).await;
    step_ok(&edge, &person, &run).await;
    let first_segment = trace_seqs(&edge, &run);
    assert!(first_segment.len() >= 2, "{first_segment:?}");
    assert_eq!(first_segment[0], 1);
    let last_seen = *first_segment.last().unwrap();

    expire(&edge).await;
    let resumed = resume(&edge, &person, &run, json!({})).await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);

    let second_segment = trace_seqs(&edge, &run);
    assert_eq!(
        second_segment.len(),
        1,
        "only the resumed segment's AgentStarted: {second_segment:?}"
    );
    assert_eq!(
        second_segment[0],
        last_seen + 2,
        "past the last event of the first segment and the AgentExpired that ended it"
    );
    step_ok(&edge, &person, &run).await;
    let after = trace_seqs(&edge, &run);
    assert!(
        after.windows(2).all(|pair| pair[1] == pair[0] + 1),
        "the resumed segment is dense: {after:?}"
    );
    assert!(
        after[0] > last_seen,
        "no seq of the first segment is reused: {first_segment:?} then {after:?}"
    );
}

#[tokio::test]
async fn event_seq_continues_after_an_edge_restart() {
    let store = Arc::new(InMemoryStepReceiptStore::default());
    let first = edge_on(store.clone()).await;
    let person = service_token();
    let run = first.start_run(&person, start_body(json!({}))).await;
    step_ok(&first, &person, &run).await;
    step_ok(&first, &person, &run).await;
    let org = org_of(&first, &run).await;
    let saved = checkpoint(&first, &org, &run).await.trace_seq;
    let used = *trace_seqs(&first, &run).last().unwrap();
    assert!(saved > used, "the checkpoint is past every seq used ({saved} vs {used})");

    // A new edge over the same store: no registry, no trace.
    let second = edge_on(store).await;
    assert!(trace_seqs(&second, &run).is_empty());
    let resumed = resume(&second, &person, &run, json!({})).await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    assert_eq!(
        trace_seqs(&second, &run),
        vec![saved],
        "the restarted edge's trace opens at the stored seq, not at 1"
    );
}

#[tokio::test]
async fn a_resume_does_not_lower_the_stored_seq_when_it_is_saved_again() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = service_token();
    let run = edge.start_run(&person, start_body(json!({}))).await;
    step_ok(&edge, &person, &run).await;
    let org = org_of(&edge, &run).await;
    expire(&edge).await;
    let at_expiry = checkpoint(&edge, &org, &run).await.trace_seq;

    let resumed = resume(&edge, &person, &run, json!({})).await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    let at_resume = checkpoint(&edge, &org, &run).await.trace_seq;
    assert_eq!(
        at_resume,
        at_expiry + 1,
        "the resume's own AgentStarted took the stored seq, so the next is one past it"
    );
    assert_eq!(
        Some(at_resume),
        edge.state.event_sink.next_seq(&kind(&run))
    );
}

#[tokio::test]
async fn a_checkpoint_from_before_trace_seq_resumes_at_seq_one() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = service_token();
    let run = edge.start_run(&person, start_body(json!({}))).await;
    step_ok(&edge, &person, &run).await;
    let org = org_of(&edge, &run).await;
    expire(&edge).await;

    // What the older edge left: the same checkpoint without the two fields.
    let mut legacy = serde_json::to_value(checkpoint(&edge, &org, &run).await).unwrap();
    let object = legacy.as_object_mut().unwrap();
    object.remove("trace_seq");
    object.remove("policy_ask");
    let legacy: AgentRunCheckpoint = serde_json::from_value(legacy).expect("a legacy checkpoint");
    assert_eq!((legacy.trace_seq, legacy.policy_ask.clone()), (0, None));
    edge.state
        .receipts
        .save_run_checkpoint(legacy)
        .await
        .expect("checkpoint write");

    let resumed = resume(&edge, &person, &run, json!({})).await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    assert_eq!(trace_seqs(&edge, &run), vec![1], "as it always did");
}

// ── D2-051: the ask survives a resume ────────────────────────────────────────

#[tokio::test]
async fn a_checkpoint_stores_the_ask_the_run_started_with() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = service_token();
    let ask = json!({ "disable": ["browser.act.interact"], "deny_domains": ["ads.example"] });
    let run = edge.start_run(&person, start_body(ask)).await;
    let org = org_of(&edge, &run).await;

    let stored = checkpoint(&edge, &org, &run).await.policy_ask.expect("the ask is stored");
    assert_eq!(stored.disable, vec!["browser.act.interact".to_string()]);
    assert_eq!(stored.deny_domains, vec!["ads.example".to_string()]);

    // A run that asked for nothing stores nothing, and the field is not written at all.
    let plain = edge.start_run(&person, start_body(json!({}))).await;
    let plain_checkpoint = checkpoint(&edge, &org, &plain).await;
    // `{}` is an ask that restricts nothing; it round-trips as an ask with no entries.
    assert_eq!(plain_checkpoint.policy_ask, Some(RequestAsk::default()));
    let none = edge
        .start_run(
            &person,
            json!({ "constraints": { "max_steps": 10, "allowed_domains": ["example.test"] } }),
        )
        .await;
    let none_checkpoint = checkpoint(&edge, &org, &none).await;
    assert_eq!(none_checkpoint.policy_ask, None);
    assert!(
        serde_json::to_value(&none_checkpoint).unwrap().get("policy_ask").is_none(),
        "no ask, no field"
    );
}

#[tokio::test]
async fn a_resume_with_no_ask_keeps_the_restriction_of_the_start() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = service_token();
    let run = edge
        .start_run(&person, start_body(json!({ "disable": ["browser.act.interact"] })))
        .await;
    assert_denied(&edge, &person, &run, click(), "browser.act.interact").await;
    step_ok(&edge, &person, &run).await;
    expire(&edge).await;

    // The resume request names no policy at all.
    let resumed = resume(&edge, &person, &run, json!({})).await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    assert_denied(&edge, &person, &run, click(), "browser.act.interact").await;
    // The run is not poisoned: an action the policy allows still runs.
    step_ok(&edge, &person, &run).await;

    // And the restriction is still in the checkpoint for the resume after this one.
    let org = org_of(&edge, &run).await;
    expire(&edge).await;
    let stored = checkpoint(&edge, &org, &run).await.policy_ask.expect("still stored");
    assert_eq!(stored.disable, vec!["browser.act.interact".to_string()]);
    let again = resume(&edge, &person, &run, json!({})).await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_denied(&edge, &person, &run, click(), "browser.act.interact").await;
}

#[tokio::test]
async fn a_resume_ask_adds_to_the_start_ask_and_never_replaces_it() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = service_token();
    let run = edge
        .start_run(&person, start_body(json!({ "disable": ["browser.act.interact"] })))
        .await;
    step_ok(&edge, &person, &run).await;
    let org = org_of(&edge, &run).await;
    expire(&edge).await;

    // The resume asks to disable typing only; clicking must stay disabled too.
    let resumed = resume(
        &edge,
        &person,
        &run,
        json!({ "policy": { "disable": ["browser.act.input"] } }),
    )
    .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    assert_denied(&edge, &person, &run, click(), "browser.act.interact").await;
    assert_denied(&edge, &person, &run, type_text(), "browser.act.input").await;

    let stored = checkpoint(&edge, &org, &run).await.policy_ask.expect("the merged ask");
    assert_eq!(
        stored.disable,
        vec!["browser.act.interact".to_string(), "browser.act.input".to_string()],
        "the stored ask is the start's with the resume's added"
    );
}

#[tokio::test]
async fn a_restriction_survives_an_edge_restart() {
    let store = Arc::new(InMemoryStepReceiptStore::default());
    let first = edge_on(store.clone()).await;
    let person = service_token();
    let run = first
        .start_run(&person, start_body(json!({ "disable": ["browser.act.interact"] })))
        .await;
    step_ok(&first, &person, &run).await;

    let second = edge_on(store).await;
    let resumed = resume(&second, &person, &run, json!({})).await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    assert_denied(&second, &person, &run, click(), "browser.act.interact").await;
    assert!(
        second.receipts(&person, &run).await.len() == 1,
        "the refusal left no receipt: only the one wait before the restart"
    );
}

#[tokio::test]
async fn a_resume_is_refused_when_the_persisted_profile_is_now_disabled() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = service_token();
    let run = edge
        .start_run(&person, start_body(json!({})))
        .await;
    step_ok(&edge, &person, &run).await;
    let org = org_of(&edge, &run).await;
    expire(&edge).await;

    // The run's checkpoint says its profile persists (as a persistent-profile run leaves it).
    let mut persisted = checkpoint(&edge, &org, &run).await;
    persisted.persist_profile = true;
    edge.state
        .receipts
        .save_run_checkpoint(persisted)
        .await
        .expect("checkpoint write");

    let reply = resume(
        &edge,
        &person,
        &run,
        json!({ "policy": { "disable": ["browser.profile.persistent"] } }),
    )
    .await;
    assert_eq!(reply.status, 403, "{}", reply.body);
    assert_eq!(reply.details()["reason"], "policy_denied");
    assert_eq!(reply.details()["capability"], "browser.profile.persistent");
    assert!(edge.live(&run).is_none(), "a refused resume leaves no live run behind");
}

// ── D2-052: the persisted ask reaches the browser egress ────────────────────

#[tokio::test]
async fn the_persisted_deny_list_refuses_a_subresource_on_a_run_with_no_allow_list() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = service_token();
    // An unconstrained run (no allowed_domains) that asked to deny an ad host.
    let run = edge
        .start_run(
            &person,
            json!({
                "constraints": { "max_steps": 10 },
                "policy": { "deny_domains": ["ads.example"] },
            }),
        )
        .await;
    let org = org_of(&edge, &run).await;
    let stored = checkpoint(&edge, &org, &run).await;
    let ask = stored.policy_ask.clone().expect("the ask is stored");

    // What a resume resolves from the stored ask, and the boundary it builds from it: the
    // same functions `start_run` runs, so a sub-resource, frame or redirect is refused where
    // it happens.
    let decision = PolicyBaseline::default()
        .decide(Principal::Autonomous, false, Some(ask.into_layer().expect("a valid ask")))
        .expect("the policy resolves");
    let egress = egress_policy(&decision, &stored.constraints.allowed_domains);
    assert!(egress.allows_url("https://news.example.org/"));
    assert!(!egress.allows_url("https://ads.example/pixel.png"));
    assert!(!egress.allows_url("https://cdn.ads.example/pixel.png"));
    assert!(egress.allows_url("https://notads.example/"));
}
