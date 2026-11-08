//! The ZDR floor on the job event routes (D2-057, X-9 order): `follow=true` is a typed 403 for a
//! caller under the token's ZDR floor, before control is asked anything, and the legacy drain
//! never returns a page URL, title or excerpt to that caller. A non-ZDR caller is served as
//! before. Through the real handlers with a control double that serves events full of page URLs.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    extract::{Path, Query, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use quarry_edge::auth::Claims;
use quarry_edge::job_stream;
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};

mod support;

const TEST_ORG: &str = "org_placeholder";
const CRAWL: &str = "job_01J9ZZZZZZZZZZZZZZZZZZZZZZ";
const BATCH: &str = "job_01J9BBBBBBBBBBBBBBBBBBBBBB";

/// What a ZDR reader must never see, in every spelling the double puts it in.
const SECRET_URL: &str = "https://secret-customer.test/private/page";
const SECRET_TITLE: &str = "Secret Customer Title";
const SECRET_EXCERPT: &str = "Secret customer excerpt words";

/// The follow flag is process-wide: every test holds this for its whole run and sets it itself.
static FLAG: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn set_follow(on: bool) {
    std::env::set_var(job_stream::ENV_FOLLOW, if on { "1" } else { "0" });
}

#[derive(Clone, Default)]
struct Seen {
    summary_reads: Arc<Mutex<u32>>,
    event_reads: Arc<Mutex<u32>>,
}

fn events() -> Vec<Value> {
    vec![
        json!({
            "event_id": "evt_1", "type": "page_fetched", "seq": 1, "ts": "2026-10-23T10:00:00Z",
            "payload": { "url": SECRET_URL, "status": 200, "duration_ms": 41, "content_type": "text/html" },
            "idempotency_key": format!("run_1:{SECRET_URL}:fetched"),
        }),
        json!({
            "event_id": "evt_2", "type": "page_extracted", "seq": 2, "ts": "2026-10-23T10:00:01Z",
            "payload": { "url": SECRET_URL, "title": SECRET_TITLE, "excerpt": SECRET_EXCERPT,
                         "word_count": 120, "driver": "static" },
            "idempotency_key": "key_2",
        }),
        json!({
            "event_id": "evt_3", "type": "run_completed", "seq": 3, "ts": "2026-10-23T10:00:02Z",
            "payload": { "pages_visited": 1 },
            "idempotency_key": "key_3",
        }),
    ]
}

fn not_found() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "data": null, "error": { "code": "NOT_FOUND", "message": "not found" } })),
    )
}

async fn control_summary(
    State(seen): State<Seen>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    *seen.summary_reads.lock().unwrap() += 1;
    if query.get("org_id").map(String::as_str) != Some(TEST_ORG) {
        return not_found();
    }
    let kind = if id == BATCH { "batch" } else { "crawl" };
    (
        StatusCode::OK,
        Json(json!({ "data": {
            "job": { "id": id, "org_id": TEST_ORG, "kind": kind, "status": "completed",
                     "created_at": "2026-10-23T10:00:00Z", "origin": { "type": "user", "actor": "user_placeholder" },
                     "desired_seq": 0, "applied_seq": 0 },
            "phase": "completed",
            "counts": { "completed": 1, "failed": 0, "blocked": 0, "retried": 0, "extracted": 0 },
            "total": 1, "last_seq": 3, "warnings": []
        } })),
    )
}

