//! D2 (R3-SC4 depth 2): a run's step keys never repeat across a resume.
//!
//! A step key is reserved for every admitted step, completed or failed (WS8.H), while the
//! checkpoint's `step` counts completed steps only. A resumed run used to seed its keys from
//! `step`, so a run with a failed step handed a used key out again: live, the receipts of
//! one run showed step 1 twice (a completed wait before the resume, a failed wait after
//! it). Every checkpoint now stores the next key (`next_step`), a failed step saves one
//! too (its receipt may not be kept), and a resume starts at the highest of `next_step`,
//! `step` and one past the run's last receipt step (a key whose checkpoint save failed, or
//! a run checkpointed by older code), read as an aggregate so no receipt payload is
//! decoded. `step` keeps its meaning: the step cap counts completed steps only, as before.
//!
//! The browser is the recording driver: `time_out_next_goto` fails the next navigate. The
//! receipt store is the in-memory one behind [`Flaky`], which fails chosen writes and reads.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agent_harness::*;
use async_trait::async_trait;
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::ids::kinds::RunKind;
use quarry_edge::agent_routes::sweep_once;
use quarry_edge::run_limits::RunLimits;
use quarry_edge::state::AppState;
use quarry_runtime::step_receipts::{
    AgentRunCheckpoint, BrowserTimelineEvent, StepReceipt, StepReceiptStore,
};
use quarry_runtime::InMemoryStepReceiptStore;
use serde_json::{json, Value};

/// The in-memory receipt store, with switches that fail receipt writes, checkpoint saves or
/// receipt listings (a historical row that no longer decodes). The last receipt step is an
/// aggregate, as in Postgres, so a failing listing does not touch it.
#[derive(Default)]
struct Flaky {
    inner: InMemoryStepReceiptStore,
    fail_appends: AtomicBool,
    fail_checkpoints: AtomicBool,
    fail_listings: AtomicBool,
}

fn broken(what: &str) -> QuarryError {
    QuarryError::new(ErrorCode::Internal, format!("test store: {what} failed"))
}

#[async_trait]
impl StepReceiptStore for Flaky {
    async fn append(&self, receipt: StepReceipt) -> QuarryResult<()> {
        if self.fail_appends.load(Ordering::SeqCst) {
            return Err(broken("append"));
        }
        self.inner.append(receipt).await
    }

    async fn list(&self, run_id: &RunKind) -> QuarryResult<Vec<StepReceipt>> {
        if self.fail_listings.load(Ordering::SeqCst) {
            return Err(broken("decode receipt"));
        }
        self.inner.list(run_id).await
    }

    async fn last_step_for_org(&self, org_id: &str, run_id: &RunKind) -> QuarryResult<Option<u32>> {
        self.inner.last_step_for_org(org_id, run_id).await
    }

    async fn append_browser_timeline_event(&self, event: BrowserTimelineEvent) -> QuarryResult<()> {
        self.inner.append_browser_timeline_event(event).await
    }

    async fn list_browser_timeline_events(
        &self,
        org_id: &str,
        actor_id: &str,
        run_id: &RunKind,
    ) -> QuarryResult<Vec<BrowserTimelineEvent>> {
        self.inner
            .list_browser_timeline_events(org_id, actor_id, run_id)
            .await
    }

    async fn get(&self, receipt_id: &str) -> QuarryResult<Option<StepReceipt>> {
        self.inner.get(receipt_id).await
    }

    async fn save_run_checkpoint(&self, checkpoint: AgentRunCheckpoint) -> QuarryResult<()> {
        if self.fail_checkpoints.load(Ordering::SeqCst) {
            return Err(broken("checkpoint save"));
        }
        self.inner.save_run_checkpoint(checkpoint).await
    }

    async fn load_run_checkpoint(
        &self,
        org_id: &str,
        run_id: &RunKind,
    ) -> QuarryResult<Option<AgentRunCheckpoint>> {
        self.inner.load_run_checkpoint(org_id, run_id).await
    }

