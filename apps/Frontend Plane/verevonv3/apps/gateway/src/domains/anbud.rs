//! Tender matching for the Verevon app: the anbud core's demo routes, reached through the gateway.
//!
//! The browser can browse the public notice catalogue without a company profile, or call the
//! profile matching, audit, re-tender radar and register lookup routes. The organisation is the one the session is in, never a request
//! header, and the core is called with the gateway's own key for it. A body over 64 KiB is refused here, and
//! a notice id in the path must be a plain identifier, so nothing in it can change the path it is joined
//! into. The core's answers pass through (see `upstream::proxy_anbud_json`).

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::error,
    middleware::{require_session, AuthenticatedUser},
    upstream::{authorized_org_id, proxy_anbud_json, proxy_json},
};

/// The core refuses bodies over this size itself; the gateway refuses them first.
const MAX_BODY_BYTES: usize = 64 << 10;

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/anbud/notices/search", post(search_catalogue))
        .route("/api/v1/anbud/notices/{notice_id}", post(catalogue_detail))
        .route("/api/v1/anbud/matches", post(search_matches))
        .route("/api/v1/anbud/matches/{notice_id}", post(notice_audit))
        .route("/api/v1/anbud/retenders", post(re_tenders))
        .route("/api/v1/anbud/company", post(company_lookup))
        .route("/api/v1/anbud/workspace/profile/read", post(workspace_profile_read))
        .route("/api/v1/anbud/workspace/profile/write", post(workspace_profile_write))
        .route("/api/v1/anbud/workspace/pipeline/read", post(workspace_pipeline_read))
        .route("/api/v1/anbud/workspace/pipeline/write", post(workspace_pipeline_write))
        .route("/api/v1/anbud/workspace/searches/read", post(workspace_searches_read))
        .route("/api/v1/anbud/workspace/searches/write", post(workspace_searches_write))
        .route("/api/v1/anbud/workspace/searches/delete", post(workspace_searches_delete))
        .route("/api/v1/anbud/workspace/profile/suggest", post(workspace_profile_suggest))
        .route("/api/v1/anbud/workspace/seen/read", post(workspace_seen_read))
        .route("/api/v1/anbud/workspace/seen/write", post(workspace_seen_write))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

