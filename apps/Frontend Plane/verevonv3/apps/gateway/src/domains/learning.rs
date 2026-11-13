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
//!
//! Teach mode (F-t3-081) adds `/api/v1/learning/teach`: one run the person drove or
//! took over becomes a disabled skill proposal in the same inbox, drafted by
//! model-gateway's `/v1/teach/skill-draft`.

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
    audience_tokens::get_audience_token,
    config::AppState,
    domains::chat::shared::{
        apply_org_zdr_posture, delegated_auth_unavailable, model_token,
        proxy_model_json_with_delegations, proxy_model_json_with_session,
        proxy_model_json_with_session_and_capability, required_capability_token,
        required_inference_token, required_session_token, session_token, zdr_flag,
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
        .route("/api/v1/learning/teach", post(teach_skill_from_run))
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

/// The longest goal hint a teach draft carries, in characters, as model-gateway bounds it.
const TEACH_GOAL_HINT_MAX_CHARS: usize = 500;

#[derive(Debug, Deserialize)]
pub(crate) struct TeachBody {
    pub(crate) run_id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) goal_hint: Option<String>,
}

/// "Lagre disse stegene som en ferdighet": the owner turns one run they drove or took over into a disabled skill
/// proposal in their learning inbox (F-t3-081). Only the run, the trimmed name and the trimmed hint cross; the owner
/// is the session on the far side. Model-gateway reads the run's steps as that owner, with the caller's `aud=quarry`
/// bearer, so that bearer rides with the session and inference credentials. A zero-retention turn is refused here,
/// before any call: model-gateway's teach route takes a caller's posture from its token claims and never reads the
/// body's flag. The answer is relayed unchanged, 200 with the proposal or the typed refusal.
pub(crate) async fn teach_skill_from_run(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<TeachBody>,
) -> impl IntoResponse {
    let run_id = body.run_id.trim();
    if run_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "invalid_run_id",
                    "message": "run_id is required",
                    "details": {},
                }
            })),
        )
            .into_response();
    }
    let name = body.name.trim();
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "teach_name_invalid",
                    "message": "name is required",
                    "details": {},
                }
            })),
        )
            .into_response();
    }
    let goal_hint = body.goal_hint.as_deref().map_or("", str::trim);
    if goal_hint.chars().count() > TEACH_GOAL_HINT_MAX_CHARS {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "error": {
                    "code": "teach_goal_hint_too_long",
                    "message": "goal_hint is longer than 500 characters",
                    "details": {},
                }
            })),
        )
            .into_response();
    }
    let mut outbound = json!({
        "run_id": run_id,
        "name": name,
        "goal_hint": goal_hint,
        "zdr": zdr_flag(&headers),
    });
    apply_org_zdr_posture(&state, &user, &mut outbound).await;
    if outbound.get("zdr").and_then(Value::as_bool) == Some(true) {
        return teach_zdr_refusal().into_response();
    }
    let session = match required_session_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let inference = match required_inference_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let Some(quarry) = quarry_token(&state, &user, &headers).await else {
        return teach_credential_unavailable().into_response();
    };
    let token = model_token(&state, &user, &headers).await;
    let url = format!("{}/v1/teach/skill-draft", state.model_gateway_url);
    let (status, Json(payload)) = proxy_model_json_with_delegations(
        &state,
        Method::POST,
        &url,
        Some(outbound),
        token.as_deref(),
        None,
        None,
        Some(&inference),
        None,
        None,
        Some(&session),
        None,
        None,
        Some(&quarry),
        &user,
    )
    .await;
    // A 401 is a refused credential, not the browser's session, so it is answered as unavailable rather than as a
    // sign-out, as `side_chat_reply` answers it.
    if status == StatusCode::UNAUTHORIZED {
        return teach_credential_unavailable().into_response();
    }
    (status, Json(payload)).into_response()
}

/// The caller's `aud=quarry` bearer, minted per request for the edge's read of one run's steps (F-t3-081). Absent
/// when auth-core cannot mint it; the teach then answers unavailable rather than go without it.
async fn quarry_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Option<String> {
    let cookie = headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    get_audience_token(state, &user.user_id, cookie, "quarry").await
}

/// The typed refusal a zero-retention teach gets: the same 412 model-gateway answers for a feature a ZDR caller may not
/// use, sent here before any call, so nothing is read, sent or kept.
fn teach_zdr_refusal() -> (StatusCode, Json<Value>) {
    (
        StatusCode::PRECONDITION_FAILED,
        Json(json!({
            "error": {
                "code": "zdr_feature_not_supported",
                "message": "This is not supported with zero data retention yet: saving a run's steps as a skill. The request was refused before any model call.",
                "retryable": false,
                "action": "turn_off_feature",
                "feature": "teach_mode",
            }
        })),
    )
}

/// The answer when a credential a teach needs cannot be had. It is unavailable, never a 401 that would sign the person out.
fn teach_credential_unavailable() -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": {
                "code": "delegated_auth_unavailable",
                "message": "Teach authentication is temporarily unavailable",
                "details": {},
            }
        })),
    )
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
