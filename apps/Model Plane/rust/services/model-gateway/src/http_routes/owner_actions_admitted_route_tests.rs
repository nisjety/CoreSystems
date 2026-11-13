//! W3-EP-6-S11-route (plan 3.2): `GET /v1/capabilities/owner-actions/admitted` is a proxy route.
//! It forwards the caller's own capability bearer to capability-core's
//! `GET /api/v1/capabilities/owner-actions/admitted` and relays the answer (success and refusal)
//! as capability-core gave it, adding no answer of its own. The literal path must reach this
//! handler, not the `/v1/capabilities/:id` capability read.

use super::*;

use std::sync::{Arc, Mutex};

use axum::{body::Body, http::Request as HttpRequest};
use tokio::net::TcpListener;
use tower::ServiceExt;

/// What the stand-in capability-core saw: `METHOD path?query`, the Authorization header and the body.
type Seen = Arc<Mutex<Vec<(String, Option<String>, String)>>>;

fn claims(zdr: bool) -> Claims {
    serde_json::from_value(json!({
        "sub": "user-a", "iss": "auth-core", "exp": 4_102_444_800_i64, "org_id": "org-a",
        "user_id": "user-a", "principal_type": "user", "zdr": zdr
    }))
    .expect("claims")
}

/// A capability-core that answers every request with `status` and `answer`, recording each request.
async fn capability_core(
    status: StatusCode,
    answer: Value,
) -> (String, Seen, tokio::task::JoinHandle<()>) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new().fallback(axum::routing::any({
        let seen = Arc::clone(&seen);
        move |method: axum::http::Method,
              uri: axum::http::Uri,
              headers: axum::http::HeaderMap,
              body: String| {
            let seen = Arc::clone(&seen);
            let answer = answer.clone();
            async move {
                let authorization = headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                seen.lock().expect("capability-core recorder lock").push((
                    format!("{method} {uri}"),
                    authorization,
                    body,
                ));
                (status, Json(answer))
            }
        }
    }));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind capability-core");
    let address = listener.local_addr().expect("capability-core address");
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve capability-core");
    });
    (format!("http://{address}"), seen, task)
}

/// The production proxy routes behind the auth layer's two extensions (the claims and the
/// verified capability bearer), as the auth-gated router mounts them.
fn app(base_url: String, zdr: bool, bearer: Option<&str>) -> Router {
    let mut state = AppState::new();
    state.capability_core_base_url = base_url;
    let mut router = proxy_routes().layer(Extension(claims(zdr)));
    if let Some(bearer) = bearer {
        router = router.layer(Extension(VerifiedCapabilityBearer::for_test(bearer)));
    }
    router.with_state(state)
}

async fn get(router: Router) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            HttpRequest::builder()
                .method("GET")
                .uri("/v1/capabilities/owner-actions/admitted")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("route answers");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn admitted_owner_actions_forward_the_callers_bearer_and_relay_the_answer() {
    let answer =
        json!({"data": {"mode": "table", "action_ids": ["calendar.create", "tickets.create"]}});
    let (base_url, seen, task) = capability_core(StatusCode::OK, answer.clone()).await;

    let (status, body) = get(app(base_url, false, Some("caller-capability-bearer"))).await;
    task.abort();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body, answer,
        "the gateway relays capability-core's answer unchanged"
    );
    let seen = seen.lock().expect("lock").clone();
    assert_eq!(seen.len(), 1, "exactly one upstream request: {seen:?}");
    let (line, authorization, upstream_body) = &seen[0];
    assert_eq!(line, "GET /api/v1/capabilities/owner-actions/admitted");
    assert_eq!(
        authorization.as_deref(),
        Some("Bearer caller-capability-bearer"),
        "the caller's own capability bearer, not a gateway credential"
    );
    assert!(
        upstream_body.is_empty(),
        "a GET forwards no body: {upstream_body:?}"
    );
}

#[tokio::test]
async fn admitted_owner_actions_relay_a_capability_core_refusal() {
    // capability-core refuses a ZDR principal (403); the gateway relays the refusal and its envelope,
    // and does not answer a mode of its own.
    let refusal = json!({"error": {"code": "zdr_refused", "message": "refused"}});
    let (base_url, seen, task) = capability_core(StatusCode::FORBIDDEN, refusal.clone()).await;

    let (status, body) = get(app(base_url, true, Some("zdr-caller-bearer"))).await;
    task.abort();

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, refusal);
    assert!(
        body.pointer("/data/mode").is_none(),
        "the gateway added a mode to a refusal: {body}"
    );
    assert_eq!(seen.lock().expect("lock").len(), 1);
}

#[tokio::test]
async fn admitted_owner_actions_without_a_verified_bearer_forward_nothing() {
    let (base_url, seen, task) = capability_core(
        StatusCode::OK,
        json!({"data": {"mode": "tickets_only", "action_ids": ["tickets.create"]}}),
    )
    .await;

    let (status, _) = get(app(base_url, false, None)).await;
    task.abort();

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        seen.lock().expect("lock").is_empty(),
        "a request without a verified capability bearer reached capability-core"
    );
}

#[tokio::test]
async fn admitted_owner_actions_unreachable_capability_core_is_a_bad_gateway() {
    // Nothing listens on the reserved port: the proxy answers 502 rather than a default list.
    let (status, body) = get(app(
        "http://127.0.0.1:9".to_owned(),
        false,
        Some("caller-capability-bearer"),
    ))
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(body.pointer("/data").is_none(), "{body}");
}
