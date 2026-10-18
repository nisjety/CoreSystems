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
//! Tab select and close take no label, so on a person's token they are the person's,
//! and an agent's select or close during a take-over is not refused (finding F5-4,
//! the ignored test at the end of this file). A service principal's run is owned by
//! the service, and a service is always an agent, so no HTTP route can take a
//! service-owned run over. The one test that needs a take-over of such a run sets it
//! on the live handle, the way the control route does, and says so.
//!
//! Every test is `#[serial]`: the switch is a process-wide environment variable.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use agent_harness::*;
use quarry_edge::agent_routes::RunHandle;
use quarry_edge::run_limits::{
    parse_takeover_enforcement, takeover_enforced, RunLimits, ENV_TAKEOVER_ENFORCEMENT,
};
use quarry_runtime::step_receipts::BrowserControlMode;
use quarry_runtime::StepReceiptStore;
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

// ── contract findings ───────────────────────────────────────────────────────

/// The plan's contract says timeline items carry `initiatedBy` and `controlEpoch`.
/// The Tab and Action items do; a Control item does not (`BrowserTimelineItemDetail::Control`
/// in `agent_routes/types.rs` has no `rename` on `initiated_by`), so it serialises
/// `initiated_by`. The plan fixes the name as `initiatedBy`, so this stays strict and
/// ignored until the code agrees; `cargo test -- --ignored` shows the failure.
#[tokio::test]
#[serial]
#[ignore = "finding F5-1: a control timeline item serialises initiated_by, not initiatedBy"]
async fn control_timeline_items_carry_initiated_by_under_the_documented_name() {
    set_takeover_enforcement(None);

    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    edge.control(&person, &run, json!({ "mode": "human_takeover" }))
        .await;

    let timeline = edge.timeline(&person, &run).await;
    let control = timeline
        .iter()
        .find(|item| item["kind"] == "control")
        .expect("the take-over is on the timeline");
    assert_eq!(control["initiatedBy"], "human", "{control}");
    assert_eq!(control["controlEpoch"], 1, "{control}");
}

/// Finding F5-4 (ignored until the code agrees; `cargo test -- --ignored` shows it fail).
///
/// Under enforcement an agent's tab select or close during a take-over is refused, as
/// the tab gate's own comment says (`admission.rs`: an agent cannot do them during a
/// takeover). The select and close routes take no body and no initiator, so
/// `select_tab` and `close_tab` classify every call on a person's token as the person's
/// (`derive_initiator(&claims, None)`), and the gate lets it through. This test labels
/// the two calls `initiatedBy: "agent"` in the body, the way `new_tab` takes its label.
/// A fix may take the label somewhere else (a query, say), and then this test changes
/// with it.
#[tokio::test]
#[serial]
#[ignore = "finding F5-4: tab select and close take no initiator, so an agent's call is the person's"]
async fn an_agent_labelled_tab_select_or_close_during_a_takeover_is_refused() {
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

    let label = json!({ "initiatedBy": "agent" });
    let select = edge
        .send(
            reqwest::Method::POST,
            &person,
            &format!("/v1/agent/runs/{run}/tabs/{tab_id}/select"),
            Some(label.clone()),
        )
        .await;
    let close = edge
        .send(
            reqwest::Method::DELETE,
            &person,
            &format!("/v1/agent/runs/{run}/tabs/{tab_id}"),
            Some(label),
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
    assert!(
        edge.driver.selected_tabs().is_empty() && edge.driver.closed_tabs().is_empty(),
        "a refused select or close never reaches the driver"
    );
}
