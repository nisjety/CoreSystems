//! Run caps, the sweeper and what a released run leaves behind, against the real
//! router and real token verification (the harness is in
//! `support/agent_harness.rs`).
//!
//! Expiry is driven by an injected clock: `sweep_once(state, later(ms))` decides
//! with the reading it is given, so nothing waits on a real timer. The browser is
//! the recording driver, so "released exactly once" is a count of the driver's
//! releases for that run.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_harness::*;
use quarry_core::event::EventType;
use quarry_edge::agent_routes::{drain_runs, sweep_once, DrainReport, SweepReport};
use quarry_edge::run_limits::RunLimits;
use quarry_runtime::step_receipts::{
    AgentRunStatus, BrowserTimelineEventKind, BrowserTimelineLifecycle,
};
use serde_json::{json, Value};

/// The org and actor of the standard-org member (`org-standard-golden`, `user-golden`).
const MEMBER_ORG: &str = "org-standard-golden";
const MEMBER: &str = "user-golden";
/// The org and actor of the ZDR-org member (`org-zdr-golden`, `user-golden`).
const ZDR_ORG: &str = "org-zdr-golden";

/// Caps off, and a person's idle and agent idle limits both `idle_s`, so that
/// only the sweeper decides when a run is over.
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

/// A capacity refusal: 429, the closed reason, the cap that was hit, how many were
/// live, the wait the body names, and the same wait as a `Retry-After` header.
fn assert_capacity_refusal(reply: &Reply, reason: &str, scope: &str, limit: u64, active: u64) {
    assert_eq!(reply.status, 429, "{}", reply.body);
    assert_eq!(reply.code(), "RATE_LIMITED", "{}", reply.body);
    assert_eq!(reply.details()["reason"], reason, "{}", reply.body);
    assert_eq!(reply.details()["scope"], scope, "{}", reply.body);
    assert_eq!(reply.details()["limit"], limit, "{}", reply.body);
    assert_eq!(reply.details()["active"], active, "{}", reply.body);
    assert_eq!(reply.details()["retry_after_seconds"], 10, "{}", reply.body);
    assert_eq!(
        reply
            .headers
            .get("retry-after")
            .and_then(|value| value.to_str().ok()),
        Some("10"),
        "the wait is also a Retry-After header"
    );
}

/// The lifecycle states on a run's owner timeline, in the order stored.
async fn lifecycle_states(edge: &Edge, run_id: &str) -> Vec<BrowserTimelineLifecycle> {
    edge.state
        .receipts
        .list_browser_timeline_events(MEMBER_ORG, MEMBER, &kind(run_id))
        .await
        .expect("timeline")
        .into_iter()
        .filter_map(|event| match event.event {
            BrowserTimelineEventKind::Lifecycle { state, .. } => Some(state),
            _ => None,
        })
        .collect()
}