    async fn close_run_checkpoint(&self, org_id: &str, run_id: &RunKind) -> QuarryResult<()> {
        self.inner.close_run_checkpoint(org_id, run_id).await
    }
}

/// An edge whose receipt store is `store`.
async fn edge_on(store: Arc<Flaky>) -> Edge {
    Edge::start_with(idle_limits(60), move |state| AppState {
        receipts: store,
        ..state
    })
    .await
}

/// The org of the standard-org member (`org-standard-golden`).
const MEMBER_ORG: &str = "org-standard-golden";

fn run_on_example_test(max_steps: u32) -> Value {
    json!({ "constraints": { "max_steps": max_steps, "allowed_domains": ["example.test"] } })
}

fn navigate(url: &str) -> Value {
    json!({ "type": "navigate", "url": url })
}

/// Caps off, and both idle limits `idle_s`, so only the sweeper ends the run.
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

async fn checkpoint(edge: &Edge, run: &str) -> AgentRunCheckpoint {
    edge.state
        .receipts
        .load_run_checkpoint(MEMBER_ORG, &kind(run))
        .await
        .expect("checkpoint read")
        .expect("a non-ZDR run keeps its checkpoint")
}

/// Each receipt's `(step, action type, outcome)`, in the order the route lists them.
async fn keyed(edge: &Edge, person: &str, run: &str) -> Vec<(u64, String, String)> {
    edge.receipts(person, run)
        .await
        .iter()
        .map(|receipt| {
            (
                receipt["step"].as_u64().expect("a receipt step"),
                receipt["action"]["type"].as_str().unwrap_or("").to_owned(),
                receipt["outcome"]["status"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned(),
            )
        })
        .collect()
}

fn row(step: u64, action: &str, outcome: &str) -> (u64, String, String) {
    (step, action.to_owned(), outcome.to_owned())
}

async fn step_ok(edge: &Edge, person: &str, run: &str, action: Value) -> Value {
    let reply = edge.step(person, run, action, Some("agent")).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    reply.data().clone()
}

async fn navigate_times_out(edge: &Edge, person: &str, run: &str) {
    edge.driver.time_out_next_goto();
    let reply = edge
        .step(
            person,
            run,
            navigate("https://example.test/slow"),
            Some("agent"),
        )
        .await;
    assert_eq!(reply.status, 504, "{}", reply.body);
    assert_eq!(reply.code(), "TIMEOUT", "{}", reply.body);
}

/// The sweeper ends the idle run (its checkpoint stays `active`), then the owner resumes it.
async fn expire_and_resume(edge: &Edge, person: &str, run: &str) {
    assert_eq!(sweep_once(&edge.state, later(120_000)).await.expired, 1);
    let resumed = edge
        .post(person, "/v1/agent/runs", json!({ "resume_run_id": run }))
        .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
}

/// The live R3-SC4 RS sequence: a first-step TIMEOUT (key 0), a completed wait (key 1),
/// an idle expiry and a resume. The resumed run's steps take keys 2 and 3 (they took 1 and
/// 2: the wait after the resume reused key 1). The failed step after the resume uses its
/// own key too, and every checkpoint save carries the next key.
#[tokio::test]
async fn a_resume_after_a_failed_step_hands_out_new_step_keys() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = member_token();
    let run = edge.start_run(&person, run_on_example_test(0)).await;
    edge.driver.set_page_url(Some("https://example.test/"));

    navigate_times_out(&edge, &person, &run).await;
    step_ok(&edge, &person, &run, wait_action()).await;
    let saved = checkpoint(&edge, &run).await;
    assert_eq!(
        (saved.step, saved.next_step),
        (1, 2),
        "one completed step, two keys used"
    );

    expire_and_resume(&edge, &person, &run).await;
    navigate_times_out(&edge, &person, &run).await;
    let observed = step_ok(&edge, &person, &run, wait_action()).await;
    assert_eq!(
        observed["step"], 3,
        "the observation carries the run-wide key"
    );

    assert_eq!(
        keyed(&edge, &person, &run).await,
        vec![
            row(0, "navigate", "failed"),
            row(1, "wait", "completed"),
            row(2, "navigate", "failed"),
            row(3, "wait", "completed"),
        ]
    );
    let saved = checkpoint(&edge, &run).await;
    assert_eq!((saved.step, saved.next_step), (2, 4));
}

