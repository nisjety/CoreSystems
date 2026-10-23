//! Control authority over a live agent run, through the real router and real
//! token verification (the harness is in `support/agent_harness.rs`).
//!
//! The rules under test: a person's take-over binds the agent, and Quarry
//! enforces it when `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` is on (observe, the
//! default, only logs it); the control epoch moves on real changes and never on
//! a repeat; a refused agent action leaves one `skipped` receipt per epoch and
//! consumes nothing; receipts carry the initiator and the epoch they ran under.
//!
//! Who may drive a run follows from who owns it (W1: the actor that started it).
//! A person's run is driven by the person's token. The agent's actions arrive on
//! that token labelled `initiated_by: "agent"` (a step, a tab open, a socket action
//! labelled `actor: "agent"`), and a labelled action meets the take-over gate that any
//! agent action meets.
//! Tab select and close take the label in the `initiatedBy` query (they have no body;
//! a tab open takes it in its body), read the way a step's is, so an agent's select
//! or close during a take-over is refused too. A service principal's run is owned by
//! the service, and a service is always an agent, so no HTTP route can take a
//! service-owned run over. The one test that needs a take-over of such a run sets it
//! on the live handle, the way the control route does, and says so.
//!
//! Every test is `#[serial]`: the switch is a process-wide environment variable.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use agent_harness::*;
use async_trait::async_trait;
use quarry_core::QuarryResult;
use quarry_edge::agent_routes::RunHandle;
use quarry_edge::budget::RunClass;
use quarry_edge::run_limits::{
    parse_takeover_enforcement, takeover_enforced, RunLimits, ENV_TAKEOVER_ENFORCEMENT,
};
use quarry_runtime::step_receipts::BrowserControlMode;
use quarry_runtime::{GrantValidation, GrantValidator, StepReceiptStore};
use serde_json::{json, Value};
use serial_test::serial;

/// The receipts whose outcome is `skipped`.
fn skipped(receipts: &[Value]) -> Vec<&Value> {
    receipts
        .iter()
        .filter(|receipt| receipt["outcome"]["status"] == "skipped")
        .collect()
}

/// The receipts whose outcome is `completed`.
fn completed(receipts: &[Value]) -> Vec<&Value> {
    receipts
        .iter()
        .filter(|receipt| receipt["outcome"]["status"] == "completed")
        .collect()
}

/// The `controlEpoch` of every control item on a timeline, sorted.
fn control_epochs(items: &[Value]) -> Vec<u64> {
    let mut epochs: Vec<u64> = items
        .iter()
        .filter(|item| item["kind"] == "control")
        .map(|item| {
            item["controlEpoch"]
                .as_u64()
                .expect("control items carry an epoch")
        })
        .collect();
    epochs.sort_unstable();
    epochs
}

/// Ask the control route for `mode` and return the epoch it answers with.
async fn flip(edge: &Edge, person: &str, run_id: &str, mode: &str) -> u64 {
    let reply = edge.control(person, run_id, json!({ "mode": mode })).await;
    assert_eq!(reply.status, 200, "{mode}: {}", reply.body);
    reply.data()["controlEpoch"]
        .as_u64()
        .expect("the answer carries an epoch")
}

/// Nothing is written for `run_id` under the given tenant and actor: no receipt,
/// no timeline item and no checkpoint.
async fn assert_nothing_persisted(
    store: &dyn StepReceiptStore,
    org: &str,
    actor: &str,
    run_id: &str,
    when: &str,
) {
    let run = kind(run_id);
    assert!(
        store
            .list_for_actor(org, actor, &run)
            .await
            .expect("list")
            .is_empty(),
        "{when}: no receipt is written"
    );
    assert!(
        store
            .list_browser_timeline_events(org, actor, &run)
            .await
            .expect("timeline")
            .is_empty(),
        "{when}: no timeline item is written"
    );
    assert!(
        store
            .load_run_checkpoint(org, &run)
            .await
            .expect("checkpoint")
            .is_none(),
        "{when}: no checkpoint is written"
    );
}

/// Set a take-over on a run the way the control route does: flip the lock-free
/// authority, then record it on the entry. Returns the new epoch.
async fn take_over_live(edge: &Edge, run_id: &str) -> u64 {
    let handle: RunHandle = edge.live(run_id).expect("the run is live");
    let (changed, epoch) = handle
        .live
        .set_mode(BrowserControlMode::HumanTakeover, None)
        .expect("no expected epoch to compare");
    assert!(changed, "the run was not already taken over");
    let mut entry = handle.entry.lock().await;
    entry.control_mode = BrowserControlMode::HumanTakeover;
    entry.control_epoch = epoch;
    epoch
}

// ── enforcement off: the default observes ───────────────────────────────────

/// With the switch unset Quarry observes: the person's take-over is recorded, an
/// agent action during it runs, and the action is stamped with the epoch it ran in.
#[tokio::test]
#[serial]
async fn with_the_switch_unset_an_agent_action_during_a_takeover_runs_and_is_recorded() {
    set_takeover_enforcement(None);
    assert!(!takeover_enforced(), "the default is observe");

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;

    let takeover = edge
        .control(&person, &run, json!({ "mode": "human_takeover" }))
        .await;
    assert_eq!(takeover.status, 200, "{}", takeover.body);
    assert_eq!(takeover.data()["controlEpoch"], 1);
    assert_eq!(takeover.data()["controlMode"], "human_takeover");

    let agent = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(
        agent.status, 200,
        "observe lets the action through: {}",
        agent.body
    );

    let receipts = edge.receipts(&person, &run).await;
    assert_eq!(receipts.len(), 1, "{receipts:?}");
    assert_eq!(receipts[0]["initiated_by"], "agent");
    assert_eq!(receipts[0]["control_epoch"], 1);
    assert_eq!(receipts[0]["outcome"]["status"], "completed");
    assert!(
        skipped(&receipts).is_empty(),
        "observe writes no skipped receipt: {receipts:?}"
    );
}

/// The take-over switch reads what it is set to, and a value it cannot read is
/// off at request time (the edge refuses to boot on it, which `main` checks).
#[test]
#[serial]
fn the_takeover_switch_is_off_unless_asked_and_a_typo_is_named() {
    for (value, enforced) in [
        (None, false),
        (Some(""), false),
        (Some("0"), false),
        (Some("off"), false),
        (Some("1"), true),
        (Some(" On "), true),
        (Some("TRUE"), true),
    ] {
        set_takeover_enforcement(value);
        assert_eq!(takeover_enforced(), enforced, "value {value:?}");
    }

    let error = parse_takeover_enforcement(Some("enforce")).expect_err("a typo is an error");
    assert!(error.contains(ENV_TAKEOVER_ENFORCEMENT), "{error}");

    set_takeover_enforcement(Some("enforce"));
    assert!(
        !takeover_enforced(),
        "an unreadable value is off until boot refuses it"
    );
    set_takeover_enforcement(None);
}

// ── enforcement on: the agent is refused ────────────────────────────────────

/// With enforcement on, every agent step during a take-over is refused with
/// 409 USER_TOOK_OVER naming the epoch. The refusals consume no step and leave one
/// skipped receipt for the epoch, while the person's own action still goes through.
#[tokio::test]
#[serial]
async fn with_enforcement_on_an_agent_action_during_a_takeover_is_refused_and_leaves_one_skipped_receipt(
) {
    set_takeover_enforcement(Some("1"));
    assert!(takeover_enforced());

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let takeover = edge
        .control(&person, &run, json!({ "mode": "human_takeover" }))
        .await;
    assert_eq!(takeover.status, 200, "{}", takeover.body);

    for attempt in 0..3 {
        let refused = edge.step(&person, &run, wait_action(), Some("agent")).await;
        assert_eq!(refused.status, 409, "attempt {attempt}: {}", refused.body);
        assert_eq!(refused.code(), "USER_TOOK_OVER", "{}", refused.body);
        assert_eq!(refused.details()["control_mode"], "human_takeover");
        assert_eq!(refused.details()["control_epoch"], 1);
        assert_eq!(refused.body["error"]["retryable"], false);
    }

    let human = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(
        human.status, 200,
        "a person's action is admitted: {}",
        human.body
    );

    let receipts = edge.receipts(&person, &run).await;
    let skips = skipped(&receipts);
    assert_eq!(
        skips.len(),
        1,
        "one skipped receipt however many refusals in the epoch: {receipts:?}"
    );
    assert_eq!(skips[0]["initiated_by"], "agent");
    assert_eq!(skips[0]["control_epoch"], 1);
    assert_eq!(skips[0]["outcome"]["reason"], "user_took_over");

    let done = completed(&receipts);
    assert_eq!(done.len(), 1, "{receipts:?}");
    assert_eq!(done[0]["initiated_by"], "human");
    assert_eq!(done[0]["control_epoch"], 1);

    // The refused actions consumed no step: the person's action is the only one.
    let projection = edge.projection(&person, &run).await;
    assert_eq!(projection.data()["step"], 1, "{}", projection.body);
}

