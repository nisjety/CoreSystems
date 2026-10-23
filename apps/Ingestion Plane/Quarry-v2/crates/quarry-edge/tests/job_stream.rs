//! The live job-event tail (WS15.L3) through the real edge router, with a control
//! double that serves a fixed event log per job. Every stream in these tests
//! ends on its own (a terminal event, a terminal status, or the legacy drain),
//! so the bodies are read whole with `text()`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, Query},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use serial_test::serial;
use tokio::sync::mpsc;

use quarry_edge::job_stream::{self, Replay};
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};

mod support;

const TEST_BEARER: &str = "job-stream-dev-token";
const TEST_ORG: &str = "org_placeholder";

/// A job id for a fixture tag. The tags below are the only ones the control
/// double knows; any other tag is a foreign id and answers 404.
fn job_id(tag: char) -> String {
    format!("job_01J9{}", tag.to_string().repeat(22))
}

struct Fixture {
    kind: &'static str,
    status: &'static str,
    events: Vec<Value>,
}

/// `events(n, terminal)`: seq 1..=n, all `page_fetched`, the last one of the
/// given type when one is named.
fn events(n: u64, terminal: Option<&'static str>) -> Vec<Value> {
    (1..=n)
        .map(|seq| {
            let ty = match terminal {
                Some(t) if seq == n => t,
                _ => "page_fetched",
            };
            json!({
                "event_id": format!("evt_{seq}"),
                "type": ty,
                "seq": seq,
                "ts": "2026-10-23T10:00:00Z",
                "payload": { "pages_visited": seq },
                "idempotency_key": format!("key_{seq}"),
            })
        })
        .collect()
}

fn fixture_for(id: &str) -> Option<Fixture> {
    let tag = id.strip_prefix("job_01J9")?.chars().next()?;
    match tag {
        // Completed, five events, no terminal event in the log.
        'S' | 'R' => Some(Fixture { kind: "crawl", status: "completed", events: events(5, None) }),
        // Still running, but the log ends with a terminal event.
        'T' => Some(Fixture { kind: "crawl", status: "running", events: events(4, Some("run_completed")) }),
        // Cancelled with no terminal event: the record decides the end.
        'C' => Some(Fixture { kind: "crawl", status: "cancelled", events: events(2, None) }),
        // Completed with more events than one control page.
        'P' => Some(Fixture { kind: "crawl", status: "completed", events: events(1200, None) }),
        // A batch job, for the kind check.
        'B' => Some(Fixture { kind: "batch", status: "completed", events: events(3, None) }),
        _ => None,
    }
}

fn not_found() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "data": null, "error": { "code": "NOT_FOUND", "message": "not found" } })),
    )
}

async fn control_summary(
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    match fixture_for(&id) {
        Some(fx) if query.get("org_id").map(String::as_str) == Some(TEST_ORG) => {
            let last_seq = fx.events.len() as u64;
            (
                StatusCode::OK,
                Json(json!({ "data": {
                    "job": {
                        "id": id,
                        "org_id": TEST_ORG,
                        "kind": fx.kind,
                        "status": fx.status,
                        "created_at": "2026-10-23T10:00:00Z",
                        "origin": { "type": "user", "actor": "user_placeholder" },
                        "desired_seq": 0,
                        "applied_seq": 0
                    },
                    "phase": fx.status,
                    "counts": { "completed": 0, "failed": 0, "blocked": 0, "retried": 0, "extracted": 0 },
                    "total": 0,
                    "last_seq": last_seq,
                    "warnings": []
                } })),
            )
        }
        _ => not_found(),
    }
}

async fn control_events(
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    let Some(fx) = fixture_for(&id) else {
        return not_found();
    };
    if query.get("org_id").map(String::as_str) != Some(TEST_ORG) {
        return not_found();
    }
    let after: u64 = query.get("after_seq").and_then(|v| v.parse().ok()).unwrap_or(0);
    let limit: usize = query.get("limit").and_then(|v| v.parse().ok()).unwrap_or(100);
    let page: Vec<Value> = fx
        .events
        .iter()
        .filter(|event| event["seq"].as_u64().unwrap_or(0) > after)
        .take(limit)
        .cloned()
        .collect();
    (StatusCode::OK, Json(json!({ "data": page })))
}