async fn control_events(
    State(seen): State<Seen>,
    Path(_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    *seen.event_reads.lock().unwrap() += 1;
    if query.get("org_id").map(String::as_str) != Some(TEST_ORG) {
        return not_found();
    }
    let after: u64 = query.get("after_seq").and_then(|v| v.parse().ok()).unwrap_or(0);
    let page: Vec<Value> = events()
        .into_iter()
        .filter(|event| event["seq"].as_u64().unwrap_or(0) > after)
        .collect();
    (StatusCode::OK, Json(json!({ "data": page })))
}

async fn spawn_control(seen: Seen) -> String {
    let app = Router::new()
        .route("/v1/jobs/:id/summary", get(control_summary))
        .route("/v1/jobs/:id/events", get(control_events))
        .with_state(seen);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn claims(zdr: bool) -> Claims {
    Claims {
        web_policy: None,
        sub: "user_placeholder".into(),
        iss: "auth-core".into(),
        exp: i64::MAX,
        org_id: TEST_ORG.into(),
        user_id: "user_placeholder".into(),
        principal_type: Some("user".into()),
        service_id: None,
        nbf: None,
        aud: None,
        scopes: Vec::new(),
        zdr: if zdr { Some(true) } else { None },
        key_id: None,
        acting_user_id: None,
    }
}

/// The three event routes behind a layer that inserts the claims of a ZDR or a plain user.
async fn spawn_edge(seen: Seen, zdr: bool) -> SocketAddr {
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    let state = AppState {
        control_base_url: spawn_control(seen).await,
        ..support::base_state(driver, EventSink::new(tx))
    };
    let app = Router::new()
        .route("/v1/jobs/:id/events", get(quarry_edge::resource_routes::list_job_events))
        .route("/v1/crawl/:id/events", get(quarry_edge::resource_routes::list_crawl_events))
        .route("/v1/batch/:id/events", get(quarry_edge::resource_routes::list_batch_events))
        .layer(middleware::from_fn(move |mut req: Request, next: Next| async move {
            req.extensions_mut().insert(claims(zdr));
            let response: Response = next.run(req).await;
            response
        }))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn get_text(addr: SocketAddr, path: &str) -> (u16, String) {
    let response = reqwest::Client::new()
        .get(format!("http://{addr}{path}"))
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.text().await.unwrap())
}

fn assert_no_secret(body: &str) {
    for secret in [SECRET_URL, SECRET_TITLE, SECRET_EXCERPT, "secret-customer"] {
        assert!(!body.contains(secret), "a ZDR body names {secret}: {body}");
    }
}

#[tokio::test]
async fn a_zdr_caller_is_refused_follow_with_a_typed_403_before_control_is_asked() {
    let _guard = FLAG.lock().await;
    set_follow(true);
    let seen = Seen::default();
    let addr = spawn_edge(seen.clone(), true).await;

    for path in [
        format!("/v1/jobs/{CRAWL}/events?follow=true"),
        format!("/v1/crawl/{CRAWL}/events?follow=1"),
        format!("/v1/batch/{BATCH}/events?follow=true&replay=none"),
    ] {
        let (status, body) = get_text(addr, &path).await;
        assert_eq!(status, 403, "{path}: {body}");
        let parsed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["error"]["code"], "FORBIDDEN", "{body}");
        assert_eq!(parsed["error"]["details"]["reason"], "job_event_urls", "{body}");
        assert_no_secret(&body);
    }
    assert_eq!(*seen.summary_reads.lock().unwrap(), 0, "the refusal comes before control");
    assert_eq!(*seen.event_reads.lock().unwrap(), 0);
    assert_eq!(job_stream::open_for_org(TEST_ORG), 0, "no stream permit was taken");
}

#[tokio::test]
async fn a_non_zdr_caller_still_gets_the_live_tail_with_its_urls() {
    let _guard = FLAG.lock().await;
    set_follow(true);
    let addr = spawn_edge(Seen::default(), false).await;

    let (status, body) = get_text(addr, &format!("/v1/jobs/{CRAWL}/events?follow=true")).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("event: snapshot"), "{body}");
    assert!(body.contains(SECRET_URL), "a plain caller's tail is unchanged: {body}");
    assert!(body.contains(SECRET_TITLE), "{body}");
    assert!(body.contains("event: run_completed"), "{body}");
}

#[tokio::test]
async fn the_drain_strips_page_strings_for_a_zdr_caller_on_every_route() {
    let _guard = FLAG.lock().await;
    // Flag off: `follow` is a no-op and the legacy drain runs. Flag on without `follow`: same.
    for follow_flag in [false, true] {
        set_follow(follow_flag);
        let addr = spawn_edge(Seen::default(), true).await;
        // The crawl path on the batch id would be a 404; the id of each kind goes on its own path.
        for path in [
            format!("/v1/jobs/{CRAWL}/events"),
            format!("/v1/crawl/{CRAWL}/events"),
            format!("/v1/batch/{BATCH}/events"),
        ] {
            let (status, body) = get_text(addr, &path).await;
            assert_eq!(status, 200, "{path}: {body}");
            assert_no_secret(&body);
            // The shape survives: event names, seqs, counts and the closed fields.
            assert!(body.contains("event: page_fetched"), "{body}");
            assert!(body.contains("event: page_extracted"), "{body}");
            assert!(body.contains("\"status\":200"), "{body}");
            assert!(body.contains("\"word_count\":120"), "{body}");
            assert!(body.contains("\"pages_visited\":1"), "{body}");
            assert!(body.contains("\"driver\":\"static\""), "{body}");
            assert!(body.contains("data: done"), "the terminal marker stays: {body}");
        }
    }
}

#[tokio::test]
async fn follow_true_with_the_flag_off_is_the_redacted_drain_for_a_zdr_caller() {
    let _guard = FLAG.lock().await;
    set_follow(false);
    let addr = spawn_edge(Seen::default(), true).await;

    let (status, body) = get_text(addr, &format!("/v1/jobs/{CRAWL}/events?follow=true")).await;
    assert_eq!(status, 200, "{body}");
    assert!(!body.contains("event: snapshot"), "the flag is off, so this is the drain: {body}");
    assert_no_secret(&body);
}

#[tokio::test]
async fn the_drain_of_a_non_zdr_caller_is_unchanged() {
    let _guard = FLAG.lock().await;
    for follow_flag in [false, true] {
        set_follow(follow_flag);
        let addr = spawn_edge(Seen::default(), false).await;
        let (status, body) = get_text(addr, &format!("/v1/jobs/{CRAWL}/events")).await;
        assert_eq!(status, 200, "{body}");
        for kept in [SECRET_URL, SECRET_TITLE, SECRET_EXCERPT, "event: page_extracted", "data: done"] {
            assert!(body.contains(kept), "a plain caller's drain lost {kept}: {body}");
        }
        // The idempotency key and the full payload are still on the frame.
        assert!(body.contains("idempotency_key"), "{body}");
        assert!(body.contains(&format!("run_1:{SECRET_URL}:fetched")), "{body}");
    }
}
