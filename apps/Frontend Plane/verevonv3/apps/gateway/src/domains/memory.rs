//! `/api/v1/memory` — per-user durable memory management ("what do you
//! remember about me"), with per-item delete and correct.
//!
//! Thin authenticated proxy from the SPA to model-gateway's memory read/write
//! surface (`GET`/`POST /v1/memories` + `DELETE /v1/memories/{memory_id}` →
//! session-core `MemoryService.ListMemory`/`IndexMemory`/`DeleteMemory`).
//! Model-gateway names its hop plural (`/v1/memories`) to avoid colliding with
//! the unrelated `/v1/memory` capability-core proxy it already serves; the
//! public path exposed here stays singular (`/api/v1/memory`) to match this
//! BFF's existing settings-domain naming.
//!
//! Org and user scope are resolved server-side from the validated session —
//! the browser never picks whose memories it reads, deletes, or corrects.
//! session-core independently re-derives and enforces both from the verified
//! `x-session-authorization` bearer's claims (see `MemoryGrpc::list_memory`/
//! `index_memory`/`delete_memory` in session-core), so a client cannot widen
//! scope by tampering with anything sent here. A Zero Data Retention caller
//! gets an empty list / a no-op delete from session-core, never an error —
//! this BFF layer does not need its own ZDR branch, it just forwards the
//! response.
//!
//! There is no update-in-place RPC on session-core's `MemoryService` (only
//! `ListMemory`/`IndexMemory`/`DeleteMemory`), so "correct" (`PATCH`) is a
//! create-then-delete: index a fresh "USER"-topic entry with the corrected
//! content through the same write path the Dreaming extractor itself uses,
//! then delete the original by id. The new entry is created FIRST so a
//! failure partway through leaves the corrected fact present (a harmless
//! duplicate) rather than losing it outright.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, patch, post},
    Extension, Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    domains::chat::shared::{model_token, proxy_model_json_with_session, session_token},
    middleware::{require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/memory", get(list_memory).post(remember_memory))
        // Static segments win over `{id}`, so these never reach the delete route.
        .route(
            "/api/v1/memory/controls",
            get(get_memory_controls).put(set_memory_controls),
        )
        .route("/api/v1/memory/reset", post(reset_memory))
        .route(
            "/api/v1/memory/{id}",
            patch(correct_memory).delete(delete_memory),
        )
        // Per-org/user rate limiting, ordered like `agents_runs.rs`:
        // `require_session` (written last → outer) runs first and inserts
        // `AuthenticatedUser`, so `rate_limit_middleware` (written first →
        // inner) keys by the validated org/user rather than client IP.
        .route_layer(axum::middleware::from_fn(rate_limit_middleware))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

#[derive(Debug, Default, Deserialize)]
struct MemoryQuery {
    limit: Option<u32>,
}

async fn list_memory(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(query): Query<MemoryQuery>,
) -> impl IntoResponse {
    let token = model_token(&state, &user, &headers).await;
    let session_token = session_token(&state, &user, &headers).await;
    let url = match query.limit {
        Some(limit) => format!("{}/v1/memories?limit={limit}", state.model_gateway_url),
        None => format!("{}/v1/memories", state.model_gateway_url),
    };
    let (status, body) = proxy_model_json_with_session(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        session_token.as_deref(),
        &user,
    )
    .await;
    (status, body).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorrectMemoryBody {
    /// The current chat thread the correction was made from. Only used to
    /// satisfy `IndexMemory`'s thread-ownership check — the resulting
    /// "USER"-topic entry is not scoped to it and appears in every thread's
    /// recall, same as the entry it replaces.
    thread_id: String,
    content: String,
}

/// "Rediger" — corrects a single recalled memory's content in place from the
/// chat memory panel.
///
/// See the module doc for why this is create-then-delete rather than a real
/// update: the corrected entry is indexed under a NEW id, so a caller must
/// swap `memory_id` in whatever list it is rendering rather than expecting
/// the original id back.
///
/// `deleted` is true only when the original was actually removed, and
/// `original` says why when it was not (see [`OriginalOutcome`]): a 404 on
/// the delete means the original is not the caller's (or is gone), so it
/// still exists for whoever owns it and must not be reported as replaced.
async fn correct_memory(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(memory_id): Path<String>,
    Json(body): Json<CorrectMemoryBody>,
) -> impl IntoResponse {
    let thread_id = body.thread_id.trim();
    if thread_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "thread_id is required"})),
        )
            .into_response();
    }
    let content = body.content.trim();
    if content.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "content is required"})),
        )
            .into_response();
    }

    let token = model_token(&state, &user, &headers).await;
    let session_token = session_token(&state, &user, &headers).await;

    let create_url = format!("{}/v1/memories", state.model_gateway_url);
    let (create_status, Json(create_body)) = proxy_model_json_with_session(
        &state,
        Method::POST,
        &create_url,
        Some(json!({ "thread_id": thread_id, "content": content })),
        token.as_deref(),
        session_token.as_deref(),
        &user,
    )
    .await;
    if !create_status.is_success() {
        return (create_status, Json(create_body)).into_response();
    }
    let new_memory_id = create_body
        .get("memory_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();

    let delete_url = format!(
        "{}/v1/memories/{}",
        state.model_gateway_url,
        urlencoding::encode(&memory_id)
    );
    let (delete_status, Json(delete_body)) = proxy_model_json_with_session(
        &state,
        Method::DELETE,
        &delete_url,
        None,
        token.as_deref(),
        session_token.as_deref(),
        &user,
    )
    .await;
    // The corrected fact is already saved even if this cleanup step fails —
    // report the new id either way so the caller can show the correction, and
    // surface whether the stale original was actually removed rather than
    // silently claiming success.
    let original = OriginalOutcome::of_delete(delete_status, &delete_body);

    Json(json!({
        "memory_id": new_memory_id,
        "deleted": original == OriginalOutcome::Removed,
        "original": original.as_str(),
        "degraded": create_body.get("degraded").cloned().unwrap_or(json!(false)),
        "degradation_reason": create_body
            .get("degradation_reason")
            .cloned()
            .unwrap_or(json!("")),
    }))
    .into_response()
}

