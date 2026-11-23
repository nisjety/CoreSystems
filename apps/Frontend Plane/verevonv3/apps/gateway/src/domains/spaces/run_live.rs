//! Member live view of an agent run, the BFF half of WS09-S4.
//!
//! `GET /api/v1/spaces/{space_ref}/runs/{run_id}/live` (SSE, `Last-Event-ID` accepted) lets a room
//! member watch another member's agent turn: step by step, and with the answer text when
//! `SPACE_RUN_LIVE_TEXT` allows. This hop decides nothing about what a member may see. The frames
//! are projected by model-gateway from an allowlist (`room_run.rs`) after Session Core authorized
//! the attach (`RunService.AuthorizeRoomRunAttach`), and they are relayed here byte for byte.
//!
//! What this hop owns, in order (each step is a refusal before the next one runs):
//!
//! 1. the rollout flag `SPACE_RUN_LIVE_VIEW` (404 `feature_unavailable` when off; never an
//!    authorization input),
//! 2. live organization membership (403 `active_membership_required`) and the request shape (400
//!    `invalid_space` / `invalid_run`),
//! 3. the org Zero Data Retention pre-check, before any Control call: ZDR is a typed refusal
//!    (403 `zdr_feature_not_supported`, feature `agent_run`) and an unknown posture is 503
//!    `zdr_decision_unavailable`; the room live view has no zero-retention form,
//! 4. the Space lifecycle (404 `space_not_found`, 503 `space_lifecycle_unavailable`),
//! 5. a fresh Control `model.thread.read` decision for THIS attach (403 `space_read_not_authorized`).
//!    The decision never reaches the browser and is never cached: it travels gateway to
//!    model-gateway in a POST body, not a URL,
//! 6. the delegated Session Core and Model credentials, then the relay.
//!
//! Errors are `{error:{code,message,details}}`. A model-gateway refusal is relayed only for the
//! closed codes in [`RELAYED`], with this module's fixed words; anything else is `model_error`.
//!
//! # Manager Stop (WS09-S8)
//!
//! `POST /api/v1/spaces/{space_ref}/runs/{run_id}/stop {idempotency_key?}` lets a room manager or
//! owner stop another member's live run. Same order as the live view, with a Control
//! `model.run.control` decision in place of the read decision: flag `SPACE_RUN_MANAGER_STOP` (404
//! `feature_unavailable`), membership, shape, before any Control call. A zero-retention organisation
//! may stop too (D23: the stop writes only ids and a reason), then Space lifecycle, then Control (403
//! `space_run_control_not_authorized` for a caller below manager, an org without
//! `run_control_entitled`, or a personal Space). The decision rides to model-gateway in a POST
//! body and is verified again by Session Core, which is the authority. Relayed codes are the closed
//! set in [`STOP_RELAYED`]; `200` carries `{data:{run_id,status,cancelled,receipt_id}}`.

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use super::{
    denial, shared_thread_read_decision_for_typed, space_lifecycle_by_ref, unix_nanos,
};
use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_json, proxy_sse_stream_with_data_plane},
    zdr::{self, OrgZdrPosture},
};

const MAX_SPACE_REF_LEN: usize = 200;
const MAX_RUN_ID_LEN: usize = 64;

/// Model-gateway refusals this route relays, as (code, upstream status, fixed message).
const RELAYED: &[(&str, u16, &str)] = &[
    ("space_read_not_authorized", 403, "Watching this run is not authorized."),
    (
        "zdr_feature_not_supported",
        403,
        "Live agent runs are not supported with zero data retention.",
    ),
    ("run_not_found", 404, "The run is not available."),
    ("feature_unavailable", 404, "The live run view is not enabled."),
    ("run_stream_expired", 410, "The live view of this run has ended."),
    ("too_many_live_views", 429, "Too many open live views."),
    ("run_live_unavailable", 503, "The live run view is temporarily unavailable."),
];

