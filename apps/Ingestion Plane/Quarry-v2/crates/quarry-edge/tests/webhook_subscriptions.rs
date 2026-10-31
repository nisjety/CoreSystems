//! Webhook subscriptions (INNH-5) through the real edge router with a control double. The dev
//! bypass identity is the org `org_placeholder`; the double answers by that org, so a foreign
//! id is a 404 from control and must reach the caller as a 404. The double records every
//! call, which is how the tests prove what control was and was not asked.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    body::Bytes,
    extract::{Request, State},
    http::{Method, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use quarry_edge::auth::{require_service_route_scope, Claims};
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};

mod support;

const TEST_BEARER: &str = "webhook-subscriptions-dev-token";
const TEST_ORG: &str = "org_placeholder";
const SUB: &str = "whk_01J9SUBSUBSUBSUBSUBSUBSUB";
const FOREIGN: &str = "whk_01J9FOREIGNFOREIGNFOREIGN";
const DELIVERY: &str = "whkd_01J9DELIVERYDELIVERYDELI";

/// The flags are process-wide: every test holds this for its whole run and sets them itself.
static FLAG: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn set_flag(on: bool) {
    if on {
        std::env::set_var("QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS", "1");
    } else {
        std::env::remove_var("QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS");
    }
}

#[derive(Debug, Clone)]
struct Call {
    method: String,
    path: String,
    query: HashMap<String, String>,
    body: Value,
}

/// What the control double saw, and whether it plays a control with the switch off.
#[derive(Clone, Default)]
struct Seen {
    calls: Arc<Mutex<Vec<Call>>>,
    unmounted: bool,
}

impl Seen {
    fn count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
    fn last(&self) -> Call {
        self.calls.lock().unwrap().last().cloned().expect("control was called")
    }
}

fn sub_json(extra: Value) -> Value {
    let mut base = json!({
        "id": SUB, "url": "https://hooks.example.com/q", "events": ["change_detected"], "active": true,
        "secret_hint": "whsec_...ab12", "created_at": "2026-10-31T10:00:00Z"
    });
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        base[k] = v;
    }
    base
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": { "code": "NOT_FOUND", "message": "not found", "details": { "id": "x" } } })),
    )
        .into_response()
}