/// The epoch moves once per real change and never on a repeat; a refused action
/// is skipped once per take-over, so a second take-over earns a second skip.
#[tokio::test]
#[serial]
async fn the_epoch_counts_real_changes_and_a_refused_action_is_skipped_once_per_takeover() {
    set_takeover_enforcement(Some("1"));

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let mut epochs = Vec::new();
    let run_id = run.as_str();

    epochs.push(flip(&edge, &person, run_id, "human_takeover").await); // 0 -> 1
    epochs.push(flip(&edge, &person, run_id, "human_takeover").await); // repeat: no change
    for _ in 0..2 {
        let refused = edge.step(&person, &run, wait_action(), Some("agent")).await;
        assert_eq!(refused.code(), "USER_TOOK_OVER", "{}", refused.body);
    }
    epochs.push(flip(&edge, &person, run_id, "agent_control").await); // 1 -> 2
    epochs.push(flip(&edge, &person, run_id, "agent_control").await); // repeat: no change

    let ran = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(ran.status, 200, "{}", ran.body);

    epochs.push(flip(&edge, &person, run_id, "human_takeover").await); // 2 -> 3
    let refused = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(refused.code(), "USER_TOOK_OVER", "{}", refused.body);
    epochs.push(flip(&edge, &person, run_id, "agent_control").await); // 3 -> 4

    assert_eq!(epochs, vec![1, 1, 2, 2, 3, 4]);

    let receipts = edge.receipts(&person, &run).await;
    let skip_epochs: Vec<u64> = skipped(&receipts)
        .iter()
        .map(|receipt| receipt["control_epoch"].as_u64().expect("an epoch"))
        .collect();
    assert_eq!(skip_epochs, vec![1, 3], "{receipts:?}");

    let done = completed(&receipts);
    assert_eq!(done.len(), 1, "{receipts:?}");
    assert_eq!(done[0]["initiated_by"], "agent");
    assert_eq!(done[0]["control_epoch"], 2);

    let timeline = edge.timeline(&person, &run).await;
    assert_eq!(
        control_epochs(&timeline),
        vec![1, 2, 3, 4],
        "one control item per real change, none for a repeat: {timeline:?}"
    );
}

/// A take-over sent with a stale `expected_epoch` is refused with the current
/// epoch and changes nothing; the same request with the current epoch succeeds.
#[tokio::test]
#[serial]
async fn a_stale_expected_epoch_is_refused_with_the_current_epoch_and_changes_nothing() {
    set_takeover_enforcement(None);

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;

    let first = edge
        .control(
            &person,
            &run,
            json!({ "mode": "human_takeover", "expected_epoch": 0 }),
        )
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.data()["controlEpoch"], 1);

    let stale = edge
        .control(
            &person,
            &run,
            json!({ "mode": "agent_control", "expected_epoch": 0 }),
        )
        .await;
    assert_eq!(stale.status, 409, "{}", stale.body);
    assert_eq!(stale.code(), "CONFLICT");
    assert_eq!(stale.details()["control_epoch"], 1);
    assert_eq!(stale.details()["expected_epoch"], 0);

    let projection = edge.projection(&person, &run).await;
    assert_eq!(
        projection.data()["controlMode"],
        "human_takeover",
        "{}",
        projection.body
    );
    assert_eq!(projection.data()["controlEpoch"], 1);

    let current = edge
        .control(
            &person,
            &run,
            json!({ "mode": "agent_control", "expected_epoch": 1 }),
        )
        .await;
    assert_eq!(current.status, 200, "{}", current.body);
    assert_eq!(current.data()["controlEpoch"], 2);
}

// ── service principals ──────────────────────────────────────────────────────

/// A service principal is an agent whatever it sends, so a hint of `human` does
/// not make its action a person's. It cannot change the control mode (403), and a
/// person cannot drive a run a service owns (404, the owner check).
#[tokio::test]
#[serial]
async fn a_service_principal_is_always_an_agent_and_cannot_change_the_control_mode() {
    set_takeover_enforcement(None);

    let edge = Edge::start(RunLimits::default()).await;
    let service = service_token();
    let person = member_token();
    let run = edge.start_run(&service, json!({})).await;

    let step = edge
        .step(&service, &run, wait_action(), Some("human"))
        .await;
    assert_eq!(step.status, 200, "{}", step.body);
    let receipts = edge.receipts(&service, &run).await;
    assert_eq!(receipts[0]["initiated_by"], "agent", "{receipts:?}");
    assert_eq!(receipts[0]["control_epoch"], 0);

    let refused_flip = edge
        .control(&service, &run, json!({ "mode": "human_takeover" }))
        .await;
    assert_eq!(refused_flip.status, 403, "{}", refused_flip.body);
    assert_eq!(refused_flip.message(), "control_transition_requires_human");

    let projection = edge.projection(&service, &run).await;
    assert_eq!(projection.data()["controlEpoch"], 0);
    assert_eq!(projection.data()["controlMode"], "agent_control");

    let person_flip = edge
        .control(&person, &run, json!({ "mode": "human_takeover" }))
        .await;
    assert_eq!(person_flip.status, 404, "{}", person_flip.body);
    assert_eq!(person_flip.message(), "agent run not found");
}

/// A service's action during a take-over is refused with enforcement on, and the
/// refusal is a skipped receipt initiated by an agent. The take-over is set on the
/// live handle because no HTTP route can take over a service-owned run (see the
/// module docs).
#[tokio::test]
#[serial]
async fn with_enforcement_on_a_service_action_during_a_takeover_is_refused_as_an_agent() {
    set_takeover_enforcement(Some("1"));

    let edge = Edge::start(RunLimits::default()).await;
    let service = service_token();
    let run = edge.start_run(&service, json!({})).await;
    let epoch = take_over_live(&edge, &run).await;
    assert_eq!(epoch, 1);

    let refused = edge.step(&service, &run, wait_action(), None).await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(refused.code(), "USER_TOOK_OVER");
    assert_eq!(refused.details()["control_epoch"], 1);

    let receipts = edge.receipts(&service, &run).await;
    let skips = skipped(&receipts);
    assert_eq!(skips.len(), 1, "{receipts:?}");
    assert_eq!(skips[0]["initiated_by"], "agent");
    assert_eq!(skips[0]["control_epoch"], 1);
}

/// With the switch unset the same service action during the take-over runs and is
/// recorded as an agent action in epoch 1.
#[tokio::test]
#[serial]
async fn with_the_switch_unset_a_service_action_during_a_takeover_still_runs() {
    set_takeover_enforcement(None);

    let edge = Edge::start(RunLimits::default()).await;
    let service = service_token();
    let run = edge.start_run(&service, json!({})).await;
    take_over_live(&edge, &run).await;

    let ran = edge.step(&service, &run, wait_action(), None).await;
    assert_eq!(ran.status, 200, "{}", ran.body);

    let receipts = edge.receipts(&service, &run).await;
    assert_eq!(completed(&receipts).len(), 1, "{receipts:?}");
    assert_eq!(completed(&receipts)[0]["initiated_by"], "agent");
    assert_eq!(completed(&receipts)[0]["control_epoch"], 1);
    assert!(skipped(&receipts).is_empty(), "{receipts:?}");
}

// ── zero data retention ─────────────────────────────────────────────────────