/// Model-gateway refusals of a stop that this route relays, as (code, status, fixed message).
const STOP_RELAYED: &[(&str, StatusCode, &str)] = &[
    (
        "space_run_control_not_authorized",
        StatusCode::FORBIDDEN,
        "Stopping this run is not authorized.",
    ),
    ("run_not_found", StatusCode::NOT_FOUND, "The run is not available."),
    ("feature_unavailable", StatusCode::NOT_FOUND, "Stopping runs is not enabled."),
    (
        "run_control_unavailable",
        StatusCode::SERVICE_UNAVAILABLE,
        "Run control is temporarily unavailable.",
    ),
];

const MAX_IDEMPOTENCY_KEY_LEN: usize = 128;

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/spaces/{space_ref}/runs/{run_id}/live",
            get(space_run_live),
        )
        .route(
            "/api/v1/spaces/{space_ref}/runs/{run_id}/stop",
            post(space_run_stop),
        )
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (status, Json(error(code, message))).into_response()
}

/// The typed ZDR refusal for the room live view, in the Model Plane catalogue shape
/// (`zdr-refusals.json`), so the browser renders the same words whichever plane refused.
fn zdr_agent_run_refusal() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": {
                "code": "zdr_feature_not_supported",
                "message": "This is not supported with zero data retention yet: agent runs. The request was refused before any model call.",
                "retryable": false,
                "action": "turn_off_feature",
                "feature": "agent_run",
            }
        })),
    )
        .into_response()
}

fn valid_space_ref(space_ref: &str) -> bool {
    !space_ref.is_empty()
        && space_ref.len() <= MAX_SPACE_REF_LEN
        && !space_ref.chars().any(|ch| ch.is_control() || ch.is_whitespace())
}

fn valid_run_id(run_id: &str) -> bool {
    !run_id.is_empty()
        && run_id.len() <= MAX_RUN_ID_LEN
        && run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

async fn space_run_live(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, run_id)): Path<(String, String)>,
) -> Response {
    // 1. Rollout flag.
    if !state.space_features.run_live_view {
        return refuse(
            StatusCode::NOT_FOUND,
            "feature_unavailable",
            "The live run view is not enabled.",
        );
    }
    // 2. Membership, then shape.
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to watch a run.",
        );
    }
    let space_ref = space_ref.trim();
    if !valid_space_ref(space_ref) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A valid Space reference is required.",
        );
    }
    let run_id = run_id.trim();
    if !valid_run_id(run_id) {
        return refuse(StatusCode::BAD_REQUEST, "invalid_run", "A valid run id is required.");
    }
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| {
            !value.is_empty() && value.len() <= 20 && value.bytes().all(|byte| byte.is_ascii_digit())
        })
        .map(str::to_owned);

    // 3. Zero Data Retention fails closed, before any Control call.
    if zdr::request_zdr(&headers, None) {
        return zdr_agent_run_refusal();
    }
    match zdr::org_zdr_lookup(&state, &user).await {
        OrgZdrPosture::Disabled => {}
        OrgZdrPosture::Enabled => return zdr_agent_run_refusal(),
        OrgZdrPosture::Unknown => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(zdr::zdr_refusal_body(zdr::ZDR_DECISION_UNAVAILABLE)),
            )
                .into_response()
        }
    }

    // 4. Space lifecycle.
    match space_lifecycle_by_ref(&state, &user, &org_id, space_ref).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return refuse(StatusCode::NOT_FOUND, "space_not_found", "Space is not available.")
        }
        Err(()) => {
            return refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            )
        }
    }

    // 5. A fresh Control read decision for this attach.
    let (read, read_denial) =
        match shared_thread_read_decision_for_typed(&state, &user, &org_id, space_ref, "").await {
            Ok(decision) => decision.split(),
            Err(response) => return response.into_response(),
        };
    let Some((decision_ref, token)) = read else {
        let message = "Watching this run is not authorized.";
        return match read_denial {
            Some(denial) => (
                StatusCode::FORBIDDEN,
                Json(error_with_details(
                    denial::OP_READ,
                    message,
                    denial.details_value(),
                )),
            )
                .into_response(),
            None => refuse(StatusCode::FORBIDDEN, denial::OP_READ, message),
        };
    };

    // 6. Delegated credentials, then the relay. The decision rides in the body, never the URL.
    let session_token =
        match crate::domains::chat::shared::required_session_token(&state, &user, &headers).await {
            Ok(token) => token,
            Err(reason) => {
                return crate::domains::chat::shared::delegated_auth_unavailable(reason)
                    .into_response()
            }
        };
    let model_token = crate::domains::chat::shared::model_token(&state, &user, &headers).await;
    let url = format!(
        "{}/v1/spaces/{}/runs/{}/live",
        state.model_gateway_url,
        urlencoding::encode(space_ref),
        urlencoding::encode(run_id),
    );
    let body: Value = json!({
        "space_read_decision_ref": decision_ref,
        "space_read_decision_token": token,
    });
    proxy_sse_stream_with_data_plane(
        &state,
        Method::POST,
        &url,
        Some(body),
        model_token.as_deref(),
        None,
        None,
        None,
        None,
        None,
        Some(&session_token),
        None,
        None,
        last_event_id.as_deref(),
        Some((&user.user_id, org_id.trim())),
        false,
        RELAYED,
    )
    .await
}

