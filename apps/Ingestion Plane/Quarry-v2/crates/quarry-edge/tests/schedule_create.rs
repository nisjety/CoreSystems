//! WS15.S1: the edge's `POST /v1/schedules` maps an executable schedule (scrape,
//! crawl, batch) onto control's create body, refuses a definition it cannot run
//! before control sees it, stamps the org from the verified claim, and starts a
//! schedule only for a user token. A fake control plane records what it receives.
//!
//! Auth is the dev bypass (`QUARRY_EDGE_AUTH_DEV_BYPASS=1`), which injects a user
//! stub (`org_placeholder`). The service-token case builds the handler behind a
//! middleware that injects service claims, as `job_lifecycle.rs` does.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    extract::{Query, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use quarry_edge::auth::Claims;
use quarry_edge::schedule_routes::create_schedule;
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};

mod support;

const TEST_BEARER: &str = "schedule-create-dev-token";
const TEST_ORG: &str = "org_placeholder";

/// What control received: the query (org_id) and the JSON body, per call.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<(HashMap<String, String>, Value)>>>);

impl Captured {
    fn calls(&self) -> Vec<(HashMap<String, String>, Value)> {
        self.0.lock().unwrap().clone()
    }
}

async fn control_create(
    State(captured): State<Captured>,
    Query(query): Query<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    captured.0.lock().unwrap().push((query, body));
    (
        StatusCode::CREATED,
        Json(json!({ "data": { "schedule_id": "sch_new", "status": "paused", "needs_activation": true } })),
    )
}

/// A control plane that refuses every create the way control does: a 400 whose
/// error carries a stable reason in details.
async fn control_refuses(Json(_body): Json<Value>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": {
            "code": "BAD_REQUEST",
            "message": "timezone \"Mars/Olympus\" is not an IANA zone",
            "details": { "reason": "invalid_timezone" }
        } })),
    )
}

async fn spawn_control(captured: Captured) -> String {
    let app = Router::new()
        .route("/v1/schedules", post(control_create))
        .with_state(captured);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn spawn_refusing_control() -> String {
    let app = Router::new().route("/v1/schedules", post(control_refuses));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn edge_state(control_url: String, refuse_zdr: bool) -> AppState {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    AppState {
        control_base_url: control_url,
        zdr_refuse_durable_resources: refuse_zdr,
        ..support::base_state(driver, EventSink::new(tx))
    }
}

async fn spawn_edge(control_url: String, refuse_zdr: bool) -> SocketAddr {
    let app = quarry_edge::routes::router(edge_state(control_url, refuse_zdr));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn post_schedule(addr: SocketAddr, body: Value) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v1/schedules"))
        .bearer_auth(TEST_BEARER)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(Value::Null))
}

fn reason_of(body: &Value) -> &str {
    body["error"]["details"]["reason"].as_str().unwrap_or_default()
}

fn crawl_body() -> Value {
    json!({
        "name": "Nightly crawl",
        "kind": "crawl",
        "cron": "0 7 * * *",
        "timezone": "Europe/Oslo",
        "overlap_policy": "cancel",
        "catchup_window_s": 600,
        "pause_on_failure": true,
        "config": { "url": "https://example.com/docs", "max_pages": 20 }
    })
}

#[tokio::test]
async fn create_crawl_schedule_maps_kind_config_and_stamps_org() {
    let captured = Captured::default();
    let addr = spawn_edge(spawn_control(captured.clone()).await, false).await;

    let (status, body) = post_schedule(addr, crawl_body()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["needs_activation"], true, "{body}");

    let calls = captured.calls();
    assert_eq!(calls.len(), 1, "one forwarded create: {calls:?}");
    let (_, forwarded) = &calls[0];
    assert_eq!(forwarded["target_kind"], "crawl");
    assert_eq!(forwarded["target_ref"], "https://example.com/docs");
    assert_eq!(forwarded["cron"], "0 7 * * *");
    assert_eq!(forwarded["name"], "Nightly crawl");
    assert_eq!(forwarded["timezone"], "Europe/Oslo");
    assert_eq!(forwarded["overlap_policy"], "cancel");
    assert_eq!(forwarded["catchup_window_s"], 600);
    assert_eq!(forwarded["pause_on_failure"], true);
    assert_eq!(forwarded["config"]["max_pages"], 20);
    assert_eq!(forwarded["org_id"], TEST_ORG);
    assert_eq!(forwarded["activate"], false, "not activated unless asked");
    assert!(forwarded.get("zdr").is_none(), "a legacy executable create carries no stamp");
}

#[tokio::test]
async fn schedule_routes_forward_org_id_query() {
    let captured = Captured::default();
    let addr = spawn_edge(spawn_control(captured.clone()).await, false).await;
    assert_eq!(post_schedule(addr, crawl_body()).await.0, 200);
    let (query, _) = &captured.calls()[0];
    assert_eq!(query.get("org_id").map(String::as_str), Some(TEST_ORG));
}

#[tokio::test]
async fn activate_is_forwarded_for_a_user_token() {
    let captured = Captured::default();
    let addr = spawn_edge(spawn_control(captured.clone()).await, false).await;
    let mut body = crawl_body();
    body["activate"] = json!(true);
    assert_eq!(post_schedule(addr, body).await.0, 200);
    assert_eq!(captured.calls()[0].1["activate"], true);
}

#[tokio::test]
async fn activate_ignored_for_service_token() {
    async fn as_service(mut req: Request, next: Next) -> Response {
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
            scopes: Vec::new(),
            zdr: None,
        });
        next.run(req).await
    }
    let captured = Captured::default();
    let control_url = spawn_control(captured.clone()).await;
    let app = Router::new()
        .route("/v1/schedules", post(create_schedule))
        .layer(middleware::from_fn(as_service))
        .with_state(edge_state(control_url, false));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let mut body = crawl_body();
    body["activate"] = json!(true);
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v1/schedules"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(captured.calls()[0].1["activate"], false, "a service token cannot start a schedule");
}

