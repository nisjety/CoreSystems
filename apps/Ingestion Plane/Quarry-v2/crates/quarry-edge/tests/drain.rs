//! What a draining edge does. The drain flags are process-global, so this is a test
//! binary of its own, every test is `#[serial]`, and each one puts the flags (and the
//! dev-bypass variable) back when it ends.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{http::StatusCode, routing::post, Json, Router};
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};
use serde_json::{json, Value};
use serial_test::serial;
use tokio::sync::mpsc;

#[cfg(feature = "browser-agent")]
#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

/// Puts the process-global drain flags and the dev-bypass variable back when a test ends,
/// whether it passed or not.
struct FlagGuard;

impl Drop for FlagGuard {
    fn drop(&mut self) {
        quarry_edge::drain::reset_for_tests();
        std::env::remove_var("QUARRY_EDGE_AUTH_DEV_BYPASS");
    }
}

const TEST_BEARER: &str = "drain-dev-token";

async fn spawn_control() -> String {
    let app = Router::new().route(
        "/v1/jobs",
        post(|| async {
            (
                StatusCode::OK,
                Json(json!({ "data": { "id": "job_stub", "run_id": null } })),
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn spawn_edge(control_url: String) -> SocketAddr {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
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

async fn post_json(addr: SocketAddr, path: &str, body: Value) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("http://{addr}{path}"))
        .bearer_auth(TEST_BEARER)
        .json(&body)
        .send()
        .await
        .unwrap();
    (
        response.status().as_u16(),
        response.json().await.unwrap_or(Value::Null),
    )
}

async fn get(addr: SocketAddr, path: &str) -> (u16, Value) {
    let response = reqwest::Client::new()
        .get(format!("http://{addr}{path}"))
        .send()
        .await
        .unwrap();
    (
        response.status().as_u16(),
        response.json().await.unwrap_or(Value::Null),
    )
}

#[tokio::test]
#[serial]
async fn a_draining_edge_takes_no_new_work_and_says_so() {
    let _flags = FlagGuard;
    let addr = spawn_edge(spawn_control().await).await;

    // Before: work is accepted and the edge reports on its readiness as usual.
    let (status, body) =
        post_json(addr, "/v1/crawl", json!({ "url": "https://example.com/" })).await;
    assert_eq!(status, 200, "{body}");
    assert!(!quarry_edge::drain::is_draining());
    assert_ne!(get(addr, "/ready").await.1["status"], "draining");

    quarry_edge::drain::begin();
    quarry_edge::drain::begin(); // idempotent
    assert!(quarry_edge::drain::is_draining());
    tokio::time::timeout(Duration::from_secs(1), quarry_edge::drain::started())
        .await
        .expect("started() resolves once draining has begun");

    // After: a crawl and a batch get a typed, retryable 503; the readiness
    // probe takes the instance out of rotation.
    for (path, request) in [
        ("/v1/crawl", json!({ "url": "https://example.com/" })),
        ("/v1/batch", json!({ "urls": ["https://example.com/a"] })),
    ] {
        let (status, envelope) = post_json(addr, path, request).await;
        assert_eq!(status, 503, "{path}: {envelope}");
        assert_eq!(envelope["error"]["code"], "RUNTIME_NOT_READY", "{path}");
        assert_eq!(envelope["error"]["details"]["reason"], "draining", "{path}");
        assert_eq!(envelope["error"]["retryable"], true, "{path}");
    }
    let (status, ready) = get(addr, "/ready").await;
    assert_eq!(status, 503);
    assert_eq!(ready["status"], "draining");
    // Liveness stays up: the process is healthy, only leaving.
    assert_eq!(get(addr, "/health").await.0, 200);
}

/// Why main caps the wait: axum's graceful shutdown waits for every open
/// connection, and a stream with a viewer never ends on its own. This is the
/// same serve-then-select shape main uses, against an endless SSE response.
#[tokio::test]
#[serial]
async fn graceful_shutdown_alone_hangs_on_an_open_stream_and_the_deadline_ends_it() {
    use axum::response::sse::{Event, Sse};
    use futures_util::StreamExt;

    async fn endless(
    ) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
        let stream = futures_util::stream::unfold((), |()| async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Some((Ok(Event::default().data("tick")), ()))
        });
        Sse::new(stream)
    }

    let app = Router::new().route("/stream", axum::routing::get(endless));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let serve = std::future::IntoFuture::into_future(
        axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = stop_rx.await;
        }),
    );
    let server = tokio::spawn(async move {
        let deadline = Duration::from_millis(400);
        let started = std::time::Instant::now();
        tokio::select! {
            result = serve => { result.unwrap(); (false, started.elapsed()) }
            _ = async { tokio::time::sleep(Duration::from_millis(50)).await; tokio::time::sleep(deadline).await } => (true, started.elapsed()),
        }
    });

    // A viewer connects and starts reading the stream.
    let response = reqwest::Client::new()
        .get(format!("http://{addr}/stream"))
        .send()
        .await
        .unwrap();
    let mut body = response.bytes_stream();
    assert!(body.next().await.is_some(), "the stream is live");

    stop_tx.send(()).unwrap();
    let (hit_deadline, waited) = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("the capped shutdown returns")
        .unwrap();
    assert!(
        hit_deadline,
        "an open stream keeps graceful shutdown from finishing by itself"
    );
    assert!(
        waited < Duration::from_secs(2),
        "and the cap ends the wait: {waited:?}"
    );
}

