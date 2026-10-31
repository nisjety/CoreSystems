//! Per-run results (INNH-12) through the real edge router with a control double. The dev
//! bypass identity is the org `org_placeholder`; the double answers by that org, so a foreign
//! id is a 404 from control and must reach the caller as a 404.

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

use quarry_edge::auth::{require_service_route_scope, Claims};
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};

mod support;

const TEST_BEARER: &str = "job-results-dev-token";
const TEST_ORG: &str = "org_placeholder";
const CRAWL: &str = "job_01J9MMMMMMMMMMMMMMMMMMMMMM";
const BATCH: &str = "job_01J9BBBBBBBBBBBBBBBBBBBBBB";
const FOREIGN: &str = "job_01J9FFFFFFFFFFFFFFFFFFFFFF";

/// The flag is process-wide: every test holds this for its whole run and sets it itself.
static FLAG: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn set_flag(on: bool) {
    if on {
        std::env::set_var("QUARRY_EDGE__JOB_RESULTS", "1");
    } else {
        std::env::remove_var("QUARRY_EDGE__JOB_RESULTS");
    }
}

/// What the control double saw: the query of every results read, and the summary reads.
#[derive(Clone, Default)]
struct Seen {
    results_queries: Arc<Mutex<Vec<HashMap<String, String>>>>,
    summary_reads: Arc<Mutex<u32>>,
}

async fn control_summary(
    State(seen): State<Seen>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    *seen.summary_reads.lock().unwrap() += 1;
    if query.get("org_id").map(String::as_str) != Some(TEST_ORG) || id == FOREIGN {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "data": null, "error": { "code": "NOT_FOUND", "message": "not found" } })),
        );
    }
    let kind = if id == BATCH { "batch" } else { "crawl" };
    (
        StatusCode::OK,
        Json(json!({ "data": {
            "job": { "id": id, "org_id": TEST_ORG, "kind": kind, "status": "completed",
                     "created_at": "2026-10-23T10:00:00Z", "desired_seq": 0, "applied_seq": 0 },
            "phase": "completed",
            "counts": { "completed": 2, "failed": 0, "blocked": 0, "retried": 0, "extracted": 0 },
            "total": 2, "total_kind": "exact", "last_seq": 4, "warnings": []
        } })),
    )
}

async fn control_results(
    State(seen): State<Seen>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    seen.results_queries.lock().unwrap().push(query.clone());
    if query.get("org_id").map(String::as_str) != Some(TEST_ORG) || id == FOREIGN {
        return (StatusCode::NOT_FOUND, Json(json!({ "data": null })));
    }
    (
        StatusCode::OK,
        Json(json!({ "data": {
            "items": [
                { "seq": 1, "url": "https://e.test/a", "status": 200, "content_type": "text/html",
                  "title": "A", "fetched_at": "2026-10-23T10:00:01Z", "word_count": 120,
                  "driver": "static", "markdown_artifact_id": "art_01MD", "source_artifact_id": "art_01SRC",
                  "bytes": 900 },
                { "seq": 3, "url": "https://e.test/b", "status": 200, "fetched_at": "2026-10-23T10:00:02Z" }
            ],
            "next_seq": 3
        } })),
    )
}

