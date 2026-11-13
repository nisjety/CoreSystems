//! Organisation flag values for the signed-in viewer (W5-079-c, F-t3-079).
//!
//! `GET /api/v1/org-flags` answers the active organisation's values for the presentation flags
//! org-core lists in `OrgOverridableFlags`, as `{data:{flags:{name:value}, revision}}`. The read goes
//! to org-core's internal route with the gateway's own service credential, never the viewer's, and
//! one organisation's answer is reused for 30 seconds through the gateway cache, admitted through
//! `CacheAdmission` like every content cache (a ZDR organisation is read on every request). Every
//! answer is `no-store`.
//!
//! A read that fails answers `{data:{flags:{}, stale:true}}`, so each presentation flag falls back
//! to its default, which the SPA reads as off. org-core's own 404, from its `ORG_FLAGS_ENABLED`
//! switch being off, passes through, and the SPA reads it as no organisation layer. That switch
//! lives upstream, so the gateway reads no flag of its own.
//!
//! The values are a presentation ceiling only. The SPA ANDs them with its build switch, and no
//! authorization decision reads them.

use std::time::Duration;

use axum::{
    extract::{Extension, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Map, Value};

use crate::{
    cache_admission::{valid_org_scope, CacheAdmission},
    config::AppState,
    middleware::{require_session, AuthenticatedUser},
    upstream::{authorized_org_id, proxy_json},
};

/// The cache namespace of one organisation's answer, and how long that answer is reused.
const CACHE_NAMESPACE: &str = "org-flags";
const CACHE_TTL_SECS: u64 = 30;

/// Bound on the org-core read. A presentation flag never holds a page for longer than this.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// The longest flag name or value this route passes on.
const MAX_TOKEN_LENGTH: usize = 64;

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/org-flags", get(get_org_flags))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

async fn get_org_flags(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    // The organisation comes from the live membership the session resolved, never from the request.
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        // No organisation: nothing is set for the viewer, so every flag keeps its default.
        return answer(
            StatusCode::OK,
            json!({ "data": { "flags": {}, "revision": 0 } }),
        );
    }
    let Some(scope) = valid_org_scope(&org_id) else {
        return stale_answer();
    };
    // Content admission decides whether this organisation's answer may be read from or written to
    // the cache at all (a ZDR organisation, an unknown posture or a switched-off cache may not).
    // A refused admission reads org-core on every request; it never changes the answer.
    let admission = CacheAdmission::resolve(&state, &user, &headers, &Value::Null, false).await;
    admission.record_bypass(CACHE_NAMESPACE);
    // The key carries the organisation id in clear and hashes the rest, so one organisation's
    // answer is never served for another.
    let key = admission.key(CACHE_NAMESPACE, &["flags"]);
    if let (Some(key), true) = (key.as_deref(), admission.may_read) {
        if let Some(data) = state
            .cache
            .lookup_within_scoped(key, scope, CACHE_TTL_SECS)
            .await
        {
            return answer(StatusCode::OK, json!({ "data": data }));
        }
    }

    let url = format!("{}/internal/orgs/{scope}/flags", state.org_core_url);
    let read = tokio::time::timeout(
        READ_TIMEOUT,
        proxy_json(&state, Method::GET, &url, None, None, None, None),
    )
    .await;
    let Ok((status, Json(body))) = read else {
        return stale_answer();
    };
    if status == StatusCode::NOT_FOUND {
        // org-core's switch is off, or the organisation is unknown: there is no layer to read.
        return answer(status, body);
    }
    match org_flags_from_core(&body) {
        Some(data) if status.is_success() => {
            if let (Some(key), true) = (key.as_deref(), admission.may_write) {
                state.cache.store_scoped(key, scope, &data).await;
            }
            answer(StatusCode::OK, json!({ "data": data }))
        }
        _ => stale_answer(),
    }
}

/// An answer the browser must not keep.
fn answer(status: StatusCode, body: Value) -> Response {
    (status, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

/// The answer when the organisation's values could not be read: every presentation flag falls back
/// to its default, which is off.
fn stale_answer() -> Response {
    answer(
        StatusCode::OK,
        json!({ "data": { "flags": {}, "stale": true } }),
    )
}

/// org-core's `{data:{flags:[...]}}` reshaped to `{flags:{name:value}, revision}`. An entry that is
/// not a flag name and value is left out, so that flag keeps its default. The revision is the sum of
/// the stored per-flag revisions, which moves on every write.
fn org_flags_from_core(body: &Value) -> Option<Value> {
    let entries = body.get("data")?.get("flags")?.as_array()?;
    let mut flags = Map::new();
    let mut revision: u64 = 0;
    for entry in entries {
        let name = entry.get("name").and_then(Value::as_str);
        let value = entry.get("value").and_then(Value::as_str);
        let (Some(name), Some(value)) = (name, value) else {
            continue;
        };
        if !is_token(name) || !is_token(value) {
            continue;
        }
        flags.insert(name.to_owned(), Value::String(value.to_owned()));
        revision =
            revision.saturating_add(entry.get("revision").and_then(Value::as_u64).unwrap_or(0));
    }
    Some(json!({ "flags": flags, "revision": revision }))
}

fn is_token(raw: &str) -> bool {
    !raw.is_empty()
        && raw.len() <= MAX_TOKEN_LENGTH
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}
