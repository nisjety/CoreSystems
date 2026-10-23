//! Resume of the agent run's typed event stream (WS10.S3), through the real router and
//! the real token verification (the harness is in `support/agent_harness.rs`).
//!
//! The rules under test: an SSE reconnect with `Last-Event-ID` (or `?from_seq=`, which
//! wins) replays the run's replay ring after that seq and then tails the live channel;
//! the SSE `id:` is the run's `seq`; a resume older than the ring gets `replay_gap`
//! first; a non-owner gets a 404 and no replay; a closed run ends its stream with
//! `done` / `run_closed`.
//!
//! The events in these tests are emitted straight into the edge's event sink: the
//! driver double never performs an action, so the sink is the only source of
//! `action.*` events here.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use agent_harness::*;
use quarry_core::event::EventType;
use quarry_edge::run_limits::RunLimits;
use quarry_runtime::RingLimits;
use serde_json::json;
use serial_test::serial;

/// The `id:` values of an SSE body, in order.
fn sse_ids(body: &str) -> Vec<u64> {
    body.lines()
        .filter_map(|line| line.strip_prefix("id:"))
        .filter_map(|id| id.trim().parse().ok())
        .collect()
}

/// Emit `count` action events for `run`, numbered from `from`.
async fn emit_actions(edge: &Edge, run: &str, from: u64, count: u64) {
    for n in from..from + count {
        edge.state
            .event_sink
            .emit_ephemeral(
                kind(run),
                EventType::ActionStarted,
                json!({ "n": n }),
                format!("{run}:action:{n}"),
            )
            .await;
    }
}

fn events_route(run: &str) -> String {
    format!("/v1/agent/runs/{run}/events/stream")
}

/// A reconnect with `Last-Event-ID: 3` gets seq 4 onward, consecutive, then the live
/// tail, and the close ends the stream.
#[tokio::test]
#[serial]
async fn reconnect_with_last_event_id_replays_missed_events_then_tails() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    // seq 1 is agent.started; the five actions are seq 2..=6.
    emit_actions(&edge, &run, 1, 5).await;

    let response = edge
        .client
        .get(format!("http://{}{}", edge.addr, events_route(&run)))
        .bearer_auth(&person)
        .header("last-event-id", "3")
        .send()
        .await
        .expect("stream request reaches the edge");
    assert_eq!(response.status().as_u16(), 200);
    // The subscription exists once the headers are back; the close now reaches it.
    let closed = edge.delete(&person, &format!("/v1/agent/runs/{run}")).await;
    assert_eq!(closed.status, 200, "{}", closed.body);

    let body = response.text().await.expect("the stream ends after done");
    assert_eq!(sse_ids(&body), vec![4, 5, 6, 7], "{body}");
    assert!(body.contains("event: done"), "{body}");
    assert!(body.contains("data: run_closed"), "{body}");
}

/// `?from_seq=5` wins over `Last-Event-ID: 1`.
#[tokio::test]
#[serial]
async fn from_seq_wins_over_last_event_id() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    emit_actions(&edge, &run, 1, 5).await;

    let response = edge
        .client
        .get(format!(
            "http://{}{}?from_seq=5",
            edge.addr,
            events_route(&run)
        ))
        .bearer_auth(&person)
        .header("last-event-id", "1")
        .send()
        .await
        .expect("stream request reaches the edge");
    assert_eq!(response.status().as_u16(), 200);
    let closed = edge.delete(&person, &format!("/v1/agent/runs/{run}")).await;
    assert_eq!(closed.status, 200, "{}", closed.body);

    let body = response.text().await.expect("the stream ends after done");
    assert_eq!(sse_ids(&body), vec![6, 7], "{body}");
}

/// A resume older than the ring is told so first: `replay_gap` names the requested
/// seq and the oldest seq the ring still holds, and it points at the durable timeline.
#[tokio::test]
#[serial]
async fn replay_gap_is_sent_first_when_the_ring_no_longer_holds_the_resume_point() {
    let edge = Edge::start_with(RunLimits::default(), |mut state| {
        state.event_sink = state.event_sink.with_ring_limits(RingLimits {
            max_events: 2,
            max_bytes: usize::MAX,
        });
        state
    })
    .await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    emit_actions(&edge, &run, 1, 5).await;

    let response = edge
        .client
        .get(format!("http://{}{}", edge.addr, events_route(&run)))
        .bearer_auth(&person)
        .header("last-event-id", "0")
        .send()
        .await
        .expect("stream request reaches the edge");
    assert_eq!(response.status().as_u16(), 200);
    let closed = edge.delete(&person, &format!("/v1/agent/runs/{run}")).await;
    assert_eq!(closed.status, 200, "{}", closed.body);

    let body = response.text().await.expect("the stream ends after done");
    let gap_at = body.find("event: replay_gap").expect("a replay_gap event");
    let first_event_at = body.find("event: action.started").unwrap_or(usize::MAX);
    assert!(gap_at < first_event_at, "{body}");
    assert!(body.contains("\"requested\":0"), "{body}");
    assert!(body.contains("browser-session/timeline"), "{body}");
}

/// A stranger gets the same 404 as an unknown run, and no replay.
#[tokio::test]
#[serial]
async fn a_non_owner_gets_404_not_a_replay() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    emit_actions(&edge, &run, 1, 2).await;

    let response = edge
        .client
        .get(format!("http://{}{}", edge.addr, events_route(&run)))
        .bearer_auth(stranger_token())
        .header("last-event-id", "0")
        .send()
        .await
        .expect("stream request reaches the edge");
    assert_eq!(response.status().as_u16(), 404);
    let text = response.text().await.unwrap_or_default();
    assert!(!text.contains("event: action.started"), "{text}");
}

/// A stream that is open when its run closes ends with `done` / `run_closed`, after the
/// terminal event.
#[tokio::test]
#[serial]
async fn a_closed_run_stream_ends_with_done_run_closed() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;

    let response = edge
        .client
        .get(format!("http://{}{}", edge.addr, events_route(&run)))
        .bearer_auth(&person)
        .send()
        .await
        .expect("stream request reaches the edge");
    assert_eq!(response.status().as_u16(), 200);
    let closed = edge.delete(&person, &format!("/v1/agent/runs/{run}")).await;
    assert_eq!(closed.status, 200, "{}", closed.body);

    let body = response.text().await.expect("the stream ends after done");
    let completed_at = body.find("event: agent.completed").expect("agent.completed sent");
    let done_at = body.find("event: done").expect("done sent");
    assert!(completed_at < done_at, "{body}");
    assert!(body.contains("data: run_closed"), "{body}");
}

/// Source scan (WS10.S3 DoD): the events stream doc comment must not claim a
/// `quarry_event_history` table. Only the job history (`quarry_job_history`) exists.
#[test]
fn events_stream_doc_comment_names_quarry_event_history_absent() {
    let live = include_str!("../src/agent_routes/live.rs");
    assert!(
        !live.contains("quarry_event_history"),
        "agent_routes/live.rs must not name a quarry_event_history table"
    );
    assert!(live.contains("Last-Event-ID"));
}
