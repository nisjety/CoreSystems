//! The person's read and stop of one addressable research run (Quarry v2 E4.6, slice Q4-062b).
//!
//! - `GET /api/v1/chat/research-runs/{id}` relays Model Gateway's `GET /v1/research/runs/:run_id`.
//! - `POST /api/v1/chat/research-runs/{id}/cancel` relays `POST /v1/research/runs/:run_id/cancel`.
//!
//! Built dark: `chat.rs` registers both routes only while `CHAT_RESEARCH_RUNS` is on
//! (`SpaceFeatureFlags::chat_research_runs`, read once in `build_state`), so off they answer 404.
//!
//! Authority stays upstream. The BFF forwards the user's own model bearer and session bearer exactly
//! as the other chat JSON handlers do, adds no scoping header the client could have forged, and never
//! names the upstream URL in an answer. Model Gateway and Session Core decide that the run is the
//! caller's own; a run of another owner reads as not found. A zero-retention or waived run comes back
//! from upstream with ids, status and counters only, so `reportId` is simply absent for it.
//!
//! Answers are normalised to camelCase: `{ data: { id, threadId, status, stopReason, pagesRead,
//! sourcesFound, spentUsd, reportId? } }` for a read, `{ data: { id, stopRequested } }` (202) for a
//! cancel, and `{ error: { code, message, details } }` for every refusal. A 404 stays 404 and a 409
//! stays 409.

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Map, Value};

use crate::{config::AppState, envelope::error_with_details, middleware::AuthenticatedUser};

use super::shared;

/// The prefix Session Core gives every research run id.
const RUN_ID_PREFIX: &str = "rsr_";

/// A ULID is 26 Crockford base32 characters.
const ULID_LEN: usize = 26;

/// The statuses a research run can carry. `cancelled` is the end a person's stop gives a run
/// (the stream's `research_run` frame uses the same word). Anything else from upstream is refused
/// rather than passed on.
const RUN_STATUSES: [&str; 5] = ["running", "completed", "stopped", "cancelled", "failed"];

/// The longest error code or message relayed from upstream; longer text is cut.
const MAX_ERROR_CODE_LEN: usize = 64;
const MAX_ERROR_MESSAGE_LEN: usize = 300;

/// The longest stop reason relayed from upstream.
const MAX_STOP_REASON_LEN: usize = 64;

/// The longest thread or report id relayed from upstream.
const MAX_ID_LEN: usize = 128;

/// `^rsr_[0-9A-HJKMNP-TV-Z]{26}$`: Session Core's id shape, checked before any upstream call.
fn is_research_run_id(raw: &str) -> bool {
    let Some(ulid) = raw.strip_prefix(RUN_ID_PREFIX) else {
        return false;
    };
    ulid.len() == ULID_LEN
        && ulid.bytes().all(|byte| {
            byte.is_ascii_digit()
                || (byte.is_ascii_uppercase() && !matches!(byte, b'I' | b'L' | b'O' | b'U'))
        })
}

fn invalid_research_run_id() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            "invalid_research_run_id",
            "The research run id is not valid.",
            json!({ "field": "id" }),
        )),
    )
        .into_response()
}

fn research_run_url(base: &str, run_id: &str) -> String {
    format!("{base}/v1/research/runs/{}", urlencoding::encode(run_id))
}

fn research_run_cancel_url(base: &str, run_id: &str) -> String {
    format!("{}/cancel", research_run_url(base, run_id))
}

