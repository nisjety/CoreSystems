//! `GET /v1/runs/:id/events` (security review of W2-U31): the run route serves the same job
//! events as the job drain, and the drain hands out the `run_id`, so a caller under the token's
//! ZDR floor gets the payload cut to its closed fields here too. Control resolves the run by id
//! alone, so an event of another org is a 404 at the edge. Through the real handler with a
//! control double.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, Request},
    middleware::{self, Next},
    response::Response,
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use quarry_edge::auth::Claims;
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};

mod support;

const TEST_ORG: &str = "org_placeholder";
const OTHER_ORG: &str = "org_other_placeholder";
const OWN_RUN: &str = "run_01J9AAAAAAAAAAAAAAAAAAAAAA";
const FOREIGN_RUN: &str = "run_01J9FFFFFFFFFFFFFFFFFFFFFF";

const SECRET_URL: &str = "https://secret-customer.test/private/page";
const SECRET_TITLE: &str = "Secret Customer Title";

fn event(run_id: &str, org_id: &str, seq: u64, payload: Value) -> Value {
    json!({
        "run_id": run_id, "org_id": org_id, "kind": "crawl", "stage": "running",
        "status": "ok", "seq": seq, "completed": 1, "discovered": 0, "queued": 0,
        "retries": 0, "blocks": 0, "timestamp": "2026-10-23T10:00:00Z", "payload": payload
    })
}

/// Control as it is: the run is resolved by id and the `org_id` query is not checked.
async fn control_run_events(Path(id): Path<String>) -> Json<Value> {
    let org = if id == FOREIGN_RUN { OTHER_ORG } else { TEST_ORG };
    Json(json!({ "data": [
        event(&id, org, 1, json!({ "url": SECRET_URL, "status": 200, "content_type": "text/html" })),
        event(&id, org, 2, json!({ "url": SECRET_URL, "title": SECRET_TITLE, "word_count": 120, "driver": "static" })),
        event(&id, org, 3, Value::Null),
    ] }))
}

async fn spawn_control() -> String {
    let app = Router::new().route("/v1/runs/:id/events", get(control_run_events));
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

async fn spawn_edge(zdr: bool) -> SocketAddr {
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    let state = AppState {
        control_base_url: spawn_control().await,
        ..support::base_state(driver, EventSink::new(tx))
    };
    let app = Router::new()
        .route("/v1/runs/:id/events", get(quarry_edge::resource_routes::list_run_events))
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

#[tokio::test]
async fn a_zdr_caller_reads_run_events_without_page_strings() {
    let addr = spawn_edge(true).await;
    let (status, body) = get_text(addr, &format!("/v1/runs/{OWN_RUN}/events")).await;
    assert_eq!(status, 200, "{body}");
    for secret in [SECRET_URL, SECRET_TITLE, "secret-customer"] {
        assert!(!body.contains(secret), "a ZDR body names {secret}: {body}");
    }
    let parsed: Value = serde_json::from_str(&body).unwrap();
    let items = parsed["data"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 3, "{body}");
    assert_eq!(items[0]["payload"]["status"], 200);
    assert_eq!(items[0]["payload"]["content_type"], "text/html");
    assert_eq!(items[1]["payload"]["word_count"], 120);
    assert_eq!(items[1]["payload"]["driver"], "static");
    assert!(items[2].get("payload").is_none(), "a null payload stays absent: {body}");
}

#[tokio::test]
async fn a_non_zdr_caller_reads_run_events_unchanged() {
    let addr = spawn_edge(false).await;
    let (status, body) = get_text(addr, &format!("/v1/runs/{OWN_RUN}/events")).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains(SECRET_URL), "{body}");
    assert!(body.contains(SECRET_TITLE), "{body}");
}

#[tokio::test]
async fn another_orgs_run_is_a_404_whatever_the_posture() {
    for zdr in [false, true] {
        let addr = spawn_edge(zdr).await;
        let (status, body) = get_text(addr, &format!("/v1/runs/{FOREIGN_RUN}/events")).await;
        assert_eq!(status, 404, "zdr={zdr}: {body}");
        for leaked in [SECRET_URL, SECRET_TITLE, OTHER_ORG] {
            assert!(!body.contains(leaked), "zdr={zdr}: the refusal names {leaked}: {body}");
        }
    }
}