async fn catalogue_detail(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(notice_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if !plain_identifier(&notice_id) {
        return refused(StatusCode::BAD_REQUEST, "bad_request", "The notice id is not valid.");
    }
    forward(&state, &user, &format!("/api/v1/notices/{notice_id}"), &body).await
}

async fn search_catalogue(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward(&state, &user, "/api/v1/notices/search", &body).await
}

async fn search_matches(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward(&state, &user, "/api/v1/demo/matches", &body).await
}

async fn notice_audit(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(notice_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if !plain_identifier(&notice_id) {
        return refused(StatusCode::BAD_REQUEST, "bad_request", "The notice id is not valid.");
    }
    forward(&state, &user, &format!("/api/v1/demo/matches/{notice_id}"), &body).await
}

async fn re_tenders(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward(&state, &user, "/api/v1/demo/retenders", &body).await
}

async fn company_lookup(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward(&state, &user, "/api/v1/demo/company", &body).await
}

async fn workspace_profile_read(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward(&state, &user, "/api/v1/workspace/profile/read", &body).await
}

async fn workspace_profile_write(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward(&state, &user, "/api/v1/workspace/profile/write", &body).await
}

async fn workspace_pipeline_read(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward(&state, &user, "/api/v1/workspace/pipeline/read", &body).await
}

async fn workspace_pipeline_write(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward(&state, &user, "/api/v1/workspace/pipeline/write", &body).await
}

async fn workspace_searches_read(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward(&state, &user, "/api/v1/workspace/searches/read", &body).await
}

async fn workspace_searches_write(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward(&state, &user, "/api/v1/workspace/searches/write", &body).await
}

async fn workspace_searches_delete(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward(&state, &user, "/api/v1/workspace/searches/delete", &body).await
}

/// When the organisation last marked its tenders as seen.
async fn workspace_seen_read(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward(&state, &user, "/api/v1/workspace/seen/read", &body).await
}

/// Marks the organisation's tenders as seen now.
async fn workspace_seen_write(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward(&state, &user, "/api/v1/workspace/seen/write", &body).await
}

/// Drafts the organisation's profile from the awards it won. The organisation number is read from its record in
/// org-core with the session's own rights, so the browser names no company. The core saves the draft only where
/// the organisation has no profile, and answers with the codes either way.
async fn workspace_profile_suggest(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let org_id = authorized_org_id(&state, &user).await;
    let org_id = org_id.trim().to_string();
    if org_id.is_empty() {
        return refused(StatusCode::BAD_REQUEST, "no_active_org", "No active organization.");
    }
    let url = format!(
        "{}/api/v1/organizations/{}",
        state.org_core_url.trim_end_matches('/'),
        urlencoding::encode(&org_id)
    );
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let (status, Json(organisation)) =
        proxy_json(&state, Method::GET, &url, None, Some(&org_id), Some(&actor), None).await;
    if !status.is_success() {
        return (status, Json(organisation)).into_response();
    }
    let number = organisation
        .get("org_number")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if number.is_empty() {
        return refused(
            StatusCode::CONFLICT,
            "no_org_number",
            "The organization has no organisation number yet.",
        );
    }
    forward(&state, &user, "/api/v1/workspace/profile/suggest", &json!({ "org_number": number })).await
}

/// Calls one route of the core for the session's organisation. The gateway's own refusals come first, before
/// anything leaves: no active organisation, no key configured, a body too large.
async fn forward(state: &AppState, user: &AuthenticatedUser, path: &str, body: &Value) -> Response {
    let org_id = authorized_org_id(state, user).await;
    let org_id = org_id.trim();
    if org_id.is_empty() {
        return refused(StatusCode::BAD_REQUEST, "no_active_org", "No active organization.");
    }
    if state.anbud_api_key.trim().is_empty() {
        return refused(
            StatusCode::SERVICE_UNAVAILABLE,
            "not_configured",
            "The tender service is not configured.",
        );
    }
    if !fits_body_limit(body) {
        return refused(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", "The request is too large.");
    }
    let url = format!("{}{}", state.anbud_core_url.trim_end_matches('/'), path);
    proxy_anbud_json(
        &state.client,
        &state.anbud_api_key_header,
        &state.anbud_api_key,
        &url,
        org_id,
        &user.user_id,
        body,
    )
    .await
    .into_response()
}

fn refused(status: StatusCode, code: &'static str, message: &str) -> Response {
    (status, Json(error(code, message))).into_response()
}

/// Whether the body is within the core's limit, measured on the JSON the core would receive.
fn fits_body_limit(body: &Value) -> bool {
    serde_json::to_vec(body).is_ok_and(|bytes| bytes.len() <= MAX_BODY_BYTES)
}

/// A notice id is a plain identifier: letters, digits, dots, underscores and dashes.
fn plain_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    use axum::http::HeaderMap;
    use serde_json::json;

    use super::*;

    #[test]
    fn notice_ids_are_plain_identifiers() {
        assert!(plain_identifier("2026-100001"));
        assert!(plain_identifier("notice.1_b"));
        let long = "x".repeat(65);
        for hostile in ["", "../company", "a?b=1", "a/b", "a b", long.as_str()] {
            assert!(!plain_identifier(hostile), "{hostile:?} must be refused");
        }
    }

    #[test]
    fn a_body_over_the_core_limit_is_refused_before_it_leaves() {
        assert!(fits_body_limit(&json!({"profile": {"cpv": ["72212000"]}})));
        let big = "x".repeat(MAX_BODY_BYTES);
        assert!(!fits_body_limit(&json!({"profile": {"keywords": [big]}})));
    }

    /// A core that answers each call with `status` and `body` after `delay`, and keeps the headers of the
    /// last call it received. Returns the URL of its matches route.
    async fn fake_core(
        status: StatusCode,
        body: Value,
        delay: Duration,
    ) -> (String, Arc<Mutex<Option<HeaderMap>>>) {
        let seen = Arc::new(Mutex::new(None));
        let recorder = Arc::clone(&seen);
        let app = Router::new().route(
            "/api/v1/demo/matches",
            post(move |headers: HeaderMap| {
                let recorder = Arc::clone(&recorder);
                let body = body.clone();
                async move {
                    *recorder.lock().unwrap() = Some(headers);
                    tokio::time::sleep(delay).await;
                    (status, Json(body))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{address}/api/v1/demo/matches"), seen)
    }

    async fn call(client: &reqwest::Client, url: &str) -> (StatusCode, Value) {
        let (status, Json(body)) = proxy_anbud_json(
            client,
            "X-API-Key",
            "core-key",
            url,
            "org_1781699307874",
            "user-1",
            &json!({}),
        )
        .await;
        (status, body)
    }

    #[tokio::test]
    async fn the_core_is_called_with_its_key_the_organisation_and_the_session_user_only() {
        let (url, seen) = fake_core(StatusCode::OK, json!({"data": {}}), Duration::ZERO).await;
        let (status, _) = call(&reqwest::Client::new(), &url).await;
        assert_eq!(status, StatusCode::OK);
        let headers = seen.lock().unwrap().clone().expect("the core was called");
        assert_eq!(headers.get("x-api-key").unwrap().to_str().unwrap(), "core-key");
        assert_eq!(headers.get("x-org-id").unwrap().to_str().unwrap(), "org_1781699307874");
        assert_eq!(headers.get("x-user-id").unwrap().to_str().unwrap(), "user-1");
        for name in ["x-internal-api-key", "x-user-email", "authorization", "cookie"] {
            assert!(headers.get(name).is_none(), "{name} must not reach the core");
        }
    }

    #[tokio::test]
    async fn an_unauthorised_answer_from_the_core_is_not_passed_on_as_401() {
        let answer = json!({"error": {"code": "unauthorized", "message": "invalid API key"}});
        let (url, _) = fake_core(StatusCode::UNAUTHORIZED, answer, Duration::ZERO).await;
        let (status, body) = call(&reqwest::Client::new(), &url).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "tender_service_misconfigured");
    }

    #[tokio::test]
    async fn the_cores_own_refusals_pass_through_with_their_envelope() {
        let envelope = json!({"error": {"code": "rate_limited", "message": "too many scans are running; try again shortly"}});
        let (url, _) = fake_core(StatusCode::TOO_MANY_REQUESTS, envelope.clone(), Duration::ZERO).await;
        let (status, body) = call(&reqwest::Client::new(), &url).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body, envelope);
    }

    #[tokio::test]
    async fn a_slow_core_is_a_timeout_and_not_an_unreachable_service() {
        let (url, _) = fake_core(StatusCode::OK, json!({"data": {}}), Duration::from_secs(2)).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        let (status, body) = call(&client, &url).await;
        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(body["error"]["code"], "timeout");
    }

    #[tokio::test]
    async fn a_core_that_cannot_be_reached_is_unavailable() {
        let (status, body) = call(&reqwest::Client::new(), "http://127.0.0.1:1/api/v1/demo/matches").await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"]["code"], "upstream_unavailable");
    }
}