/// Poll `ready` for up to a second, for a test that must wait for a request to reach a
/// point inside the edge. It never sleeps for longer than 5 ms at a time. It returns
/// whether `ready` came true, so the caller asserts with the cause in its own message.
async fn wait_for(ready: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(1);
    while !ready() {
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    true
}

/// The `agent.expired` payloads the durable channel carried, in order.
async fn expired_payloads(edge: &Edge) -> Vec<Value> {
    edge.durable_events()
        .await
        .into_iter()
        .filter(|(event_type, _)| *event_type == EventType::AgentExpired)
        .map(|(_, payload)| payload)
        .collect()
}

// ── caps ────────────────────────────────────────────────────────────────────

/// Each cap refuses with its own reason, counted across the person's and the
/// agent's runs together, and the global cap counts every organization.
#[tokio::test]
async fn a_start_over_a_cap_is_refused_with_its_reason_and_a_retry_after_header() {
    let limits = RunLimits {
        max_concurrent_runs: 3,
        max_runs_per_org: 2,
        max_interactive_runs_per_org: 1,
        ..RunLimits::default()
    };
    let edge = Edge::start(limits).await;
    let person = member_token();

    let first = edge.post(&person, "/v1/agent/runs", json!({})).await;
    assert_eq!(first.status, 200, "{}", first.body);

    // The person's own sessions in the org are capped below the org.
    let second = edge.post(&person, "/v1/agent/runs", json!({})).await;
    assert_capacity_refusal(&second, "org_interactive_run_cap", "org", 1, 1);

    // An agent's run in the same org still fits under the org cap...
    let agent = edge
        .post(&service_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_eq!(agent.status, 200, "{}", agent.body);

    // ...and the org is then full, whatever the class.
    let full = edge
        .post(&service_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_capacity_refusal(&full, "org_run_cap", "org", 2, 2);

    // Another organization takes the third global slot...
    let stranger = edge
        .post(&stranger_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_eq!(stranger.status, 200, "{}", stranger.body);

    // ...and the next start is refused by the global cap.
    let over = edge
        .post(&stranger_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_capacity_refusal(&over, "global_run_cap", "global", 3, 3);

    // Nothing was admitted by the refused starts.
    assert_eq!(edge.driver.acquired().len(), 3);
    assert_eq!(edge.state.agent_runs.stats().live, 3);
}

/// A closed run gives its slot back: the next start in a full org succeeds.
#[tokio::test]
async fn a_closed_run_frees_its_slot_for_the_next_start() {
    let limits = RunLimits {
        max_runs_per_org: 1,
        max_concurrent_runs: 0,
        max_interactive_runs_per_org: 0,
        ..RunLimits::default()
    };
    let edge = Edge::start(limits).await;
    let person = member_token();

    let run = edge.start_run(&person, json!({})).await;
    let full = edge.post(&person, "/v1/agent/runs", json!({})).await;
    assert_capacity_refusal(&full, "org_run_cap", "org", 1, 1);

    let closed = edge.delete(&person, &format!("/v1/agent/runs/{run}")).await;
    assert_eq!(closed.status, 200, "{}", closed.body);
    assert_eq!(closed.data()["closed"], true);
    assert_eq!(edge.driver.released(), vec![run.clone()]);

    let again = edge.post(&person, "/v1/agent/runs", json!({})).await;
    assert_eq!(again.status, 200, "the slot came back: {}", again.body);
}

/// A start the driver refuses after its slot was admitted gives the slot back.
#[tokio::test]
async fn a_start_whose_acquire_fails_frees_its_slot() {
    let limits = RunLimits {
        max_concurrent_runs: 1,
        max_runs_per_org: 0,
        max_interactive_runs_per_org: 0,
        ..RunLimits::default()
    };
    let edge = Edge::start(limits).await;
    let person = member_token();

    edge.driver.refuse_next_acquire();
    let failed = edge.post(&person, "/v1/agent/runs", json!({})).await;
    assert_eq!(failed.status, 502, "{}", failed.body);
    assert_eq!(failed.code(), "DRIVER_FAILED");
    let stats = edge.state.agent_runs.stats();
    assert_eq!(
        (stats.live, stats.pending),
        (0, 0),
        "nothing is held by the failed start"
    );

    let ok = edge.post(&person, "/v1/agent/runs", json!({})).await;
    assert_eq!(ok.status, 200, "the one slot is free: {}", ok.body);
}

/// Twelve starts race for an organization that holds four. Exactly four are admitted
/// and the other eight are refused by the org cap, however the requests interleave: a
/// start holds its slot from before its first await until it commits or fails.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_starts_never_exceed_the_organization_cap() {
    let limits = RunLimits {
        max_runs_per_org: 4,
        max_concurrent_runs: 0,
        max_interactive_runs_per_org: 0,
        ..RunLimits::default()
    };
    let edge = Arc::new(Edge::start(limits).await);
    let person = member_token();

    let starts: Vec<_> = (0..12)
        .map(|_| {
            let edge = edge.clone();
            let person = person.clone();
            tokio::spawn(async move { edge.post(&person, "/v1/agent/runs", json!({})).await })
        })
        .collect();
    let mut admitted = 0;
    for start in starts {
        let reply = start.await.expect("start task");
        if reply.status == 200 {
            admitted += 1;
        } else {
            // A refusal names the org cap, and the org is exactly full when it is given.
            assert_capacity_refusal(&reply, "org_run_cap", "org", 4, 4);
        }
    }
    assert_eq!(admitted, 4, "the cap admits four");
    assert_eq!(
        edge.driver.acquired().len(),
        4,
        "only admitted starts reach the driver"
    );
    let stats = edge.state.agent_runs.stats();
    assert_eq!((stats.live, stats.pending), (4, 0), "{stats:?}");
}

// ── the sweeper ─────────────────────────────────────────────────────────────

/// An idle run is released exactly once through the driver. Its non-ZDR checkpoint
/// stays active (resumable), its owner's timeline records an expiry and no close,
/// and the durable channel carries one `agent.expired` with the reason.
#[tokio::test]
async fn an_idle_run_is_released_exactly_once_and_its_checkpoint_stays_resumable() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let step = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(step.status, 200, "{}", step.body);

    // Two minutes of silence against a one-minute limit.
    let report = sweep_once(&edge.state, later(120_000)).await;
    assert_eq!(
        report,
        SweepReport {
            expired: 1,
            would_expire: 0
        }
    );
    // A second pass finds nothing left to do.
    let again = sweep_once(&edge.state, later(240_000)).await;
    assert_eq!(again, SweepReport::default());

    assert_eq!(
        edge.driver.released(),
        vec![run.clone()],
        "released exactly once"
    );
    assert!(
        edge.live(&run).is_none(),
        "the registry no longer holds the run"
    );

    let checkpoint = edge
        .state
        .receipts
        .load_run_checkpoint(MEMBER_ORG, &kind(&run))
        .await
        .expect("checkpoint read")
        .expect("a non-ZDR run keeps its checkpoint");
    assert_eq!(checkpoint.status, AgentRunStatus::Active);
    assert!(!checkpoint.zdr.is_active());
    assert_eq!(checkpoint.step, 1);

    let states = lifecycle_states(&edge, &run).await;
    assert!(
        states.contains(&BrowserTimelineLifecycle::Started),
        "{states:?}"
    );
    assert!(
        states.contains(&BrowserTimelineLifecycle::Expired),
        "{states:?}"
    );
    assert!(
        !states.contains(&BrowserTimelineLifecycle::Closed),
        "{states:?}"
    );

    // The owner timeline says the system ended it, not a person or an agent; the
    // start carries no initiator.
    let items = edge.timeline(&person, &run).await;
    let lifecycle: Vec<&Value> = items.iter().filter(|item| item["kind"] == "lifecycle").collect();
    let expired_item = lifecycle
        .iter()
        .find(|item| item["state"] == "expired")
        .expect("the expiry is on the owner timeline");
    assert_eq!(expired_item["initiatedBy"], "system", "{expired_item}");
    let started_item = lifecycle
        .iter()
        .find(|item| item["state"] == "started")
        .expect("the start is on the owner timeline");
    assert!(started_item.get("initiatedBy").is_none(), "{started_item}");

    let durable = edge.durable_events().await;
    let expired: Vec<&Value> = durable
        .iter()
        .filter(|(event_type, _)| *event_type == EventType::AgentExpired)
        .map(|(_, payload)| payload)
        .collect();
    assert_eq!(expired.len(), 1, "one agent.expired: {durable:?}");
    assert_eq!(expired[0]["reason"], "idle");
    assert!(
        expired[0].get("url").is_none(),
        "the expiry carries no URL: {}",
        expired[0]
    );
}

/// The owner's own close is not the system's: its `closed` item carries no initiator.
#[tokio::test]
async fn an_owners_close_is_not_recorded_as_the_systems() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let closed = edge
        .delete(&person, &format!("/v1/agent/runs/{run}"))
        .await;
    assert_eq!(closed.status, 200, "{}", closed.body);

    let items = edge.timeline(&person, &run).await;
    let closed_item = items
        .iter()
        .find(|item| item["kind"] == "lifecycle" && item["state"] == "closed")
        .expect("the close is on the owner timeline");
    assert!(closed_item.get("initiatedBy").is_none(), "{closed_item}");
}

/// A ZDR run is released exactly once too, and nothing is written for it: no
/// checkpoint, no timeline item, no receipt. Its expiry is announced to live
/// subscribers only, and its owner's next step is the plain 404.
#[tokio::test]
async fn a_zdr_run_is_released_once_and_leaves_nothing_behind() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = zdr_member_token();
    let run = edge.start_run(&person, json!({})).await;
    let mut live = edge.state.event_sink.subscribe(&kind(&run));
    let step = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(step.status, 200, "{}", step.body);

    let report = sweep_once(&edge.state, later(120_000)).await;
    assert_eq!(report.expired, 1);
    assert_eq!(
        edge.driver.released(),
        vec![run.clone()],
        "released exactly once"
    );

    let store = edge.state.receipts.clone();
    assert!(store
        .load_run_checkpoint(ZDR_ORG, &kind(&run))
        .await
        .expect("checkpoint read")
        .is_none());
    assert!(store
        .list_browser_timeline_events(ZDR_ORG, MEMBER, &kind(&run))
        .await
        .expect("timeline read")
        .is_empty());
    assert!(store
        .list_for_actor(ZDR_ORG, MEMBER, &kind(&run))
        .await
        .expect("receipt read")
        .is_empty());

    let mut heard = Vec::new();
    while let Ok(event) = live.try_recv() {
        heard.push(event.event_type);
    }
    assert!(
        heard.contains(&EventType::AgentExpired),
        "live subscribers hear it: {heard:?}"
    );

    let durable = edge.durable_events().await;
    assert!(
        durable
            .iter()
            .all(|(event_type, _)| *event_type != EventType::AgentExpired),
        "a ZDR expiry never enters the durable channel: {durable:?}"
    );

    let gone = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(gone.status, 404, "{}", gone.body);
    assert_eq!(gone.message(), "agent run not found");
    assert!(
        gone.details()["resumable"].is_null() && gone.details()["reason"].is_null(),
        "a ZDR run gets no resume hint: {}",
        gone.body
    );
}

/// After the sweeper releases a run, a step answers 404 `run_not_live` with
/// `resumable: true` and the run id for its owner. Another member of the same org
/// gets the plain 404. The owner resumes under the same id; another member cannot.
#[tokio::test]
async fn after_expiry_a_step_is_run_not_live_and_resumable_for_the_owner_only() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let step = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(step.status, 200, "{}", step.body);
    assert_eq!(sweep_once(&edge.state, later(120_000)).await.expired, 1);

    let gone = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(gone.status, 404, "{}", gone.body);
    assert_eq!(gone.code(), "NOT_FOUND");
    assert_eq!(gone.details()["reason"], "run_not_live", "{}", gone.body);
    assert_eq!(gone.details()["resumable"], true, "{}", gone.body);
    assert_eq!(
        gone.details()["resume_run_id"],
        run.as_str(),
        "{}",
        gone.body
    );

    // A tab call on the released run gets the same answer, and opens nothing.
    let tab = edge.new_tab(&person, &run, None).await;
    assert_eq!(tab.status, 404, "{}", tab.body);
    assert_eq!(tab.details()["reason"], "run_not_live", "{}", tab.body);
    assert_eq!(tab.details()["resumable"], true, "{}", tab.body);
    assert_eq!(
        edge.driver.tabs_opened(),
        0,
        "no tab reaches the driver after expiry"
    );

    let other = edge
        .step(&other_member_token(), &run, wait_action(), None)
        .await;
    assert_eq!(other.status, 404, "{}", other.body);
    assert_eq!(other.message(), "agent run not found");
    assert!(
        other.details()["resumable"].is_null() && other.details()["reason"].is_null(),
        "another member learns nothing about the run: {}",
        other.body
    );

    let resumed = edge
        .post(&person, "/v1/agent/runs", json!({ "resume_run_id": run }))
        .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    assert_eq!(resumed.data()["run_id"], run.as_str());
    assert_eq!(edge.driver.acquired(), vec![run.clone(), run.clone()]);

    let after = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(after.status, 200, "the resumed run steps: {}", after.body);

    let stolen = edge
        .post(
            &other_member_token(),
            "/v1/agent/runs",
            json!({ "resume_run_id": run }),
        )
        .await;
    assert_eq!(
        stolen.status, 404,
        "only the owner resumes: {}",
        stolen.body
    );
    assert_eq!(stolen.message(), "agent run not found");
}