// ---------------------------------------------------------------------------
// Manager Stop (WS09-S8)
// ---------------------------------------------------------------------------

type Reply = (StatusCode, Json<Value>);

fn reply(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn valid_idempotency_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_IDEMPOTENCY_KEY_LEN
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

async fn space_run_stop(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, run_id)): Path<(String, String)>,
    body: Option<Json<Value>>,
) -> Response {
    let body = body.map(|Json(value)| value).unwrap_or(Value::Null);
    let (status, json) = stop_run_core(&state, &user, Some(&headers), &space_ref, &run_id, &body).await;
    (status, json).into_response()
}

/// The stop route's whole work, shared with the `spaces.stop_run` action dispatcher so the flag, the
/// Control decision and the typed errors exist once.
pub(crate) async fn stop_run_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: Option<&HeaderMap>,
    space_ref: &str,
    run_id: &str,
    body: &Value,
) -> Reply {
    // 1. Rollout flag. A rollout control, never an authorization input.
    if !state.space_features.run_manager_stop {
        return reply(
            StatusCode::NOT_FOUND,
            "feature_unavailable",
            "Stopping runs is not enabled.",
        );
    }
    // 2. Membership, then shape.
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return reply(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to stop a run.",
        );
    }
    let space_ref = space_ref.trim();
    if !valid_space_ref(space_ref) {
        return reply(StatusCode::BAD_REQUEST, "invalid_space", "A valid Space reference is required.");
    }
    let run_id = run_id.trim();
    if !valid_run_id(run_id) {
        return reply(StatusCode::BAD_REQUEST, "invalid_run", "A valid run id is required.");
    }
    let idempotency_key = match body.get("idempotency_key") {
        None | Some(Value::Null) => format!("space-stop-{}", unix_nanos()),
        Some(Value::String(key)) if valid_idempotency_key(key.trim()) => key.trim().to_owned(),
        Some(_) => {
            return reply(
                StatusCode::BAD_REQUEST,
                "invalid_idempotency_key",
                "The idempotency key is not valid.",
            )
        }
    };

    // 3. No Zero Data Retention refusal (D23): a stop writes only ids and a reason, so it is the safe
    // direction under every posture. Session Core refuses only what it must.

    // 4. Space lifecycle.
    match space_lifecycle_by_ref(state, user, &org_id, space_ref).await {
        Ok(Some(_)) => {}
        Ok(None) => return reply(StatusCode::NOT_FOUND, "space_not_found", "Space is not available."),
        Err(()) => {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            )
        }
    }

    // 5. A fresh Control run-control decision for THIS stop.
    let (decision_ref, token) =
        match run_control_decision(state, user, &org_id, space_ref, run_id, &idempotency_key).await {
            Ok(decision) => decision,
            Err(reply) => return reply,
        };

    // 6. Delegated credentials, then model-gateway. The decision rides in the body, never the URL.
    let empty = HeaderMap::new();
    let headers = headers.unwrap_or(&empty);
    let session_token =
        match crate::domains::chat::shared::required_session_token(state, user, headers).await {
            Ok(token) => token,
            Err(reason) => {
                let response =
                    crate::domains::chat::shared::delegated_auth_unavailable(reason).into_response();
                return (
                    response.status(),
                    Json(error(
                        "delegated_auth_unavailable",
                        "Delegated credentials are unavailable.",
                    )),
                );
            }
        };
    let model_token = crate::domains::chat::shared::model_token(state, user, headers).await;
    let url = format!(
        "{}/v1/spaces/{}/runs/{}/stop",
        state.model_gateway_url,
        urlencoding::encode(space_ref),
        urlencoding::encode(run_id),
    );
    let (status, Json(response)) = crate::domains::chat::shared::proxy_model_json_with_session(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_control_decision_ref": decision_ref,
            "space_control_decision_token": token,
            "idempotency_key": idempotency_key,
        })),
        model_token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    relay_stop_answer(status, &response, run_id)
}