/// A ZDR run is gated by the take-over exactly like any other run, and persists
/// nothing: no receipt, no skipped receipt, no timeline item and no checkpoint, in
/// either mode.
#[tokio::test]
#[serial]
async fn a_zdr_run_is_gated_by_the_takeover_but_persists_nothing() {
    set_takeover_enforcement(Some("1"));

    let edge = Edge::start(RunLimits::default()).await;
    let person = zdr_member_token();
    let run = edge.start_run(&person, json!({})).await;
    let takeover = edge
        .control(&person, &run, json!({ "mode": "human_takeover" }))
        .await;
    assert_eq!(takeover.status, 200, "{}", takeover.body);
    assert_eq!(takeover.data()["zdr"], true);
    assert_eq!(takeover.data()["controlEpoch"], 1);

    let refused = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(refused.code(), "USER_TOOK_OVER");

    // The owner of the ZDR org's run (`org-zdr-golden`, `user-golden`).
    let (org, actor) = ("org-zdr-golden", "user-golden");
    let store = edge.state.receipts.clone();
    assert_nothing_persisted(store.as_ref(), org, actor, &run, "after a refusal").await;

    set_takeover_enforcement(None);
    let ran = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(
        ran.status, 200,
        "observe lets the action through: {}",
        ran.body
    );
    assert_nothing_persisted(store.as_ref(), org, actor, &run, "after an observed action").await;

    assert!(edge.receipts(&person, &run).await.is_empty());
    // The timeline is not served for a ZDR run at all: the read is refused (409,
    // with the reason in the message), so there is nothing to read back.
    let timeline = edge
        .get(
            &person,
            &format!("/v1/agent/runs/{run}/browser-session/timeline"),
        )
        .await;
    assert_eq!(timeline.status, 409, "{}", timeline.body);
    assert!(
        timeline.message().contains("Zero Data Retention"),
        "{}",
        timeline.body
    );
}

/// A run belongs to the actor that started it (W1). A service principal cannot step
/// or open a tab on a person's run, even with a hint of `agent`, so the agent's path
/// into a person's browser is the person's own token labelled `initiated_by: "agent"`
/// (the gateway's route). The refusal is the plain 404: a service learns nothing about
/// the run, and no receipt or skip is written for it.
#[tokio::test]
#[serial]
async fn a_service_principal_cannot_drive_a_persons_run() {
    set_takeover_enforcement(Some("1"));

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let takeover = edge
        .control(&person, &run, json!({ "mode": "human_takeover" }))
        .await;
    assert_eq!(takeover.status, 200, "{}", takeover.body);

    let service = service_token();
    let step = edge
        .step(&service, &run, wait_action(), Some("agent"))
        .await;
    assert_eq!(step.status, 404, "{}", step.body);
    assert_eq!(step.message(), "agent run not found");
    assert!(
        step.details().is_null(),
        "no resume hint is given to another actor: {}",
        step.body
    );

    let tab = edge.new_tab(&service, &run, Some("agent")).await;
    assert_eq!(tab.status, 404, "{}", tab.body);
    assert_eq!(
        edge.driver.tabs_opened(),
        0,
        "a refused tab never reaches the driver"
    );

    let receipts = edge.receipts(&person, &run).await;
    assert!(skipped(&receipts).is_empty(), "{receipts:?}");
    assert!(completed(&receipts).is_empty(), "{receipts:?}");
}

// ── tabs ────────────────────────────────────────────────────────────────────

/// Opening a tab is an effectful operation on the person's browser, so an agent
/// cannot open one during a take-over: refused with enforcement, and reaches the
/// driver without it.
#[tokio::test]
#[serial]
async fn a_tab_opened_by_an_agent_during_a_takeover_is_refused_with_enforcement_and_recorded_without_it(
) {
    set_takeover_enforcement(Some("1"));

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let takeover = edge
        .control(&person, &run, json!({ "mode": "human_takeover" }))
        .await;
    assert_eq!(takeover.status, 200, "{}", takeover.body);

    let refused = edge.new_tab(&person, &run, Some("agent")).await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(refused.code(), "USER_TOOK_OVER");
    assert_eq!(refused.details()["control_epoch"], 1);
    assert_eq!(
        edge.driver.tabs_opened(),
        0,
        "a refused tab never reaches the driver"
    );

    set_takeover_enforcement(None);
    let opened = edge.new_tab(&person, &run, Some("agent")).await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    assert_eq!(edge.driver.tabs_opened(), 1);

    let timeline = edge.timeline(&person, &run).await;
    let tabs: Vec<&Value> = timeline
        .iter()
        .filter(|item| item["kind"] == "tab")
        .collect();
    assert_eq!(tabs.len(), 1, "{timeline:?}");
    assert_eq!(tabs[0]["initiatedBy"], "agent");
    assert_eq!(tabs[0]["controlEpoch"], 1);
    assert_eq!(tabs[0]["operation"], "opened");
}

/// A person may select and close a tab during a take-over: the person holds the browser,
/// so the gate admits them with enforcement on. Both reach the driver, and the timeline
/// records each as the person's, under the epoch the take-over set.
#[tokio::test]
#[serial]
async fn a_persons_tab_select_and_close_during_a_takeover_are_admitted() {
    set_takeover_enforcement(Some("1"));

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let opened = edge.new_tab(&person, &run, None).await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    let tab_id = opened.data()["tab"]["tabId"]
        .as_str()
        .expect("the new tab has an id")
        .to_owned();
    let takeover = edge
        .control(&person, &run, json!({ "mode": "human_takeover" }))
        .await;
    assert_eq!(takeover.status, 200, "{}", takeover.body);

    let select = edge
        .post(
            &person,
            &format!("/v1/agent/runs/{run}/tabs/{tab_id}/select"),
            json!({}),
        )
        .await;
    assert_eq!(
        select.status, 200,
        "a person's select is admitted: {}",
        select.body
    );
    assert_eq!(edge.driver.selected_tabs(), vec![tab_id.clone()]);

    let close = edge
        .delete(&person, &format!("/v1/agent/runs/{run}/tabs/{tab_id}"))
        .await;
    assert_eq!(
        close.status, 200,
        "a person's close is admitted: {}",
        close.body
    );
    assert_eq!(edge.driver.closed_tabs(), vec![tab_id]);

    let timeline = edge.timeline(&person, &run).await;
    let tabs: Vec<&Value> = timeline
        .iter()
        .filter(|item| item["kind"] == "tab")
        .collect();
    let operations: Vec<&str> = tabs
        .iter()
        .map(|item| item["operation"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(
        operations,
        vec!["opened", "selected", "closed"],
        "{timeline:?}"
    );
    for item in &tabs[1..] {
        assert_eq!(item["initiatedBy"], "human", "{item}");
        assert_eq!(item["controlEpoch"], 1, "{item}");
    }
}

// ── the browser socket ──────────────────────────────────────────────────────

/// An action on a person's browser socket goes through the same chain as a REST step.
/// During a takeover an action the socket labels `actor: "agent"` is refused with the
/// control error and runs nothing, and leaves one skipped receipt. The person's own
/// action on the same socket runs and is recorded as theirs.
#[tokio::test]
#[serial]
async fn a_socket_action_labelled_agent_during_a_takeover_is_refused() {
    set_takeover_enforcement(Some("1"));

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let takeover = edge
        .control(&person, &run, json!({ "mode": "human_takeover" }))
        .await;
    assert_eq!(takeover.status, 200, "{}", takeover.body);

    let mut socket =
        TestSocket::connect(&edge, &person, &format!("/v1/agent/runs/{run}/frames/ws")).await;

    socket
        .send_text(
            &json!({ "type": "action", "action": wait_action(), "actor": "agent" }).to_string(),
        )
        .await;
    let refused = socket
        .next_matching(|message| message["type"] == "error")
        .await;
    // The socket's error carries the error's Display, which begins with its code.
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|text| text.starts_with("UserTookOver:")),
        "{refused}"
    );

    socket
        .send_text(&json!({ "type": "action", "action": wait_action() }).to_string())
        .await;
    let ran = socket
        .next_matching(|message| message["type"] == "observation" || message["type"] == "error")
        .await;
    assert_eq!(ran["type"], "observation", "{ran}");
    drop(socket);

    let receipts = edge.receipts(&person, &run).await;
    let skips = skipped(&receipts);
    assert_eq!(skips.len(), 1, "{receipts:?}");
    assert_eq!(skips[0]["initiated_by"], "agent");
    assert_eq!(skips[0]["control_epoch"], 1);
    let done = completed(&receipts);
    assert_eq!(done.len(), 1, "{receipts:?}");
    assert_eq!(done[0]["initiated_by"], "human");
    assert_eq!(done[0]["control_epoch"], 1);
}

// ── contract: the wire names of a control item, and the tab label ───────────