async fn spawn_control() -> String {
    let app = Router::new()
        .route("/v1/jobs/:id/summary", get(control_summary))
        .route("/v1/jobs/:id/events", get(control_events));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// An edge wired to the control double, with the job-stream flag set on or off.
async fn spawn_edge(control_url: String, follow_flag: bool) -> SocketAddr {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    std::env::set_var(job_stream::ENV_FOLLOW, if follow_flag { "1" } else { "0" });
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    let state = AppState {
        control_base_url: control_url,
        ..support::base_state(driver, EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn get_text(addr: SocketAddr, path: &str, headers: &[(&str, &str)]) -> (u16, String) {
    let mut request = reqwest::Client::new()
        .get(format!("http://{addr}{path}"))
        .bearer_auth(TEST_BEARER);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = request.send().await.unwrap();
    let status = response.status().as_u16();
    (status, response.text().await.unwrap())
}

/// One SSE frame as the client sees it.
#[derive(Debug, Default, Clone)]
struct Frame {
    id: Option<String>,
    event: Option<String>,
    data: String,
}

/// Parses an SSE body into frames, dropping the retry-only block axum sends first.
fn frames(body: &str) -> Vec<Frame> {
    body.split("\n\n")
        .filter(|block| !block.trim().is_empty())
        .map(|block| {
            let mut frame = Frame::default();
            for line in block.lines() {
                if let Some(value) = line.strip_prefix("id:") {
                    frame.id = Some(value.trim_start().to_string());
                } else if let Some(value) = line.strip_prefix("event:") {
                    frame.event = Some(value.trim_start().to_string());
                } else if let Some(value) = line.strip_prefix("data:") {
                    if !frame.data.is_empty() {
                        frame.data.push('\n');
                    }
                    frame.data.push_str(value.trim_start());
                }
            }
            frame
        })
        .filter(|frame| frame.event.is_some() || frame.id.is_some() || !frame.data.is_empty())
        .collect()
}

fn events_named(all: &[Frame], name: &str) -> Vec<Frame> {
    all.iter()
        .filter(|frame| frame.event.as_deref() == Some(name))
        .cloned()
        .collect()
}

fn event_ids(all: &[Frame]) -> Vec<String> {
    events_named(all, "page_fetched")
        .into_iter()
        .map(|frame| frame.id.unwrap_or_default())
        .collect()
}

#[tokio::test]
#[serial]
async fn snapshot_is_first_frame_and_carries_last_seq() {
    let addr = spawn_edge(spawn_control().await, true).await;

    let path = format!("/v1/jobs/{}/events?follow=true", job_id('S'));
    let (status, body) = get_text(addr, &path, &[]).await;
    assert_eq!(status, 200, "{body}");

    let all = frames(&body);
    assert_eq!(all[0].event.as_deref(), Some("snapshot"), "{body}");
    assert_eq!(all[0].id.as_deref(), Some("0"), "a history stream starts at cursor 0");
    let snapshot: Value = serde_json::from_str(&all[0].data).unwrap();
    assert_eq!(snapshot["last_seq"], 5);
    assert_eq!(snapshot["status"], "completed");
    assert_eq!(snapshot["terminal"], true);
    assert_eq!(snapshot["kind"], "crawl");

    assert_eq!(event_ids(&all), vec!["1", "2", "3", "4", "5"], "{body}");
    assert_eq!(all.last().unwrap().data, "done", "the stream ends with the legacy done");
}

#[tokio::test]
#[serial]
async fn resume_with_last_event_id_replays_only_newer() {
    let addr = spawn_edge(spawn_control().await, true).await;

    let path = format!("/v1/jobs/{}/events?follow=true", job_id('R'));
    let (status, body) = get_text(addr, &path, &[("last-event-id", "2")]).await;
    assert_eq!(status, 200, "{body}");

    let all = frames(&body);
    assert_eq!(all[0].event.as_deref(), Some("snapshot"));
    assert_eq!(all[0].id.as_deref(), Some("2"), "the snapshot carries the resume cursor");
    assert_eq!(event_ids(&all), vec!["3", "4", "5"], "exactly the missing events, once: {body}");
}

#[tokio::test]
#[serial]
async fn header_wins_over_after_seq() {
    let addr = spawn_edge(spawn_control().await, true).await;

    let path = format!("/v1/jobs/{}/events?follow=true&after_seq=4", job_id('R'));
    let (status, body) = get_text(addr, &path, &[("last-event-id", "1")]).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(event_ids(&frames(&body)), vec!["2", "3", "4", "5"], "{body}");

    let (_, body) = get_text(addr, &path, &[]).await;
    assert_eq!(event_ids(&frames(&body)), vec!["5"], "without the header after_seq applies");
}

#[tokio::test]
#[serial]
async fn replay_none_starts_at_the_snapshot() {
    let addr = spawn_edge(spawn_control().await, true).await;

    let path = format!("/v1/jobs/{}/events?follow=true&replay=none", job_id('S'));
    let (status, body) = get_text(addr, &path, &[]).await;
    assert_eq!(status, 200, "{body}");
    let all = frames(&body);
    assert_eq!(all[0].id.as_deref(), Some("5"), "the cursor is the job's last seq");
    assert!(event_ids(&all).is_empty(), "no history is replayed: {body}");

    let path = format!("/v1/jobs/{}/events?follow=true&replay=some", job_id('S'));
    let (status, _) = get_text(addr, &path, &[]).await;
    assert_eq!(status, 400, "an unknown replay value is refused");
}

#[tokio::test]
#[serial]
async fn stream_ends_with_done_on_terminal_event() {
    let addr = spawn_edge(spawn_control().await, true).await;

    let path = format!("/v1/jobs/{}/events?follow=true", job_id('T'));
    let (status, body) = get_text(addr, &path, &[]).await;
    assert_eq!(status, 200, "{body}");

    let all = frames(&body);
    let names: Vec<String> = events_named(&all, "run_completed")
        .into_iter()
        .map(|frame| frame.id.unwrap_or_default())
        .collect();
    assert_eq!(names, vec!["4"], "the terminal event is sent once: {body}");
    assert_eq!(all.last().unwrap().data, "done");
}

#[tokio::test]
#[serial]
async fn stream_ends_on_terminal_status_without_event() {
    let addr = spawn_edge(spawn_control().await, true).await;

    let path = format!("/v1/jobs/{}/events?follow=true", job_id('C'));
    let (status, body) = get_text(addr, &path, &[]).await;
    assert_eq!(status, 200, "{body}");

    let all = frames(&body);
    assert_eq!(event_ids(&all), vec!["1", "2"], "{body}");
    assert_eq!(all.last().unwrap().data, "done", "the cancelled record ends the stream");
}

#[tokio::test]
#[serial]
async fn legacy_drain_unchanged_without_follow() {
    let addr = spawn_edge(spawn_control().await, true).await;

    let path = format!("/v1/jobs/{}/events", job_id('S'));
    let (status, body) = get_text(addr, &path, &[]).await;
    assert_eq!(status, 200, "{body}");

    let all = frames(&body);
    assert!(events_named(&all, "snapshot").is_empty(), "no snapshot without follow: {body}");
    assert!(all.iter().all(|frame| frame.id.is_none()), "legacy frames carry no id: {body}");
    assert_eq!(events_named(&all, "page_fetched").len(), 5, "{body}");
    assert_eq!(all.last().unwrap().data, "done");
}

#[tokio::test]
#[serial]
async fn legacy_pages_past_500_events() {
    let addr = spawn_edge(spawn_control().await, false).await;

    let path = format!("/v1/jobs/{}/events", job_id('P'));
    let (status, body) = get_text(addr, &path, &[]).await;
    assert_eq!(status, 200, "{body}");

    let all = frames(&body);
    assert_eq!(events_named(&all, "page_fetched").len(), 1200, "every page is read, none truncated");
    assert_eq!(all.last().unwrap().data, "done");
}

#[tokio::test]
#[serial]
async fn foreign_job_is_404_before_any_frame() {
    let addr = spawn_edge(spawn_control().await, true).await;

    let path = format!("/v1/jobs/{}/events?follow=true", job_id('Z'));
    let (status, body) = get_text(addr, &path, &[]).await;
    assert_eq!(status, 404, "{body}");
    assert!(!body.contains("snapshot"), "no frame before the refusal: {body}");

    // A batch id on the crawl path is the same 404 as a foreign id.
    let path = format!("/v1/crawl/{}/events?follow=true", job_id('B'));
    let (status, body) = get_text(addr, &path, &[]).await;
    assert_eq!(status, 404, "{body}");
}

#[tokio::test]
#[serial]
async fn the_twenty_first_stream_of_an_org_is_refused() {
    let addr = spawn_edge(spawn_control().await, true).await;

    let mut held = Vec::new();
    for _ in 0..job_stream::DEFAULT_MAX_PER_ORG {
        held.push(job_stream::acquire(TEST_ORG).expect("under the per-org cap"));
    }

    let path = format!("/v1/jobs/{}/events?follow=true", job_id('S'));
    let (status, body) = get_text(addr, &path, &[]).await;
    assert_eq!(status, 429, "{body}");
    let refusal: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(refusal["code"], "RATE_LIMITED");
    assert_eq!(refusal["retry_after_seconds"], 30);

    // A dropped permit frees its slot, and the next stream is served.
    held.pop();
    let (status, body) = get_text(addr, &path, &[]).await;
    assert_eq!(status, 200, "{body}");
}

#[test]
fn idle_backoff_reaches_five_seconds() {
    use std::time::Duration;
    assert_eq!(job_stream::poll_interval(true, Duration::ZERO), Duration::from_millis(500));
    assert_eq!(job_stream::poll_interval(false, Duration::from_secs(1)), Duration::from_secs(2));
    assert_eq!(job_stream::poll_interval(false, Duration::from_secs(60)), Duration::from_secs(5));
    assert_eq!(job_stream::parse_replay(None), Some(Replay::History));
}