/// What happened to the ORIGINAL entry of a correction, from the
/// create-then-delete's delete step.
///
/// `NotFound` is its own answer, not a failed delete: session-core returns
/// NOT_FOUND (model-gateway `memory_not_found`, 404) for a memory that exists
/// but is not the caller's to remove — an org-wide entry or another user's —
/// exactly as it does for one that no longer exists, so that an id cannot be
/// probed. Either way the original was not removed by this correction, and
/// the caller must not say it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OriginalOutcome {
    /// Deleted: the correction replaced it.
    Removed,
    /// Not the caller's to remove, or already gone.
    NotFound,
    /// The delete failed or declined (upstream error, ZDR no-op).
    NotRemoved,
}

impl OriginalOutcome {
    fn of_delete(status: StatusCode, body: &Value) -> Self {
        if status == StatusCode::NOT_FOUND {
            return Self::NotFound;
        }
        let deleted = status.is_success()
            && body
                .get("deleted")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        if deleted {
            Self::Removed
        } else {
            Self::NotRemoved
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Removed => "removed",
            Self::NotFound => "not_found",
            Self::NotRemoved => "not_removed",
        }
    }
}

/// One user-bound hop to model-gateway's `/v1/memories/*` surface. Org and user
/// come from the verified session on the far side; nothing here picks them.
async fn forward_memory(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> axum::response::Response {
    let token = model_token(state, user, headers).await;
    let session_token = session_token(state, user, headers).await;
    let url = format!("{}{path}", state.model_gateway_url);
    let (status, body) = proxy_model_json_with_session(
        state,
        method,
        &url,
        body,
        token.as_deref(),
        session_token.as_deref(),
        user,
    )
    .await;
    (status, body).into_response()
}

/// The user's memory state (on / paused / off) and the decision taken from it.
async fn get_memory_controls(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl IntoResponse {
    forward_memory(
        &state,
        &user,
        &headers,
        Method::GET,
        "/v1/memories/controls",
        None,
    )
    .await
}

#[derive(Debug, Deserialize)]
pub(crate) struct SetControlsBody {
    pub(crate) state: String,
}

/// Only `state` is forwarded: a client cannot name another user or org here.
pub(crate) async fn set_memory_controls(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<SetControlsBody>,
) -> impl IntoResponse {
    forward_memory(
        &state,
        &user,
        &headers,
        Method::PUT,
        "/v1/memories/controls",
        Some(json!({ "state": body.state })),
    )
    .await
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ResetBody {
    #[serde(default)]
    pub(crate) confirm: bool,
}

/// Irreversible: `confirm` is passed through exactly as sent, so an absent
/// field is refused by model-gateway rather than defaulted here.
pub(crate) async fn reset_memory(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<ResetBody>,
) -> impl IntoResponse {
    forward_memory(
        &state,
        &user,
        &headers,
        Method::POST,
        "/v1/memories/reset",
        Some(json!({ "confirm": body.confirm })),
    )
    .await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RememberBody {
    /// A thread the caller owns; `IndexMemory` only uses it to prove ownership.
    pub(crate) thread_id: String,
    pub(crate) content: String,
}

/// "Husk dette" — saves one fact the user chose, through the same write path
/// the correct flow uses. A paused or off memory refuses it (409
/// `memory_not_enabled`), so the button never claims a save that did not happen.
pub(crate) async fn remember_memory(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<RememberBody>,
) -> impl IntoResponse {
    let thread_id = body.thread_id.trim();
    let content = body.content.trim();
    if thread_id.is_empty() || content.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "thread_id and content are required"})),
        )
            .into_response();
    }
    forward_memory(
        &state,
        &user,
        &headers,
        Method::POST,
        "/v1/memories",
        Some(json!({ "thread_id": thread_id, "content": content })),
    )
    .await
}