/// Timeline items carry `initiatedBy` and `controlEpoch` (the names the SPA's
/// `BrowserOwnerTimelineItem` reads). Every kind of item says so, the control item
/// too: it once serialised `initiated_by` (finding F5-1).
#[tokio::test]
#[serial]
async fn control_timeline_items_carry_initiated_by_under_the_documented_name() {
    set_takeover_enforcement(None);

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let takeover = edge
        .control(&person, &run, json!({ "mode": "human_takeover" }))
        .await;
    assert_eq!(takeover.status, 200, "{}", takeover.body);

    let timeline = edge.timeline(&person, &run).await;
    let control = timeline
        .iter()
        .find(|item| item["kind"] == "control")
        .expect("the take-over is on the timeline");
    assert_eq!(control["initiatedBy"], "human", "{control}");
    assert_eq!(control["controlEpoch"], 1, "{control}");
    assert!(
        control.get("initiated_by").is_none() && control.get("control_epoch").is_none(),
        "one name for each field, camelCase: {control}"
    );
}

/// Open a tab on a run, take the browser over, and return the run and the tab's id.
async fn run_with_a_tab_under_takeover(edge: &Edge, person: &str) -> (String, String) {
    let run = edge.start_run(person, json!({})).await;
    let opened = edge.new_tab(person, &run, None).await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    let tab_id = opened.data()["tab"]["tabId"]
        .as_str()
        .expect("the new tab has an id")
        .to_owned();
    let takeover = edge
        .control(person, &run, json!({ "mode": "human_takeover" }))
        .await;
    assert_eq!(takeover.status, 200, "{}", takeover.body);
    (run, tab_id)
}

/// Under enforcement an agent's tab select or close during a take-over is refused, as
/// the tab gate says (`admission.rs`: an agent cannot do them during a takeover).
/// Neither call has a body, so the label is the `initiatedBy` query, as `new_tab` takes
/// it in its body; the Verevon gateway sends `?initiatedBy=agent` for a model's call
/// (finding F5-4: before, `derive_initiator(&claims, None)` made every call on a
/// person's token the person's, and the gate let it through).
#[tokio::test]
#[serial]
async fn an_agent_labelled_tab_select_or_close_during_a_takeover_is_refused() {
    set_takeover_enforcement(Some("1"));

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let (run, tab_id) = run_with_a_tab_under_takeover(&edge, &person).await;

    let select = edge
        .send(
            reqwest::Method::POST,
            &person,
            &format!("/v1/agent/runs/{run}/tabs/{tab_id}/select?initiatedBy=agent"),
            None,
        )
        .await;
    let close = edge
        .send(
            reqwest::Method::DELETE,
            &person,
            &format!("/v1/agent/runs/{run}/tabs/{tab_id}?initiatedBy=agent"),
            None,
        )
        .await;
    assert_eq!(
        (select.status, close.status),
        (409, 409),
        "select: {} close: {}",
        select.body,
        close.body
    );
    assert_eq!(select.code(), "USER_TOOK_OVER", "{}", select.body);
    assert_eq!(close.code(), "USER_TOOK_OVER", "{}", close.body);
    assert_eq!(select.details()["control_epoch"], 1, "{}", select.body);
    assert!(
        edge.driver.selected_tabs().is_empty() && edge.driver.closed_tabs().is_empty(),
        "a refused select or close never reaches the driver"
    );

    // The person is still admitted on the same tab, unlabelled or labelled human.
    let select = edge
        .send(
            reqwest::Method::POST,
            &person,
            &format!("/v1/agent/runs/{run}/tabs/{tab_id}/select?initiatedBy=human"),
            None,
        )
        .await;
    assert_eq!(select.status, 200, "{}", select.body);
    assert_eq!(edge.driver.selected_tabs(), vec![tab_id]);
}

/// The label is read the way `/step` reads it: it can only tighten. An agent's call
/// with no take-over in progress is admitted and recorded as the agent's (the timeline
/// names who moved the tab), and with enforcement off a call that would be refused is
/// admitted too, still recorded as the agent's.
#[tokio::test]
#[serial]
async fn an_agent_labelled_tab_select_is_recorded_as_the_agents() {
    set_takeover_enforcement(None);

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let (run, tab_id) = run_with_a_tab_under_takeover(&edge, &person).await;

    let observed = edge
        .send(
            reqwest::Method::POST,
            &person,
            &format!("/v1/agent/runs/{run}/tabs/{tab_id}/select?initiatedBy=agent"),
            None,
        )
        .await;
    assert_eq!(
        observed.status, 200,
        "observe mode admits it: {}",
        observed.body
    );
    let handed_back = edge
        .control(&person, &run, json!({ "mode": "agent_control" }))
        .await;
    assert_eq!(handed_back.status, 200, "{}", handed_back.body);
    let after = edge
        .send(
            reqwest::Method::POST,
            &person,
            &format!("/v1/agent/runs/{run}/tabs/{tab_id}/select?initiatedBy=agent"),
            None,
        )
        .await;
    assert_eq!(after.status, 200, "{}", after.body);

    let timeline = edge.timeline(&person, &run).await;
    let selected: Vec<&Value> = timeline
        .iter()
        .filter(|item| item["kind"] == "tab" && item["operation"] == "selected")
        .collect();
    assert_eq!(selected.len(), 2, "{timeline:?}");
    for item in selected {
        assert_eq!(item["initiatedBy"], "agent", "{item}");
    }
}

/// A hint that is neither `human` nor `agent` is a 400 on both routes; it is not read as
/// the person's, and nothing reaches the driver.
#[tokio::test]
#[serial]
async fn an_unknown_tab_label_is_refused_and_runs_nothing() {
    set_takeover_enforcement(Some("1"));

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let (run, tab_id) = run_with_a_tab_under_takeover(&edge, &person).await;

    for label in ["system", "", "AGENT"] {
        let select = edge
            .send(
                reqwest::Method::POST,
                &person,
                &format!("/v1/agent/runs/{run}/tabs/{tab_id}/select?initiatedBy={label}"),
                None,
            )
            .await;
        let close = edge
            .send(
                reqwest::Method::DELETE,
                &person,
                &format!("/v1/agent/runs/{run}/tabs/{tab_id}?initiatedBy={label}"),
                None,
            )
            .await;
        assert_eq!(
            (select.status, close.status),
            (400, 400),
            "label {label:?}: select {} close {}",
            select.body,
            close.body
        );
    }
    assert!(
        edge.driver.selected_tabs().is_empty() && edge.driver.closed_tabs().is_empty(),
        "a malformed label never reaches the driver"
    );
}

// ── both spellings of the initiator hint, on every body that carries it ─────

/// The wire spelling of the response and the timeline is camelCase (`initiatedBy`); a
/// request may carry the hint as `initiated_by` or `initiatedBy` on every route that
/// takes it. One test per carrier; each is refused with enforcement on during a
/// take-over when it names the agent, which is what proves the hint was read (before,
/// a snake_case `initiated_by` on a tab open was silently ignored and the agent's call
/// opened a tab in a person's browser: the rebuilt-edge evidence run, check 5).
#[tokio::test]
#[serial]
async fn a_tab_open_takes_the_hint_under_either_spelling() {
    set_takeover_enforcement(Some("1"));

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let takeover = edge
        .control(&person, &run, json!({ "mode": "human_takeover" }))
        .await;
    assert_eq!(takeover.status, 200, "{}", takeover.body);

    for body in [
        json!({ "initiated_by": "agent" }),
        json!({ "initiatedBy": "agent" }),
    ] {
        let refused = edge
            .post(&person, &format!("/v1/agent/runs/{run}/tabs"), body.clone())
            .await;
        assert_eq!(refused.status, 409, "{body}: {}", refused.body);
        assert_eq!(refused.code(), "USER_TOOK_OVER", "{body}");
    }
    assert_eq!(
        edge.driver.tabs_opened(),
        0,
        "a refused tab never reaches the driver"
    );

    // The two spellings in one body are a client bug, not a reason to pick one.
    let both = edge
        .post(
            &person,
            &format!("/v1/agent/runs/{run}/tabs"),
            json!({ "initiated_by": "human", "initiatedBy": "agent" }),
        )
        .await;
    assert_eq!(both.status, 422, "{}", both.body);
    assert_eq!(edge.driver.tabs_opened(), 0);

    // A value that is neither human nor agent is a 422 in a body (a 400 in the select
    // and close query: an_unknown_tab_label_is_refused_and_runs_nothing).
    for body in [
        json!({ "initiated_by": "system" }),
        json!({ "initiatedBy": "AGENT" }),
    ] {
        let unknown = edge
            .post(&person, &format!("/v1/agent/runs/{run}/tabs"), body.clone())
            .await;
        assert_eq!(unknown.status, 422, "{body}: {}", unknown.body);
    }
    assert_eq!(edge.driver.tabs_opened(), 0);

    // The person's own open, under either spelling, is admitted.
    for body in [
        json!({ "initiated_by": "human" }),
        json!({ "initiatedBy": "human" }),
    ] {
        let opened = edge
            .post(&person, &format!("/v1/agent/runs/{run}/tabs"), body.clone())
            .await;
        assert_eq!(opened.status, 200, "{body}: {}", opened.body);
    }
}