/// A dry run logs what it would release and releases nothing; the run keeps
/// working. This is how the first deploy runs.
#[tokio::test]
async fn a_dry_run_sweep_reports_would_expire_and_releases_nothing() {
    let limits = RunLimits {
        sweeper_dry_run: true,
        ..idle_limits(60)
    };
    let edge = Edge::start(limits).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;

    let report = sweep_once(&edge.state, later(120_000)).await;
    assert_eq!(
        report,
        SweepReport {
            expired: 0,
            would_expire: 1
        }
    );
    assert!(edge.driver.released().is_empty());

    let projection = edge.projection(&person, &run).await;
    assert_eq!(projection.status, 200, "{}", projection.body);
    assert_eq!(projection.data()["live"], true);
    let step = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(step.status, 200, "{}", step.body);
}

/// An action in flight is never expired, however long the run has been quiet; once
/// it finishes the run is expired on the next pass. This checks the guard itself;
/// `a_step_queued_on_the_run_lock_is_in_flight_and_holds_the_run` checks that the
/// step handler takes it. The socket path has its own test,
/// `a_socket_action_queued_on_the_run_lock_holds_the_run_past_the_backstop`.
#[tokio::test]
async fn an_action_in_flight_is_never_expired() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let handle = edge.live(&run).expect("the run is live");

    let in_flight = handle.live.in_flight_guard();
    let held = sweep_once(&edge.state, later(120_000)).await;
    assert_eq!(held.expired, 0, "an action in flight holds the run");
    assert!(edge.driver.released().is_empty());

    drop(in_flight);
    let released = sweep_once(&edge.state, later(120_000)).await;
    assert_eq!(
        released.expired, 1,
        "the run expires once the action is done"
    );
    assert_eq!(edge.driver.released(), vec![run]);
}