/// A failed step whose own checkpoint save fails is not in the checkpoint's `next_step`;
/// its receipt is, and the resume starts past it.
#[tokio::test]
async fn a_key_that_failed_after_the_last_checkpoint_is_not_reused() {
    let store = Arc::new(Flaky::default());
    let edge = edge_on(store.clone()).await;
    let person = member_token();
    let run = edge.start_run(&person, run_on_example_test(0)).await;
    edge.driver.set_page_url(Some("https://example.test/"));

    step_ok(&edge, &person, &run, wait_action()).await;
    store.fail_checkpoints.store(true, Ordering::SeqCst);
    navigate_times_out(&edge, &person, &run).await;
    store.fail_checkpoints.store(false, Ordering::SeqCst);
    assert_eq!(
        checkpoint(&edge, &run).await.next_step,
        1,
        "the failed step's checkpoint save did not land"
    );

    expire_and_resume(&edge, &person, &run).await;
    step_ok(&edge, &person, &run, wait_action()).await;
    assert_eq!(
        keyed(&edge, &person, &run).await,
        vec![
            row(0, "wait", "completed"),
            row(1, "navigate", "failed"),
            row(2, "wait", "completed"),
        ]
    );
}

/// A run checkpointed by older code has no `next_step` (it reads as 0): the resume starts
/// one past its last receipt, not at its count of completed steps. A checkpoint's
/// `next_step` beyond every receipt (keys whose receipts were not kept) is honoured too.
#[tokio::test]
async fn a_legacy_checkpoint_resumes_past_its_receipts_and_next_step_is_honoured() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = member_token();
    let run = edge.start_run(&person, run_on_example_test(0)).await;
    edge.driver.set_page_url(Some("https://example.test/"));

    navigate_times_out(&edge, &person, &run).await;
    navigate_times_out(&edge, &person, &run).await;
    step_ok(&edge, &person, &run, wait_action()).await;

    // What the older edge left: the same checkpoint without the field.
    assert_eq!(sweep_once(&edge.state, later(120_000)).await.expired, 1);
    let mut legacy = serde_json::to_value(checkpoint(&edge, &run).await).unwrap();
    legacy.as_object_mut().unwrap().remove("next_step");
    let legacy: AgentRunCheckpoint = serde_json::from_value(legacy).expect("a legacy checkpoint");
    assert_eq!((legacy.step, legacy.next_step), (1, 0));
    edge.state
        .receipts
        .save_run_checkpoint(legacy)
        .await
        .expect("checkpoint write");
    let resumed = edge
        .post(&person, "/v1/agent/runs", json!({ "resume_run_id": run }))
        .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    let observed = step_ok(&edge, &person, &run, wait_action()).await;
    assert_eq!(
        observed["step"], 3,
        "one past the last receipt (key 2), not the 1 completed step"
    );

    // A stored `next_step` past every receipt.
    assert_eq!(sweep_once(&edge.state, later(240_000)).await.expired, 1);
    let mut ahead = checkpoint(&edge, &run).await;
    ahead.next_step = 7;
    edge.state
        .receipts
        .save_run_checkpoint(ahead)
        .await
        .expect("checkpoint write");
    let resumed = edge
        .post(&person, "/v1/agent/runs", json!({ "resume_run_id": run }))
        .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    let observed = step_ok(&edge, &person, &run, wait_action()).await;
    assert_eq!(observed["step"], 7);

    let steps: Vec<u64> = keyed(&edge, &person, &run)
        .await
        .into_iter()
        .map(|(step, _, _)| step)
        .collect();
    assert_eq!(steps, vec![0, 1, 2, 3, 7], "unique and rising");
}