#[tokio::test]
#[serial]
async fn a_tab_select_and_close_take_the_hint_under_either_spelling_in_the_query() {
    set_takeover_enforcement(Some("1"));

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let (run, tab_id) = run_with_a_tab_under_takeover(&edge, &person).await;

    for query in ["initiated_by=agent", "initiatedBy=agent"] {
        let select = edge
            .send(
                reqwest::Method::POST,
                &person,
                &format!("/v1/agent/runs/{run}/tabs/{tab_id}/select?{query}"),
                None,
            )
            .await;
        let close = edge
            .send(
                reqwest::Method::DELETE,
                &person,
                &format!("/v1/agent/runs/{run}/tabs/{tab_id}?{query}"),
                None,
            )
            .await;
        assert_eq!(
            (select.status, close.status),
            (409, 409),
            "{query}: select {} close {}",
            select.body,
            close.body
        );
    }
    assert!(
        edge.driver.selected_tabs().is_empty() && edge.driver.closed_tabs().is_empty(),
        "a refused tab call never reaches the driver"
    );
}

#[tokio::test]
#[serial]
async fn a_step_takes_the_hint_under_either_spelling() {
    set_takeover_enforcement(Some("1"));

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let takeover = edge
        .control(&person, &run, json!({ "mode": "human_takeover" }))
        .await;
    assert_eq!(takeover.status, 200, "{}", takeover.body);

    for key in ["initiated_by", "initiatedBy"] {
        let refused = edge
            .post(
                &person,
                &format!("/v1/agent/runs/{run}/step"),
                json!({ "action": wait_action(), key: "agent" }),
            )
            .await;
        assert_eq!(refused.status, 409, "{key}: {}", refused.body);
        assert_eq!(refused.code(), "USER_TOOK_OVER", "{key}");
    }

    // One skipped receipt per take-over (the second refusal is the same episode): it
    // records the agent's identity, as the person's session, never a principal.
    let receipts = edge.receipts(&person, &run).await;
    let skips = skipped(&receipts);
    assert_eq!(skips.len(), 1, "{receipts:?}");
    assert_eq!(skips[0]["initiated_by"], "agent", "{}", skips[0]);
    assert_eq!(
        skips[0]["agent"],
        json!({ "basis": "user_session" }),
        "{}",
        skips[0]
    );
}

#[tokio::test]
#[serial]
async fn a_socket_action_takes_the_hint_under_either_spelling() {
    set_takeover_enforcement(Some("1"));

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let takeover = edge
        .control(&person, &run, json!({ "mode": "human_takeover" }))
        .await;
    assert_eq!(takeover.status, 200, "{}", takeover.body);

    let mut socket =
        TestSocket::connect(&edge, &person, &format!("/v1/agent/runs/{run}/frames/ws")).await;
    // `actor` is the socket's own name for it.
    for key in ["actor", "initiated_by", "initiatedBy"] {
        socket
            .send_text(
                &json!({ "type": "action", "action": wait_action(), key: "agent" }).to_string(),
            )
            .await;
        let refused = socket
            .next_matching(|message| message["type"] == "error" || message["type"] == "observation")
            .await;
        assert_eq!(refused["type"], "error", "{key}: {refused}");
        assert!(
            refused["message"]
                .as_str()
                .is_some_and(|text| text.starts_with("UserTookOver:")),
            "{key}: {refused}"
        );
    }
}

/// A run start takes the hint under both spellings too: it classes the run as an agent
/// run, which is budgeted. The projection does not expose the class, so this reads the
/// registry's own record of it.
#[tokio::test]
#[serial]
async fn a_run_start_takes_the_hint_under_either_spelling() {
    set_takeover_enforcement(None);

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    for key in ["initiated_by", "initiatedBy"] {
        let reply = edge
            .post(&person, "/v1/agent/runs", json!({ key: "agent" }))
            .await;
        assert_eq!(reply.status, 200, "{key}: {}", reply.body);
        let run = reply.data()["run_id"]
            .as_str()
            .expect("a run id")
            .to_owned();
        let handle = edge.live(&run).expect("the run is live");
        assert_eq!(
            handle.live.class(),
            RunClass::Agent,
            "{key}: the run was classed as an interactive one"
        );
    }
}

// ── who opened, selected or closed a tab ────────────────────────────────────

fn tab_items(timeline: &[Value]) -> Vec<&Value> {
    timeline
        .iter()
        .filter(|item| item["kind"] == "tab")
        .collect()
}