/// An action in flight is never expired on the step path itself. A step counts itself
/// in flight before it waits for the run's lock (`step.rs`), so while a second step is
/// queued behind a running one, neither the verdict nor a sweep releases the run. When
/// the queued step has finished, the next sweep expires the run. This test fails if the
/// step handler stops counting itself in flight before it takes the lock.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_step_queued_on_the_run_lock_is_in_flight_and_holds_the_run() {
    let limits = idle_limits(60);
    let edge = Arc::new(Edge::start(limits.clone()).await);
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let handle = edge.live(&run).expect("the run is live");

    // A running action holds the run's lock, and a second step queues behind it.
    let running = handle.entry.lock().await;
    let queued = tokio::spawn({
        let edge = edge.clone();
        let person = person.clone();
        let run = run.clone();
        async move { edge.step(&person, &run, wait_action(), None).await }
    });
    let counted = wait_for(|| handle.live.in_flight() == 1).await;
    assert!(
        counted,
        "a step queued on the run lock must count itself in flight before it waits for the lock \
         (step() takes the in-flight guard first); in_flight is {}",
        handle.live.in_flight()
    );

    // Two minutes of silence against a one-minute limit, and the run is still held.
    assert_eq!(
        handle.live.verdict(later(120_000), &limits),
        None,
        "a queued step is in flight"
    );
    let held = sweep_once(&edge.state, later(120_000)).await;
    assert_eq!(
        held,
        SweepReport::default(),
        "a sweep leaves a queued run alone"
    );
    assert!(edge.driver.released().is_empty());

    // The running action ends. The queued step then runs, and the run is idle from there.
    drop(running);
    let reply = queued.await.expect("queued step");
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(
        handle.live.in_flight(),
        0,
        "the queued step has let go of the run"
    );

    let released = sweep_once(&edge.state, later(120_000)).await;
    assert_eq!(
        released.expired, 1,
        "the run expires once its last action is done"
    );
    assert_eq!(edge.driver.released(), vec![run]);
}

/// An action from a person's browser socket counts itself in flight before it waits for
/// the run's lock (`execute_ws_action`), as a step does. The open socket is a viewer, and a
/// viewer does not stop the absolute backstop, so past the backstop only the in-flight guard
/// holds the run while the socket action is queued. This fails if the socket path stops
/// counting itself in flight.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_socket_action_queued_on_the_run_lock_holds_the_run_past_the_backstop() {
    let limits = idle_limits(60);
    let edge = Edge::start(limits.clone()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let handle = edge.live(&run).expect("the run is live");
    let mut socket =
        TestSocket::connect(&edge, &person, &format!("/v1/agent/runs/{run}/frames/ws")).await;
    assert_eq!(handle.live.viewers(), 1, "the open socket is a viewer");

    // A running action holds the run's lock, and a socket action queues behind it.
    let running = handle.entry.lock().await;
    socket
        .send_text(&json!({ "type": "action", "action": wait_action() }).to_string())
        .await;
    let counted = wait_for(|| handle.live.in_flight() == 1).await;
    assert!(
        counted,
        "a socket action queued on the run lock must count itself in flight before it waits \
         for the lock (execute_ws_action takes the in-flight guard first); in_flight is {}",
        handle.live.in_flight()
    );

    // Past the 14 400 s backstop, which the open viewer does not stop, the run is still held.
    let past_backstop = later(14_500_000);
    assert_eq!(
        handle.live.verdict(past_backstop, &limits),
        None,
        "a socket action queued on the run lock is in flight"
    );
    let held = sweep_once(&edge.state, past_backstop).await;
    assert_eq!(
        held,
        SweepReport::default(),
        "a sweep leaves a run with a socket action queued alone"
    );
    assert!(edge.driver.released().is_empty());

    // The running action ends. The queued socket action then runs and is answered.
    drop(running);
    let answered = socket
        .next_matching(|message| message["type"] == "observation" || message["type"] == "error")
        .await;
    assert_eq!(answered["type"], "observation", "{answered}");
    assert_eq!(
        handle.live.in_flight(),
        0,
        "the socket action has let go of the run"
    );

    // Nothing is in flight now and the run is past its backstop, so the next sweep releases it.
    let released = sweep_once(&edge.state, past_backstop).await;
    assert_eq!(
        released.expired, 1,
        "the run expires once its last action is done"
    );
    assert_eq!(edge.driver.released(), vec![run]);
    drop(socket);
}