/// The step cap still counts completed steps only, across the resume: two failed
/// navigates and one completed wait leave a `max_steps: 2` run one more step, whose key is
/// 3; the step after it is refused, and the refusal takes no key.
#[tokio::test]
async fn max_steps_still_counts_completed_steps_across_a_resume() {
    let edge = Edge::start(idle_limits(60)).await;
    let person = member_token();
    let run = edge.start_run(&person, run_on_example_test(2)).await;
    edge.driver.set_page_url(Some("https://example.test/"));

    navigate_times_out(&edge, &person, &run).await;
    step_ok(&edge, &person, &run, wait_action()).await;
    navigate_times_out(&edge, &person, &run).await;

    expire_and_resume(&edge, &person, &run).await;
    let observed = step_ok(&edge, &person, &run, wait_action()).await;
    assert_eq!(observed["step"], 3);
    let refused = edge.step(&person, &run, wait_action(), Some("agent")).await;
    assert_eq!(
        refused.status, 429,
        "the cap of 2 completed steps: {}",
        refused.body
    );

    let saved = checkpoint(&edge, &run).await;
    assert_eq!(
        (saved.step, saved.next_step),
        (2, 4),
        "the refusal reserved no key"
    );
    let projection = edge.projection(&person, &run).await;
    assert_eq!(projection.status, 200, "{}", projection.body);
    assert_eq!(
        projection.data()["step"],
        2,
        "the projection counts completed steps: {}",
        projection.body
    );
    assert_eq!(keyed(&edge, &person, &run).await.len(), 4);
}

/// A failed step whose receipt is not kept (here its write fails; a person's step under
/// `HUMAN_RECEIPT_MODE=none` keeps none either) still spent its key: its checkpoint carries
/// the next one, so the resume does not hand it out again.
#[tokio::test]
async fn a_failed_step_without_a_receipt_still_checkpoints_its_key() {
    let store = Arc::new(Flaky::default());
    let edge = edge_on(store.clone()).await;
    let person = member_token();
    let run = edge.start_run(&person, run_on_example_test(0)).await;
    edge.driver.set_page_url(Some("https://example.test/"));

    step_ok(&edge, &person, &run, wait_action()).await;
    store.fail_appends.store(true, Ordering::SeqCst);
    edge.driver.time_out_next_goto();
    let failed = edge
        .step(
            &person,
            &run,
            navigate("https://example.test/slow"),
            Some("agent"),
        )
        .await;
    store.fail_appends.store(false, Ordering::SeqCst);
    assert_ne!(failed.status, 200, "{}", failed.body);
    let saved = checkpoint(&edge, &run).await;
    assert_eq!(
        (saved.step, saved.next_step),
        (1, 2),
        "the failed step's key is saved, its step is not counted"
    );

    expire_and_resume(&edge, &person, &run).await;
    let observed = step_ok(&edge, &person, &run, wait_action()).await;
    assert_eq!(observed["step"], 2, "key 1 was spent by the failed step");
    assert_eq!(
        keyed(&edge, &person, &run).await,
        vec![row(0, "wait", "completed"), row(2, "wait", "completed")]
    );
}

/// A resume reads only the run's last receipt step, never the receipt payloads: a run whose
/// listing fails (a historical row that no longer decodes) still resumes, past its keys.
#[tokio::test]
async fn a_resume_does_not_decode_the_run_receipts() {
    let store = Arc::new(Flaky::default());
    let edge = edge_on(store.clone()).await;
    let person = member_token();
    let run = edge.start_run(&person, run_on_example_test(0)).await;
    edge.driver.set_page_url(Some("https://example.test/"));

    navigate_times_out(&edge, &person, &run).await;
    step_ok(&edge, &person, &run, wait_action()).await;
    assert_eq!(sweep_once(&edge.state, later(120_000)).await.expired, 1);

    store.fail_listings.store(true, Ordering::SeqCst);
    let resumed = edge
        .post(&person, "/v1/agent/runs", json!({ "resume_run_id": run }))
        .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    let observed = step_ok(&edge, &person, &run, wait_action()).await;
    assert_eq!(observed["step"], 2);
    store.fail_listings.store(false, Ordering::SeqCst);
}
