//! The owner's read of one background process's output in a run (W4-P3, F-t3-035).
//!
//! `GET /api/v1/chat/runs/{run_id}/processes/{pid}/output?space_ref=&after_seq=&max_bytes=` relays
//! Model Gateway's `GET /v1/runs/{run_id}/processes/{pid}/output`: one page of the terminal a
//! background process wrote, the output the computer panel draws. This hop decides nothing about
//! what the caller may read. Each step is a refusal before the next one runs:
//!
//! 1. the rollout switch `CHAT_PROCESS_OUTPUT_READ_ENABLED`, read once in `build_state` (404
//!    `process_output_disabled` while it is off; never an authorization input),
//! 2. live organization membership (403 `active_membership_required`), then the request shape:
//!    `run_id` and `pid` are 1 to 64 characters of `[A-Za-z0-9_-]`, `space_ref` is a required
//!    URL-safe reference, and a negative cursor is refused. These are the checks Model Gateway makes,
//!    with its own id and cursor codes (400 `invalid_run_id`, `invalid_process_id`,
//!    `invalid_cursor`) and `invalid_space` for the reference,
//! 3. the caller's own `aud=sandbox-manager` bearer, minted per request (503
//!    `delegated_auth_unavailable` when it cannot be),
//! 4. a fresh Control read decision addressed to sandbox-manager, minted here for this request (403
//!    `process_read_not_authorized` when Control declines; an erroring Control is relayed and is
//!    never read as a decline),
//! 5. the relay, with the caller's model and sandbox bearers in headers and the decision in the
//!    query. Model Gateway checks through Session Core that the caller owns the run, then reads the
//!    process under that decision.
//!
//! The org and the user are this server's own, never the browser's, and no bearer or decision is
//! taken from the query. A zero-retention caller is refused by Model Gateway (`run_inspection`), and
//! that refusal reaches the browser unchanged. The answer holds the owner's own process output and is
//! never cached.

use axum::{
    extract::{Extension, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde::Deserialize;

use crate::{
    config::AppState,
    domains::spaces::{shared_thread_read_decision_for_typed, READ_AUDIENCE_SANDBOX_MANAGER},
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
};

use super::shared;

/// The typed refusal for a Control read decision that declined sandbox-manager. Named for the
/// operation, as the Space routes name theirs, so the browser can tell this read from the others.
const PROCESS_READ_NOT_AUTHORIZED: &str = "process_read_not_authorized";

/// The most characters a run id or a process id may have. This is Model Gateway's limit.
const MAX_ID_LEN: usize = 64;

/// The longest Space reference the Space routes accept.
const MAX_SPACE_REF_LEN: usize = 200;

/// The query the browser sends. `space_ref` names the room the read is made under, and the cursor and
/// the page size are passed on as given. Any other key is ignored by the extractor, so a decision, an
/// org or a user id the browser sends never reaches the upstream.
#[derive(Debug, Deserialize)]
pub(super) struct ProcessOutputQuery {
    /// The room whose read authority the upstream checks.
    space_ref: Option<String>,
    /// The sequence number the page starts after; absent reads from the start.
    after_seq: Option<i64>,
    /// The page size in bytes; absent is sent as 0, which sandbox-manager reads as its own page limit.
    max_bytes: Option<i64>,
}

/// `GET /api/v1/chat/runs/{run_id}/processes/{pid}/output`. The steps are in the module doc.
pub(super) async fn get_run_process_output(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((run_id, pid)): Path<(String, String)>,
    Query(query): Query<ProcessOutputQuery>,
) -> Response {
    no_store(read_process_output(&state, &user, &headers, &run_id, &pid, query).await)
}

async fn read_process_output(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    run_id: &str,
    pid: &str,
    query: ProcessOutputQuery,
) -> Response {
    // 1. The rollout switch, read once by build_state. Off, the route is not there for anyone.
    if !state.chat_process_output_enabled {
        return refuse(
            StatusCode::NOT_FOUND,
            "process_output_disabled",
            "Reading background process output is not enabled.",
        );
    }
    // 2. Membership, then shape, with Model Gateway's own checks.
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to read process output.",
        );
    }
    let run_id = run_id.trim();
    if !valid_id(run_id) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_run_id",
            "run_id must be 1 to 64 characters of [A-Za-z0-9_-].",
        );
    }
    let pid = pid.trim();
    if !valid_id(pid) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_process_id",
            "process id must be 1 to 64 characters of [A-Za-z0-9_-].",
        );
    }
    let space_ref = query.space_ref.as_deref().unwrap_or_default().trim();
    if !valid_space_ref(space_ref) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A valid Space reference is required.",
        );
    }
    let after_seq = query.after_seq.unwrap_or(0);
    let max_bytes = query.max_bytes.unwrap_or(0);
    if after_seq < 0 || max_bytes < 0 {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
            "after_seq and max_bytes must not be negative.",
        );
    }
    // 3. The caller's own sandbox-manager bearer, minted per request as the Work tab mints it.
    let sandbox = match shared::required_sandbox_token(state, user, headers).await {
        Ok(token) => token,
        Err(reason) => return shared::delegated_auth_unavailable(reason).into_response(),
    };
    // 4. A fresh Control read decision for sandbox-manager's audience. It is minted here and never
    // taken from the browser, so a decision addressed to another audience cannot stand in for it.
    let (grant, denial) = match shared_thread_read_decision_for_typed(
        state,
        user,
        &org_id,
        space_ref,
        READ_AUDIENCE_SANDBOX_MANAGER,
    )
    .await
    {
        Ok(decision) => decision.split(),
        Err(response) => return response.into_response(),
    };
    let Some((decision_ref, token)) = grant else {
        let message = "Reading this run's process output is not authorized.";
        return match denial {
            Some(denial) => (
                StatusCode::FORBIDDEN,
                Json(error_with_details(
                    PROCESS_READ_NOT_AUTHORIZED,
                    message,
                    denial.details_value(),
                )),
            )
                .into_response(),
            None => refuse(StatusCode::FORBIDDEN, PROCESS_READ_NOT_AUTHORIZED, message),
        };
    };
    // 5. The relay. The model and sandbox bearers travel in headers and the decision travels in the
    // query, where Model Gateway reads it. The org and the user are this server's own.
    let model_token = shared::model_token(state, user, headers).await;
    let url = format!(
        "{}/v1/runs/{}/processes/{}/output?space_ref={}&space_read_decision_ref={}&space_read_decision_token={}&after_seq={}&max_bytes={}",
        state.model_gateway_url,
        urlencoding::encode(run_id),
        urlencoding::encode(pid),
        urlencoding::encode(space_ref),
        urlencoding::encode(&decision_ref),
        urlencoding::encode(&token),
        after_seq,
        max_bytes,
    );
    let (status, Json(payload)) = shared::proxy_model_json_with_sandbox(
        state,
        Method::GET,
        &url,
        None,
        model_token.as_deref(),
        Some(&sandbox),
        user,
    )
    .await;
    (status, Json(payload)).into_response()
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (status, Json(error(code, message))).into_response()
}

/// The owner's output can hold anything a process printed, so no cache keeps the answer.
fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Model Gateway's id shape for a run and for a process: 1 to 64 ASCII letters, digits, `-` and `_`.
fn valid_id(value: &str) -> bool {
    (1..=MAX_ID_LEN).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// The URL-safe Space reference Model Gateway accepts (ASCII letters, digits, `-`, `_`, `.` and `~`),
/// at most `MAX_SPACE_REF_LEN` characters.
fn valid_space_ref(value: &str) -> bool {
    (1..=MAX_SPACE_REF_LEN).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'))
}