/// A person watching the run (an open event stream) keeps an idle session alive,
/// but not past the absolute backstop, which applies even with a viewer open.
#[tokio::test]
async fn a_viewer_holds_an_idle_session_until_the_absolute_backstop() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let watching = edge
        .open_stream(&person, &format!("/v1/agent/runs/{run}/events/stream"))
        .await;

    let blocked = sweep_once(&edge.state, later(120_000)).await;
    assert_eq!(blocked.expired, 0, "a viewer blocks the idle limit");
    assert!(edge.driver.released().is_empty());

    // 14 500 s is past the 14 400 s backstop for a person's session.
    let backstop = sweep_once(&edge.state, later(14_500_000)).await;
    assert_eq!(
        backstop.expired, 1,
        "the backstop applies with a viewer open"
    );
    assert_eq!(edge.driver.released(), vec![run]);
    drop(watching);
}

/// Closing a run releases its session even while another request still holds a
/// reference to it, and the held run is marked closed so a waiting action is refused.
#[tokio::test]
async fn closing_a_run_releases_it_even_while_another_request_holds_it() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let held = edge.live(&run).expect("the run is live");

    let closed = edge.delete(&person, &format!("/v1/agent/runs/{run}")).await;
    assert_eq!(closed.status, 200, "{}", closed.body);
    assert_eq!(closed.data()["closed"], true);
    assert_eq!(edge.driver.released(), vec![run.clone()]);
    assert_eq!(edge.state.agent_runs.stats().live, 0);
    assert!(
        held.entry.lock().await.closed,
        "the held run is marked closed"
    );
    drop(held);
}

/// A person who takes the browser and then goes quiet keeps it for the takeover idle
/// limit (1 800 s by default), longer than an ordinary session's 900 s, and only then
/// is it released, with reason `takeover_idle`.
#[tokio::test]
async fn a_run_held_in_a_takeover_expires_on_the_takeover_idle_limit() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let takeover = edge
        .control(&person, &run, json!({ "mode": "human_takeover" }))
        .await;
    assert_eq!(takeover.status, 200, "{}", takeover.body);

    // 1 000 s is past the 900 s an ordinary session may sit idle, inside the takeover limit.
    let held = sweep_once(&edge.state, later(1_000_000)).await;
    assert_eq!(
        held,
        SweepReport::default(),
        "a takeover outlives the ordinary idle limit"
    );
    assert!(edge.driver.released().is_empty());

    let released = sweep_once(&edge.state, later(1_900_000)).await;
    assert_eq!(
        released,
        SweepReport {
            expired: 1,
            would_expire: 0
        }
    );
    assert_eq!(edge.driver.released(), vec![run]);
    let expired = expired_payloads(&edge).await;
    assert_eq!(expired.len(), 1, "{expired:?}");
    assert_eq!(expired[0]["reason"], "takeover_idle");
}

/// An agent run that asks for a runtime lives that long plus the grace (100 s and 30 s
/// here), whatever its idle limit says, and is then released with reason `ttl`.
#[tokio::test]
async fn an_agent_run_lives_the_runtime_it_asked_for_plus_the_grace() {
    let limits = RunLimits {
        agent_idle_ttl_s: 10_000,
        max_concurrent_runs: 0,
        max_runs_per_org: 0,
        max_interactive_runs_per_org: 0,
        ..RunLimits::default()
    };
    let edge = Edge::start(limits).await;
    let service = service_token();
    let run = edge
        .start_run(
            &service,
            json!({ "constraints": { "max_steps": 0, "max_runtime_s": 100 } }),
        )
        .await;

    // 120 s is past the 100 s asked for, inside the 130 s the grace allows.
    let held = sweep_once(&edge.state, later(120_000)).await;
    assert_eq!(
        held,
        SweepReport::default(),
        "the grace is part of the lifetime"
    );
    assert!(edge.driver.released().is_empty());

    let released = sweep_once(&edge.state, later(140_000)).await;
    assert_eq!(
        released,
        SweepReport {
            expired: 1,
            would_expire: 0
        }
    );
    assert_eq!(edge.driver.released(), vec![run]);
    let expired = expired_payloads(&edge).await;
    assert_eq!(expired.len(), 1, "{expired:?}");
    assert_eq!(expired[0]["reason"], "ttl");
}

/// Two sweeps that reach the same idle run at the same moment release it once: one
/// browser release and one `agent.expired`, whichever sweep gets there first. The other
/// either misses the run or finds it already closed and releases nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_overlapping_sweeps_release_an_idle_run_once() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;

    let now = later(120_000);
    let first = tokio::spawn({
        let state = edge.state.clone();
        async move { sweep_once(&state, now).await }
    });
    let second = tokio::spawn({
        let state = edge.state.clone();
        async move { sweep_once(&state, now).await }
    });
    first.await.expect("first sweep");
    second.await.expect("second sweep");

    assert!(
        edge.live(&run).is_none(),
        "the registry no longer holds the run"
    );
    assert_eq!(
        edge.driver.released(),
        vec![run.clone()],
        "released exactly once"
    );
    let expired = expired_payloads(&edge).await;
    assert_eq!(expired.len(), 1, "one agent.expired: {expired:?}");
}