/// One handler for every control route: it records the call and answers by method and path.
async fn control(State(seen): State<Seen>, method: Method, uri: Uri, body: Bytes) -> Response {
    let query: HashMap<String, String> = uri
        .query()
        .map(|q| url_pairs(q))
        .unwrap_or_default();
    let body_json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    seen.calls.lock().unwrap().push(Call {
        method: method.to_string(),
        path: uri.path().to_string(),
        query: query.clone(),
        body: body_json.clone(),
    });
    if seen.unmounted {
        return (StatusCode::NOT_FOUND, "404 page not found\n").into_response();
    }
    if query.get("org_id").map(String::as_str) != Some(TEST_ORG) {
        return not_found();
    }
    let path = uri.path();
    let foreign = path.contains(FOREIGN);
    match (method.as_str(), path) {
        ("POST", "/v1/webhook-subscriptions") => {
            let url = body_json["url"].as_str().unwrap_or_default();
            if url.starts_with("http://") {
                return (StatusCode::BAD_REQUEST, Json(json!({ "error": { "code": "BAD_REQUEST",
                    "message": "url must use https", "details": { "field": "url", "reason": "url_not_https", "dsn": "postgres://leak" } } }))).into_response();
            }
            if url.contains("dup") {
                return (StatusCode::CONFLICT, Json(json!({ "error": { "code": "CONFLICT",
                    "message": "a subscription with this url and events already exists", "details": { "reason": "subscription_exists" } } }))).into_response();
            }
            if url.contains("full") {
                return (StatusCode::TOO_MANY_REQUESTS, Json(json!({ "error": { "code": "RATE_LIMITED",
                    "message": "the organization is at its webhook subscription limit", "details": { "reason": "subscription_limit", "limit": 10 } } }))).into_response();
            }
            (StatusCode::CREATED, Json(json!({ "data": sub_json(json!({ "secret": "whsec_once" })) }))).into_response()
        }
        ("GET", "/v1/webhook-subscriptions") => (StatusCode::OK, Json(json!({ "data": {
            "items": [sub_json(json!({ "secret": "whsec_stray", "last_delivery": { "status": "success", "at": "2026-10-31T10:05:00Z", "event_id": "evt_1" } }))],
            "next_cursor": "abc_DEF=" } }))).into_response(),
        ("GET", _) if path.ends_with("/deliveries") => (StatusCode::OK, Json(json!({ "data": { "items": [{
            "id": DELIVERY, "event_id": "evt_1", "event_type": "change_detected", "status": "dlq", "attempt": 5,
            "last_error": "http_410", "created_at": "2026-10-31T10:00:00Z" }] } }))).into_response(),
        ("POST", _) if path.ends_with("/rotate-secret") => {
            if foreign { return not_found(); }
            (StatusCode::OK, Json(json!({ "data": { "id": SUB, "secret": "whsec_rotated", "secret_hint": "whsec_...f00d" } }))).into_response()
        }
        ("POST", _) if path.ends_with("/retry") => (StatusCode::ACCEPTED, Json(json!({ "data": {
            "id": DELIVERY, "event_id": "evt_1", "event_type": "change_detected", "status": "pending", "attempt": 0,
            "next_attempt_at": "2026-10-31T10:06:00Z", "created_at": "2026-10-31T10:00:00Z" } }))).into_response(),
        ("GET", _) => {
            if foreign { return not_found(); }
            (StatusCode::OK, Json(json!({ "data": sub_json(json!({ "secret": "whsec_stray" })) }))).into_response()
        }
        ("PATCH", _) => {
            if foreign { return not_found(); }
            (StatusCode::OK, Json(json!({ "data": sub_json(json!({ "active": false })) }))).into_response()
        }
        ("DELETE", _) => {
            if foreign { return not_found(); }
            StatusCode::NO_CONTENT.into_response()
        }
        _ => not_found(),
    }
}