pub(crate) async fn delete_memory(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(memory_id): Path<String>,
) -> impl IntoResponse {
    let token = model_token(&state, &user, &headers).await;
    let session_token = session_token(&state, &user, &headers).await;
    let url = format!(
        "{}/v1/memories/{}",
        state.model_gateway_url,
        urlencoding::encode(&memory_id)
    );
    let (status, body) = proxy_model_json_with_session(
        &state,
        Method::DELETE,
        &url,
        None,
        token.as_deref(),
        session_token.as_deref(),
        &user,
    )
    .await;
    (status, body).into_response()
}

#[cfg(test)]
mod tests {
    use super::{
        get_memory_controls, remember_memory, reset_memory, set_memory_controls, OriginalOutcome,
        RememberBody, ResetBody, SetControlsBody,
    };
    use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
    use serde_json::json;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[test]
    fn a_confirmed_delete_is_the_only_removal() {
        let outcome = OriginalOutcome::of_delete(StatusCode::OK, &json!({ "deleted": true }));
        assert_eq!(outcome, OriginalOutcome::Removed);
        assert_eq!(outcome.as_str(), "removed");
    }

    /// Session Core answers 404 for a memory that exists but is not the
    /// caller's (an org-wide entry or another user's). The original survives
    /// for its owner, so the correction must not claim it was replaced.
    #[test]
    fn a_not_found_delete_is_reported_as_not_found_not_as_removed() {
        let body =
            json!({ "error": { "code": "memory_not_found", "message": "memory not found" } });
        let outcome = OriginalOutcome::of_delete(StatusCode::NOT_FOUND, &body);
        assert_eq!(outcome, OriginalOutcome::NotFound);
        assert_eq!(outcome.as_str(), "not_found");
    }

    #[test]
    fn a_declined_or_failed_delete_is_not_removed() {
        // ZDR: session-core declines without an error.
        let declined = OriginalOutcome::of_delete(
            StatusCode::OK,
            &json!({ "deleted": false, "degraded": true }),
        );
        assert_eq!(declined, OriginalOutcome::NotRemoved);
        let failed = OriginalOutcome::of_delete(StatusCode::BAD_GATEWAY, &json!({ "error": "x" }));
        assert_eq!(failed, OriginalOutcome::NotRemoved);
        let refused =
            OriginalOutcome::of_delete(StatusCode::FORBIDDEN, &json!({ "deleted": true }));
        assert_eq!(refused, OriginalOutcome::NotRemoved);
        assert_eq!(failed.as_str(), "not_removed");
    }