/// A resumed run keeps the stricter of its stored and derived class, for the budget
/// (budget.rs) and for the sweeper alike (`resumed_run_class`). A person who resumes an
/// agent run without the `initiated_by: "agent"` hint is swept on the agent's 60 s idle
/// limit, not the interactive 900 s (finding F5-2: the sweeper's class was built from the
/// request alone).
#[tokio::test]
async fn a_resumed_agent_run_keeps_the_agent_lifetime() {
    let limits = RunLimits {
        agent_idle_ttl_s: 60,
        interactive_idle_ttl_s: 900,
        max_concurrent_runs: 0,
        max_runs_per_org: 0,
        max_interactive_runs_per_org: 0,
        ..RunLimits::default()
    };
    let edge = Edge::start(limits).await;
    let person = member_token();
    let run = edge
        .start_run(&person, json!({ "initiated_by": "agent" }))
        .await;
    let step = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(step.status, 200, "{}", step.body);
    let first = sweep_once(&edge.state, later(120_000)).await;
    assert_eq!(first.expired, 1, "the agent run is released at 60 s idle");

    let resumed = edge
        .post(&person, "/v1/agent/runs", json!({ "resume_run_id": run }))
        .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);

    let second = sweep_once(&edge.state, later(120_000)).await;
    assert_eq!(
        second.expired, 1,
        "the resumed run keeps its agent lifetime, so 120 s idle releases it"
    );
}

/// The other direction of `resumed_run_class`: a run checkpointed as interactive that
/// is resumed with the `initiated_by: "agent"` hint is swept as an agent run, because
/// the derived class is the stricter. Keeping only the stored class would leave it on
/// the interactive 900 s idle limit.
#[tokio::test]
async fn a_stored_interactive_run_resumed_by_an_agent_is_swept_as_an_agent_run() {
    let limits = RunLimits {
        agent_idle_ttl_s: 60,
        interactive_idle_ttl_s: 900,
        max_concurrent_runs: 0,
        max_runs_per_org: 0,
        max_interactive_runs_per_org: 0,
        ..RunLimits::default()
    };
    let edge = Edge::start(limits).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let step = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(step.status, 200, "{}", step.body);
    let idle_past_interactive = sweep_once(&edge.state, later(1_000_000)).await;
    assert_eq!(
        idle_past_interactive.expired, 1,
        "the interactive run is released at 900 s idle"
    );

    let resumed = edge
        .post(
            &person,
            "/v1/agent/runs",
            json!({ "resume_run_id": run, "initiated_by": "agent" }),
        )
        .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);

    let swept = sweep_once(&edge.state, later(120_000)).await;
    assert_eq!(
        swept.expired, 1,
        "the resume is an agent's, so the agent's 60 s idle limit applies, not the interactive 900 s"
    );
}

/// A step that is past the registry lookup and waiting for the run's lock when the
/// sweeper releases the run gets the 404 `run_not_live` with the `resumable` hint a step
/// on a run already gone carries (finding F5-3: it used to get the reason alone). The
/// hint is the checkpoint's: see `a_call_queued_behind_the_owners_close_is_not_offered_a_resume`
/// for the owner's close, and `a_tab_call_queued_behind_an_expiry_gets_the_resume_hint`
/// for the tab calls.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_step_queued_behind_an_expiry_gets_the_resume_hint() {
    let edge = Arc::new(Edge::start(idle_limits(60)).await);
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let handle = edge.live(&run).expect("the run is live");

    // An action is running: hold the run's lock while a second step arrives.
    let mut running = handle.entry.lock().await;
    let queued = tokio::spawn({
        let edge = edge.clone();
        let person = person.clone();
        let run = run.clone();
        async move { edge.step(&person, &run, wait_action(), None).await }
    });
    let counted = wait_for(|| handle.live.in_flight() == 1).await;
    assert!(
        counted,
        "a step queued on the run lock must count itself in flight before it waits for the lock \
         (step() takes the in-flight guard first); in_flight is {}",
        handle.live.in_flight()
    );

    // The sweeper releases the run meanwhile: closed, out of the registry, and its
    // checkpoint left active, as an expiry leaves it.
    running.closed = true;
    edge.state
        .agent_runs
        .lock()
        .expect("agent_runs mutex")
        .remove(&run);
    drop(running);

    let reply = queued.await.expect("queued step");
    assert_eq!(reply.status, 404, "{}", reply.body);
    assert_eq!(reply.details()["reason"], "run_not_live", "{}", reply.body);
    assert_eq!(reply.details()["resumable"], true, "{}", reply.body);
    assert_eq!(
        reply.details()["resume_run_id"],
        run.as_str(),
        "{}",
        reply.body
    );
}

// ── tab calls: the in-flight guard, and a call queued behind a release ──────────

const TAB_CALLS: [&str; 3] = ["open", "select", "close"];

/// Open a tab on the run and return its id.
async fn a_tab_id(edge: &Edge, person: &str, run: &str) -> String {
    let opened = edge.new_tab(person, run, None).await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    opened.data()["tab"]["tabId"]
        .as_str()
        .expect("the new tab has an id")
        .to_owned()
}

/// The verb, route and body of a tab call (`open`, `select` or `close`).
fn tab_call(which: &str, run: &str, tab_id: &str) -> (reqwest::Method, String, Option<Value>) {
    match which {
        "open" => (
            reqwest::Method::POST,
            format!("/v1/agent/runs/{run}/tabs"),
            Some(json!({})),
        ),
        "select" => (
            reqwest::Method::POST,
            format!("/v1/agent/runs/{run}/tabs/{tab_id}/select"),
            None,
        ),
        "close" => (
            reqwest::Method::DELETE,
            format!("/v1/agent/runs/{run}/tabs/{tab_id}"),
            None,
        ),
        other => panic!("not a tab call: {other}"),
    }
}

