//! WS04-S3: people directory search for the room member picker.
//!
//! `GET /api/v1/directory/people?q=&limit=&cursor=&exclude_member_of=` proxies Control's
//! `GET /api/v1/internal/directory/people` under the caller's signed delegation. The organization
//! and the caller come from the session, never from the query string; Control runs the single
//! SQL join of `users` and `user_org_memberships` and decides what the caller may see. This module
//! holds no directory state. It carries ids, names and emails only: no message content, so there
//! is nothing for Zero Data Retention to refuse and no model is called.
//!
//! Gate order: flag (404 `feature_disabled`), live organization membership, per-user limiter
//! (60 per minute, 429 `rate_limited` with `Retry-After`), query shape (400 `invalid_request`),
//! then Control. Control's typed 4xx passes through unchanged; a Control outage is a typed 503
//! `directory_unavailable`, never an empty list.

use std::sync::OnceLock;

use axum::{
    extract::{Extension, Query, State},
    http::{header::RETRY_AFTER, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    rate_limit::RateLimiter,
    upstream::proxy_json,
};

/// Directory reads per user per minute (plan: 60/min).
const DIRECTORY_READS_PER_MINUTE: f64 = 60.0;
const MAX_QUERY_CHARS: usize = 100;
const MAX_LIMIT: u32 = 50;
const MAX_CURSOR_LEN: usize = 512;
const MAX_SPACE_REF_LEN: usize = 200;

/// Process-wide limiter, built on first use from the gateway's Dragonfly connection (in-process
/// fallback when the cache is off). It is not an `AppState` field so that adding it does not
/// touch every test state literal.
static DIRECTORY_LIMITER: OnceLock<RateLimiter> = OnceLock::new();

#[derive(Debug, Default, Deserialize)]
pub(super) struct PeopleParams {
    q: Option<String>,
    limit: Option<String>,
    cursor: Option<String>,
    exclude_member_of: Option<String>,
}

fn invalid(field: &'static str, reason: &'static str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            "invalid_request",
            "The directory query is invalid.",
            json!({ "field": field, "reason": reason }),
        )),
    )
        .into_response()
}

fn unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "directory_unavailable",
            "The directory is unavailable.",
        )),
    )
        .into_response()
}

/// Query string for Control, from the validated parameters only (unknown parameters are dropped,
/// so a client cannot smuggle `org_id` or a caller id through).
fn control_query(params: &PeopleParams) -> Result<String, Response> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(q) = params.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        if q.chars().count() > MAX_QUERY_CHARS {
            return Err(invalid("q", "too_long"));
        }
        parts.push(format!("q={}", urlencoding::encode(q)));
    }
    if let Some(raw) = params
        .limit
        .as_deref()
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
    {
        match raw.parse::<u32>() {
            Ok(limit) if (1..=MAX_LIMIT).contains(&limit) => parts.push(format!("limit={limit}")),
            _ => return Err(invalid("limit", "out_of_range")),
        }
    }
    if let Some(cursor) = params
        .cursor
        .as_deref()
        .map(str::trim)
        .filter(|cursor| !cursor.is_empty())
    {
        if cursor.len() > MAX_CURSOR_LEN {
            return Err(invalid("cursor", "too_long"));
        }
        parts.push(format!("cursor={}", urlencoding::encode(cursor)));
    }
    if let Some(space_ref) = params
        .exclude_member_of
        .as_deref()
        .map(str::trim)
        .filter(|space_ref| !space_ref.is_empty())
    {
        if space_ref.len() > MAX_SPACE_REF_LEN {
            return Err(invalid("exclude_member_of", "too_long"));
        }
        parts.push(format!("exclude_member_of={}", urlencoding::encode(space_ref)));
    }
    Ok(parts.join("&"))
}

/// The four fields the picker needs, nothing else Control might add later.
fn person(value: &Value) -> Option<Value> {
    let subject_id = value.get("subject_id").and_then(Value::as_str)?;
    Some(json!({
        "subject_id": subject_id,
        "display_name": value.get("display_name").and_then(Value::as_str).unwrap_or_default(),
        "email": value.get("email").and_then(Value::as_str).unwrap_or_default(),
        "is_self": value.get("is_self").and_then(Value::as_bool).unwrap_or(false),
    }))
}

/// `GET /api/v1/directory/people`.
pub(super) async fn people_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(params): Query<PeopleParams>,
) -> Response {
    if !state.space_features.people_search_api {
        return (
            StatusCode::NOT_FOUND,
            Json(error(
                "feature_disabled",
                "People search is not enabled.",
            )),
        )
            .into_response();
    }
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership is required.",
            )),
        )
            .into_response();
    }
    let limiter = DIRECTORY_LIMITER.get_or_init(|| {
        RateLimiter::from_cache_with_rpm(&state.cache, DIRECTORY_READS_PER_MINUTE)
    });
    if let Err(retry_after) = limiter
        .try_acquire(&format!("space-directory:{}:{}", org_id.trim(), user.user_id))
        .await
    {
        let mut response = (
            StatusCode::TOO_MANY_REQUESTS,
            Json(error(
                "rate_limited",
                "Too many directory searches. Retry shortly.",
            )),
        )
            .into_response();
        if let Ok(value) = HeaderValue::try_from(retry_after.max(1).to_string()) {
            response.headers_mut().insert(RETRY_AFTER, value);
        }
        return response;
    }
    let query = match control_query(&params) {
        Ok(query) => query,
        Err(response) => return response,
    };
    let mut url = format!("{}/api/v1/internal/directory/people", state.user_core_url);
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query);
    }
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let (status, Json(control)) = proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;
    if status.is_server_error() {
        return unavailable();
    }
    if !status.is_success() {
        // Control's typed 4xx (invalid_request, member_not_in_organization, delegation) as is.
        return (status, Json(control)).into_response();
    }
    let data = unwrap_data(&control);
    let people: Vec<Value> = data
        .get("people")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(person).collect())
        .unwrap_or_default();
    let next_cursor = data
        .get("next_cursor")
        .and_then(Value::as_str)
        .filter(|cursor| !cursor.is_empty())
        .map_or(Value::Null, |cursor| json!(cursor));
    let mut response = (
        StatusCode::OK,
        Json(json!({ "data": { "people": people, "next_cursor": next_cursor } })),
    )
        .into_response();
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}
