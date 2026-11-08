//! `/api/v1/learning` — the owner's inbox for what Verevon learned: learned-skill
//! proposals to accept or reject, and the ledger of skill changes and learned
//! memories, with undo.
//!
//! Thin authenticated proxy from the SPA to model-gateway's `/v1/learning/*`
//! surface, which forwards to session-core's `LearningGovernanceService`. The owner
//! is the verified session: the browser never picks whose proposals it decides or
//! whose ledger it reads, and session-core re-checks ownership on every call. An
//! organization admin cannot decide a member's proposal either.
//!
//! A learned memory is not undone here. Session-core refuses it with
//! `memory_undo_via_delete`, and the SPA erases it through `/api/v1/memory/{id}`,
//! the same path that records the memory as forgotten.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Extension, Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    domains::chat::shared::{
        delegated_auth_unavailable, model_token, proxy_model_json_with_session,
        proxy_model_json_with_session_and_capability, required_capability_token, session_token,
    },
    middleware::{require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/learning/ledger", get(ledger))
        .route(
            "/api/v1/learning/proposals/{skill_id}/decision",
            post(decide_proposal),
        )
        .route("/api/v1/learning/ledger/{entry_id}/undo", post(undo_entry))
        .route("/api/v1/learning/learn", post(learn_from_thread))
        // Ordered like memory.rs: `require_session` (written last, outer) runs first
        // and inserts `AuthenticatedUser`, so the rate limit keys by the verified user.
        .route_layer(axum::middleware::from_fn(rate_limit_middleware))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

#[derive(Debug, Deserialize)]
pub(crate) struct LedgerQuery {
    pub(crate) cursor: Option<String>,
    pub(crate) limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LearningDecisionBody {
    pub(crate) decision: String,
}

/// One user-bound hop to model-gateway's `/v1/learning/*` surface. Org and user come
/// from the verified session on the far side; nothing here picks them.
async fn forward_learning(
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

/// "What Verevon learned": the owner's ledger, newest first, one page at a time. Only
/// the cursor and the page size cross; whose ledger it is comes from the session.
pub(crate) async fn ledger(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(query): Query<LedgerQuery>,
) -> impl IntoResponse {
    let mut params = Vec::new();
    if let Some(cursor) = query.cursor.as_deref().filter(|cursor| !cursor.is_empty()) {
        params.push(format!("cursor={}", urlencoding::encode(cursor)));
    }
    if let Some(limit) = query.limit {
        params.push(format!("limit={limit}"));
    }
    let path = if params.is_empty() {
        "/v1/learning/ledger".to_owned()
    } else {
        format!("/v1/learning/ledger?{}", params.join("&"))
    };
    forward_learning(&state, &user, &headers, Method::GET, &path, None).await
}

/// "Godta" and "Avvis": the owner accepts or rejects one learned-skill proposal. Only
/// the two literal words are forwarded; anything else is refused here, before the hop.
pub(crate) async fn decide_proposal(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(skill_id): Path<String>,
    Json(body): Json<LearningDecisionBody>,
) -> impl IntoResponse {
    let skill_id = skill_id.trim();
    if skill_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "skill_id is required"})),
        )
            .into_response();
    }
    let decision = body.decision.trim();
    if !matches!(decision, "accept" | "reject") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "invalid_skill_decision",
                    "message": "decision must be one of: accept, reject",
                }
            })),
        )
            .into_response();
    }
    forward_learning(
        &state,
        &user,
        &headers,
        Method::POST,
        &format!(
            "/v1/learning/proposals/{}/decision",
            urlencoding::encode(skill_id)
        ),
        Some(json!({ "decision": decision })),
    )
    .await
}

/// "Angre": the owner undoes the newest change to one of their learned skills. The
/// entry id is the only thing forwarded.
pub(crate) async fn undo_entry(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(entry_id): Path<String>,
) -> impl IntoResponse {
    let entry_id = entry_id.trim();
    if entry_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "entry_id is required"})),
        )
            .into_response();
    }
    forward_learning(
        &state,
        &user,
        &headers,
        Method::POST,
        &format!("/v1/learning/ledger/{}/undo", urlencoding::encode(entry_id)),
        None,
    )
    .await
}

/// The longest note a `/learn` carries, in characters, as model-gateway and capability-core bound it.
const LEARN_NOTE_MAX_CHARS: usize = 500;

