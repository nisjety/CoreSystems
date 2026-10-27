//! Room artifacts aggregate: the BFF half of WS07-S9.
//!
//! `GET /api/v1/spaces/{space_ref}/artifacts?limit=50&cursor=...` lists the artifacts the room's
//! agent turns produced, one entry per `(thread, artifact)` at its latest version, newest first.
//! Session Core owns the query and the audience ceiling (`SpaceRoomReadService.ListSpaceArtifacts`,
//! reached through model-gateway `GET /v1/spaces/:space_id/artifacts`); this module decides
//! nothing and stores nothing. It validates the shape, checks the Space lifecycle, asks Control
//! for the signed shared-read decision under the caller's own delegation, forwards with the
//! caller's own bearers, and reduces the answer to a fixed field set.
//!
//! The response carries summaries only, never artifact content (the viewer reads the thread
//! through the existing transcript route):
//! `200 {data:{artifacts:[{thread_id, message_id, artifact_id, kind, title, version,
//! version_count, size_chars, agent_name?, created_at}], next_cursor, shared_read:{available},
//! retention:{zdr}}}`. `retention.zdr` is `true` or `false` from the org posture and `null` when
//! the org-core lookup fails (an outage is not a posture); a ZDR turn produces no artifacts, so
//! the list for a ZDR org is empty and the SPA says why. `shared_read.available` is always `true`
//! on a 200: a Control decline is a 403, never a silent narrowing to the caller's own turns.
//!
//! Order (each step is a refusal before the next one runs): flag, org, Space reference, query
//! shape, Space lifecycle, Control read decision, bearers, model-gateway, response validation.
//!
//! Errors are `{error:{code,message,details}}`: 400 `invalid_space` / `invalid_limit` /
//! `invalid_cursor`, 403 `active_membership_required` / `space_read_not_authorized`, 404
//! `space_not_found` / `space_room_artifacts_disabled`, 502 `invalid_model_gateway_response`
//! (also a foreign-space item), 503 `space_lifecycle_unavailable` / `delegated_auth_unavailable`
//! / `space_artifacts_unavailable`. The read decision token is carried in the upstream query
//! string only, never logged here and never put in any response body.

use axum::{
    extract::{Extension, Path, Query, State},
    http::{header::CACHE_CONTROL, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{denial, shared_thread_read_decision_for_typed, space_lifecycle_by_ref};
use crate::{
    config::AppState,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
    zdr::{self, OrgZdrPosture},
};

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 200;
const MAX_SPACE_REF_LEN: usize = 200;
const MAX_CURSOR_LEN: usize = 512;

/// The summary fields that cross to the browser; anything else model-gateway adds is dropped.
const STRING_FIELDS: &[&str] = &["thread_id", "message_id", "artifact_id", "kind", "title", "created_at"];
const NUMBER_FIELDS: &[&str] = &["version", "version_count", "size_chars"];

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/spaces/{space_ref}/artifacts", get(list_room_artifacts))
}

#[derive(Debug, Default, Deserialize)]
struct ArtifactsParams {
    limit: Option<String>,
    cursor: Option<String>,
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (status, Json(error(code, message))).into_response()
}

fn invalid(code: &'static str, message: &'static str, field: &'static str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(code, message, json!({ "field": field }))),
    )
        .into_response()
}

fn bad_upstream() -> Response {
    refuse(
        StatusCode::BAD_GATEWAY,
        "invalid_model_gateway_response",
        "Model Gateway returned an invalid artifact listing.",
    )
}