// ── WS6.J: the runs, the streams and the usage segment ──────────────────────
//
// These use the real router with real token verification (`support/agent_harness.rs`),
// and the drain functions directly: no signals. Every test is `#[serial]` and puts the
// process-global drain flags back when it ends.

#[cfg(feature = "browser-agent")]
mod runs {
    use super::*;

    use std::sync::Mutex;
    use std::time::Instant;

    use async_trait::async_trait;
    use futures_util::StreamExt;
    use quarry_core::event::EventType;
    use quarry_edge::agent_routes::{drain_runs_for_shutdown, DrainReport};
    use quarry_edge::run_limits::RunLimits;
    use quarry_runtime::step_receipts::{
        AgentRunStatus, BrowserTimelineEventKind, BrowserTimelineLifecycle,
    };
    use quarry_runtime::usage_metrics as metric_names;
    use quarry_runtime::{UsageEvent, UsageMeter};
    use serial_test::serial;

    use crate::agent_harness::*;

    const MEMBER_ORG: &str = "org-standard-golden";
    const MEMBER: &str = "user-golden";
    const ZDR_ORG: &str = "org-zdr-golden";

    const PER_RUN: Duration = Duration::from_secs(5);

    /// A usage meter that keeps every event and counts the flushes.
    #[derive(Default)]
    struct RecordingMeter {
        events: Mutex<Vec<UsageEvent>>,
        flushes: Mutex<Vec<Duration>>,
    }

    #[async_trait]
    impl UsageMeter for RecordingMeter {
        async fn meter(&self, event: UsageEvent) {
            self.events.lock().unwrap().push(event);
        }

        async fn flush(&self, deadline: Duration) {
            self.flushes.lock().unwrap().push(deadline);
        }
    }

    /// The agent states of a run's timeline, for the owner.
    async fn lifecycle_states(
        edge: &Edge,
        org: &str,
        actor: &str,
        run: &str,
    ) -> Vec<BrowserTimelineLifecycle> {
        edge.state
            .receipts
            .list_browser_timeline_events(org, actor, &kind(run))
            .await
            .expect("timeline read")
            .into_iter()
            .filter_map(|event| match event.event {
                BrowserTimelineEventKind::Lifecycle { state, .. } => Some(state),
                _ => None,
            })
            .collect()
    }

