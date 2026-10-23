//! Job lifecycle reads and the origin-carrying handoff (WS15.L1), through the
//! real edge router with a control double. The dev bypass identity is the org
//! `org_placeholder`; the control double answers every job by that org, so a
//! foreign id is a 404 from control and must reach the caller as a 404.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, post},
    Json, Router,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use quarry_core::resources::JobSummary;
use quarry_edge::auth::{require_service_route_scope, Claims};
use quarry_edge::handoff::{idempotency_key_from_request, run_origin_from_request};
use quarry_edge::internal_auth::InternalSigner;
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};

mod support;

const TEST_BEARER: &str = "job-lifecycle-dev-token";
const TEST_ORG: &str = "org_placeholder";

/// Every POST the control double received: its headers and its JSON body.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<(HeaderMap, Value)>>>);

fn summary_json(id: &str, kind: &str) -> Value {
    json!({
        "job": {
            "id": id,
            "org_id": TEST_ORG,
            "kind": kind,
            "status": "running",
            "created_at": "2026-10-23T10:00:00Z",
            "origin": { "type": "user", "actor": "user_placeholder" },
            "desired_seq": 0,
            "applied_seq": 0
        },
        "phase": "running",
        "counts": { "completed": 3, "failed": 1, "blocked": 0, "retried": 0, "extracted": 0 },
        "total": 20,
        "total_kind": "budget",
        "last_seq": 5,
        "warnings": []
    })
}

async fn control_summary(
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    if query.get("org_id").map(String::as_str) != Some(TEST_ORG) || id == "job_01J9FFFFFFFFFFFFFFFFFFFFFF" {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "data": null, "error": { "code": "NOT_FOUND", "message": "not found" } })),
        );
    }
    let kind = if id == "job_01J9BBBBBBBBBBBBBBBBBBBBBB" { "batch" } else { "crawl" };
    (StatusCode::OK, Json(json!({ "data": summary_json(&id, kind) })))
}

async fn control_errors(Path(id): Path<String>) -> (StatusCode, Json<Value>) {
    if id == "job_01J9FFFFFFFFFFFFFFFFFFFFFF" {
        return (StatusCode::NOT_FOUND, Json(json!({ "data": null })));
    }
    (
        StatusCode::OK,
        Json(json!({ "data": {
            "errors": [
                { "id": "evt_1", "seq": 1, "ts": "2026-10-23T10:00:01Z", "url": "https://e.test/a",
                  "code": "client_4xx", "category": "client_4xx", "retryable": false, "status": 404 },
                { "id": "evt_2", "seq": 3, "ts": "2026-10-23T10:00:03Z", "url": "https://e.test/c",
                  "category": "server_5xx", "retryable": true }
            ],
            "blocked": [
                { "url": "https://blocked.test/", "ts": "2026-10-23T10:00:02Z", "reason": "host_blocked" }
            ]
        } })),
    )
}

async fn control_create_job(
    State(captured): State<Captured>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    captured.0.lock().unwrap().push((headers, body));
    (StatusCode::CREATED, Json(json!({ "data": { "id": "job_new", "run_id": null } })))
}