/// Asks Control for the `model.run.control` decision of this stop. A refusal is 403
/// `space_run_control_not_authorized` (typed details when Control gave a reason); an absent endpoint
/// (an older Control) is the same refusal; anything else is an unavailable Control, never a grant.
async fn run_control_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    run_id: &str,
    idempotency_key: &str,
) -> Result<(String, String), Reply> {
    const DENIED: &str = "Stopping this run is not authorized.";
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!("{}/api/v1/internal/spaces/run-control-decision", state.user_core_url);
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_ref": space_ref,
            "run_id": run_id,
            "operation": "cancel",
            "idempotency_key": idempotency_key,
        })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    let status = denial::refusal_status(status, &response);
    if status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND {
        let typed = denial::SpaceDenial::from_response(state, status, &response);
        let body = match typed {
            Some(denial) => error_with_details(
                "space_run_control_not_authorized",
                DENIED,
                denial.details_value(),
            ),
            None => error("space_run_control_not_authorized", DENIED),
        };
        return Err((StatusCode::FORBIDDEN, Json(body)));
    }
    if !status.is_success() {
        return Err(reply(
            StatusCode::SERVICE_UNAVAILABLE,
            "run_control_unavailable",
            "Run control is temporarily unavailable.",
        ));
    }
    let data = unwrap_data(&response);
    let decision_ref = data
        .get("decision")
        .and_then(Value::as_object)
        .and_then(|decision| decision.get("decision_ref"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    let token = data
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    if decision_ref.is_empty() || token.is_empty() {
        return Err(reply(
            StatusCode::BAD_GATEWAY,
            "invalid_space_decision",
            "Control returned an invalid run-control decision.",
        ));
    }
    Ok((decision_ref, token))
}

/// The browser's answer to a model-gateway stop answer: the success shape rebuilt field by field, a
/// ZDR catalogue refusal passed through, the closed refusal codes in fixed words, everything else
/// `run_control_unavailable`. Upstream text, ids and decision material never cross.
fn relay_stop_answer(status: StatusCode, response: &Value, run_id: &str) -> Reply {
    if status.is_success() {
        let outcome = response.get("status").and_then(Value::as_str).unwrap_or("cancelled");
        let outcome = if outcome == "already_terminal" { "already_terminal" } else { "cancelled" };
        return (
            StatusCode::OK,
            Json(json!({"data": {
                "run_id": run_id,
                "status": outcome,
                "cancelled": response.get("cancelled").and_then(Value::as_bool).unwrap_or(false),
                "receipt_id": response.get("receipt_id").and_then(Value::as_str).unwrap_or_default(),
            }})),
        );
    }
    let code = response
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if code == "zdr_durable_mutation_forbidden" || code == "zdr_feature_not_supported" {
        return (
            StatusCode::PRECONDITION_FAILED,
            Json(zdr::zdr_refusal_body(zdr::ZDR_DURABLE_MUTATION_FORBIDDEN)),
        );
    }
    if let Some((code, status, message)) = STOP_RELAYED.iter().find(|(known, _, _)| *known == code) {
        return (*status, Json(error(*code, *message)));
    }
    reply(
        StatusCode::SERVICE_UNAVAILABLE,
        "run_control_unavailable",
        "Run control is temporarily unavailable.",
    )
}

#[cfg(test)]
mod tests {
    //! WS09-S4 (the live view) and WS09-S8 (the manager stop) through the real router. The session,
    //! Control, org-core, Convex and model-gateway are doubles, so each refusal order and the
    //! decision's path (POST body only, never a URL or an answer) is checked on the wire.
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const LIVE: &str = "/api/v1/spaces/space-room/runs/run-1/live";
    const STOP: &str = "/api/v1/spaces/space-room/runs/run-1/stop";
    const READ_TOKEN: &str = "read-token-secret-xyz";
    const STOP_TOKEN: &str = "stop-token-secret-xyz";

    struct Fixture {
        state: crate::config::AppState,
        user_core: MockServer,
        model_gateway: MockServer,
        _others: Vec<MockServer>,
    }

    /// `org` is the organisation read the Zero Data Retention guard makes; `read_decision` and
    /// `control_decision` are Control's answers to the live read and the stop. The session is the
    /// default one: a user with `role` in org-1.
    async fn fixture(
        role: &str,
        org: ResponseTemplate,
        read_decision: ResponseTemplate,
        control_decision: ResponseTemplate,
    ) -> Fixture {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        for slug in ["session-core", "capability-core"] {
            Mock::given(wm_method("GET"))
                .and(wm_path(format!("/api/{slug}/token")))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"token": "plane-token"})),
                )
                .mount(&auth)
                .await;
        }
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": role, "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/thread-read-decision"))
            .respond_with(read_decision)
            .mount(&user_core)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/run-control-decision"))
            .respond_with(control_decision)
            .mount(&user_core)
            .await;
        let org_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(org)
            .mount(&org_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                {"spaceRef": "space-room", "name": "Room", "kind": "room", "lifecycle": "active"}
            ]})))
            .mount(&application)
            .await;
        let model_gateway = MockServer::start().await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.org_core_url = org_core.uri();
        state.application_convex_url = application.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.model_gateway_url = model_gateway.uri();
        Fixture {
            state,
            user_core,
            model_gateway,
            _others: vec![auth, org_core, application],
        }
    }

    fn org_record(zdr: bool) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_json(json!({"data": {"metadata": {"interactiveRetention": {"zdr": zdr}}}}))
    }

    fn granted_read() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"data": {
            "decision": {"decision_ref": "read-ref-1"},
            "token": READ_TOKEN
        }}))
    }

    fn granted_stop() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"data": {
            "decision": {"decision_ref": "stop-ref-1"},
            "token": STOP_TOKEN
        }}))
    }

    fn declined() -> ResponseTemplate {
        ResponseTemplate::new(403).set_body_json(json!({"error": {"code": "forbidden"}}))
    }

    async fn call(
        state: crate::config::AppState,
        method: &str,
        uri: &str,
        extra: &[(&str, &str)],
        body: Option<Value>,
    ) -> (u16, axum::http::HeaderMap, Vec<u8>) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", "better-auth.session_token=session-1");
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        for (name, value) in extra {
            builder = builder.header(*name, *value);
        }
        let request = builder
            .body(Body::from(body.map(|value| value.to_string()).unwrap_or_default()))
            .unwrap();
        let response = crate::build_router(state)
            .oneshot(request)
            .await
            .expect("the router answers");
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("a body")
            .to_bytes()
            .to_vec();
        (status, headers, bytes)
    }

    fn json_of(bytes: &[u8]) -> Value {
        serde_json::from_slice(bytes).unwrap_or(Value::Null)
    }

    async fn calls_to(server: &MockServer, path_part: &str) -> Vec<wiremock::Request> {
        server
            .received_requests()
            .await
            .expect("request recording")
            .into_iter()
            .filter(|request| request.url.path().contains(path_part))
            .collect()
    }

    // ---- WS09-S4: the live view -------------------------------------------------------------

    #[tokio::test]
    async fn the_live_view_is_404_and_calls_nothing_while_its_flag_is_off() {
        let fixture = fixture("member", org_record(false), granted_read(), declined()).await;
        let (status, _, body) = call(fixture.state.clone(), "GET", LIVE, &[], None).await;
        assert_eq!(status, 404);
        assert_eq!(json_of(&body)["error"]["code"], "feature_unavailable");
        assert!(calls_to(&fixture.user_core, "thread-read-decision").await.is_empty());
        assert!(fixture.model_gateway.received_requests().await.unwrap_or_default().is_empty());
    }

    #[tokio::test]
    async fn a_zdr_org_is_refused_with_the_typed_agent_run_refusal_before_any_control_call() {
        let mut fixture = fixture("member", org_record(true), granted_read(), declined()).await;
        fixture.state.space_features.run_live_view = true;
        let (status, _, body) = call(fixture.state.clone(), "GET", LIVE, &[], None).await;
        assert_eq!(status, 403);
        assert_eq!(json_of(&body)["error"]["code"], "zdr_feature_not_supported");
        assert_eq!(json_of(&body)["error"]["feature"], "agent_run");
        assert!(calls_to(&fixture.user_core, "thread-read-decision").await.is_empty());
        assert!(fixture.model_gateway.received_requests().await.unwrap_or_default().is_empty());
    }

    #[tokio::test]
    async fn an_unknown_org_posture_is_503_before_any_control_call() {
        let mut fixture = fixture(
            "member",
            ResponseTemplate::new(503),
            granted_read(),
            declined(),
        )
        .await;
        fixture.state.space_features.run_live_view = true;
        let (status, _, body) = call(fixture.state.clone(), "GET", LIVE, &[], None).await;
        assert_eq!(status, 503);
        assert_eq!(json_of(&body)["error"]["code"], "zdr_decision_unavailable");
        assert!(calls_to(&fixture.user_core, "thread-read-decision").await.is_empty());
    }

    #[tokio::test]
    async fn control_declining_the_read_is_space_read_not_authorized() {
        let mut fixture = fixture("member", org_record(false), declined(), declined()).await;
        fixture.state.space_features.run_live_view = true;
        let (status, _, body) = call(fixture.state.clone(), "GET", LIVE, &[], None).await;
        assert_eq!(status, 403);
        assert_eq!(json_of(&body)["error"]["code"], "space_read_not_authorized");
        assert!(fixture.model_gateway.received_requests().await.unwrap_or_default().is_empty());
    }

    #[tokio::test]
    async fn the_read_decision_rides_in_the_post_body_and_forged_headers_never_reach_the_model() {
        let mut fixture = fixture("member", org_record(false), granted_read(), declined()).await;
        fixture.state.space_features.run_live_view = true;
        let live_answer = "event: step\ndata: {\"ok\":true}\n\n";
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/spaces/space-room/runs/run-1/live"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(live_answer),
            )
            .mount(&fixture.model_gateway)
            .await;
        let (status, _, body) = call(
            fixture.state.clone(),
            "GET",
            LIVE,
            &[
                ("x-space-read-decision-token", "forged-token"),
                ("x-space-read-decision-ref", "forged-ref"),
            ],
            None,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(String::from_utf8_lossy(&body), live_answer, "the stream is relayed byte for byte");
        assert!(!String::from_utf8_lossy(&body).contains(READ_TOKEN));

        let sent = calls_to(&fixture.model_gateway, "/runs/run-1/live").await;
        assert_eq!(sent.len(), 1, "one model call");
        let request = &sent[0];
        assert_eq!(request.method.as_str(), "POST");
        assert!(!request.url.as_str().contains(READ_TOKEN), "the decision is never in a URL");
        assert!(!request.url.as_str().contains("read-ref-1"), "the decision ref is never in a URL");
        let sent_body: Value = serde_json::from_slice(&request.body).expect("a JSON body");
        assert_eq!(sent_body["space_read_decision_token"], READ_TOKEN);
        assert_eq!(sent_body["space_read_decision_ref"], "read-ref-1");
        assert!(request.headers.get("x-space-read-decision-token").is_none(), "forged header dropped");
        assert!(request.headers.get("x-space-read-decision-ref").is_none(), "forged header dropped");
    }

    #[tokio::test]
    async fn a_relayed_model_refusal_keeps_its_fixed_words_and_an_unknown_one_is_model_error() {
        let mut fixture = fixture("member", org_record(false), granted_read(), declined()).await;
        fixture.state.space_features.run_live_view = true;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/spaces/space-room/runs/run-1/live"))
            .respond_with(ResponseTemplate::new(410).set_body_json(json!({
                "error": {"code": "run_stream_expired", "message": "upstream words"}
            })))
            .up_to_n_times(1)
            .mount(&fixture.model_gateway)
            .await;
        let (status, _, body) = call(fixture.state.clone(), "GET", LIVE, &[], None).await;
        assert_eq!(status, 410);
        assert_eq!(json_of(&body)["error"]["code"], "run_stream_expired");
        assert_eq!(
            json_of(&body)["error"]["message"],
            "The live view of this run has ended."
        );

        fixture.model_gateway.reset().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/spaces/space-room/runs/run-1/live"))
            .respond_with(ResponseTemplate::new(500).set_body_json(json!({
                "error": {"code": "stack_internal_detail", "message": "secret internals"}
            })))
            .mount(&fixture.model_gateway)
            .await;
        let (_, _, body) = call(fixture.state.clone(), "GET", LIVE, &[], None).await;
        let text = String::from_utf8_lossy(&body).to_string();
        assert_eq!(json_of(&body)["error"]["code"], "model_error", "{text}");
        assert!(!text.contains("secret internals"), "upstream text never crosses: {text}");
        assert!(!text.contains("stack_internal_detail"), "unknown codes never cross: {text}");
    }

    // ---- WS09-S8: the manager stop ----------------------------------------------------------

    #[tokio::test]
    async fn the_stop_is_404_and_calls_nothing_while_its_flag_is_off() {
        let fixture = fixture("admin", org_record(false), declined(), granted_stop()).await;
        let (status, _, body) =
            call(fixture.state.clone(), "POST", STOP, &[], Some(json!({}))).await;
        assert_eq!(status, 404);
        assert_eq!(json_of(&body)["error"]["code"], "feature_unavailable");
        assert!(calls_to(&fixture.user_core, "run-control-decision").await.is_empty());
    }

    /// D23: a stop is the safe direction under zero data retention, so a zero-retention organisation's
    /// manager may stop a run. The stop goes to Control and then to model-gateway like any other.
    #[tokio::test]
    async fn a_zdr_org_may_stop_a_run_and_the_stop_reaches_control_and_model_gateway() {
        let mut fixture = fixture("admin", org_record(true), declined(), granted_stop()).await;
        fixture.state.space_features.run_manager_stop = true;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/spaces/space-room/runs/run-1/stop"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "status": "cancelled", "cancelled": true, "receipt_id": "rcpt-zdr"
            })))
            .mount(&fixture.model_gateway)
            .await;
        let (status, _, body) =
            call(fixture.state.clone(), "POST", STOP, &[], Some(json!({}))).await;
        let text = String::from_utf8_lossy(&body).to_string();
        assert_eq!(status, 200, "{text}");
        assert_eq!(json_of(&body)["data"]["cancelled"], true);
        assert_eq!(calls_to(&fixture.user_core, "run-control-decision").await.len(), 1);
        assert_eq!(calls_to(&fixture.model_gateway, "/runs/run-1/stop").await.len(), 1);
    }

    #[tokio::test]
    async fn a_control_refusal_or_a_missing_control_route_is_403_and_carries_no_token() {
        for refusal in [403_u16, 404] {
            let mut fixture = fixture(
                "member",
                org_record(false),
                declined(),
                ResponseTemplate::new(refusal)
                    .set_body_json(json!({"error": {"code": "forbidden"}})),
            )
            .await;
            fixture.state.space_features.run_manager_stop = true;
            let (status, _, body) =
                call(fixture.state.clone(), "POST", STOP, &[], Some(json!({}))).await;
            let text = String::from_utf8_lossy(&body).to_string();
            assert_eq!(status, 403, "control {refusal}");
            assert_eq!(json_of(&body)["error"]["code"], "space_run_control_not_authorized");
            assert!(!text.contains(STOP_TOKEN), "no token in the answer: {text}");
            assert!(fixture.model_gateway.received_requests().await.unwrap_or_default().is_empty());
        }
    }

    #[tokio::test]
    async fn the_stop_decision_rides_in_the_model_body_and_the_answer_is_the_fixed_receipt() {
        let mut fixture = fixture("admin", org_record(false), declined(), granted_stop()).await;
        fixture.state.space_features.run_manager_stop = true;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/spaces/space-room/runs/run-1/stop"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "status": "cancelled", "cancelled": true, "receipt_id": "rcpt-1"
            })))
            .mount(&fixture.model_gateway)
            .await;
        let (status, _, body) = call(
            fixture.state.clone(),
            "POST",
            STOP,
            &[("x-space-control-decision-token", "forged-token")],
            Some(json!({"idempotency_key": "stop-key-1"})),
        )
        .await;
        let text = String::from_utf8_lossy(&body).to_string();
        assert_eq!(status, 200, "{text}");
        assert_eq!(
            json_of(&body)["data"],
            json!({"run_id": "run-1", "status": "cancelled", "cancelled": true, "receipt_id": "rcpt-1"})
        );
        assert!(!text.contains(STOP_TOKEN) && !text.contains("stop-ref-1"), "decision stays off the answer");

        let sent = calls_to(&fixture.model_gateway, "/runs/run-1/stop").await;
        assert_eq!(sent.len(), 1);
        let request = &sent[0];
        assert!(!request.url.as_str().contains(STOP_TOKEN), "the decision is never in a URL");
        let sent_body: Value = serde_json::from_slice(&request.body).expect("a JSON body");
        assert_eq!(sent_body["space_control_decision_token"], STOP_TOKEN);
        assert_eq!(sent_body["space_control_decision_ref"], "stop-ref-1");
        assert_eq!(sent_body["idempotency_key"], "stop-key-1");
        assert!(request.headers.get("x-space-control-decision-token").is_none(), "forged header dropped");
    }

    #[tokio::test]
    async fn relayed_stop_codes_keep_fixed_words_and_anything_else_is_run_control_unavailable() {
        let mut fixture = fixture("admin", org_record(false), declined(), granted_stop()).await;
        fixture.state.space_features.run_manager_stop = true;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/spaces/space-room/runs/run-1/stop"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "error": {"code": "run_not_found", "message": "internal row id 42"}
            })))
            .up_to_n_times(1)
            .mount(&fixture.model_gateway)
            .await;
        let (status, _, body) =
            call(fixture.state.clone(), "POST", STOP, &[], Some(json!({}))).await;
        assert_eq!(status, 404);
        assert_eq!(json_of(&body)["error"]["code"], "run_not_found");
        assert_eq!(json_of(&body)["error"]["message"], "The run is not available.");

        fixture.model_gateway.reset().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/spaces/space-room/runs/run-1/stop"))
            .respond_with(ResponseTemplate::new(500).set_body_json(json!({
                "error": {"code": "db_secret_detail", "message": "connection string"}
            })))
            .mount(&fixture.model_gateway)
            .await;
        let (status, _, body) =
            call(fixture.state.clone(), "POST", STOP, &[], Some(json!({}))).await;
        let text = String::from_utf8_lossy(&body).to_string();
        assert_eq!(status, 503, "{text}");
        assert_eq!(json_of(&body)["error"]["code"], "run_control_unavailable");
        assert!(!text.contains("db_secret_detail") && !text.contains("connection string"), "{text}");
    }

    #[tokio::test]
    async fn an_invalid_idempotency_key_is_400_before_any_control_call() {
        let mut fixture = fixture("admin", org_record(false), declined(), granted_stop()).await;
        fixture.state.space_features.run_manager_stop = true;
        let (status, _, body) = call(
            fixture.state.clone(),
            "POST",
            STOP,
            &[],
            Some(json!({"idempotency_key": "not a key!"})),
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(json_of(&body)["error"]["code"], "invalid_idempotency_key");
        assert!(calls_to(&fixture.user_core, "run-control-decision").await.is_empty());
    }
}