#[tokio::test]
async fn change_monitor_create_is_stamped_and_starts_as_before() {
    let captured = Captured::default();
    let addr = spawn_edge(spawn_control(captured.clone()).await, false).await;
    let (status, body) = post_schedule(
        addr,
        json!({ "name": "Pricing", "preset": "daily", "target_ref": "https://example.com/pricing", "zdr": false }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let (_, forwarded) = &captured.calls()[0];
    assert_eq!(forwarded["target_kind"], "change_monitor");
    assert_eq!(forwarded["preset"], "daily");
    assert_eq!(forwarded["activate"], true, "a monitor starts when created, as before WS15.S1");
    assert_eq!(forwarded["zdr"], false, "the monitor carries the posture stamp");
    assert!(forwarded.get("cron").is_none(), "control derives the cron from the preset");
}

#[tokio::test]
async fn refused_for_zdr_when_enforced_and_never_reaches_control() {
    let captured = Captured::default();
    let addr = spawn_edge(spawn_control(captured.clone()).await, true).await;
    let mut body = crawl_body();
    body["zdr"] = json!(true);
    let (status, refused) = post_schedule(addr, body).await;
    assert_eq!(status, 403, "{refused}");
    assert_eq!(refused["error"]["details"]["zdr_refusal"]["action"], "create", "{refused}");
    assert!(captured.calls().is_empty(), "a refused create must never reach control");
}

#[tokio::test]
async fn zdr_declaration_is_only_observed_when_the_flag_is_off() {
    let captured = Captured::default();
    let addr = spawn_edge(spawn_control(captured.clone()).await, false).await;
    let mut body = crawl_body();
    body["zdr"] = json!(true);
    assert_eq!(post_schedule(addr, body).await.0, 200);
    let calls = captured.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].1.get("zdr").is_none(), "a legacy create never forwards the declaration");
}

#[tokio::test]
async fn schedule_at_is_400() {
    let captured = Captured::default();
    let addr = spawn_edge(spawn_control(captured.clone()).await, false).await;
    let mut body = crawl_body();
    body["schedule_at"] = json!("2026-11-01T07:00:00Z");
    body.as_object_mut().unwrap().remove("cron");
    let (status, got) = post_schedule(addr, body).await;
    assert_eq!(status, 400, "{got}");
    assert_eq!(reason_of(&got), "unsupported_schedule_at", "{got}");
    assert!(captured.calls().is_empty());
}

#[tokio::test]
async fn six_field_cron_is_400_at_edge() {
    let captured = Captured::default();
    let addr = spawn_edge(spawn_control(captured.clone()).await, false).await;
    let mut body = crawl_body();
    body["cron"] = json!("0 0 7 * * *");
    let (status, got) = post_schedule(addr, body).await;
    assert_eq!(status, 400, "{got}");
    assert_eq!(reason_of(&got), "invalid_cron", "{got}");
    assert!(captured.calls().is_empty());
}

#[tokio::test]
async fn unsupported_kind_extract_is_400() {
    let captured = Captured::default();
    let addr = spawn_edge(spawn_control(captured.clone()).await, false).await;
    let mut body = crawl_body();
    body["kind"] = json!("extract");
    let (status, got) = post_schedule(addr, body).await;
    assert_eq!(status, 400, "{got}");
    assert_eq!(reason_of(&got), "unsupported_schedule_kind", "{got}");
    assert!(captured.calls().is_empty());
}

#[tokio::test]
async fn scrape_without_a_url_is_400() {
    let captured = Captured::default();
    let addr = spawn_edge(spawn_control(captured.clone()).await, false).await;
    let (status, got) = post_schedule(
        addr,
        json!({ "kind": "scrape", "cron": "*/5 * * * *", "config": {} }),
    )
    .await;
    assert_eq!(status, 400, "{got}");
    assert_eq!(reason_of(&got), "invalid_config", "{got}");
    assert!(captured.calls().is_empty());
}

#[tokio::test]
async fn control_refusal_keeps_its_reason() {
    let addr = spawn_edge(spawn_refusing_control().await, false).await;
    let (status, got) = post_schedule(addr, crawl_body()).await;
    assert_eq!(status, 400, "{got}");
    assert_eq!(
        reason_of(&got),
        "invalid_timezone",
        "the control reason must reach the caller: {got}"
    );
}