/// Send `call` on its own task, so it can queue on the run's lock while the test holds it.
fn spawn_call(
    edge: &Arc<Edge>,
    person: &str,
    (method, route, body): (reqwest::Method, String, Option<Value>),
) -> tokio::task::JoinHandle<Reply> {
    let edge = edge.clone();
    let person = person.to_owned();
    tokio::spawn(async move { edge.send(method, &person, &route, body).await })
}

/// A tab call counts itself in flight before it waits for the run's lock, as a step
/// does (`tabs.rs`), so while one is queued behind a running action neither the verdict
/// nor a sweep releases the run. When it has finished, the next sweep expires the run.
/// This fails if a tab handler stops counting itself in flight before it takes the lock.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tab_call_queued_on_the_run_lock_is_in_flight_and_holds_the_run() {
    for which in TAB_CALLS {
        let limits = idle_limits(60);
        let edge = Arc::new(Edge::start(limits.clone()).await);
        let person = member_token();
        let run = edge.start_run(&person, json!({})).await;
        let tab_id = a_tab_id(&edge, &person, &run).await;
        let handle = edge.live(&run).expect("the run is live");

        // A running action holds the run's lock, and the tab call queues behind it.
        let running = handle.entry.lock().await;
        let queued = spawn_call(&edge, &person, tab_call(which, &run, &tab_id));
        let counted = wait_for(|| handle.live.in_flight() == 1).await;
        assert!(
            counted,
            "a tab {which} queued on the run lock must count itself in flight before it waits \
             for the lock (the tab handlers take the in-flight guard first); in_flight is {}",
            handle.live.in_flight()
        );

        assert_eq!(
            handle.live.verdict(later(120_000), &limits),
            None,
            "a queued tab {which} is in flight"
        );
        let held = sweep_once(&edge.state, later(120_000)).await;
        assert_eq!(
            held,
            SweepReport::default(),
            "a sweep leaves a run with a queued tab {which} alone"
        );
        assert!(edge.driver.released().is_empty());

        drop(running);
        let reply = queued.await.expect("queued tab call");
        assert_eq!(reply.status, 200, "tab {which}: {}", reply.body);
        assert_eq!(
            handle.live.in_flight(),
            0,
            "the tab {which} has let go of the run"
        );
        let released = sweep_once(&edge.state, later(120_000)).await;
        assert_eq!(
            released.expired, 1,
            "the run expires once its last call is done (tab {which})"
        );
        assert_eq!(edge.driver.released(), vec![run]);
    }
}

/// O5: a tab call counts as in flight only for a caller who owns the run. The ownership check
/// is the lock-free one a step uses and it comes first, so a stranger's call is the 404 at
/// once, is not counted, and cannot keep the run from expiring while it would have queued on
/// the run lock. Before, the count was taken first and a stranger's queued call held it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tab_call_from_someone_who_does_not_own_the_run_is_not_counted_in_flight() {
    for which in TAB_CALLS {
        let limits = idle_limits(60);
        let edge = Arc::new(Edge::start(limits.clone()).await);
        let person = member_token();
        let run = edge.start_run(&person, json!({})).await;
        let tab_id = a_tab_id(&edge, &person, &run).await;
        let handle = edge.live(&run).expect("the run is live");

        // A running action holds the run's lock. The owner's call would queue on it; the
        // stranger's must not wait for it at all.
        let running = handle.entry.lock().await;
        for stranger in [stranger_token(), other_member_token()] {
            let queued = spawn_call(&edge, &stranger, tab_call(which, &run, &tab_id));
            let reply = tokio::time::timeout(Duration::from_secs(10), queued)
                .await
                .unwrap_or_else(|_| panic!("a stranger's tab {which} waited on the run lock"))
                .expect("the call task");
            assert_eq!(reply.status, 404, "tab {which}: {}", reply.body);
            assert_eq!(
                handle.live.in_flight(),
                0,
                "a stranger's tab {which} is not in flight on someone else's run"
            );
        }
        assert_eq!(
            handle.live.verdict(later(120_000), &limits).is_some(),
            true,
            "the run still expires: nothing is holding it (tab {which})"
        );
        drop(running);
    }
}

/// Queue `call` on the run's lock, then release the run under it as the sweeper does
/// (closed, out of the registry, the checkpoint left active). With `explicit` the
/// owner's close also closes the checkpoint, as `DELETE /v1/agent/runs/{id}` does.
/// Returns the reply the queued call got.
async fn call_queued_behind_a_release(
    call: impl FnOnce(&str, &str) -> (reqwest::Method, String, Option<Value>),
    explicit: bool,
) -> Reply {
    let edge = Arc::new(Edge::start(idle_limits(60)).await);
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let tab_id = a_tab_id(&edge, &person, &run).await;
    let handle = edge.live(&run).expect("the run is live");

    let mut running = handle.entry.lock().await;
    let queued = spawn_call(&edge, &person, call(&run, &tab_id));
    let counted = wait_for(|| handle.live.in_flight() == 1).await;
    assert!(
        counted,
        "the call must count itself in flight before it waits for the run lock; in_flight is {}",
        handle.live.in_flight()
    );

    running.closed = true;
    edge.state
        .agent_runs
        .lock()
        .expect("agent_runs mutex")
        .remove(&run);
    if explicit {
        edge.state
            .receipts
            .close_run_checkpoint(MEMBER_ORG, &kind(&run))
            .await
            .expect("the owner's close closes the checkpoint");
    }
    drop(running);
    queued.await.expect("queued call")
}

