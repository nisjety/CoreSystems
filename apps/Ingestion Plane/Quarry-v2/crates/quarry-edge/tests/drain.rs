//! What a draining edge does. The drain flag is process-global, so this is a
//! test binary of its own and one test: it is the only thing that ever sets it.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{http::StatusCode, routing::post, Json, Router};
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};
use serde_json::{json, Value};
use tokio::sync::mpsc;

mod support;

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
async fn a_draining_edge_takes_no_new_work_and_says_so() {
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