async fn spawn_control(seen: Seen) -> String {
    let app = Router::new()
        .route("/v1/jobs/:id/summary", get(control_summary))
        .route("/v1/jobs/:id/results", get(control_results))
        .with_state(seen);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn state_for(control_url: String) -> AppState {
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    AppState {
        control_base_url: control_url,
        ..support::base_state(driver, EventSink::new(tx))
    }
}

async fn serve(app: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

/// The real router with the dev bypass identity (a non-ZDR user of `org_placeholder`).
async fn spawn_edge(seen: Seen) -> SocketAddr {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    let state = state_for(spawn_control(seen).await);
    serve(quarry_edge::routes::router(state)).await
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
async fn crawl_results_pass_through_with_the_page_meta() {
    let _guard = FLAG.lock().await;
    set_flag(true);
    let seen = Seen::default();
    let addr = spawn_edge(seen.clone()).await;

    let (status, body) = get_json(addr, &format!("/v1/crawl/{CRAWL}/results")).await;
    assert_eq!(status, 200, "{body}");
    let data = &body["data"];
    assert_eq!(data["items"].as_array().map(Vec::len), Some(2), "{body}");
    assert_eq!(data["items"][0]["url"], "https://e.test/a");
    assert_eq!(data["items"][0]["markdown_artifact_id"], "art_01MD");
    assert_eq!(data["items"][0]["source_artifact_id"], "art_01SRC");
    assert_eq!(data["items"][0]["bytes"], 900);
    assert_eq!(data["items"][0]["fetched_at"], "2026-10-23T10:00:01Z");
    // A row without the new fields carries no key for them, not a null.
    assert!(
        data["items"][1].get("markdown_artifact_id").is_none(),
        "{body}"
    );
    assert_eq!(data["next_seq"], 3);
    assert_eq!(body["meta"]["page"]["next"], "3");
    assert_eq!(body["meta"]["page"]["limit"], 100);
    assert!(body["meta"]["request_id"].is_string());

    // The alias on the job id reads the same rows, whatever the kind.
    let (status, body) = get_json(addr, &format!("/v1/jobs/{BATCH}/results")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["items"].as_array().map(Vec::len), Some(2));
    let (status, _) = get_json(addr, &format!("/v1/batch/{BATCH}/results")).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn the_query_reaches_control_checked_and_the_org_comes_from_the_token() {
    let _guard = FLAG.lock().await;
    set_flag(true);
    let seen = Seen::default();
    let addr = spawn_edge(seen.clone()).await;

    // A caller-supplied org_id is ignored: control is asked for the token's org.
    let (status, body) = get_json(
        addr,
        &format!("/v1/crawl/{CRAWL}/results?limit=25&after_seq=7&latest=true&org_id=org_other"),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["meta"]["page"]["limit"], 25);
    let queries = seen.results_queries.lock().unwrap();
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].get("org_id").map(String::as_str), Some(TEST_ORG));
    assert_eq!(queries[0].get("limit").map(String::as_str), Some("25"));
    assert_eq!(queries[0].get("after_seq").map(String::as_str), Some("7"));
    assert_eq!(queries[0].get("latest").map(String::as_str), Some("true"));
}

#[tokio::test]
async fn a_bad_query_is_a_400_before_control_is_asked() {
    let _guard = FLAG.lock().await;
    set_flag(true);
    let seen = Seen::default();
    let addr = spawn_edge(seen.clone()).await;

    for query in [
        "limit=0",
        "limit=501",
        "limit=abc",
        "after_seq=-1",
        "latest=maybe",
    ] {
        let (status, body) = get_json(addr, &format!("/v1/crawl/{CRAWL}/results?{query}")).await;
        assert_eq!(status, 400, "{query}: {body}");
        assert_eq!(body["error"]["code"], "BAD_REQUEST");
    }
    assert_eq!(
        *seen.summary_reads.lock().unwrap(),
        0,
        "control was not called"
    );
}

#[tokio::test]
async fn kind_mismatch_foreign_and_malformed_ids_are_404() {
    let _guard = FLAG.lock().await;
    set_flag(true);
    let seen = Seen::default();
    let addr = spawn_edge(seen.clone()).await;

    let (status, body) = get_json(addr, &format!("/v1/batch/{CRAWL}/results")).await;
    assert_eq!(status, 404, "a crawl id on the batch path: {body}");
    let (status, body) = get_json(addr, &format!("/v1/crawl/{BATCH}/results")).await;
    assert_eq!(status, 404, "a batch id on the crawl path: {body}");
    let (status, body) = get_json(addr, &format!("/v1/crawl/{FOREIGN}/results")).await;
    assert_eq!(status, 404, "a foreign or unknown id: {body}");
    let before = *seen.summary_reads.lock().unwrap();
    let (status, body) = get_json(addr, "/v1/jobs/not-a-job-id/results").await;
    assert_eq!(status, 404, "an id that is not a job id: {body}");
    assert_eq!(
        *seen.summary_reads.lock().unwrap(),
        before,
        "never reaches control"
    );
    assert!(
        seen.results_queries.lock().unwrap().is_empty(),
        "no rows were read for any of them"
    );
}

#[tokio::test]
async fn flag_off_is_501_and_control_is_not_called() {
    let _guard = FLAG.lock().await;
    set_flag(false);
    let seen = Seen::default();
    let addr = spawn_edge(seen.clone()).await;

    for path in [
        format!("/v1/crawl/{CRAWL}/results"),
        format!("/v1/batch/{BATCH}/results"),
        format!("/v1/jobs/{CRAWL}/results"),
    ] {
        let (status, body) = get_json(addr, &path).await;
        assert_eq!(status, 501, "{path}: {body}");
        assert_eq!(body["error"]["code"], "UNSUPPORTED");
    }
    assert_eq!(*seen.summary_reads.lock().unwrap(), 0);
}

#[tokio::test]
async fn a_zdr_token_is_403_with_the_reason_and_control_is_not_called() {
    let _guard = FLAG.lock().await;
    set_flag(true);
    async fn as_zdr_user(mut req: Request, next: Next) -> Response {
        req.extensions_mut().insert(Claims {
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
            zdr: Some(true),
            key_id: None,
            acting_user_id: None,
        });
        next.run(req).await
    }
    let seen = Seen::default();
    let state = state_for(spawn_control(seen.clone()).await);
    let app = Router::new()
        .route(
            "/v1/crawl/:id/results",
            get(quarry_edge::job_results_routes::crawl_results),
        )
        .route(
            "/v1/batch/:id/results",
            get(quarry_edge::job_results_routes::batch_results),
        )
        .route(
            "/v1/jobs/:id/results",
            get(quarry_edge::job_results_routes::job_results),
        )
        .layer(middleware::from_fn(as_zdr_user))
        .with_state(state);
    let addr = serve(app).await;

    for path in [
        format!("/v1/crawl/{CRAWL}/results"),
        format!("/v1/batch/{BATCH}/results"),
        format!("/v1/jobs/{CRAWL}/results"),
    ] {
        let (status, body) = get_json(addr, &path).await;
        assert_eq!(status, 403, "{path}: {body}");
        assert_eq!(body["error"]["code"], "FORBIDDEN");
        assert_eq!(body["error"]["details"]["reason"], "job_result_urls");
    }
    assert_eq!(
        *seen.summary_reads.lock().unwrap(),
        0,
        "the refusal comes before control"
    );
    assert!(seen.results_queries.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_service_token_is_403_on_the_results_paths() {
    async fn ok() -> &'static str {
        "ok"
    }
    async fn as_model_gateway(mut req: Request, next: Next) -> Response {
        req.extensions_mut().insert(Claims {
            web_policy: None,
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
            key_id: None,
            acting_user_id: None,
        });
        next.run(req).await
    }
    let app = Router::new()
        .route("/v1/crawl/:id/results", get(ok))
        .route("/v1/batch/:id/results", get(ok))
        .route("/v1/jobs/:id/results", get(ok))
        .layer(middleware::from_fn(require_service_route_scope))
        .layer(middleware::from_fn(as_model_gateway));
    let addr = serve(app).await;

    for path in ["crawl", "batch", "jobs"] {
        let url = format!("http://{addr}/v1/{path}/job_01J9AAAAAAAAAAAAAAAAAAAAAA/results");
        let status = reqwest::get(url).await.unwrap().status();
        assert_eq!(
            status.as_u16(),
            403,
            "{path}: default-deny for a service principal"
        );
    }
}