    fn member() -> crate::middleware::AuthenticatedUser {
        crate::middleware::AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "Memory User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    /// Token minting and model-gateway stood in by local mocks; model-gateway
    /// answers `status` with `reply` on `verb route`.
    async fn upstreams(
        verb: &str,
        route: &str,
        status: u16,
        reply: serde_json::Value,
    ) -> (MockServer, MockServer, crate::config::AppState) {
        let auth_core = MockServer::start().await;
        for audience in ["model-plane", "session-core"] {
            Mock::given(method("GET"))
                .and(path(format!("/api/{audience}/token")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "token": format!("{audience}-test-token"),
                    "expiresInSeconds": 300,
                })))
                .mount(&auth_core)
                .await;
        }
        let model_gateway = MockServer::start().await;
        Mock::given(method(verb))
            .and(path(route))
            .respond_with(ResponseTemplate::new(status).set_body_json(reply))
            .mount(&model_gateway)
            .await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth_core.uri();
        state.model_gateway_url = model_gateway.uri();
        (auth_core, model_gateway, state)
    }

    async fn forwarded(model_gateway: &MockServer, route: &str) -> Vec<serde_json::Value> {
        model_gateway
            .received_requests()
            .await
            .expect("model-gateway requests are recorded")
            .into_iter()
            .filter(|request| request.url.path() == route)
            .map(|request| {
                serde_json::from_slice(&request.body).unwrap_or(serde_json::Value::Null)
            })
            .collect()
    }

    #[tokio::test]
    async fn controls_read_is_a_user_bound_get_to_model_gateway() {
        let (_auth, model_gateway, state) = upstreams(
            "GET",
            "/v1/memories/controls",
            200,
            json!({ "state": "paused", "reason": "user_paused" }),
        )
        .await;
        let response = get_memory_controls(
            State(state),
            Extension(member()),
            axum::http::HeaderMap::new(),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            forwarded(&model_gateway, "/v1/memories/controls").await.len(),
            1
        );
    }

    /// Only `state` crosses: a client-supplied user or org id is dropped before
    /// the hop, so a user can only ever set their own state.
    #[tokio::test]
    async fn setting_the_state_forwards_only_the_state() {
        let (_auth, model_gateway, state) = upstreams(
            "PUT",
            "/v1/memories/controls",
            200,
            json!({ "state": "off" }),
        )
        .await;
        let body: SetControlsBody = serde_json::from_value(
            json!({ "state": "off", "user_id": "user-2", "org_id": "org-2" }),
        )
        .expect("extra fields are ignored");
        let response = set_memory_controls(
            State(state),
            Extension(member()),
            axum::http::HeaderMap::new(),
            Json(body),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            forwarded(&model_gateway, "/v1/memories/controls").await,
            vec![json!({ "state": "off" })]
        );
    }

    #[tokio::test]
    async fn reset_without_confirm_is_forwarded_as_false_not_defaulted_true() {
        let (_auth, model_gateway, state) = upstreams(
            "POST",
            "/v1/memories/reset",
            400,
            json!({ "error": { "code": "confirm_required" } }),
        )
        .await;
        let body: ResetBody = serde_json::from_value(json!({})).expect("confirm is optional");
        let response = reset_memory(
            State(state),
            Extension(member()),
            axum::http::HeaderMap::new(),
            Json(body),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            forwarded(&model_gateway, "/v1/memories/reset").await,
            vec![json!({ "confirm": false })]
        );
    }

    #[tokio::test]
    async fn remember_surfaces_a_paused_refusal_instead_of_a_save() {
        let (_auth, _model_gateway, state) = upstreams(
            "POST",
            "/v1/memories",
            409,
            json!({ "error": { "code": "memory_not_enabled", "details": { "reason": "user_paused" } } }),
        )
        .await;
        let response = remember_memory(
            State(state),
            Extension(member()),
            axum::http::HeaderMap::new(),
            Json(RememberBody {
                thread_id: "thread-1".to_owned(),
                content: "Jeg heter Ima".to_owned(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn remember_trims_and_refuses_empty_content_without_a_hop() {
        let (_auth, model_gateway, state) =
            upstreams("POST", "/v1/memories", 200, json!({ "memory_id": "m1" })).await;
        let refused = remember_memory(
            State(state.clone()),
            Extension(member()),
            axum::http::HeaderMap::new(),
            Json(RememberBody {
                thread_id: "thread-1".to_owned(),
                content: "   ".to_owned(),
            }),
        )
        .await
        .into_response();
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
        assert!(forwarded(&model_gateway, "/v1/memories").await.is_empty());

        let saved = remember_memory(
            State(state),
            Extension(member()),
            axum::http::HeaderMap::new(),
            Json(RememberBody {
                thread_id: "thread-1".to_owned(),
                content: "  Jeg heter Ima  ".to_owned(),
            }),
        )
        .await
        .into_response();
        assert_eq!(saved.status(), StatusCode::OK);
        assert_eq!(
            forwarded(&model_gateway, "/v1/memories").await,
            vec![json!({ "thread_id": "thread-1", "content": "Jeg heter Ima" })]
        );
    }
}