fn url_pairs(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

async fn spawn_control(seen: Seen) -> String {
    let app = Router::new().fallback(control).with_state(seen);
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

async fn send(addr: SocketAddr, method: Method, path: &str, body: Option<Value>) -> (u16, Value, String) {
    let mut request = reqwest::Client::new()
        .request(method, format!("http://{addr}{path}"))
        .bearer_auth(TEST_BEARER);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.unwrap();
    let status = response.status().as_u16();
    let text = response.text().await.unwrap();
    let json = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, json, text)
}

fn create_body() -> Value {
    json!({ "url": "https://hooks.example.com/q", "events": ["change_detected"], "description": "prod" })
}

#[tokio::test]
async fn flag_off_is_501_and_control_is_not_called() {
    let _guard = FLAG.lock().await;
    set_flag(false);
    let seen = Seen::default();
    let addr = spawn_edge(seen.clone()).await;

    for (method, path, body) in [
        (Method::POST, "/v1/webhook-subscriptions".to_string(), Some(create_body())),
        (Method::GET, "/v1/webhook-subscriptions".to_string(), None),
        (Method::GET, format!("/v1/webhook-subscriptions/{SUB}"), None),
        (Method::PATCH, format!("/v1/webhook-subscriptions/{SUB}"), Some(json!({ "active": false }))),
        (Method::DELETE, format!("/v1/webhook-subscriptions/{SUB}"), None),
        (Method::POST, format!("/v1/webhook-subscriptions/{SUB}/rotate-secret"), None),
        (Method::GET, format!("/v1/webhook-subscriptions/{SUB}/deliveries"), None),
        (Method::POST, format!("/v1/webhook-subscriptions/{SUB}/deliveries/{DELIVERY}/retry"), None),
    ] {
        let (status, body, _) = send(addr, method.clone(), &path, body).await;
        assert_eq!(status, 501, "{method} {path}: {body}");
        assert_eq!(body["error"]["code"], "UNSUPPORTED");
        assert!(
            body["error"]["message"].as_str().unwrap().contains("QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS"),
            "{body}"
        );
    }
    assert_eq!(seen.count(), 0, "control was not called");
}

#[tokio::test]
async fn create_forwards_only_the_checked_fields_and_returns_the_secret_once() {
    let _guard = FLAG.lock().await;
    set_flag(true);
    let seen = Seen::default();
    let addr = spawn_edge(seen.clone()).await;

    // A caller-supplied org_id in the query is ignored: control is asked for the token's org.
    let (status, body, raw) = send(
        addr,
        Method::POST,
        "/v1/webhook-subscriptions?org_id=org_other",
        Some(create_body()),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["data"]["id"], SUB);
    assert_eq!(body["data"]["secret"], "whsec_once");
    assert_eq!(body["data"]["secret_hint"], "whsec_...ab12");
    assert_eq!(body["data"]["events"], json!(["change_detected"]));
    assert!(body["meta"]["request_id"].is_string());
    assert!(!raw.contains("postgres://"));

    let call = seen.last();
    assert_eq!((call.method.as_str(), call.path.as_str()), ("POST", "/v1/webhook-subscriptions"));
    assert_eq!(call.query.get("org_id").map(String::as_str), Some(TEST_ORG));
    assert_eq!(call.body["url"], "https://hooks.example.com/q");
    assert_eq!(call.body["description"], "prod");
    assert_eq!(call.body["created_by"], "user_placeholder", "the creator is the token's, not the body's");
    assert!(call.body.get("org_id").is_none() && call.body.get("secret").is_none(), "{}", call.body);
}

#[tokio::test]
async fn an_unknown_or_malformed_body_is_a_400_before_control_is_asked() {
    let _guard = FLAG.lock().await;
    set_flag(true);
    let seen = Seen::default();
    let addr = spawn_edge(seen.clone()).await;

    for body in [
        json!({ "url": "https://h.example/q", "events": ["change_detected"], "org_id": "org_other" }),
        json!({ "url": "https://h.example/q", "events": ["change_detected"], "secret": "whsec_mine" }),
        json!({ "url": "https://h.example/q", "events": ["change_detected"], "created_by": "someone_else" }),
        json!({ "events": ["change_detected"] }),
        json!({ "url": "https://h.example/q" }),
        json!({ "url": 7, "events": [] }),
    ] {
        let (status, out, _) = send(addr, Method::POST, "/v1/webhook-subscriptions", Some(body.clone())).await;
        assert_eq!(status, 400, "{body}: {out}");
        assert_eq!(out["error"]["details"]["reason"], "body_invalid");
    }
    let (status, out, _) = send(addr, Method::PATCH, &format!("/v1/webhook-subscriptions/{SUB}"), Some(json!({}))).await;
    assert_eq!((status, out["error"]["details"]["reason"].as_str()), (400, Some("empty_patch")));
    let (status, _, _) = send(addr, Method::PATCH, &format!("/v1/webhook-subscriptions/{SUB}"), Some(json!({ "url": "https://x.example" }))).await;
    assert_eq!(status, 400, "the url is not editable");
    assert_eq!(seen.count(), 0, "control was not called");
}

#[tokio::test]
async fn reads_come_back_typed_without_a_secret_even_if_control_sends_one() {
    let _guard = FLAG.lock().await;
    set_flag(true);
    let seen = Seen::default();
    let addr = spawn_edge(seen.clone()).await;

    let (status, body, raw) = send(addr, Method::GET, "/v1/webhook-subscriptions?limit=25&cursor=abc_DEF%3D&org_id=org_other", None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["items"][0]["id"], SUB);
    assert_eq!(body["data"]["items"][0]["last_delivery"]["status"], "success");
    assert_eq!(body["data"]["next_cursor"], "abc_DEF=");
    assert!(!raw.contains("whsec_stray") && !raw.contains("\"secret\""), "{raw}");
    let call = seen.last();
    assert_eq!(call.query.get("org_id").map(String::as_str), Some(TEST_ORG));
    assert_eq!(call.query.get("limit").map(String::as_str), Some("25"));

    let (status, body, raw) = send(addr, Method::GET, &format!("/v1/webhook-subscriptions/{SUB}"), None).await;
    assert_eq!(status, 200, "{body}");
    assert!(!raw.contains("whsec_stray") && !raw.contains("\"secret\""), "{raw}");
    assert_eq!(body["data"]["secret_hint"], "whsec_...ab12");

    let (status, body, raw) = send(addr, Method::GET, &format!("/v1/webhook-subscriptions/{SUB}/deliveries?status=dlq&limit=10"), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["items"][0]["last_error"], "http_410");
    assert!(!raw.contains("payload"));
    assert_eq!(seen.last().query.get("status").map(String::as_str), Some("dlq"));
}

#[tokio::test]
async fn patch_delete_rotate_and_retry_reach_control_with_the_token_org() {
    let _guard = FLAG.lock().await;
    set_flag(true);
    let seen = Seen::default();
    let addr = spawn_edge(seen.clone()).await;

    let (status, body, _) = send(
        addr,
        Method::PATCH,
        &format!("/v1/webhook-subscriptions/{SUB}"),
        Some(json!({ "active": false, "events": ["change_detected"] })),
    )
    .await;
    assert_eq!((status, body["data"]["active"].as_bool()), (200, Some(false)), "{body}");
    assert_eq!(seen.last().method, "PATCH");
    assert_eq!(seen.last().body, json!({ "active": false, "events": ["change_detected"] }));

    let (status, body, text) = send(addr, Method::DELETE, &format!("/v1/webhook-subscriptions/{SUB}"), None).await;
    assert_eq!(status, 204, "{body}");
    assert!(text.is_empty(), "a 204 has no body: {text:?}");

    let (status, body, _) = send(addr, Method::POST, &format!("/v1/webhook-subscriptions/{SUB}/rotate-secret"), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["secret"], "whsec_rotated");

    let (status, body, _) = send(
        addr,
        Method::POST,
        &format!("/v1/webhook-subscriptions/{SUB}/deliveries/{DELIVERY}/retry"),
        None,
    )
    .await;
    assert_eq!(status, 202, "{body}");
    assert_eq!(body["data"]["status"], "pending");
    for call in seen.calls.lock().unwrap().iter() {
        assert_eq!(call.query.get("org_id").map(String::as_str), Some(TEST_ORG), "{call:?}");
    }
}

#[tokio::test]
async fn a_foreign_or_unknown_id_is_a_404_and_a_malformed_one_never_reaches_control() {
    let _guard = FLAG.lock().await;
    set_flag(true);
    let seen = Seen::default();
    let addr = spawn_edge(seen.clone()).await;

    for method in [Method::GET, Method::DELETE] {
        let (status, body, _) = send(addr, method.clone(), &format!("/v1/webhook-subscriptions/{FOREIGN}"), None).await;
        assert_eq!(status, 404, "{method}: {body}");
        assert_eq!(body["error"]["code"], "NOT_FOUND");
        assert!(body["error"]["details"].is_null(), "control's details are not echoed: {body}");
    }
    let before = seen.count();
    for path in [
        "/v1/webhook-subscriptions/not-an-id",
        "/v1/webhook-subscriptions/whk_..%2Fjobs",
        "/v1/webhook-subscriptions/wh_01J9",
        &format!("/v1/webhook-subscriptions/{SUB}/deliveries/not-a-delivery/retry"),
        &format!("/v1/webhook-subscriptions/{SUB}/deliveries/{SUB}/retry"),
    ] {
        let method = if path.ends_with("/retry") { Method::POST } else { Method::GET };
        let (status, body, _) = send(addr, method, path, None).await;
        assert_eq!(status, 404, "{path}: {body}");
    }
    assert_eq!(seen.count(), before, "malformed ids never reach control");

    for query in ["limit=0", "limit=201", "cursor=a%20b", "status=weird"] {
        let path = format!("/v1/webhook-subscriptions/{SUB}/deliveries?{query}");
        let (status, body, _) = send(addr, Method::GET, &path, None).await;
        assert_eq!(status, 400, "{query}: {body}");
    }
    assert_eq!(seen.count(), before, "a bad query never reaches control");
}

#[tokio::test]
async fn control_validation_reasons_pass_through_and_nothing_else_does() {
    let _guard = FLAG.lock().await;
    set_flag(true);
    let seen = Seen::default();
    let addr = spawn_edge(seen.clone()).await;

    let mk = |url: &str| json!({ "url": url, "events": ["change_detected"] });
    let (status, body, raw) = send(addr, Method::POST, "/v1/webhook-subscriptions", Some(mk("http://hooks.example.com/q"))).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"]["details"]["field"], "url");
    assert_eq!(body["error"]["details"]["reason"], "url_not_https");
    assert!(!raw.contains("postgres://leak"), "{raw}");

    let (status, body, _) = send(addr, Method::POST, "/v1/webhook-subscriptions", Some(mk("https://dup.example.com/q"))).await;
    assert_eq!((status, body["error"]["code"].as_str()), (409, Some("CONFLICT")), "{body}");
    assert_eq!(body["error"]["details"]["reason"], "subscription_exists");

    let (status, body, _) = send(addr, Method::POST, "/v1/webhook-subscriptions", Some(mk("https://full.example.com/q"))).await;
    assert_eq!((status, body["error"]["code"].as_str()), (429, Some("RATE_LIMITED")), "{body}");
    assert_eq!(body["error"]["details"]["limit"], 10);
}

#[tokio::test]
async fn a_control_with_the_switch_off_is_501_naming_the_switch() {
    let _guard = FLAG.lock().await;
    set_flag(true);
    let seen = Seen { unmounted: true, ..Seen::default() };
    let addr = spawn_edge(seen.clone()).await;

    for (method, path) in [
        (Method::GET, "/v1/webhook-subscriptions".to_string()),
        (Method::GET, format!("/v1/webhook-subscriptions/{SUB}")),
        (Method::DELETE, format!("/v1/webhook-subscriptions/{SUB}")),
    ] {
        let (status, body, _) = send(addr, method.clone(), &path, None).await;
        assert_eq!(status, 501, "{method} {path}: {body}");
        assert!(
            body["error"]["message"].as_str().unwrap().contains("QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS"),
            "{body}"
        );
    }
}

/// A router with just the subscription handlers and a chosen identity, for the token tests.
fn router_as(claims: Claims, state: AppState) -> Router {
    async fn inject(claims: Claims, mut req: Request, next: Next) -> Response {
        req.extensions_mut().insert(claims);
        next.run(req).await
    }
    use quarry_edge::webhook_subscription_routes as w;
    Router::new()
        .route("/v1/webhook-subscriptions", get(w::list).post(w::create))
        .route(
            "/v1/webhook-subscriptions/:id",
            get(w::get).patch(w::patch).delete(w::delete),
        )
        .route("/v1/webhook-subscriptions/:id/rotate-secret", post(w::rotate_secret))
        .route("/v1/webhook-subscriptions/:id/deliveries", get(w::deliveries))
        .route("/v1/webhook-subscriptions/:id/deliveries/:did/retry", post(w::retry_delivery))
        .layer(middleware::from_fn(move |req: Request, next: Next| inject(claims.clone(), req, next)))
        .with_state(state)
}

fn claims(principal: &str, zdr: Option<bool>) -> Claims {
    let service = principal == "service";
    Claims {
        web_policy: None,
        sub: if service { "service:model-gateway" } else { "user_placeholder" }.into(),
        iss: "auth-core".into(),
        exp: i64::MAX,
        org_id: TEST_ORG.into(),
        user_id: if service { "service:model-gateway" } else { "user_placeholder" }.into(),
        principal_type: Some(principal.into()),
        service_id: service.then(|| "service:model-gateway".to_string()),
        nbf: None,
        aud: None,
        scopes: if service { vec!["scrape:read".into(), "browser:execute".into()] } else { Vec::new() },
        zdr,
        key_id: None,
        acting_user_id: None,
    }
}

#[tokio::test]
async fn a_zdr_token_cannot_create_patch_or_retry_and_control_is_not_called() {
    let _guard = FLAG.lock().await;
    set_flag(true);
    let seen = Seen::default();
    let state = state_for(spawn_control(seen.clone()).await);
    let addr = serve(router_as(claims("user", Some(true)), state)).await;

    for (method, path, body) in [
        (Method::POST, "/v1/webhook-subscriptions".to_string(), Some(create_body())),
        (Method::PATCH, format!("/v1/webhook-subscriptions/{SUB}"), Some(json!({ "active": true }))),
        (Method::POST, format!("/v1/webhook-subscriptions/{SUB}/deliveries/{DELIVERY}/retry"), None),
    ] {
        let (status, out, _) = send(addr, method.clone(), &path, body).await;
        assert_eq!(status, 403, "{method} {path}: {out}");
        assert_eq!(out["error"]["code"], "FORBIDDEN");
        assert_eq!(out["error"]["details"]["reason"], "webhook_off_process");
    }
    assert_eq!(seen.count(), 0, "the refusal comes before control");

    // Wind-down stays open: read, delete and rotate a secret of a subscription already held.
    for (method, path, want) in [
        (Method::GET, "/v1/webhook-subscriptions".to_string(), 200),
        (Method::GET, format!("/v1/webhook-subscriptions/{SUB}"), 200),
        (Method::GET, format!("/v1/webhook-subscriptions/{SUB}/deliveries"), 200),
        (Method::DELETE, format!("/v1/webhook-subscriptions/{SUB}"), 204),
        (Method::POST, format!("/v1/webhook-subscriptions/{SUB}/rotate-secret"), 200),
    ] {
        let (status, out, _) = send(addr, method.clone(), &path, None).await;
        assert_eq!(status, want, "{method} {path}: {out}");
    }
}

#[tokio::test]
async fn a_service_token_is_403_on_the_subscription_paths() {
    let _guard = FLAG.lock().await;
    set_flag(true);
    // The route scope denies it by default; the handler denies it again.
    async fn ok() -> &'static str {
        "ok"
    }
    async fn as_model_gateway(mut req: Request, next: Next) -> Response {
        req.extensions_mut().insert(claims("service", Some(true)));
        next.run(req).await
    }
    let scoped = Router::new()
        .route("/v1/webhook-subscriptions", get(ok).post(ok))
        .route("/v1/webhook-subscriptions/:id", get(ok).patch(ok).delete(ok))
        .route("/v1/webhook-subscriptions/:id/rotate-secret", post(ok))
        .route("/v1/webhook-subscriptions/:id/deliveries", get(ok))
        .route("/v1/webhook-subscriptions/:id/deliveries/:did/retry", post(ok))
        .layer(middleware::from_fn(require_service_route_scope))
        .layer(middleware::from_fn(as_model_gateway));
    let addr = serve(scoped).await;
    for (method, path) in [
        (Method::GET, "/v1/webhook-subscriptions".to_string()),
        (Method::POST, "/v1/webhook-subscriptions".to_string()),
        (Method::GET, format!("/v1/webhook-subscriptions/{SUB}")),
        (Method::DELETE, format!("/v1/webhook-subscriptions/{SUB}")),
        (Method::GET, format!("/v1/webhook-subscriptions/{SUB}/deliveries")),
    ] {
        let (status, _, _) = send(addr, method.clone(), &path, None).await;
        assert_eq!(status, 403, "{method} {path}: default-deny for a service principal");
    }

    let seen = Seen::default();
    let state = state_for(spawn_control(seen.clone()).await);
    let addr = serve(router_as(claims("service", None), state)).await;
    let (status, out, _) = send(addr, Method::GET, "/v1/webhook-subscriptions", None).await;
    assert_eq!(status, 403, "{out}");
    assert_eq!(seen.count(), 0);
}