/// One validated summary, reduced to the allowed fields. `None` is a malformed item.
fn summary(item: &Value, space_ref: &str) -> Option<Value> {
    let object = item.as_object()?;
    // Session Core filters by Space; an item that names another Space is a contract break, not
    // something to narrow away.
    if let Some(item_space) = object.get("space_id").and_then(Value::as_str) {
        if !item_space.is_empty() && item_space != space_ref {
            return None;
        }
    }
    let mut out = Map::new();
    for key in STRING_FIELDS {
        let value = object.get(*key).and_then(Value::as_str)?;
        out.insert((*key).to_owned(), json!(value));
    }
    for key in NUMBER_FIELDS {
        let value = object.get(*key).and_then(Value::as_u64)?;
        out.insert((*key).to_owned(), json!(value));
    }
    if let Some(name) = object
        .get("agent_name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
    {
        out.insert("agent_name".to_owned(), json!(name));
    }
    Some(Value::Object(out))
}

async fn list_room_artifacts(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
    Query(params): Query<ArtifactsParams>,
) -> Response {
    if !state.space_features.room_artifacts {
        return refuse(
            StatusCode::NOT_FOUND,
            "space_room_artifacts_disabled",
            "Room artifacts are not enabled.",
        );
    }
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to read Space artifacts.",
        );
    }
    let space_ref = space_ref.trim();
    if space_ref.is_empty() || space_ref.len() > MAX_SPACE_REF_LEN {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A Space reference is required.",
        );
    }
    let limit = match params
        .limit
        .as_deref()
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
    {
        None => DEFAULT_LIMIT,
        Some(raw) => match raw.parse::<u32>() {
            Ok(limit) if (1..=MAX_LIMIT).contains(&limit) => limit,
            _ => {
                return invalid(
                    "invalid_limit",
                    "limit must be between 1 and 200.",
                    "limit",
                )
            }
        },
    };
    let cursor = params
        .cursor
        .as_deref()
        .map(str::trim)
        .filter(|cursor| !cursor.is_empty());
    if let Some(cursor) = cursor {
        if cursor.len() > MAX_CURSOR_LEN || cursor.chars().any(char::is_control) {
            return invalid("invalid_cursor", "The cursor is not valid.", "cursor");
        }
    }

    match space_lifecycle_by_ref(&state, &user, &org_id, space_ref).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return refuse(
                StatusCode::NOT_FOUND,
                "space_not_found",
                "Space is not available.",
            )
        }
        Err(()) => {
            return refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            )
        }
    }

    let (read, read_denial) =
        match shared_thread_read_decision_for_typed(&state, &user, &org_id, space_ref, "").await {
            Ok(decision) => decision.split(),
            Err(response) => return response.into_response(),
        };
    let Some((decision_ref, token)) = read else {
        let message = "Reading this Space's artifacts is not authorized.";
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

    let session_token =
        match crate::domains::chat::shared::required_session_token(&state, &user, &headers).await {
            Ok(token) => token,
            Err(reason) => {
                return crate::domains::chat::shared::delegated_auth_unavailable(reason)
                    .into_response()
            }
        };
    let mut url = format!(
        "{}/v1/spaces/{}/artifacts?limit={limit}&space_read_decision_ref={}&space_read_decision_token={}",
        state.model_gateway_url,
        urlencoding::encode(space_ref),
        urlencoding::encode(&decision_ref),
        urlencoding::encode(&token),
    );
    if let Some(cursor) = cursor {
        url.push_str("&cursor=");
        url.push_str(&urlencoding::encode(cursor));
    }
    let model_token = crate::domains::chat::shared::model_token(&state, &user, &headers).await;
    let (status, Json(payload)) = crate::domains::chat::shared::proxy_model_json_with_session(
        &state,
        Method::GET,
        &url,
        None,
        model_token.as_deref(),
        Some(&session_token),
        &user,
    )
    .await;
    if !status.is_success() {
        // model-gateway's typed envelope (401, 403 `space_read_not_authorized`, 400, 503
        // `space_artifacts_unavailable`) as is; it never echoes the decision.
        return (status, Json(payload)).into_response();
    }
    let Some(items) = payload.get("artifacts").and_then(Value::as_array) else {
        return bad_upstream();
    };
    if items.len() > MAX_LIMIT as usize {
        return bad_upstream();
    }
    let mut artifacts: Vec<Value> = Vec::with_capacity(items.len());
    for item in items {
        match summary(item, space_ref) {
            Some(entry) => artifacts.push(entry),
            None => return bad_upstream(),
        }
    }
    let next_cursor = match payload.get("next_cursor") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(cursor)) if cursor.is_empty() => Value::Null,
        Some(Value::String(cursor)) if cursor.len() <= MAX_CURSOR_LEN => json!(cursor),
        Some(_) => return bad_upstream(),
    };
    // Org posture for the empty-state copy only. A failed lookup is `null`, not "not ZDR".
    let zdr_posture = match zdr::org_zdr_lookup(&state, &user).await {
        OrgZdrPosture::Enabled => json!(true),
        OrgZdrPosture::Disabled => json!(false),
        OrgZdrPosture::Unknown => Value::Null,
    };
    let mut response = (
        StatusCode::OK,
        Json(json!({
            "data": {
                "artifacts": artifacts,
                "next_cursor": next_cursor,
                "shared_read": { "available": true },
                "retention": { "zdr": zdr_posture },
            }
        })),
    )
        .into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