async fn spawn_control(captured: Captured) -> String {
    let app = Router::new()
        .route("/v1/jobs", post(control_create_job))
        .route("/v1/jobs/ongoing", get(|| async { Json(json!({ "data": [] })) }))
        .route("/v1/jobs/:id/summary", get(control_summary))
        .route("/v1/jobs/:id/errors", get(control_errors))
        .route("/v1/jobs/:id/control", post(control_job_verb))
        .route(
            "/v1/crawl/jobs",
            get(|| async { Json(json!({ "data": { "items": [], "next_cursor": null, "total_estimated": 0 } })) }),
        )
        .with_state(captured);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn spawn_edge(control_url: String, signer: Option<Arc<InternalSigner>>) -> SocketAddr {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    let state = AppState {
        control_base_url: control_url,
        internal_signer: signer,
        ..support::base_state(driver, EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn get_json(addr: SocketAddr, path: &str) -> (u16, Value) {
    let response = reqwest::Client::new()
        .get(format!("http://{addr}{path}"))
        .bearer_auth(TEST_BEARER)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn crawl_status_is_org_scoped_and_404s_foreign_id() {
    let addr = spawn_edge(spawn_control(Captured::default()).await, None).await;

    let (status, body) = get_json(addr, "/v1/crawl/job_01J9MMMMMMMMMMMMMMMMMMMMMM").await;
    assert_eq!(status, 200, "{body}");
    let data = &body["data"];
    assert_eq!(data["job_id"], "job_01J9MMMMMMMMMMMMMMMMMMMMMM");
    assert_eq!(data["kind"], "crawl");
    assert_eq!(data["terminal"], false);
    assert_eq!(data["phase"], "running");
    assert_eq!(data["counts"]["completed"], 3);
    assert_eq!(data["total"], 20);
    assert_eq!(data["total_kind"], "budget");
    assert_eq!(data["origin"]["type"], "user");

    let (status, body) = get_json(addr, "/v1/crawl/job_01J9FFFFFFFFFFFFFFFFFFFFFF").await;
    assert_eq!(status, 404, "{body}");
}

#[tokio::test]
async fn crawl_jobs_list_route_still_matches_after_id_route() {
    let addr = spawn_edge(spawn_control(Captured::default()).await, None).await;

    let (status, body) = get_json(addr, "/v1/crawl/jobs").await;
    assert_eq!(status, 200, "{body}");
    assert!(body["data"]["items"].is_array(), "{body}");
}

#[tokio::test]
async fn errors_include_blocked_and_final_retried() {
    let addr = spawn_edge(spawn_control(Captured::default()).await, None).await;

    let (status, body) = get_json(addr, "/v1/crawl/job_01J9MMMMMMMMMMMMMMMMMMMMMM/errors").await;
    assert_eq!(status, 200, "{body}");
    let data = &body["data"];
    assert_eq!(data["errors"].as_array().map(Vec::len), Some(2), "{body}");
    assert_eq!(data["errors"][0]["code"], "client_4xx");
    assert_eq!(data["blocked"].as_array().map(Vec::len), Some(1), "{body}");
    assert_eq!(data["blocked"][0]["reason"], "host_blocked");
}

#[tokio::test]
async fn kind_mismatch_is_404() {
    let addr = spawn_edge(spawn_control(Captured::default()).await, None).await;

    let (status, body) = get_json(addr, "/v1/batch/job_01J9MMMMMMMMMMMMMMMMMMMMMM").await;
    assert_eq!(status, 404, "{body}");
    let (status, _) = get_json(addr, "/v1/batch/job_01J9BBBBBBBBBBBBBBBBBBBBBB").await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn handoff_is_signed_and_carries_origin() {
    let captured = Captured::default();
    let signer = Arc::new(InternalSigner::new("0123456789abcdef0123456789abcdef".to_string()).unwrap());
    let addr = spawn_edge(spawn_control(captured.clone()).await, Some(signer)).await;

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v1/crawl"))
        .bearer_auth(TEST_BEARER)
        .header("idempotency-key", "idem-1")
        .json(&json!({ "url": "https://example.com/" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200, "{:?}", response.text().await);

    let posted = captured.0.lock().unwrap();
    assert_eq!(posted.len(), 1, "one job must be created");
    let (headers, body) = &posted[0];
    assert_eq!(body["kind"], "crawl");
    assert_eq!(body["origin"]["type"], "user");
    assert_eq!(body["origin"]["actor"], "user_placeholder");
    assert_eq!(body["params"]["url"], "https://example.com/");
    assert_eq!(headers.get("idempotency-key").unwrap(), "idem-1");
    assert!(headers.get("x-quarry-sig").is_some(), "the handoff must be signed");
}

#[tokio::test]
async fn service_token_gets_403_on_lifecycle_routes() {
    async fn ok() -> &'static str {
        "ok"
    }
    async fn as_model_gateway(mut req: Request, next: Next) -> Response {
        req.extensions_mut().insert(Claims {
            sub: "service:model-gateway".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: TEST_ORG.into(),
            user_id: "service:model-gateway".into(),
            principal_type: Some("service".into()),
            service_id: Some("service:model-gateway".into()),
            nbf: None,
            aud: None,
            scopes: vec!["scrape:read".into(), "browser:execute".into()],
            zdr: Some(true),
        });
        next.run(req).await
    }
    let app = Router::new()
        .route("/v1/crawl/:id", get(ok))
        .route("/v1/crawl/:id/errors", get(ok))
        .route("/v1/batch/:id", get(ok))
        .route("/v1/jobs/:id", get(ok))
        .route("/v1/crawl/ongoing", get(ok))
        .layer(middleware::from_fn(require_service_route_scope))
        .layer(middleware::from_fn(as_model_gateway));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    for path in [
        "/v1/crawl/job_01J9AAAAAAAAAAAAAAAAAAAAAA",
        "/v1/crawl/job_01J9AAAAAAAAAAAAAAAAAAAAAA/errors",
        "/v1/batch/job_01J9AAAAAAAAAAAAAAAAAAAAAA",
        "/v1/jobs/job_01J9AAAAAAAAAAAAAAAAAAAAAA",
        "/v1/crawl/ongoing",
    ] {
        let status = reqwest::get(format!("http://{addr}{path}")).await.unwrap().status();
        assert_eq!(status.as_u16(), 403, "{path} must be default-deny for a service principal");
    }
}

#[test]
fn a_model_plane_service_origin_is_an_agent_with_its_run_id() {
    let claims = Claims {
        sub: "service:execution-core".into(),
        iss: "auth-core".into(),
        exp: i64::MAX,
        org_id: TEST_ORG.into(),
        user_id: "service:execution-core".into(),
        principal_type: Some("service".into()),
        service_id: Some("service:execution-core".into()),
        nbf: None,
        aud: None,
        scopes: Vec::new(),
        zdr: Some(true),
    };
    let mut headers = HeaderMap::new();
    headers.insert("x-verevon-run-id", "run_abc".parse().unwrap());
    let origin = run_origin_from_request(&claims, &headers);
    assert_eq!(origin.origin_type, "agent");
    assert_eq!(origin.origin_ref.as_deref(), Some("run_abc"));

    // A user token never carries a run reference, whatever the header says.
    let user = Claims {
        principal_type: Some("user".into()),
        service_id: None,
        ..claims
    };
    let origin = run_origin_from_request(&user, &headers);
    assert_eq!(origin.origin_type, "user");
    assert_eq!(origin.origin_ref, None);
}

#[test]
fn idempotency_keys_are_bounded() {
    let mut headers = HeaderMap::new();
    assert_eq!(idempotency_key_from_request(&headers), None);
    headers.insert("idempotency-key", "k1".parse().unwrap());
    assert_eq!(idempotency_key_from_request(&headers).as_deref(), Some("k1"));
    headers.insert("idempotency-key", "x".repeat(129).parse().unwrap());
    assert_eq!(idempotency_key_from_request(&headers), None);
}

#[test]
fn job_summary_accepts_the_origin_and_phase_control_sends() {
    let wire = json!({
        "job_id": "job_01J9AAAAAAAAAAAAAAAAAAAAAA",
        "kind": "crawl",
        "org_id": TEST_ORG,
        "status": "running",
        "created_at": "2026-10-23T10:00:00Z",
        "origin": { "type": "agent", "actor": "service:model-gateway", "ref": "run_1" },
        "phase": "running",
        "stats": { "completed": 2, "failed": 0, "total": 20 }
    });
    let job: JobSummary = serde_json::from_value(wire).unwrap();
    let origin = job.origin.expect("origin");
    assert_eq!(origin.origin_type, "agent");
    assert_eq!(origin.origin_ref.as_deref(), Some("run_1"));
    assert_eq!(job.phase.as_deref(), Some("running"));
}

// ---- Job control (WS15.L2) -----------------------------------------------------

/// Control's answer to a control verb: 409 for a terminal job, otherwise the job
/// record with the request recorded (desired_seq ahead of applied_seq).
async fn control_job_verb(
    State(captured): State<Captured>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    captured.0.lock().unwrap().push((headers, body.clone()));
    if query.get("org_id").map(String::as_str) != Some(TEST_ORG) || id == "job_01J9FFFFFFFFFFFFFFFFFFFFFF" {
        return (StatusCode::NOT_FOUND, Json(json!({ "data": null })));
    }
    if id == "job_01J9TTTTTTTTTTTTTTTTTTTTTT" {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "data": null, "error": { "code": "CONFLICT", "message": "the job has already ended" } })),
        );
    }
    let action = body["action"].as_str().unwrap_or_default();
    let cancelled = action == "cancel";
    (
        StatusCode::OK,
        Json(json!({ "data": {
            "id": id,
            "org_id": TEST_ORG,
            "kind": "crawl",
            "status": if cancelled { "cancelled" } else { "running" },
            "created_at": "2026-10-23T10:00:00Z",
            "desired_state": action,
            "desired_seq": 1,
            "applied_seq": if cancelled { 1 } else { 0 },
        } })),
    )
}

/// The control verbs are on for these tests. The flag is process-wide, so every
/// control test sets it.
fn enable_job_control() {
    std::env::set_var("QUARRY_EDGE__JOB_CONTROL", "1");
}

async fn send_control(addr: SocketAddr, method: reqwest::Method, path: &str) -> (u16, Value) {
    let response = reqwest::Client::new()
        .request(method, format!("http://{addr}{path}"))
        .bearer_auth(TEST_BEARER)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn cancel_is_org_checked_and_409_on_terminal() {
    enable_job_control();
    let addr = spawn_edge(spawn_control(Captured::default()).await, None).await;

    let (status, body) = send_control(addr, reqwest::Method::DELETE, "/v1/crawl/job_01J9MMMMMMMMMMMMMMMMMMMMMM").await;
    assert_eq!(status, 202, "{body}");
    assert_eq!(body["data"]["job_id"], "job_01J9MMMMMMMMMMMMMMMMMMMMMM");
    assert_eq!(body["data"]["phase"], "cancelling");
    assert_eq!(body["data"]["control"]["desired"], "cancel");
    assert_eq!(body["data"]["control"]["pending"], true);

    let (status, body) = send_control(addr, reqwest::Method::DELETE, "/v1/crawl/job_01J9FFFFFFFFFFFFFFFFFFFFFF").await;
    assert_eq!(status, 404, "a foreign or missing id is a 404: {body}");

    let (status, body) = send_control(addr, reqwest::Method::DELETE, "/v1/crawl/job_01J9TTTTTTTTTTTTTTTTTTTTTT").await;
    assert_eq!(status, 409, "a second cancel of an ended job is a 409: {body}");

    let (status, body) = send_control(addr, reqwest::Method::POST, "/v1/batch/job_01J9MMMMMMMMMMMMMMMMMMMMMM/pause").await;
    assert_eq!(status, 404, "a crawl id on the batch path is a 404: {body}");
}

#[tokio::test]
async fn verbs_forward_actor_from_claims() {
    enable_job_control();
    let captured = Captured::default();
    let addr = spawn_edge(spawn_control(captured.clone()).await, None).await;

    let (status, body) = send_control(addr, reqwest::Method::POST, "/v1/crawl/job_01J9MMMMMMMMMMMMMMMMMMMMMM/pause").await;
    assert_eq!(status, 202, "{body}");
    assert_eq!(body["data"]["phase"], "pausing");

    let posted = captured.0.lock().unwrap();
    assert_eq!(posted.len(), 1, "one control request must reach control");
    let (_, body) = &posted[0];
    assert_eq!(body["action"], "pause");
    assert_eq!(body["actor"], "user_placeholder", "the actor comes from the verified claims");
    assert_eq!(body["actor_type"], "user");
}

#[tokio::test]
async fn service_token_cannot_control() {
    async fn ok() -> &'static str {
        "ok"
    }
    async fn as_model_gateway(mut req: Request, next: Next) -> Response {
        req.extensions_mut().insert(Claims {
            sub: "service:model-gateway".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: TEST_ORG.into(),
            user_id: "service:model-gateway".into(),
            principal_type: Some("service".into()),
            service_id: Some("service:model-gateway".into()),
            nbf: None,
            aud: None,
            scopes: vec!["scrape:read".into(), "browser:execute".into()],
            zdr: Some(true),
        });
        next.run(req).await
    }
    let app = Router::new()
        .route("/v1/crawl/:id", delete(ok))
        .route("/v1/crawl/:id/pause", post(ok))
        .route("/v1/crawl/:id/resume", post(ok))
        .route("/v1/batch/:id", delete(ok))
        .layer(middleware::from_fn(require_service_route_scope))
        .layer(middleware::from_fn(as_model_gateway));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    for (method, path) in [
        (reqwest::Method::DELETE, "/v1/crawl/job_01J9AAAAAAAAAAAAAAAAAAAAAA"),
        (reqwest::Method::POST, "/v1/crawl/job_01J9AAAAAAAAAAAAAAAAAAAAAA/pause"),
        (reqwest::Method::POST, "/v1/crawl/job_01J9AAAAAAAAAAAAAAAAAAAAAA/resume"),
        (reqwest::Method::DELETE, "/v1/batch/job_01J9AAAAAAAAAAAAAAAAAAAAAA"),
    ] {
        let status = reqwest::Client::new()
            .request(method, format!("http://{addr}{path}"))
            .send()
            .await
            .unwrap()
            .status();
        assert_eq!(status.as_u16(), 403, "{path} must be default-deny for a service principal");
    }
}

/// A ZDR user may cancel a job (the verb keeps no content), but the error URLs of
/// the same job stay behind the ZDR floor.
#[tokio::test]
async fn zdr_caller_can_cancel_but_not_read_errors() {
    enable_job_control();
    async fn as_zdr_user(mut req: Request, next: Next) -> Response {
        req.extensions_mut().insert(Claims {
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
            zdr: Some(true),
        });
        next.run(req).await
    }
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    let state = AppState {
        control_base_url: spawn_control(Captured::default()).await,
        ..support::base_state(driver, EventSink::new(tx))
    };
    let app = Router::new()
        .route("/v1/crawl/:id", delete(quarry_edge::job_lifecycle_routes::cancel_crawl))
        .route("/v1/crawl/:id/errors", get(quarry_edge::job_lifecycle_routes::crawl_errors))
        .layer(middleware::from_fn(as_zdr_user))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let (status, body) = send_control(addr, reqwest::Method::DELETE, "/v1/crawl/job_01J9MMMMMMMMMMMMMMMMMMMMMM").await;
    assert_eq!(status, 202, "a ZDR caller can cancel: {body}");

    let (status, body) = send_control(addr, reqwest::Method::GET, "/v1/crawl/job_01J9MMMMMMMMMMMMMMMMMMMMMM/errors").await;
    assert_eq!(status, 403, "error URLs stay behind the ZDR floor: {body}");
}