    /// Everything a stream sends until it ends (or `limit` passes).
    async fn read_stream(response: reqwest::Response, limit: Duration) -> String {
        let mut body = response.bytes_stream();
        let mut text = String::new();
        let _ = tokio::time::timeout(limit, async {
            while let Some(chunk) = body.next().await {
                match chunk {
                    Ok(bytes) => text.push_str(&String::from_utf8_lossy(&bytes)),
                    Err(_) => break,
                }
            }
        })
        .await;
        text
    }

    /// A draining edge refuses to start a run with a typed, retryable 503 and the standard
    /// `Retry-After` header, while a run that is already live keeps taking steps.
    #[tokio::test]
    #[serial]
    async fn drain_refuses_new_runs_with_503_and_retry_after() {
        let _flags = FlagGuard;
        let edge = Edge::start(RunLimits::default()).await;
        let person = member_token();
        let live = edge.start_run(&person, json!({})).await;

        quarry_edge::drain::begin();

        let refused = edge.post(&person, "/v1/agent/runs", json!({})).await;
        assert_eq!(refused.status, 503, "{}", refused.body);
        assert_eq!(refused.code(), "RUNTIME_NOT_READY", "{}", refused.body);
        assert_eq!(refused.details()["reason"], "draining", "{}", refused.body);
        assert_eq!(refused.body["error"]["retryable"], true, "{}", refused.body);
        assert_eq!(
            refused
                .headers
                .get("retry-after")
                .and_then(|value| value.to_str().ok()),
            Some("5"),
            "the wait is also a standard header"
        );

        // Steps on a run that is already live continue.
        let step = edge.step(&person, &live, wait_action(), None).await;
        assert_eq!(step.status, 200, "{}", step.body);
    }