/// An agent's tab calls on a person's token are the agent's by the hint, and the agent
/// is the person's session, not a verified principal: the timeline says so, carries the
/// caller's run id and label as advisory fields only, and never a `principalId`. The
/// projection names the opener on the tab. A person's own calls carry no agent.
#[tokio::test]
#[serial]
async fn a_tab_call_records_which_agent_made_it_and_never_a_forged_principal() {
    set_takeover_enforcement(None);

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;

    let by_agent = edge
        .post(
            &person,
            &format!("/v1/agent/runs/{run}/tabs"),
            json!({
                "initiatedBy": "agent",
                "aiRunId": "run_01J9",
                "agentLabel": "research-planner",
                // Not part of the contract; a client can send what it likes.
                "principalId": "service:execution-core",
                "agent": { "basis": "service_principal", "principalId": "service:execution-core" },
            }),
        )
        .await;
    assert_eq!(by_agent.status, 200, "{}", by_agent.body);
    let agent_tab = by_agent.data()["tab"]["tabId"].as_str().unwrap().to_owned();
    let by_person = edge.new_tab(&person, &run, None).await;
    assert_eq!(by_person.status, 200, "{}", by_person.body);
    let person_tab = by_person.data()["tab"]["tabId"]
        .as_str()
        .unwrap()
        .to_owned();

    let select = edge
        .send(
            reqwest::Method::POST,
            &person,
            &format!(
                "/v1/agent/runs/{run}/tabs/{agent_tab}/select?initiatedBy=agent&aiRunId=run_01J9"
            ),
            None,
        )
        .await;
    assert_eq!(select.status, 200, "{}", select.body);
    // The tab record: the agent's tab says who opened it, the person's says a person,
    // and the browser's own tab has no opener.
    let projection = edge.projection(&person, &run).await;
    let tabs = projection.data()["tabs"].as_array().unwrap().clone();
    let opened_by = |tab_id: &str| {
        tabs.iter()
            .find(|tab| tab["tabId"] == tab_id)
            .map(|tab| tab["openedBy"].clone())
    };
    let expected_open =
        json!({ "basis": "user_session", "aiRunId": "run_01J9", "label": "research-planner" });
    assert_eq!(
        opened_by(&agent_tab),
        Some(json!({ "initiatedBy": "agent", "agent": expected_open.clone() })),
        "{tabs:?}"
    );
    assert_eq!(
        opened_by(&person_tab),
        Some(json!({ "initiatedBy": "human" })),
        "{tabs:?}"
    );
    assert_eq!(
        opened_by("tab_home"),
        Some(Value::Null),
        "the browser's own tab has no opener"
    );

    let close = edge
        .send(
            reqwest::Method::DELETE,
            &person,
            &format!("/v1/agent/runs/{run}/tabs/{person_tab}?initiated_by=agent&agent_label=research-planner"),
            None,
        )
        .await;
    assert_eq!(close.status, 200, "{}", close.body);
    // A closed tab's record is dropped.
    let handle = edge.live(&run).expect("the run is live");
    assert!(!handle
        .entry
        .lock()
        .await
        .tab_origins
        .contains_key(&person_tab));
    assert!(handle
        .entry
        .lock()
        .await
        .tab_origins
        .contains_key(&agent_tab));

    let timeline = edge.timeline(&person, &run).await;
    let items = tab_items(&timeline);
    let operations: Vec<&str> = items
        .iter()
        .map(|item| item["operation"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(
        operations,
        vec!["opened", "opened", "selected", "closed"],
        "{timeline:?}"
    );

    assert_eq!(items[0]["initiatedBy"], "agent");
    assert_eq!(items[0]["agent"], expected_open, "{}", items[0]);
    assert!(
        items[0]["agent"].get("principalId").is_none(),
        "{}",
        items[0]
    );
    // The person's own open: a person, and no agent at all.
    assert_eq!(items[1]["initiatedBy"], "human");
    assert!(items[1].get("agent").is_none(), "{}", items[1]);
    // The select names the run only; the close names the label only.
    assert_eq!(
        items[2]["agent"],
        json!({ "basis": "user_session", "aiRunId": "run_01J9" })
    );
    assert_eq!(
        items[3]["agent"],
        json!({ "basis": "user_session", "label": "research-planner" })
    );
}

/// A service principal is an agent whatever it sends, and its identity is the signed
/// service id, not anything in the request. A label that is not identifier-shaped (it
/// could carry a URL or a title) is dropped, not stored.
#[tokio::test]
#[serial]
async fn a_service_principals_tab_call_is_recorded_with_its_verified_id() {
    set_takeover_enforcement(None);

    let edge = Edge::start(RunLimits::default()).await;
    let service = service_token();
    let run = edge.start_run(&service, json!({})).await;

    let opened = edge
        .post(
            &service,
            &format!("/v1/agent/runs/{run}/tabs"),
            json!({
                "initiatedBy": "human",
                "aiRunId": "run_01J9",
                "agentLabel": "https://example.com/secret title",
                "principalId": "service:somebody-else",
                "agent": { "basis": "user_session", "principalId": "service:somebody-else" },
            }),
        )
        .await;
    assert_eq!(opened.status, 200, "{}", opened.body);

    let timeline = edge.timeline(&service, &run).await;
    let items = tab_items(&timeline);
    assert_eq!(items.len(), 1, "{timeline:?}");
    assert_eq!(
        items[0]["initiatedBy"], "agent",
        "a service cannot claim human"
    );
    assert_eq!(
        items[0]["agent"],
        json!({
            "basis": "service_principal",
            "principalId": "service:execution-core",
            "aiRunId": "run_01J9"
        }),
        "{}",
        items[0]
    );
    let stored = serde_json::to_string(&timeline).unwrap();
    assert!(
        !stored.contains("example.com") && !stored.contains("secret"),
        "{stored}"
    );

    // A step records the same identity on its receipt.
    let step = edge.step(&service, &run, wait_action(), None).await;
    assert_eq!(step.status, 200, "{}", step.body);
    let receipts = edge.receipts(&service, &run).await;
    assert_eq!(receipts[0]["initiated_by"], "agent");
    assert_eq!(receipts[0]["agent"]["basis"], "service_principal");
    assert_eq!(
        receipts[0]["agent"]["principalId"],
        "service:execution-core"
    );
    let timeline = edge.timeline(&service, &run).await;
    let action = timeline
        .iter()
        .find(|item| item["kind"] == "action")
        .expect("the step is on the timeline");
    assert_eq!(
        action["agent"]["principalId"], "service:execution-core",
        "{action}"
    );
}

/// A person's step labelled agent records the agent as the person's session too.
#[tokio::test]
#[serial]
async fn a_persons_step_labelled_agent_is_recorded_as_a_user_session_agent() {
    set_takeover_enforcement(None);

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let step = edge
        .post(
            &person,
            &format!("/v1/agent/runs/{run}/step"),
            json!({ "action": wait_action(), "initiatedBy": "agent", "aiRunId": "run_01J9" }),
        )
        .await;
    assert_eq!(step.status, 200, "{}", step.body);
    let human = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(human.status, 200, "{}", human.body);

    let receipts = edge.receipts(&person, &run).await;
    assert_eq!(receipts[0]["initiated_by"], "agent");
    assert_eq!(
        receipts[0]["agent"],
        json!({ "basis": "user_session", "aiRunId": "run_01J9" })
    );
    assert_eq!(receipts[1]["initiated_by"], "human");
    assert!(receipts[1].get("agent").is_none(), "{}", receipts[1]);
}

/// A service principal's tab call during a take-over is refused with enforcement on,
/// whatever hint it sends (the service owns the run, so the take-over is set on the live
/// handle, as the module docs say), and a service token without `browser:execute` never
/// reaches the tab routes.
#[tokio::test]
#[serial]
async fn a_service_principal_cannot_loosen_the_takeover_rule_on_a_tab_call() {
    set_takeover_enforcement(Some("1"));

    let edge = Edge::start(RunLimits::default()).await;
    let service = service_token();
    let run = edge.start_run(&service, json!({})).await;
    let opened = edge.new_tab(&service, &run, None).await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    let tab_id = opened.data()["tab"]["tabId"].as_str().unwrap().to_owned();
    take_over_live(&edge, &run).await;

    for hint in [None, Some("human"), Some("agent")] {
        let open = edge.new_tab(&service, &run, hint).await;
        assert_eq!(open.status, 409, "hint {hint:?}: {}", open.body);
        let query = hint.map_or(String::new(), |hint| format!("?initiatedBy={hint}"));
        let select = edge
            .send(
                reqwest::Method::POST,
                &service,
                &format!("/v1/agent/runs/{run}/tabs/{tab_id}/select{query}"),
                None,
            )
            .await;
        let close = edge
            .send(
                reqwest::Method::DELETE,
                &service,
                &format!("/v1/agent/runs/{run}/tabs/{tab_id}{query}"),
                None,
            )
            .await;
        assert_eq!((select.status, close.status), (409, 409), "hint {hint:?}");
    }

    // The refused step is recorded once, with the verified principal and nothing the
    // request said.
    let step = edge
        .post(
            &service,
            &format!("/v1/agent/runs/{run}/step"),
            json!({
                "action": wait_action(),
                "initiatedBy": "human",
                "agent": { "basis": "user_session", "principalId": "service:somebody-else" },
            }),
        )
        .await;
    assert_eq!(step.status, 409, "{}", step.body);
    let receipts = edge.receipts(&service, &run).await;
    let skips = skipped(&receipts);
    assert_eq!(skips.len(), 1, "{receipts:?}");
    assert_eq!(
        skips[0]["initiated_by"], "agent",
        "a service cannot claim human"
    );
    assert_eq!(
        skips[0]["agent"],
        json!({ "basis": "service_principal", "principalId": "service:execution-core" }),
        "{}",
        skips[0]
    );

    let no_scope = model_gateway_service_token();
    let refused = edge.new_tab(&no_scope, &run, None).await;
    assert_eq!(refused.status, 403, "{}", refused.body);
}

/// The two spellings of the hint in one request are a client bug on every route that
/// takes it, not a reason to pick one: the tab open (above), the step, the run start and
/// the select and close query are all refused, and nothing reaches the driver.
#[tokio::test]
#[serial]
async fn both_spellings_of_the_hint_in_one_request_are_refused_on_every_route() {
    set_takeover_enforcement(None);

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();

    let start = edge
        .post(
            &person,
            "/v1/agent/runs",
            json!({ "initiated_by": "human", "initiatedBy": "agent" }),
        )
        .await;
    assert_eq!(start.status, 422, "{}", start.body);

    let (run, tab_id) = {
        let run = edge.start_run(&person, json!({})).await;
        let opened = edge.new_tab(&person, &run, None).await;
        assert_eq!(opened.status, 200, "{}", opened.body);
        let tab_id = opened.data()["tab"]["tabId"].as_str().unwrap().to_owned();
        (run, tab_id)
    };
    let steps_before = edge.receipts(&person, &run).await.len();

    let step = edge
        .post(
            &person,
            &format!("/v1/agent/runs/{run}/step"),
            json!({ "action": wait_action(), "initiated_by": "human", "initiatedBy": "agent" }),
        )
        .await;
    assert_eq!(step.status, 422, "{}", step.body);

    let query = "initiated_by=human&initiatedBy=agent";
    let select = edge
        .send(
            reqwest::Method::POST,
            &person,
            &format!("/v1/agent/runs/{run}/tabs/{tab_id}/select?{query}"),
            None,
        )
        .await;
    let close = edge
        .send(
            reqwest::Method::DELETE,
            &person,
            &format!("/v1/agent/runs/{run}/tabs/{tab_id}?{query}"),
            None,
        )
        .await;
    for (name, reply) in [("select", &select), ("close", &close)] {
        assert!(
            (400..500).contains(&reply.status),
            "{name}: a query with both spellings is a client error, got {}: {}",
            reply.status,
            reply.body
        );
    }
    assert!(
        edge.driver.selected_tabs().is_empty() && edge.driver.closed_tabs().is_empty(),
        "a refused tab call never reaches the driver"
    );
    assert_eq!(
        edge.receipts(&person, &run).await.len(),
        steps_before,
        "a refused step leaves no receipt"
    );
}

/// A ZDR run persists no timeline item and no receipt for a tab call, whoever made it,
/// but the owner's projection still says who opened each tab: that record is in memory
/// only, ids and classes, and goes with the run.
#[tokio::test]
#[serial]
async fn a_zdr_runs_tab_calls_persist_nothing_but_the_projection_names_the_opener() {
    set_takeover_enforcement(None);

    let edge = Edge::start(RunLimits::default()).await;
    let person = zdr_member_token();
    let run = edge.start_run(&person, json!({})).await;
    let (org, actor) = ("org-zdr-golden", "user-golden");
    let store = edge.state.receipts.clone();

    let by_agent = edge
        .post(
            &person,
            &format!("/v1/agent/runs/{run}/tabs"),
            json!({ "initiated_by": "agent", "aiRunId": "run_01J9", "agentLabel": "research-planner" }),
        )
        .await;
    assert_eq!(by_agent.status, 200, "{}", by_agent.body);
    let agent_tab = by_agent.data()["tab"]["tabId"].as_str().unwrap().to_owned();
    let by_person = edge.new_tab(&person, &run, None).await;
    assert_eq!(by_person.status, 200, "{}", by_person.body);
    let person_tab = by_person.data()["tab"]["tabId"]
        .as_str()
        .unwrap()
        .to_owned();
    let select = edge
        .send(
            reqwest::Method::POST,
            &person,
            &format!("/v1/agent/runs/{run}/tabs/{agent_tab}/select?initiatedBy=agent"),
            None,
        )
        .await;
    assert_eq!(select.status, 200, "{}", select.body);
    let close = edge
        .send(
            reqwest::Method::DELETE,
            &person,
            &format!("/v1/agent/runs/{run}/tabs/{person_tab}?initiatedBy=human"),
            None,
        )
        .await;
    assert_eq!(close.status, 200, "{}", close.body);

    assert_nothing_persisted(store.as_ref(), org, actor, &run, "after tab calls").await;

    let projection = edge.projection(&person, &run).await;
    assert_eq!(projection.status, 200, "{}", projection.body);
    assert_eq!(projection.data()["zdr"], true);
    let tabs = projection.data()["tabs"].as_array().unwrap().clone();
    let opened_by = tabs
        .iter()
        .find(|tab| tab["tabId"] == agent_tab.as_str())
        .map(|tab| tab["openedBy"].clone());
    assert_eq!(
        opened_by,
        Some(json!({
            "initiatedBy": "agent",
            "agent": { "basis": "user_session", "aiRunId": "run_01J9", "label": "research-planner" }
        })),
        "{tabs:?}"
    );
    assert!(
        tabs.iter().all(|tab| tab["tabId"] != person_tab.as_str()),
        "the closed tab is gone from the projection: {tabs:?}"
    );
}

/// A run that holds the most full opener records still says, for a tab opened after
/// that, whether a person or an agent opened it, and marks it `unattributed` instead of
/// leaving it looking like a tab the browser opened itself. A ZDR run has no other
/// record of who opened its tabs. No URL or title is stored, and the 256 earlier
/// records keep their agent ids.
#[tokio::test]
#[serial]
async fn tabs_opened_past_the_opener_cap_are_marked_unattributed_in_the_projection() {
    set_takeover_enforcement(None);

    let edge = Edge::start(RunLimits::default()).await;
    let person = zdr_member_token();
    let run = edge.start_run(&person, json!({})).await;

    let mut first = String::new();
    for n in 0..256 {
        let opened = edge
            .post(
                &person,
                &format!("/v1/agent/runs/{run}/tabs"),
                json!({ "initiated_by": "agent", "aiRunId": format!("run_{n}"), "agentLabel": "planner" }),
            )
            .await;
        assert_eq!(opened.status, 200, "tab {n}: {}", opened.body);
        if n == 0 {
            first = opened.data()["tab"]["tabId"].as_str().unwrap().to_owned();
        }
    }
    let late_agent = edge
        .post(
            &person,
            &format!("/v1/agent/runs/{run}/tabs"),
            json!({ "initiated_by": "agent", "aiRunId": "run_late", "agentLabel": "late-planner" }),
        )
        .await;
    assert_eq!(late_agent.status, 200, "{}", late_agent.body);
    let late_agent = late_agent.data()["tab"]["tabId"].as_str().unwrap().to_owned();
    let late_person = edge.new_tab(&person, &run, None).await;
    assert_eq!(late_person.status, 200, "{}", late_person.body);
    let late_person = late_person.data()["tab"]["tabId"].as_str().unwrap().to_owned();

    let projection = edge.projection(&person, &run).await;
    assert_eq!(projection.status, 200, "{}", projection.body);
    let tabs = projection.data()["tabs"].as_array().unwrap().clone();
    let opened_by = |tab_id: &str| {
        tabs.iter()
            .find(|tab| tab["tabId"] == tab_id)
            .map(|tab| tab["openedBy"].clone())
    };
    assert_eq!(
        opened_by(&late_agent),
        Some(json!({ "initiatedBy": "agent", "unattributed": true })),
        "the class is kept, the agent's ids are not"
    );
    assert_eq!(
        opened_by(&late_person),
        Some(json!({ "initiatedBy": "human", "unattributed": true }))
    );
    assert_eq!(
        opened_by(&first),
        Some(json!({
            "initiatedBy": "agent",
            "agent": { "basis": "user_session", "aiRunId": "run_0", "label": "planner" }
        })),
        "a record taken before the cap is complete"
    );
    assert_eq!(
        opened_by("tab_home"),
        Some(Value::Null),
        "the browser's own tab still has no opener, which is what the marker tells apart"
    );
}

/// A tab that goes away with no close call (the page closes itself, or the user closes
/// it in the browser) must not keep its opener: the next listing of the run's tabs,
/// the projection or any other route that lists them drops it, and so does the next
/// open, so the record never holds an entry for a tab the browser no longer has. Its
/// opener never reaches the projection either.
#[tokio::test]
#[serial]
async fn a_tab_that_disappears_without_a_close_call_loses_its_opener() {
    set_takeover_enforcement(None);

    let edge = Edge::start(RunLimits::default()).await;
    let person = zdr_member_token();
    let run = edge.start_run(&person, json!({})).await;
    let open = |label: &'static str| {
        let (edge, person, run) = (&edge, &person, &run);
        async move {
            let opened = edge
                .post(
                    person,
                    &format!("/v1/agent/runs/{run}/tabs"),
                    json!({ "initiated_by": "agent", "aiRunId": "run_x", "agentLabel": label }),
                )
                .await;
            assert_eq!(opened.status, 200, "{}", opened.body);
            opened.data()["tab"]["tabId"].as_str().unwrap().to_owned()
        }
    };
    let gone = open("opener-a").await;
    let kept = open("opener-b").await;
    let handle = edge.live(&run).expect("the run is live");
    let recorded = || async {
        let entry = handle.entry.lock().await;
        let mut ids: Vec<String> = entry.tab_origins.keys().cloned().collect();
        ids.sort();
        ids
    };
    let mut both = vec![gone.clone(), kept.clone()];
    both.sort();
    assert_eq!(recorded().await, both);

    // The browser drops the tab; nobody called close.
    edge.driver.vanish_tab(&gone);
    assert!(edge.driver.closed_tabs().is_empty());

    // The projection does not show it, and the read that listed the tabs pruned it.
    let projection = edge.projection(&person, &run).await;
    assert_eq!(projection.status, 200, "{}", projection.body);
    let tabs = projection.data()["tabs"].as_array().unwrap().clone();
    assert!(tabs.iter().all(|tab| tab["tabId"] != gone.as_str()), "{tabs:?}");
    assert_eq!(recorded().await, vec![kept.clone()]);

    // The same through the tab list and through the next open.
    let third = open("opener-c").await;
    edge.driver.vanish_tab(&kept);
    let listed = edge.get(&person, &format!("/v1/agent/runs/{run}/tabs")).await;
    assert_eq!(listed.status, 200, "{}", listed.body);
    assert_eq!(recorded().await, vec![third.clone()]);
    edge.driver.vanish_tab(&third);
    let fourth = open("opener-d").await;
    assert_eq!(recorded().await, vec![fourth]);
}

// ── when the takeover began, and handing the browser back to a service ──────

/// Hand the browser back on the live handle the way the control route does. A service
/// principal's run cannot be handed back over HTTP (no route lets a service change the
/// mode), so the test that needs it does it here and says so.
async fn give_back_live(edge: &Edge, run_id: &str) -> u64 {
    let handle: RunHandle = edge.live(run_id).expect("the run is live");
    let (changed, epoch) = handle
        .live
        .set_mode(BrowserControlMode::AgentControl, None)
        .expect("no expected epoch to compare");
    assert!(changed, "the run was in a takeover");
    let mut entry = handle.entry.lock().await;
    entry.control_mode = BrowserControlMode::AgentControl;
    entry.control_epoch = epoch;
    epoch
}

fn taken_over_at(reply: &Reply) -> chrono::DateTime<chrono::Utc> {
    let text = reply.details()["taken_over_at"]
        .as_str()
        .unwrap_or_else(|| panic!("the 409 names when the takeover began: {}", reply.body));
    chrono::DateTime::parse_from_rfc3339(text)
        .unwrap_or_else(|error| panic!("taken_over_at is RFC 3339 ({text}): {error}"))
        .with_timezone(&chrono::Utc)
}

/// The 409 carries `taken_over_at` beside the mode and the epoch, on a step and on a tab
/// call, and the time is the one the person's request set: a second takeover has its own.
#[tokio::test]
#[serial]
async fn a_takeover_refusal_says_when_the_person_took_the_browser() {
    set_takeover_enforcement(Some("1"));

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;

    let before = chrono::Utc::now() - chrono::Duration::seconds(1);
    assert_eq!(flip(&edge, &person, &run, "human_takeover").await, 1);
    let after = chrono::Utc::now() + chrono::Duration::seconds(1);

    let step = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(step.status, 409, "{}", step.body);
    assert_eq!(step.code(), "USER_TOOK_OVER");
    assert_eq!(step.details()["control_mode"], "human_takeover");
    assert_eq!(step.details()["control_epoch"], 1);
    let first = taken_over_at(&step);
    assert!(
        first >= before && first <= after,
        "{first} is not between {before} and {after}"
    );

    let tab = edge.new_tab(&person, &run, Some("agent")).await;
    assert_eq!(tab.status, 409, "{}", tab.body);
    assert_eq!(taken_over_at(&tab), first, "the same takeover, the same time");

    // Handed back: no refusal, and the next takeover is a new one with its own time.
    assert_eq!(flip(&edge, &person, &run, "agent_control").await, 2);
    let ran = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(ran.status, 200, "{}", ran.body);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(flip(&edge, &person, &run, "human_takeover").await, 3);
    let again = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(again.status, 409, "{}", again.body);
    assert_eq!(again.details()["control_epoch"], 3);
    assert!(taken_over_at(&again) > first, "the second takeover has a later time");
}

/// A service principal is refused during a takeover and admitted again once the browser
/// is handed back, as an agent, in the new epoch. The service cannot change the mode, so
/// the takeover and the hand-back are made on the live handle (see `give_back_live`); the
/// refusal and the admission are the real routes.
#[tokio::test]
#[serial]
async fn a_service_principal_is_admitted_again_after_the_browser_is_handed_back() {
    set_takeover_enforcement(Some("1"));

    let edge = Edge::start(RunLimits::default()).await;
    let service = service_token();
    let run = edge.start_run(&service, json!({})).await;
    assert_eq!(take_over_live(&edge, &run).await, 1);

    let refused = edge.step(&service, &run, wait_action(), None).await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(refused.code(), "USER_TOOK_OVER");
    assert_eq!(refused.details()["control_epoch"], 1);
    let _ = taken_over_at(&refused);
    let tab = edge.new_tab(&service, &run, None).await;
    assert_eq!(tab.status, 409, "a service's tab call is an agent's: {}", tab.body);

    assert_eq!(give_back_live(&edge, &run).await, 2);

    let ran = edge.step(&service, &run, wait_action(), None).await;
    assert_eq!(ran.status, 200, "{}", ran.body);
    let opened = edge.new_tab(&service, &run, None).await;
    assert_eq!(opened.status, 200, "{}", opened.body);

    let receipts = edge.receipts(&service, &run).await;
    let skips = skipped(&receipts);
    assert_eq!(skips.len(), 1, "one skipped receipt for the takeover: {receipts:?}");
    assert_eq!(skips[0]["initiated_by"], "agent");
    assert_eq!(skips[0]["control_epoch"], 1);
    let done = completed(&receipts);
    assert_eq!(done.len(), 1, "{receipts:?}");
    assert_eq!(done[0]["initiated_by"], "agent");
    assert_eq!(done[0]["control_epoch"], 2);
}

// ── the tab routes and the grant ────────────────────────────────────────────

/// A broker whose grant can be revoked mid-run, and which counts every check.
struct RevocableBroker {
    active: Arc<AtomicBool>,
    checks: Arc<AtomicU32>,
}

#[async_trait]
impl GrantValidator for RevocableBroker {
    async fn validate(&self, grant_id: &str) -> QuarryResult<GrantValidation> {
        self.checks.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::from_value(json!({
            "grant_id": grant_id,
            "active": self.active.load(Ordering::SeqCst),
            "kind": "run",
        }))
        .expect("a grant validation"))
    }
}

