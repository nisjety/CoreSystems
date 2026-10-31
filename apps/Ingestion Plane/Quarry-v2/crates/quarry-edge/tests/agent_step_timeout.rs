//! WS3.D, the edge half: a step whose browser operation hits its deadline answers a typed
//! `504 TIMEOUT`, and the run is not left stuck behind it.
//!
//! The driver's own deadlines are tested in `quarry-browser` (`deadlines.rs` and the live
//! lane). What the edge owes is what the plan's deployment evidence asks for after a
//! navigate times out: the error reaches the caller with `op`, `deadline_ms` and
//! `effect: "unknown"` and is not retryable (the navigation may have started), and "the next
//! `/step` is accepted immediately": the run lock and the in-flight count are free, so the
//! caller can look at the page and carry on.
//!
//! The driver double fails its next `goto` the way the real deadline does
//! (`RecordingDriver::time_out_next_goto`), either at once or held until the test opens a gate
//! (`time_out_next_goto_when_released`): the second is the case that matters, a step whose
//! driver call is still running when the next step arrives. Nothing in these tests waits on a
//! clock for a state: the test waits for the driver to report it has entered `goto`, and the
//! held call ends only when the test says so.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use std::time::Duration;

use agent_harness::*;
use quarry_edge::run_limits::RunLimits;
use serde_json::json;

fn navigate() -> serde_json::Value {
    json!({ "type": "navigate", "url": "https://example.test/slow" })
}

#[tokio::test]
async fn a_navigation_that_times_out_is_a_typed_504_and_the_next_step_is_accepted_at_once() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;

    edge.driver.time_out_next_goto();
    let timed_out = edge.step(&person, &run, navigate(), None).await;
    assert_eq!(timed_out.status, 504, "{}", timed_out.body);
    assert_eq!(timed_out.code(), "TIMEOUT", "{}", timed_out.body);
    assert_eq!(timed_out.details()["op"], "navigate");
    assert_eq!(timed_out.details()["effect"], "unknown");
    assert_eq!(timed_out.details()["deadline_ms"], 30_000);
    assert_eq!(
        timed_out.body["error"]["retryable"], false,
        "a navigation that may have started is not retried blindly"
    );
    let rendered = timed_out.body.to_string();
    assert!(!rendered.contains("example.test"), "no URL in the error: {rendered}");

    // Nothing is left holding the run.
    let handle = edge.live(&run).expect("the run is still live");
    assert_eq!(handle.live.in_flight(), 0, "no action is still counted as running");
    let next = tokio::time::timeout(
        Duration::from_secs(2),
        edge.step(&person, &run, wait_action(), None),
    )
    .await
    .expect("the next step is not queued behind the timed-out one");
    assert_eq!(next.status, 200, "{}", next.body);

    // The run recovers: the same navigation now goes through.
    let again = edge.step(&person, &run, navigate(), None).await;
    assert_eq!(again.status, 200, "{}", again.body);

    // The history says what happened: one failed step, then the two that ran.
    let receipts = edge.receipts(&person, &run).await;
    let failed: Vec<_> = receipts
        .iter()
        .filter(|receipt| receipt["outcome"]["status"] == "failed")
        .collect();
    let completed = receipts
        .iter()
        .filter(|receipt| receipt["outcome"]["status"] == "completed")
        .count();
    assert_eq!(failed.len(), 1, "{receipts:?}");
    assert_eq!(completed, 2, "{receipts:?}");
}

/// The call is still running (holding the run lock and the in-flight count) when its deadline
/// fires, and a second step is already queued behind it. The 504 is delivered, the lock and
/// the count are released on that error path, and the queued step runs straight after.
///
/// No clock decides anything here. The navigation is held by a gate the test opens; the
/// test starts the second step only after the driver reports it is inside `goto`, shows that
/// the second step cannot finish while the gate is shut, and only then opens it.
#[tokio::test]
async fn a_step_queued_behind_a_navigation_that_is_stuck_runs_right_after_its_504() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let handle = edge.live(&run).expect("the run is live");

    let gate = edge.driver.time_out_next_goto_when_released();
    let first = edge.step(&person, &run, navigate(), None);
    let second = async {
        edge.driver.wait_goto_entered(1, "the stuck navigation").await;
        assert_eq!(handle.live.in_flight(), 1, "the stuck navigation is counted as running");

        let queued = edge.step(&person, &run, wait_action(), None);
        tokio::pin!(queued);
        let overtook = tokio::time::timeout(Duration::from_millis(300), &mut queued).await;
        assert!(
            overtook.is_err(),
            "the second step queued behind the stuck one instead of overtaking it"
        );
        // Both steps are admitted and counted; only the first one is inside the driver.
        assert_eq!(handle.live.in_flight(), 2, "the stuck navigation and the queued step are both counted");
        assert_eq!(edge.driver.goto_entered(), 1, "the queued step never reached the driver");

        gate.notify_one();
        queued.await
    };
    let (timed_out, next) = tokio::join!(first, second);

    assert_eq!(timed_out.status, 504, "{}", timed_out.body);
    assert_eq!(timed_out.code(), "TIMEOUT", "{}", timed_out.body);
    assert_eq!(timed_out.details()["op"], "navigate");
    assert_eq!(next.status, 200, "the queued step ran after the 504: {}", next.body);
    assert_eq!(handle.live.in_flight(), 0, "both steps have left the in-flight count");

    let receipts = edge.receipts(&person, &run).await;
    let failed = receipts.iter().filter(|r| r["outcome"]["status"] == "failed").count();
    let completed = receipts.iter().filter(|r| r["outcome"]["status"] == "completed").count();
    assert_eq!((failed, completed), (1, 1), "{receipts:?}");
}