/// `GET /api/v1/chat/research-runs/{id}`: one research run the caller owns.
pub(super) async fn get_research_run(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> Response {
    if !is_research_run_id(&run_id) {
        return invalid_research_run_id();
    }
    let url = research_run_url(&state.model_gateway_url, &run_id);
    let (status, payload) = match relay(&state, &user, &headers, Method::GET, &url).await {
        Ok(answer) => answer,
        Err(response) => return response,
    };
    if !status.is_success() {
        return error_response(status, &payload);
    }
    match run_body(&payload) {
        Some(data) => (StatusCode::OK, Json(json!({ "data": data }))).into_response(),
        None => upstream_unavailable(),
    }
}

/// `POST /api/v1/chat/research-runs/{id}/cancel`: asks the live stream of a research run the caller
/// owns to stop. Answered 202; the stream then ends the run as `cancelled`.
pub(super) async fn cancel_research_run(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> Response {
    if !is_research_run_id(&run_id) {
        return invalid_research_run_id();
    }
    let url = research_run_cancel_url(&state.model_gateway_url, &run_id);
    let (status, payload) = match relay(&state, &user, &headers, Method::POST, &url).await {
        Ok(answer) => answer,
        Err(response) => return response,
    };
    if !status.is_success() {
        return error_response(status, &payload);
    }
    let data = payload.get("data").and_then(Value::as_object);
    let Some(id) = data
        .and_then(|data| data.get("id"))
        .and_then(Value::as_str)
        .filter(|id| is_research_run_id(id))
    else {
        return upstream_unavailable();
    };
    let stop_requested = data
        .and_then(|data| data.get("stopRequested"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    (
        StatusCode::ACCEPTED,
        Json(json!({ "data": { "id": id, "stopRequested": stop_requested } })),
    )
        .into_response()
}

/// One call to Model Gateway under the user's own model bearer and session bearer, the same pair the
/// other session-backed chat JSON handlers forward. The org comes from the verified session; a caller
/// without an authorized org scope is refused before any call.
async fn relay(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    method: Method,
    url: &str,
) -> Result<(StatusCode, Value), Response> {
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error_with_details(
                "org_scope_required",
                "An authorized organization scope is required.",
                json!({}),
            )),
        )
            .into_response());
    }
    let token = shared::model_token(state, user, headers).await;
    let session_token = match shared::required_session_token(state, user, headers).await {
        Ok(token) => token,
        Err(error) => return Err(shared::delegated_auth_unavailable(error).into_response()),
    };
    let (status, Json(payload)) = shared::proxy_model_json_with_session(
        state,
        method,
        url,
        None,
        token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    Ok((status, payload))
}

/// Text cut to at most `max` characters, on a character boundary.
fn capped(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// A refusal in the gateway's envelope, `{ error: { code, message, details } }`. A typed upstream
/// refusal keeps its status, code, message and details (404 stays 404, 409 stays 409); an untyped one
/// becomes the gateway's own envelope, and any other failure without a typed body becomes 502.
fn error_response(status: StatusCode, payload: &Value) -> Response {
    let upstream = payload.get("error").and_then(Value::as_object);
    let code = upstream
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|code| !code.is_empty());
    let Some(code) = code else {
        return match status {
            StatusCode::NOT_FOUND => (
                StatusCode::NOT_FOUND,
                Json(error_with_details(
                    "research_run_not_found",
                    "Research run not found.",
                    json!({}),
                )),
            )
                .into_response(),
            StatusCode::CONFLICT => (
                StatusCode::CONFLICT,
                Json(error_with_details(
                    "research_run_conflict",
                    "The research run cannot be changed in its current state.",
                    json!({}),
                )),
            )
                .into_response(),
            _ => upstream_unavailable(),
        };
    };
    let message = upstream
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("The research run request was refused.");
    let details = upstream
        .and_then(|error| error.get("details"))
        .filter(|details| details.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let status = if status.is_client_error() || status.is_server_error() {
        status
    } else {
        StatusCode::BAD_GATEWAY
    };
    (
        status,
        Json(json!({
            "error": {
                "code": capped(code, MAX_ERROR_CODE_LEN),
                "message": capped(message, MAX_ERROR_MESSAGE_LEN),
                "details": details,
            }
        })),
    )
        .into_response()
}

fn upstream_unavailable() -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(crate::envelope::upstream_unavailable()),
    )
        .into_response()
}

/// A text field of the upstream run, under its camelCase name or its snake_case one.
fn text_field<'a>(data: &'a Map<String, Value>, camel: &str, snake: &str) -> Option<&'a str> {
    data.get(camel)
        .or_else(|| data.get(snake))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// A counter of the upstream run; a missing or malformed counter reads as zero.
fn count_field(data: &Map<String, Value>, camel: &str, snake: &str) -> u64 {
    data.get(camel)
        .or_else(|| data.get(snake))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// The run as the browser reads it. Only the named fields are passed on, so anything else upstream
/// adds (a partial result, the budget) never reaches the browser through this route. `None` when the
/// upstream answer is not a run: no valid id, or a status outside the closed set.
fn run_body(payload: &Value) -> Option<Value> {
    let data = payload.get("data").and_then(Value::as_object)?;
    let id = text_field(data, "id", "id").filter(|id| is_research_run_id(id))?;
    let status = text_field(data, "status", "status")
        .map(str::to_ascii_lowercase)
        .filter(|status| RUN_STATUSES.contains(&status.as_str()))?;
    let thread_id = text_field(data, "threadId", "thread_id").map_or(Value::Null, |thread_id| {
        Value::String(capped(thread_id, MAX_ID_LEN))
    });
    let stop_reason = text_field(data, "stopReason", "stop_reason").map_or(Value::Null, |reason| {
        Value::String(capped(reason, MAX_STOP_REASON_LEN))
    });
    let spent_usd = data
        .get("spentUsd")
        .or_else(|| data.get("spent_usd"))
        .and_then(Value::as_f64)
        .filter(|usd| usd.is_finite() && *usd >= 0.0)
        .unwrap_or(0.0);
    let mut body = json!({
        "id": id,
        "threadId": thread_id,
        "status": status,
        "stopReason": stop_reason,
        "pagesRead": count_field(data, "pagesRead", "pages_read"),
        "sourcesFound": count_field(data, "sourcesFound", "sources_found"),
        "spentUsd": spent_usd,
    });
    // Upstream leaves the report id out for a zero-retention or waived run, and so does this route.
    if let Some(report_id) = text_field(data, "reportArtifactId", "report_artifact_id")
        .or_else(|| text_field(data, "reportId", "report_id"))
    {
        body["reportId"] = Value::String(capped(report_id, MAX_ID_LEN));
    }
    Some(body)
}