/// A tab call queued behind an expiry gets the same hint a step does: `run_not_live`
/// with `resumable` and the id to resume, because the expiry left the checkpoint active.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tab_call_queued_behind_an_expiry_gets_the_resume_hint() {
    for which in TAB_CALLS {
        let reply =
            call_queued_behind_a_release(|run, tab_id| tab_call(which, run, tab_id), false).await;
        assert_eq!(reply.status, 404, "tab {which}: {}", reply.body);
        assert_eq!(reply.details()["reason"], "run_not_live", "{}", reply.body);
        assert_eq!(
            reply.details()["resumable"],
            true,
            "tab {which}: {}",
            reply.body
        );
        assert!(
            reply.details()["resume_run_id"].as_str().is_some(),
            "tab {which}: {}",
            reply.body
        );
    }
}

/// The hint follows the checkpoint, not the run's closed flag: when the owner's own
/// close ended the run, its checkpoint is closed and the run cannot be resumed, so a
/// step or tab call queued behind that close is told the run is not live and nothing
/// more.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_call_queued_behind_the_owners_close_is_not_offered_a_resume() {
    let step = call_queued_behind_a_release(
        |run, _| {
            (
                reqwest::Method::POST,
                format!("/v1/agent/runs/{run}/step"),
                Some(json!({ "action": wait_action() })),
            )
        },
        true,
    )
    .await;
    assert_eq!(step.status, 404, "{}", step.body);
    assert_eq!(step.details()["reason"], "run_not_live", "{}", step.body);
    assert!(step.details().get("resumable").is_none(), "{}", step.body);

    for which in TAB_CALLS {
        let reply =
            call_queued_behind_a_release(|run, tab_id| tab_call(which, run, tab_id), true).await;
        assert_eq!(reply.status, 404, "tab {which}: {}", reply.body);
        assert_eq!(reply.details()["reason"], "run_not_live", "{}", reply.body);
        assert!(
            reply.details().get("resumable").is_none(),
            "tab {which}: {}",
            reply.body
        );
    }
}

/// A resumed agent run keeps the lifetime its stored constraints set, not the one the
/// resume request names: the stored `max_runtime_s` is authoritative (a resume cannot
/// widen it), and the sweeper reads it, not the request's.
#[tokio::test]
async fn a_resumed_agent_run_keeps_its_stored_runtime_not_the_resume_requests() {
    let limits = RunLimits {
        agent_idle_ttl_s: 100_000,
        agent_ttl_grace_s: 5,
        agent_max_ttl_s: 100_000,
        interactive_idle_ttl_s: 100_000,
        max_concurrent_runs: 0,
        max_runs_per_org: 0,
        max_interactive_runs_per_org: 0,
        ..RunLimits::default()
    };
    let edge = Edge::start(limits).await;
    let person = member_token();
    let run = edge
        .start_run(
            &person,
            json!({
                "initiated_by": "agent",
                "constraints": { "max_steps": 0, "max_runtime_s": 30 },
            }),
        )
        .await;
    let first = sweep_once(&edge.state, later(120_000)).await;
    assert_eq!(first.expired, 1, "30 s plus the grace is over at 120 s");

    let resumed = edge
        .post(
            &person,
            "/v1/agent/runs",
            json!({
                "resume_run_id": run,
                "initiated_by": "agent",
                "constraints": { "max_steps": 0, "max_runtime_s": 3000 },
            }),
        )
        .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);

    let early = sweep_once(&edge.state, later(20_000)).await;
    assert_eq!(
        early.expired, 0,
        "20 s is inside the stored 30 s plus grace"
    );
    let late = sweep_once(&edge.state, later(40_000)).await;
    assert_eq!(
        late.expired, 1,
        "40 s is past the stored 30 s plus grace; the request's 3000 s does not apply"
    );
}

/// The shutdown drain through the real registry and the recording driver: every idle run's
/// browser session is released (the driver sees the release, once, for that run), the
/// registry is emptied, and a run an action is still running on is neither released nor
/// counted as released (the browser dies with the process).
#[tokio::test]
async fn the_shutdown_drain_releases_every_idle_runs_session_and_leaves_a_busy_one() {
    let edge = Edge::start(idle_limits(600)).await;
    let person = member_token();
    let first = edge.start_run(&person, json!({})).await;
    let second = edge.start_run(&person, json!({})).await;
    let busy = edge.start_run(&person, json!({})).await;

    // The busy run's navigation holds the run lock until this test opens the gate, so the
    // lock is held for the whole drain however slow the machine is: the drain is started only
    // once the driver reports it is inside the navigation, and gives it 300 ms.
    let gate = edge.driver.time_out_next_goto_when_released();
    let stuck_step = edge.step(
        &person,
        &busy,
        json!({ "type": "navigate", "url": "https://example.test/slow" }),
        None,
    );
    let draining = async {
        edge.driver.wait_goto_entered(1, "the busy run's navigation").await;
        let report = drain_runs(&edge.state.agent_runs, &edge.state.agent_driver, Duration::from_millis(300)).await;
        gate.notify_one();
        report
    };
    let (stuck, report) = tokio::join!(stuck_step, draining);

    assert_eq!(
        report,
        DrainReport { released: 2, failed: 0, in_use: 1 },
        "two idle runs released, one left to the exit"
    );
    let mut released = edge.driver.released();
    released.sort();
    let mut expected = vec![first.clone(), second.clone()];
    expected.sort();
    assert_eq!(released, expected, "the driver saw exactly the idle runs' releases, once each");
    assert!(
        !edge.driver.released().contains(&busy),
        "the run an action was still running on keeps its session"
    );
    assert!(edge.live(&first).is_none() && edge.live(&second).is_none(), "the registry is empty");
    assert_eq!(stuck.status, 504, "the stuck navigation still ends with its own typed timeout");
}