#[derive(Debug, Deserialize)]
pub(crate) struct LearnBody {
    #[serde(rename = "threadId")]
    pub(crate) thread_id: String,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

/// "Lær av denne samtalen": the owner asks Verevon to distil one of their threads into a
/// skill proposal (EP-11 S7). Only the thread and the trimmed note cross; the owner is the
/// session on the far side. The user's capability token rides with the model and session
/// tokens, because model-gateway checks the thread on the session credential and capability-core
/// runs the review on the capability one. The answer is relayed unchanged: 202 `queued`, 404
/// while `LEARN_COMMAND_ENABLED` is off there, or the typed refusal (zero data retention
/// included).
pub(crate) async fn learn_from_thread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<LearnBody>,
) -> impl IntoResponse {
    let thread_id = body.thread_id.trim();
    if thread_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "invalid_thread_id",
                    "message": "threadId is required",
                    "details": {},
                }
            })),
        )
            .into_response();
    }
    let note = body.note.as_deref().map_or("", str::trim);
    if note.chars().count() > LEARN_NOTE_MAX_CHARS {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "error": {
                    "code": "learn_note_too_long",
                    "message": "the note is longer than 500 characters",
                    "details": {},
                }
            })),
        )
            .into_response();
    }
    let capability = match required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let token = model_token(&state, &user, &headers).await;
    let session_token = session_token(&state, &user, &headers).await;
    let url = format!("{}/v1/learning/learn", state.model_gateway_url);
    proxy_model_json_with_session_and_capability(
        &state,
        Method::POST,
        &url,
        Some(json!({ "thread_id": thread_id, "note": note })),
        token.as_deref(),
        session_token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::{
        decide_proposal, learn_from_thread, ledger, undo_entry, LearnBody, LearningDecisionBody,
        LedgerQuery,
    };
    use axum::{
        extract::{Path, Query, State},
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        Extension, Json,
    };
    use serde_json::json;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn member() -> crate::middleware::AuthenticatedUser {
        crate::middleware::AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "Learning User".to_owned(),
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

    /// Token minting and model-gateway stood in by local mocks; model-gateway answers
    /// `status` with `reply` on `verb route`.
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
            .map(|request| serde_json::from_slice(&request.body).unwrap_or(serde_json::Value::Null))
            .collect()
    }

    #[tokio::test]
    async fn the_ledger_forwards_only_the_cursor_and_the_page_size() {
        let (_auth, model_gateway, state) = upstreams(
            "GET",
            "/v1/learning/ledger",
            200,
            json!({ "entries": [], "next_cursor": "", "memory_skip_reason": "" }),
        )
        .await;
        let response = ledger(
            State(state),
            Extension(member()),
            HeaderMap::new(),
            Query(LedgerQuery {
                cursor: Some("page 2".to_owned()),
                limit: Some(20),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let queries: Vec<String> = model_gateway
            .received_requests()
            .await
            .expect("model-gateway requests are recorded")
            .into_iter()
            .filter(|request| request.url.path() == "/v1/learning/ledger")
            .map(|request| request.url.query().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(queries, vec!["cursor=page%202&limit=20".to_owned()]);
    }

    #[tokio::test]
    async fn a_decision_other_than_accept_or_reject_is_refused_without_a_hop() {
        let (_auth, model_gateway, state) = upstreams(
            "POST",
            "/v1/learning/proposals/skill_1/decision",
            200,
            json!({ "review_state": "accepted", "enabled": true, "revision": 2 }),
        )
        .await;
        let response = decide_proposal(
            State(state),
            Extension(member()),
            HeaderMap::new(),
            Path("skill_1".to_owned()),
            Json(LearningDecisionBody {
                decision: "approve".to_owned(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(
            forwarded(&model_gateway, "/v1/learning/proposals/skill_1/decision")
                .await
                .is_empty()
        );
    }

    /// Only the decision word crosses: the proposal is named by the path, and the
    /// owner by the session on the far side.
    #[tokio::test]
    async fn accepting_a_proposal_forwards_only_the_decision_word() {
        let (_auth, model_gateway, state) = upstreams(
            "POST",
            "/v1/learning/proposals/skill_1/decision",
            200,
            json!({ "review_state": "accepted", "enabled": true, "revision": 2 }),
        )
        .await;
        let response = decide_proposal(
            State(state),
            Extension(member()),
            HeaderMap::new(),
            Path("skill_1".to_owned()),
            Json(LearningDecisionBody {
                decision: " accept ".to_owned(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            forwarded(&model_gateway, "/v1/learning/proposals/skill_1/decision").await,
            vec![json!({ "decision": "accept" })]
        );
    }

    #[tokio::test]
    async fn undoing_an_entry_posts_its_id_as_the_path_segment() {
        let (_auth, model_gateway, state) = upstreams(
            "POST",
            "/v1/learning/ledger/led_1/undo",
            200,
            json!({ "action": "restored", "skill_id": "skill_1", "revision": 3 }),
        )
        .await;
        let response = undo_entry(
            State(state),
            Extension(member()),
            HeaderMap::new(),
            Path("led_1".to_owned()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            forwarded(&model_gateway, "/v1/learning/ledger/led_1/undo")
                .await
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn a_blank_entry_id_is_refused_without_a_hop() {
        let (_auth, model_gateway, state) =
            upstreams("POST", "/v1/learning/ledger/x/undo", 200, json!({})).await;
        let response = undo_entry(
            State(state),
            Extension(member()),
            HeaderMap::new(),
            Path("   ".to_owned()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(model_gateway
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty());
    }

    /// `upstreams` plus the capability-core audience, which `/learn` also mints.
    async fn learn_upstreams(
        status: u16,
        reply: serde_json::Value,
    ) -> (MockServer, MockServer, crate::config::AppState) {
        let (auth_core, model_gateway, state) =
            upstreams("POST", "/v1/learning/learn", status, reply).await;
        Mock::given(method("GET"))
            .and(path("/api/capability-core/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": "capability-core-test-token",
                "expiresInSeconds": 300,
            })))
            .mount(&auth_core)
            .await;
        (auth_core, model_gateway, state)
    }

    /// Only the thread and the trimmed note cross, with the user's capability token; the
    /// gateway's 202 comes back unchanged.
    #[tokio::test]
    async fn learn_forwards_the_thread_and_note_with_the_capability_token() {
        let (_auth, model_gateway, state) =
            learn_upstreams(202, json!({ "data": { "state": "queued" } })).await;
        let response = learn_from_thread(
            State(state),
            Extension(member()),
            HeaderMap::new(),
            Json(LearnBody {
                thread_id: " thread-1 ".to_owned(),
                note: Some("  keep the release steps  ".to_owned()),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let requests: Vec<_> = model_gateway
            .received_requests()
            .await
            .expect("model-gateway requests are recorded")
            .into_iter()
            .filter(|request| request.url.path() == "/v1/learning/learn")
            .collect();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&requests[0].body).unwrap(),
            json!({ "thread_id": "thread-1", "note": "keep the release steps" })
        );
        assert!(requests[0].headers.get("x-capability-authorization").is_some());
        assert!(requests[0].headers.get("x-session-authorization").is_some());
    }

    #[tokio::test]
    async fn learn_relays_a_typed_zero_retention_refusal() {
        let refusal = json!({ "error": {
            "code": "zdr_feature_not_supported",
            "message": "This is not supported with zero data retention yet: learning from this conversation.",
            "retryable": false,
            "action": "turn_off_feature",
            "feature": "learn_command",
        }});
        let (_auth, _model_gateway, state) = learn_upstreams(412, refusal.clone()).await;
        let response = learn_from_thread(
            State(state),
            Extension(member()),
            HeaderMap::new(),
            Json(LearnBody {
                thread_id: "thread-1".to_owned(),
                note: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
    }

    #[tokio::test]
    async fn learn_refuses_a_blank_thread_or_a_long_note_without_a_hop() {
        let (_auth, model_gateway, state) =
            learn_upstreams(202, json!({ "data": { "state": "queued" } })).await;
        for body in [
            LearnBody {
                thread_id: "  ".to_owned(),
                note: None,
            },
            LearnBody {
                thread_id: "thread-1".to_owned(),
                note: Some("n".repeat(super::LEARN_NOTE_MAX_CHARS + 1)),
            },
        ] {
            let response = learn_from_thread(
                State(state.clone()),
                Extension(member()),
                HeaderMap::new(),
                Json(body),
            )
            .await
            .into_response();
            assert!(response.status().is_client_error());
        }
        assert!(forwarded(&model_gateway, "/v1/learning/learn").await.is_empty());
    }
}