/// The documented exception to "every action surface goes through `admit_action`": a tab
/// route has no action to check, so it takes the closed and control gates, and a tab OPEN
/// (which can load a page) also revalidates the grant, the domains and the URL itself. A
/// tab SELECT or CLOSE moves no data and loads nothing, so it does not ask the broker: a
/// run whose grant was revoked or has expired can still close its tabs, and the grant is
/// asked again at the next action. This pins both halves, and that an action does ask.
#[tokio::test]
#[serial]
async fn a_revoked_grant_stops_a_tab_open_and_an_action_but_not_a_tab_select_or_close() {
    set_takeover_enforcement(None);

    let active = Arc::new(AtomicBool::new(true));
    let checks = Arc::new(AtomicU32::new(0));
    let broker = RevocableBroker {
        active: active.clone(),
        checks: checks.clone(),
    };
    let edge = Edge::start_with(RunLimits::default(), move |state| {
        let mut state = state;
        state.grant_validator = Arc::new(broker);
        state
    })
    .await;
    let person = member_token();
    let run = edge
        .start_run(&person, json!({ "grant_id": "grant-revocable" }))
        .await;

    let opened = edge.new_tab(&person, &run, None).await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    let tab = opened.data()["tab"]["tabId"].as_str().unwrap().to_owned();

    active.store(false, Ordering::SeqCst);

    let refused_open = edge.new_tab(&person, &run, None).await;
    assert_eq!(refused_open.status, 403, "{}", refused_open.body);
    assert_eq!(refused_open.code(), "SECURITY_BLOCKED", "{}", refused_open.body);
    let refused_step = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(refused_step.code(), "SECURITY_BLOCKED", "{}", refused_step.body);

    let asked = checks.load(Ordering::SeqCst);
    let select = edge
        .send(
            reqwest::Method::POST,
            &person,
            &format!("/v1/agent/runs/{run}/tabs/{tab}/select"),
            None,
        )
        .await;
    assert_eq!(select.status, 200, "{}", select.body);
    let close = edge
        .delete(&person, &format!("/v1/agent/runs/{run}/tabs/{tab}"))
        .await;
    assert_eq!(close.status, 200, "{}", close.body);
    assert_eq!(
        checks.load(Ordering::SeqCst),
        asked,
        "select and close did not ask the broker"
    );
    assert_eq!(edge.driver.selected_tabs(), vec![tab.clone()]);
    assert_eq!(edge.driver.closed_tabs(), vec![tab]);
}
