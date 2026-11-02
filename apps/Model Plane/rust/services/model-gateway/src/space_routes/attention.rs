//! `POST /v1/spaces/attention` (build plan WS11-S2): the cross-room attention aggregate.
//!
//! A thin relay of Session Core's `SpaceAttention.GetSpaceAttention`. Authority is Session
//! Core's: this route forwards the caller's verified bearer plus one Control `model.thread.read`
//! decision per Space and holds none of its own. The org and user come from the verified claims,
//! never from the body. User principals only. The decisions travel in the body, never in a
//! query string, so they stay out of access logs; nothing in the body or the answer is logged by
//! this handler. A zero-retention caller is answered `zdr` per Space by Session Core.
//!
//! Errors, as `{error:{code,message,details}}`: flag off 404 `feature_unavailable`,
//! `UNAUTHENTICATED` 401 `unauthorized`, `PERMISSION_DENIED` 403 `space_attention_forbidden`,
//! `INVALID_ARGUMENT` or a body over the bounds 400 `invalid_request`, anything else 503
//! `space_attention_unavailable`. Messages are fixed words, never the upstream text.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Extension, Json, Router,
};
use mp_contracts::model_plane::v1::{
    AttentionStatus, GetSpaceAttentionRequest, SpaceAttentionQuery, SpaceAttentionResult, UnreadRule,
};
use mp_contracts::space_attention::{MAX_HEADLINE_IDS, MAX_SPACES};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{Claims, VerifiedSessionBearer};
use crate::http_routes::authenticated_session_request;
use crate::state::AppState;

const MAX_ID_BYTES: usize = 128;
const MAX_DECISION_REF_BYTES: usize = 256;
const MAX_DECISION_TOKEN_BYTES: usize = 4096;

/// The attention aggregate route.
pub fn router() -> Router<AppState> {
    Router::new().route("/v1/spaces/attention", post(space_attention))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttentionBody {
    /// `last_author` (default) or `legacy_owner`.
    #[serde(default)]
    rule: Option<String>,
    spaces: Vec<AttentionSpace>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttentionSpace {
    space_id: String,
    decision_ref: String,
    decision_token: String,
    #[serde(default)]
    since_ms: i64,
    #[serde(default)]
    headline_thread_ids: Vec<String>,
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": {} } })),
    )
        .into_response()
}

fn invalid(message: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, "invalid_request", message)
}

/// The HTTP form of an upstream refusal.
fn status_response(status: &tonic::Status) -> Response {
    match status.code() {
        tonic::Code::Unauthenticated => {
            error_response(StatusCode::UNAUTHORIZED, "unauthorized", "authentication required")
        }
        tonic::Code::PermissionDenied => error_response(
            StatusCode::FORBIDDEN,
            "space_attention_forbidden",
            "reading Space attention is not permitted",
        ),
        tonic::Code::InvalidArgument => invalid("the Space attention request is invalid"),
        tonic::Code::Unimplemented => error_response(
            StatusCode::NOT_FOUND,
            "feature_unavailable",
            "Space attention is not enabled",
        ),
        _ => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_attention_unavailable",
            "Space attention is unavailable",
        ),
    }
}

fn status_word(status: i32) -> &'static str {
    match AttentionStatus::try_from(status) {
        Ok(AttentionStatus::Ok) => "ok",
        Ok(AttentionStatus::Denied) => "denied",
        Ok(AttentionStatus::Zdr) => "zdr",
        _ => "error",
    }
}

fn result_json(result: &SpaceAttentionResult) -> Value {
    json!({
        "space_id": result.space_id,
        "status": status_word(result.status),
        "unread_threads": result.unread_threads,
        "unread_capped": result.unread_capped,
        "latest_activity_ms": result.latest_activity_ms,
        "working_runs": result.working_runs,
        "pending": result.pending.iter().map(|p| json!({
            "approval_id": p.approval_id,
            "run_id": p.run_id,
            "thread_id": p.thread_id,
            "requested_at_ms": p.requested_at_ms,
            "kind": p.kind,
        })).collect::<Vec<_>>(),
        "headlines": result.headlines.iter().map(|h| json!({
            "thread_id": h.thread_id,
            "title": h.title,
        })).collect::<Vec<_>>(),
    })
}

/// Bounds and shape of the body; the upstream restates them.
fn validate(body: &AttentionBody) -> Result<UnreadRule, &'static str> {
    let rule = match body.rule.as_deref() {
        None | Some("" | "last_author") => UnreadRule::LastAuthor,
        Some("legacy_owner") => UnreadRule::LegacyOwner,
        Some(_) => return Err("rule must be last_author or legacy_owner"),
    };
    if body.spaces.len() > MAX_SPACES {
        return Err("at most 60 Spaces per request");
    }
    for space in &body.spaces {
        let id = space.space_id.trim();
        if id.is_empty() || id.len() > MAX_ID_BYTES || id != space.space_id {
            return Err("space_id is invalid");
        }
        if space.decision_ref.trim().is_empty()
            || space.decision_ref.len() > MAX_DECISION_REF_BYTES
            || space.decision_token.trim().is_empty()
            || space.decision_token.len() > MAX_DECISION_TOKEN_BYTES
        {
            return Err("a Space decision is required for each Space");
        }
        if space.headline_thread_ids.len() > MAX_HEADLINE_IDS
            || space
                .headline_thread_ids
                .iter()
                .any(|thread| thread.is_empty() || thread.len() > MAX_ID_BYTES)
        {
            return Err("headline_thread_ids is invalid");
        }
    }
    Ok(rule)
}

async fn space_attention(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Json(body): Json<AttentionBody>,
) -> Response {
    // 1. Rollout flag. A rollout control only.
    if !state.space_attention_enabled {
        return error_response(
            StatusCode::NOT_FOUND,
            "feature_unavailable",
            "Space attention is not enabled",
        );
    }
    // 2. A user principal.
    if claims.user_id.trim().is_empty() {
        return error_response(
            StatusCode::FORBIDDEN,
            "space_attention_forbidden",
            "a user principal is required",
        );
    }
    // 3. Shape and bounds.
    let rule = match validate(&body) {
        Ok(rule) => rule,
        Err(message) => return invalid(message),
    };
    let request = match authenticated_session_request(
        GetSpaceAttentionRequest {
            org_id: claims.org_id.clone(),
            user_id: claims.user_id.clone(),
            rule: rule as i32,
            spaces: body
                .spaces
                .into_iter()
                .map(|space| SpaceAttentionQuery {
                    space_id: space.space_id,
                    decision_ref: space.decision_ref,
                    decision_token: space.decision_token,
                    since_ms: space.since_ms,
                    headline_thread_ids: space.headline_thread_ids,
                })
                .collect(),
        },
        &bearer,
    ) {
        Ok(request) => request,
        Err((status, body)) => return (status, body).into_response(),
    };
    match state.space_attention_client.clone().get_space_attention(request).await {
        Ok(response) => Json(json!({
            "results": response.into_inner().results.iter().map(result_json).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(status) => status_response(&status),
    }
}