    /// Graceful shutdown cannot finish while a viewer is connected, so the drain ends the
    /// run (the viewer is told why) and then closes the streams that are left: an event
    /// stream, a frame stream, and with them the server.
    #[tokio::test]
    #[serial]
    async fn drain_closes_streams_so_graceful_shutdown_completes() {
        let _flags = FlagGuard;
        let edge = Edge::start(RunLimits::default()).await;
        let person = member_token();
        let watched = edge.start_run(&person, json!({})).await;
        let framed = edge.start_run(&person, json!({})).await;

        // The real router behind a graceful shutdown that starts when the drain begins,
        // the way main wires it.
        let app = quarry_edge::routes::router(edge.state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut server = tokio::spawn(std::future::IntoFuture::into_future(
            axum::serve(listener, app).with_graceful_shutdown(async {
                quarry_edge::drain::started().await;
            }),
        ));
        let get = |route: String| {
            edge.client
                .get(format!("http://{addr}{route}"))
                .bearer_auth(&person)
                .send()
        };
        let events = get(format!("/v1/agent/runs/{watched}/events/stream"))
            .await
            .unwrap();
        let frames = get(format!("/v1/agent/runs/{framed}/frames/stream"))
            .await
            .unwrap();
        assert_eq!(events.status().as_u16(), 200);
        assert_eq!(frames.status().as_u16(), 200);

        quarry_edge::drain::begin();
        // Two open streams hold the shutdown open.
        assert!(
            tokio::time::timeout(Duration::from_millis(500), &mut server)
                .await
                .is_err(),
            "graceful shutdown waits for open streams"
        );

        // The drain ends the run the viewer watches, then closes what is left.
        let report = drain_runs_for_shutdown(&edge.state, PER_RUN).await;
        assert_eq!(
            report,
            DrainReport {
                released: 2,
                failed: 0,
                in_use: 0
            }
        );
        quarry_edge::drain::close_streams();

        let (events, frames) = tokio::join!(
            read_stream(events, Duration::from_secs(10)),
            read_stream(frames, Duration::from_secs(10)),
        );
        assert!(events.contains("event: agent.expired"), "{events}");
        assert!(events.contains("\"reason\":\"shutdown\""), "{events}");
        assert!(events.contains("event: done"), "{events}");
        assert!(frames.contains("event: done"), "{frames}");
        tokio::time::timeout(Duration::from_secs(10), server)
            .await
            .expect("the server finishes once its streams have ended")
            .expect("server task")
            .expect("serve");
    }

    /// Every live run is released exactly once, however it ended: by the drain, by its
    /// owner a moment before, or by a second drain pass.
    #[tokio::test]
    #[serial]
    async fn drain_releases_every_session_exactly_once() {
        let _flags = FlagGuard;
        let edge = Edge::start(RunLimits {
            max_concurrent_runs: 0,
            max_runs_per_org: 0,
            max_interactive_runs_per_org: 0,
            ..RunLimits::default()
        })
        .await;
        let person = member_token();
        let first = edge.start_run(&person, json!({})).await;
        let second = edge.start_run(&person, json!({})).await;
        let third = edge.start_run(&person, json!({})).await;
        let closed_before = edge.start_run(&person, json!({})).await;
        let closed = edge
            .delete(&person, &format!("/v1/agent/runs/{closed_before}"))
            .await;
        assert_eq!(closed.status, 200, "{}", closed.body);

        let report = drain_runs_for_shutdown(&edge.state, PER_RUN).await;
        assert_eq!(
            report,
            DrainReport {
                released: 3,
                failed: 0,
                in_use: 0
            }
        );
        // A second pass finds nothing left to do.
        assert_eq!(
            drain_runs_for_shutdown(&edge.state, PER_RUN).await,
            DrainReport::default()
        );

        let mut released = edge.driver.released();
        released.sort();
        let mut expected = vec![first, second, third, closed_before];
        expected.sort();
        assert_eq!(released, expected, "one release per run, none twice");
        assert_eq!(edge.state.agent_runs.stats().live, 0);
    }

    /// A non-ZDR run's checkpoint stays active (it can be resumed after the restart) and its
    /// owner's timeline records the end; a ZDR run leaves no checkpoint, timeline item or
    /// receipt.
    #[tokio::test]
    #[serial]
    async fn drain_keeps_non_zdr_checkpoints_active_and_writes_none_for_zdr() {
        let _flags = FlagGuard;
        let edge = Edge::start(RunLimits::default()).await;
        let standard = member_token();
        let zdr = zdr_member_token();
        let kept = edge.start_run(&standard, json!({})).await;
        let ephemeral = edge.start_run(&zdr, json!({})).await;
        for (token, run) in [(&standard, &kept), (&zdr, &ephemeral)] {
            let step = edge.step(token, run, wait_action(), None).await;
            assert_eq!(step.status, 200, "{}", step.body);
        }

        let report = drain_runs_for_shutdown(&edge.state, PER_RUN).await;
        assert_eq!(report.released, 2, "{report:?}");

        let checkpoint = edge
            .state
            .receipts
            .load_run_checkpoint(MEMBER_ORG, &kind(&kept))
            .await
            .expect("checkpoint read")
            .expect("a non-ZDR run keeps its checkpoint");
        assert_eq!(checkpoint.status, AgentRunStatus::Active);
        let states = lifecycle_states(&edge, MEMBER_ORG, MEMBER, &kept).await;
        assert!(
            states.contains(&BrowserTimelineLifecycle::Expired),
            "{states:?}"
        );
        assert!(
            !states.contains(&BrowserTimelineLifecycle::Closed),
            "a drain is not an owner's close: {states:?}"
        );

        let store = edge.state.receipts.clone();
        assert!(store
            .load_run_checkpoint(ZDR_ORG, &kind(&ephemeral))
            .await
            .expect("checkpoint read")
            .is_none());
        assert!(lifecycle_states(&edge, ZDR_ORG, MEMBER, &ephemeral)
            .await
            .is_empty());
        assert!(store
            .list_for_actor(ZDR_ORG, MEMBER, &kind(&ephemeral))
            .await
            .expect("receipt read")
            .is_empty());
    }

    /// The last usage segment of each agent run is handed to the meter before the drain
    /// returns, the viewer-facing `agent.expired` carries the reason `shutdown`, and the
    /// meter can then be flushed.
    #[tokio::test]
    #[serial]
    async fn drain_emits_the_usage_segment_before_exit() {
        let _flags = FlagGuard;
        let meter = Arc::new(RecordingMeter::default());
        let edge = Edge::start_with(RunLimits::default(), |state| {
            let usage: Arc<dyn UsageMeter> = meter.clone();
            quarry_edge::state::AppState { usage, ..state }
        })
        .await;
        let service = service_token();
        let run = edge.start_run(&service, json!({})).await;
        let step = edge.step(&service, &run, wait_action(), None).await;
        assert_eq!(step.status, 200, "{}", step.body);

        let report = drain_runs_for_shutdown(&edge.state, PER_RUN).await;
        assert_eq!(report.released, 1, "{report:?}");

        {
            let events = meter.events.lock().unwrap();
            let for_run: Vec<&UsageEvent> = events
                .iter()
                .filter(|event| event.event_id.contains(run.as_str()))
                .collect();
            let metrics: Vec<&str> = for_run.iter().map(|event| event.metric.as_str()).collect();
            assert!(metrics.contains(&metric_names::AGENT_RUN), "{metrics:?}");
            assert!(metrics.contains(&metric_names::AGENT_STEP), "{metrics:?}");
            assert!(for_run.iter().all(|event| event.org_id == MEMBER_ORG));
            let segment = for_run
                .iter()
                .find(|event| event.metric == metric_names::AGENT_RUN)
                .expect("the agent.run event");
            assert_eq!(
                segment.metadata["outcome"], "expired",
                "{}",
                segment.metadata
            );
            assert!(segment.event_id.contains(":s0:"), "{}", segment.event_id);
        }

        let durable = edge.durable_events().await;
        let expired: Vec<_> = durable
            .iter()
            .filter(|(event_type, _)| *event_type == EventType::AgentExpired)
            .collect();
        assert_eq!(expired.len(), 1, "{durable:?}");
        assert_eq!(expired[0].1["reason"], "shutdown");
        assert!(expired[0].1["idle_s"].is_u64() && expired[0].1["age_s"].is_u64());

        // What main does last.
        edge.state.usage.flush(Duration::from_secs(5)).await;
        assert_eq!(*meter.flushes.lock().unwrap(), vec![Duration::from_secs(5)]);
    }

    /// An action that still holds a run at the drain deadline leaves that run to the exit; the
    /// drain neither waits for it nor lets it hold up the idle runs.
    #[tokio::test]
    #[serial]
    async fn a_busy_run_does_not_block_the_drain() {
        let _flags = FlagGuard;
        let edge = Edge::start(RunLimits {
            max_concurrent_runs: 0,
            max_runs_per_org: 0,
            max_interactive_runs_per_org: 0,
            ..RunLimits::default()
        })
        .await;
        let person = member_token();
        let first = edge.start_run(&person, json!({})).await;
        let second = edge.start_run(&person, json!({})).await;
        let busy = edge.start_run(&person, json!({})).await;

        // The busy run's navigation holds the run lock until the test opens the gate.
        let gate = edge.driver.time_out_next_goto_when_released();
        let stuck_step = edge.step(
            &person,
            &busy,
            json!({ "type": "navigate", "url": "https://example.test/slow" }),
            None,
        );
        let draining = async {
            edge.driver
                .wait_goto_entered(1, "the busy run's navigation")
                .await;
            let started = Instant::now();
            let report = drain_runs_for_shutdown(&edge.state, Duration::from_millis(300)).await;
            let took = started.elapsed();
            gate.notify_one();
            (report, took)
        };
        let (_stuck, (report, took)) = tokio::join!(stuck_step, draining);

        assert_eq!(
            report,
            DrainReport {
                released: 2,
                failed: 0,
                in_use: 1
            },
            "two idle runs released, one left to the exit"
        );
        assert!(
            took < Duration::from_secs(4),
            "the drain was bounded: {took:?}"
        );
        let mut released = edge.driver.released();
        released.sort();
        let mut expected = vec![first, second];
        expected.sort();
        assert_eq!(released, expected);
    }
}
