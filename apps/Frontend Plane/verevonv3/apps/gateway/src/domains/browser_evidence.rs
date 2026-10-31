//! Browser evidence proxies (WS9.C, D9 S3a): the receipts, egress-receipts and
//! procedure reads behind the BrowserChrome evidence panels, and the allowlist
//! that the devtools read passes through before Quarry sees it.
//!
//! Every route here is owner-gated. The verified session is checked against
//! Quarry's own `browser-session` projection (the gate the devtools read already
//! uses), and Quarry calls carry the verified caller's token. No handler reads a
//! caller-supplied org header. No raw Quarry receipt or procedure JSON reaches the
//! browser: receipts are projected to a fixed field set, and a procedure step's
//! typed `text` is replaced by its length.

use std::collections::BTreeMap;

use axum::{
    body::Bytes,
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    envelope::{error, ok, unwrap_data},
    middleware::{require_session, AuthenticatedUser},
    rate_limit::{with_rate_class, RateClass},
};

use super::browser::{
    cookie_header, forward_quarry_failure, is_valid_path_segment,
    owner_browser_session_projection, quarry_call, quarry_token,
};

/// Receipts the gateway returns at most. Quarry's receipt stream is unpaginated.
const RECEIPTS_MAX_LIMIT: usize = 200;
const EGRESS_DEFAULT_LIMIT: u64 = 100;
const EGRESS_MAX_LIMIT: u64 = 512;
const DEVTOOLS_LIMIT_MAX: u64 = 512;
const DEVTOOLS_FILTER_MAX_CHARS: usize = 200;
const DEVTOOLS_LEVELS_MAX: usize = 5;
const DEVTOOLS_LEVELS: [&str; 7] = ["debug", "info", "log", "warn", "warning", "error", "verbose"];
const DEVTOOLS_CATEGORIES: [&str; 3] = ["console", "network", "lifecycle"];
const PROCEDURE_ID_MAX_CHARS: usize = 64;

pub(crate) fn router(state: AppState) -> Router<AppState> {
    let routes = Router::new()
        .route(
            "/api/v1/browser/sessions/{session_id}/receipts",
            get(get_session_receipts),
        )
        .route(
            "/api/v1/browser/sessions/{session_id}/egress-receipts",
            get(get_session_egress_receipts),
        )
        .route(
            "/api/v1/browser/sessions/{session_id}/procedure",
            post(compile_session_procedure),
        );
    with_rate_class(routes, &state.rate_limiter, RateClass::Interactive)
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

// ---------------------------------------------------------------------------
// Error responses
// ---------------------------------------------------------------------------

fn bad_request(code: &'static str, message: &'static str) -> Response {
    (StatusCode::BAD_REQUEST, Json(error(code, message))).into_response()
}

fn invalid_session() -> Response {
    bad_request(
        "invalid_browser_session",
        "The requested browser session id is invalid.",
    )
}

/// The receipts of a zero-data-retention session are not kept, so the typed
/// answer is returned instead of a 404 that would look like a missing session.
fn zdr_receipts_unavailable() -> Response {
    (
        StatusCode::CONFLICT,
        Json(error(
            "browser_receipts_unavailable_zdr",
            "Receipts are not kept for a zero-data-retention browser session.",
        )),
    )
        .into_response()
}

fn procedure_not_compilable() -> Response {
    (
        StatusCode::CONFLICT,
        Json(error(
            "browser_procedure_not_compilable",
            "This session's receipts cannot be compiled into a replay procedure.",
        )),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Query allowlists
// ---------------------------------------------------------------------------

/// Decoded query pairs. Repeated keys keep their last value.
fn query_map(uri: &Uri) -> BTreeMap<String, String> {
    uri.query()
        .map(|query| {
            url::form_urlencoded::parse(query.as_bytes())
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect()
        })
        .unwrap_or_default()
}

fn valid_tab_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn validate_filter(value: &str) -> Result<(), &'static str> {
    if value.chars().any(char::is_control) {
        return Err("filter contains a control character");
    }
    if value.chars().count() > DEVTOOLS_FILTER_MAX_CHARS {
        return Err("filter is longer than 200 characters");
    }
    Ok(())
}

/// Level names, lower-cased and comma-joined, or None when none were given.
fn devtools_levels(raw: &str) -> Result<Option<String>, &'static str> {
    let names: Vec<String> = raw
        .split(',')
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    if names.is_empty() {
        return Ok(None);
    }
    if names.len() > DEVTOOLS_LEVELS_MAX {
        return Err("at most five levels may be named");
    }
    if names
        .iter()
        .any(|name| !DEVTOOLS_LEVELS.contains(&name.as_str()))
    {
        return Err("a level is not supported");
    }
    Ok(Some(names.join(",")))
}

/// The devtools query Quarry may receive (WS9.C): the cursor and the four
/// filters, under camelCase keys and each validated. Any other key is dropped,
/// so nothing the browser sends reaches Quarry untyped. Returns `?query` or an
/// empty string.
pub(super) fn safe_devtools_query(uri: &Uri) -> Result<String, &'static str> {
    let mut kept: BTreeMap<&str, String> = BTreeMap::new();
    for (key, value) in query_map(uri) {
        match key.as_str() {
            "afterSequence" => {
                let sequence: u64 = value
                    .parse()
                    .map_err(|_| "afterSequence must be a non-negative integer")?;
                kept.insert("afterSequence", sequence.to_string());
            }
            "limit" => {
                let limit: u64 = value
                    .parse()
                    .map_err(|_| "limit must be a whole number")?;
                if !(1..=DEVTOOLS_LIMIT_MAX).contains(&limit) {
                    return Err("limit must be between 1 and 512");
                }
                kept.insert("limit", limit.to_string());
            }
            "levels" => {
                if let Some(levels) = devtools_levels(&value)? {
                    kept.insert("levels", levels);
                }
            }
            "filter" => {
                if !value.trim().is_empty() {
                    validate_filter(&value)?;
                    kept.insert("filter", value);
                }
            }
            "category" => {
                let category = value.trim().to_ascii_lowercase();
                if category.is_empty() {
                    continue;
                }
                if !DEVTOOLS_CATEGORIES.contains(&category.as_str()) {
                    return Err("category is not supported");
                }
                kept.insert("category", category);
            }
            "tabId" => {
                if value.is_empty() {
                    continue;
                }
                if !valid_tab_id(&value) {
                    return Err("tabId is not valid");
                }
                kept.insert("tabId", value);
            }
            _ => {}
        }
    }
    if kept.is_empty() {
        return Ok(String::new());
    }
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in &kept {
        serializer.append_pair(key, value);
    }
    Ok(format!("?{}", serializer.finish()))
}

/// `?limit=` for the receipts read: 1 to 200, default 200.
fn receipts_limit(uri: &Uri) -> Result<usize, &'static str> {
    match query_map(uri).get("limit") {
        None => Ok(RECEIPTS_MAX_LIMIT),
        Some(raw) => {
            let limit: usize = raw.parse().map_err(|_| "limit must be a whole number")?;
            if !(1..=RECEIPTS_MAX_LIMIT).contains(&limit) {
                return Err("limit must be between 1 and 200");
            }
            Ok(limit)
        }
    }
}

/// `?afterSequence=&limit=` for the egress read: a cursor and 1 to 512 rows.
fn egress_query(uri: &Uri) -> Result<(u64, u64), &'static str> {
    let query = query_map(uri);
    let after: u64 = match query.get("afterSequence") {
        None => 0,
        Some(raw) => raw
            .parse()
            .map_err(|_| "afterSequence must be a non-negative integer")?,
    };
    let limit = match query.get("limit") {
        None => EGRESS_DEFAULT_LIMIT,
        Some(raw) => {
            let limit: u64 = raw.parse().map_err(|_| "limit must be a whole number")?;
            if !(1..=EGRESS_MAX_LIMIT).contains(&limit) {
                return Err("limit must be between 1 and 512");
            }
            limit
        }
    };
    Ok((after, limit))
}

/// `procedureId` from a compile request: absent or blank means "let Quarry name it".
fn procedure_id_of(input: &Value) -> Result<Option<String>, &'static str> {
    match input.get("procedureId") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(raw)) => {
            let id = raw.trim();
            if id.is_empty() {
                Ok(None)
            } else if id.chars().count() > PROCEDURE_ID_MAX_CHARS
                || !id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                Err("procedureId must be 1 to 64 letters, digits, hyphens or underscores")
            } else {
                Ok(Some(id.to_owned()))
            }
        }
        Some(_) => Err("procedureId must be a string"),
    }
}

// ---------------------------------------------------------------------------
// Projections
// ---------------------------------------------------------------------------

/// Drops top-level keys whose value is null, so absent fields stay absent.
fn without_nulls(mut value: Value) -> Value {
    if let Some(map) = value.as_object_mut() {
        map.retain(|_, field| !field.is_null());
    }
    value
}

/// Display label for an action type, such as `click_ref` to `Click ref`.
fn action_label(action_type: &str) -> String {
    let spaced = action_type.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// One receipt as the browser may see it (WS9.C). The observation, every typed
/// `text`, and Quarry's failure message and skip reason are never copied.
fn receipt_view(receipt: &Value) -> Value {
    let action_type = receipt
        .pointer("/action/type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let url = receipt
        .pointer("/observation/url")
        .and_then(Value::as_str)
        .or_else(|| receipt.pointer("/action/url").and_then(Value::as_str))
        .map(str::to_owned);
    let outcome = without_nulls(json!({
        "status": receipt.pointer("/outcome/status").cloned(),
        "latencyMs": receipt.pointer("/outcome/latency_ms").cloned(),
        "errorCode": receipt.pointer("/outcome/error_code").cloned(),
    }));
    without_nulls(json!({
        "receiptId": receipt.get("receipt_id").cloned(),
        "step": receipt.get("step").cloned(),
        "startedAt": receipt.get("started_at").cloned(),
        "finishedAt": receipt.get("finished_at").cloned(),
        "actionType": action_type,
        "actionLabel": action_label(&action_type),
        "url": url,
        "outcome": outcome,
        "costMicroUsd": receipt.get("cost_micro_usd").cloned(),
        "actionOutcome": receipt.pointer("/observation/action_outcome/status").cloned(),
        "correctionOf": receipt.get("correction_of").cloned(),
        "initiatedBy": receipt.get("initiated_by").cloned(),
        "controlEpoch": receipt.get("control_epoch").cloned(),
    }))
}

/// One redacted egress decision (WS9.C). Quarry's snake_case fields become the
/// camelCase the browser reads; Quarry already keeps the URL to origin and path.
fn egress_view(receipt: &Value) -> Value {
    without_nulls(json!({
        "sequence": receipt.get("sequence").cloned(),
        "tabId": receipt.get("tab_id").cloned(),
        "method": receipt.get("method").cloned(),
        "url": receipt.get("url").cloned(),
        "decision": receipt.get("decision").cloned(),
        "policy": receipt.get("policy").cloned(),
        "timestampMs": receipt.get("timestamp_ms").cloned(),
    }))
}

/// The step's action with its typed `text` replaced by `{redacted, length}`
/// (WS9.C). Selectors, refs, URLs and keys are kept: they are the replay evidence.
fn redact_action(mut action: Value) -> Value {
    let text_length = action
        .get("text")
        .and_then(Value::as_str)
        .map(|text| text.chars().count());
    if let (Some(length), Some(map)) = (text_length, action.as_object_mut()) {
        map.insert(
            "text".to_owned(),
            json!({ "redacted": true, "length": length }),
        );
    }
    action
}

fn procedure_view(procedure: &Value) -> Value {
    let steps: Vec<Value> = procedure
        .get("steps")
        .and_then(Value::as_array)
        .map(|steps| {
            steps
                .iter()
                .map(|step| {
                    without_nulls(json!({
                        "step": step.get("step").cloned(),
                        "action": redact_action(
                            step.get("action").cloned().unwrap_or(Value::Null),
                        ),
                        "expectedOutcome": step.get("expected_outcome").cloned(),
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    without_nulls(json!({
        "procedureId": procedure.get("procedure_id").cloned(),
        "version": procedure.get("version").cloned(),
        "sourceReceiptIds": procedure.get("source_receipt_ids").cloned(),
        "steps": steps,
    }))
}

fn quality_view(report: &Value) -> Value {
    without_nulls(json!({
        "stepCount": report.get("step_count").cloned(),
        "verifiedStepCount": report.get("verified_step_count").cloned(),
        "promotionAllowed": report.get("promotion_allowed").cloned(),
        "reasons": report.get("reasons").cloned(),
    }))
}

// ---------------------------------------------------------------------------
// Owner gate and Quarry calls
// ---------------------------------------------------------------------------

/// The owner gate every evidence route runs first. Returns the Quarry token for
/// the caller and Quarry's projection of the session the caller owns.
async fn owner_gate(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    session_id: &str,
) -> Result<(Option<String>, Value), Response> {
    let cookie = cookie_header(headers);
    let token = quarry_token(state, user, &cookie).await;
    let projection =
        owner_browser_session_projection(state, user, token.as_deref(), session_id).await?;
    Ok((token, projection))
}

/// Receipts are refused for a zero-data-retention session. A projection that
/// does not state the flag is treated the same way.
fn receipts_refused(projection: &Value) -> bool {
    projection.get("zdr").and_then(Value::as_bool) != Some(false)
}

fn projection_zdr(projection: &Value) -> bool {
    projection
        .get("zdr")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

async fn receipts_view(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    session_id: &str,
    limit: usize,
) -> Result<Value, Response> {
    let (token, projection) = owner_gate(state, user, headers, session_id).await?;
    if receipts_refused(&projection) {
        return Err(zdr_receipts_unavailable());
    }
    let (status, body) = quarry_call(
        state,
        Method::GET,
        &format!("/v1/agent/runs/{}/receipts", urlencoding::encode(session_id)),
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await;
    // Quarry answers 404 for a run whose receipt stream is empty. The owner
    // gate has already passed, so that is an empty list, not a missing session.
    if status == StatusCode::NOT_FOUND {
        return Ok(json!({ "receipts": [], "truncated": false, "zdr": false }));
    }
    if !status.is_success() {
        return Err(forward_quarry_failure(status, body));
    }
    let all: Vec<Value> = unwrap_data(&body)
        .as_array()
        .cloned()
        .unwrap_or_default();
    let total = all.len();
    let receipts: Vec<Value> = all
        .iter()
        .skip(total.saturating_sub(limit))
        .map(receipt_view)
        .collect();
    Ok(json!({
        "receipts": receipts,
        "truncated": total > limit,
        "zdr": false,
    }))
}

async fn egress_view_for_session(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    session_id: &str,
    after: u64,
    limit: u64,
) -> Result<Value, Response> {
    let (token, projection) = owner_gate(state, user, headers, session_id).await?;
    // Live egress decisions are the one read a zero-data-retention session may make.
    let (status, body) = quarry_call(
        state,
        Method::GET,
        &format!(
            "/v1/agent/runs/{}/egress-receipts?afterSequence={after}&limit={limit}",
            urlencoding::encode(session_id)
        ),
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return Err(forward_quarry_failure(status, body));
    }
    let data = unwrap_data(&body);
    let rows: Vec<Value> = data
        .get("receipts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut allow: u64 = 0;
    let mut block: u64 = 0;
    let mut views = Vec::with_capacity(rows.len());
    for row in &rows {
        match row.get("decision").and_then(Value::as_str) {
            Some("allow") => allow += 1,
            Some("block") => block += 1,
            _ => {}
        }
        views.push(egress_view(row));
    }
    let next_sequence = views
        .last()
        .and_then(|view| view.get("sequence"))
        .and_then(Value::as_u64)
        .unwrap_or(after);
    let zdr = data
        .get("zdr")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| projection_zdr(&projection));
    Ok(json!({
        "receipts": views,
        "counts": { "allow": allow, "block": block },
        "nextSequence": next_sequence,
        "zdr": zdr,
    }))
}

async fn compile_procedure_view(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    session_id: &str,
    procedure_id: Option<String>,
) -> Result<Value, Response> {
    let (token, projection) = owner_gate(state, user, headers, session_id).await?;
    if receipts_refused(&projection) {
        return Err(zdr_receipts_unavailable());
    }
    let compile_body = match procedure_id {
        Some(id) => json!({ "procedure_id": id }),
        None => json!({}),
    };
    let (status, body) = quarry_call(
        state,
        Method::POST,
        &format!(
            "/v1/agent/runs/{}/procedure",
            urlencoding::encode(session_id)
        ),
        Some(compile_body),
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if status == StatusCode::CONFLICT {
        return Err(procedure_not_compilable());
    }
    if !status.is_success() {
        return Err(forward_quarry_failure(status, body));
    }
    let procedure = unwrap_data(&body);
    // The quality check gets the procedure as Quarry compiled it: it is an
    // internal call, and the typed text is redacted only in the browser's view.
    let (quality_status, quality_body) = quarry_call(
        state,
        Method::POST,
        "/v1/agent/procedures/quality-check",
        Some(procedure.clone()),
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !quality_status.is_success() {
        return Err(forward_quarry_failure(quality_status, quality_body));
    }
    let quality = unwrap_data(&quality_body);
    Ok(json!({
        "procedure": procedure_view(&procedure),
        "quality": quality_view(&quality),
    }))
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// `GET /api/v1/browser/sessions/{session_id}/receipts?limit=`
async fn get_session_receipts(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    uri: Uri,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return invalid_session();
    }
    let limit = match receipts_limit(&uri) {
        Ok(limit) => limit,
        Err(message) => return bad_request("invalid_browser_evidence_query", message),
    };
    match receipts_view(&state, &user, &headers, &session_id, limit).await {
        Ok(data) => (StatusCode::OK, Json(ok(data))).into_response(),
        Err(response) => response,
    }
}

/// `GET /api/v1/browser/sessions/{session_id}/egress-receipts?afterSequence=&limit=`
async fn get_session_egress_receipts(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    uri: Uri,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return invalid_session();
    }
    let (after, limit) = match egress_query(&uri) {
        Ok(query) => query,
        Err(message) => return bad_request("invalid_browser_evidence_query", message),
    };
    match egress_view_for_session(&state, &user, &headers, &session_id, after, limit).await {
        Ok(data) => (StatusCode::OK, Json(ok(data))).into_response(),
        Err(response) => response,
    }
}

/// `POST /api/v1/browser/sessions/{session_id}/procedure` with `{ procedureId? }`
async fn compile_session_procedure(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    body: Bytes,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return invalid_session();
    }
    let input: Value = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(&body) {
            Ok(input) => input,
            Err(_) => {
                return bad_request(
                    "invalid_input",
                    "The procedure request body must be JSON.",
                )
            }
        }
    };
    let procedure_id = match procedure_id_of(&input) {
        Ok(id) => id,
        Err(message) => return bad_request("invalid_input", message),
    };
    match compile_procedure_view(&state, &user, &headers, &session_id, procedure_id).await {
        Ok(data) => (StatusCode::OK, Json(ok(data))).into_response(),
        Err(response) => response,
    }
}

/// `browser_evidence.compile_procedure` with `{ sessionId, procedureId? }` on the
/// generic action surface (WS9.C). The envelope matches the other human actions.
pub(crate) async fn dispatch_compile_procedure(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let action_id = "browser_evidence.compile_procedure";
    let Some(session_id) = input
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return bad_request(
            "invalid_input",
            "browser_evidence.compile_procedure requires a 'sessionId'",
        );
    };
    if !is_valid_path_segment(session_id) {
        return invalid_session();
    }
    let procedure_id = match procedure_id_of(input) {
        Ok(id) => id,
        Err(message) => return bad_request("invalid_input", message),
    };
    match compile_procedure_view(state, user, headers, session_id, procedure_id).await {
        Ok(result) => (
            StatusCode::OK,
            Json(ok(json!({
                "actionId": action_id,
                "runId": format!("{}_{}", action_id, user.user_id),
                "status": "completed",
                "auditId": format!("audit_{}_{}", action_id, user.user_id),
                "result": result,
            }))),
        )
            .into_response(),
        Err(response) => response,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;
    use wiremock::{
        matchers::{method as wm_method, path as wm_path},
        Mock, MockServer, ResponseTemplate,
    };

    use crate::domains::browser::tests::test_app_state;
    use crate::middleware::AuthorizedMembership;

    fn test_user(user_id: &str, org_id: &str) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: user_id.to_owned(),
            user_email: format!("{user_id}@example.test"),
            user_name: user_id.to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: None,
            active_org_id: Some(org_id.to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: org_id.to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    async fn insert_test_user(
        mut request: axum::extract::Request,
        next: axum::middleware::Next,
    ) -> Response {
        request
            .extensions_mut()
            .insert(test_user("user-a", "org_a"));
        next.run(request).await
    }

    /// The evidence routes behind a stand-in session layer for `user-a`.
    fn evidence_app(state: AppState) -> axum::Router {
        axum::Router::new()
            .route(
                "/api/v1/browser/sessions/{session_id}/receipts",
                get(get_session_receipts),
            )
            .route(
                "/api/v1/browser/sessions/{session_id}/egress-receipts",
                get(get_session_egress_receipts),
            )
            .route(
                "/api/v1/browser/sessions/{session_id}/procedure",
                post(compile_session_procedure),
            )
            .layer(axum::middleware::from_fn(insert_test_user))
            .with_state(state)
    }

    async fn send(app: axum::Router, method: &str, uri: &str, body: &str) -> (StatusCode, Value) {
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_owned()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }

    /// Quarry knows run-1 for the caller, with the given ZDR flag.
    async fn mount_owned_run(quarry: &MockServer, zdr: bool) {
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/agent/runs/run-1/browser-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "runId": "run-1", "zdr": zdr, "controlMode": "agent_control" }
            })))
            .mount(quarry)
            .await;
    }

    fn sample_receipt() -> Value {
        json!({
            "receipt_id": "rcpt-1",
            "run_id": "run-1",
            "step": 1,
            "started_at": "2026-10-23T10:00:00Z",
            "finished_at": "2026-10-23T10:00:01Z",
            "action": { "type": "type_ref", "snapshot_id": "s1", "generation": 1, "ref_id": "r1", "text": "hunter2" },
            "outcome": { "status": "failed", "error_code": "TARGET_GONE", "message": "page said hunter2" },
            "observation": { "url": "https://example.com/form", "action_outcome": { "status": "unknown" }, "console_summary": [{ "level": "info", "text": "secret page text" }] },
            "cost_micro_usd": 12,
            "initiated_by": "agent",
            "control_epoch": 3
        })
    }

    #[test]
    fn devtools_query_allowlist_drops_unknown_keys() {
        let uri: Uri = "/x?afterSequence=3&limit=20&levels=warn,error&filter=boom&category=console&tabId=tab-1&cookie=abc&evil=1"
            .parse()
            .unwrap();
        let query = safe_devtools_query(&uri).expect("a valid query");
        assert!(query.starts_with('?'), "{query}");
        assert!(query.contains("afterSequence=3"), "{query}");
        assert!(query.contains("limit=20"), "{query}");
        assert!(query.contains("tabId=tab-1"), "{query}");
        assert!(query.contains("filter=boom"), "{query}");
        assert!(!query.contains("cookie"), "unknown keys never reach Quarry: {query}");
        assert!(!query.contains("evil"), "unknown keys never reach Quarry: {query}");

        let bare: Uri = "/x?cookie=abc".parse().unwrap();
        assert_eq!(safe_devtools_query(&bare).unwrap(), "");
    }

    #[test]
    fn devtools_query_rejects_control_characters() {
        let long_filter = format!("/x?filter={}", "a".repeat(201));
        let cases: Vec<&str> = vec![
            "/x?filter=a%0Ab",
            "/x?limit=0",
            "/x?limit=513",
            "/x?levels=trace",
            "/x?levels=debug,info,log,warn,error,warning",
            "/x?category=cookies",
            "/x?tabId=a%2Fb",
            long_filter.as_str(),
        ];
        for raw in cases {
            let uri: Uri = raw.parse().unwrap();
            assert!(safe_devtools_query(&uri).is_err(), "{raw} is refused");
        }
    }

    #[test]
    fn receipts_projection_drops_observation_and_text() {
        let view = receipt_view(&sample_receipt());
        let rendered = view.to_string();
        assert!(!rendered.contains("observation"), "{rendered}");
        assert!(!rendered.contains("hunter2"), "typed text never leaves: {rendered}");
        assert!(!rendered.contains("secret page text"), "{rendered}");
        assert!(!rendered.contains("page said"), "failure messages are dropped: {rendered}");
        assert_eq!(view["actionType"], "type_ref");
        assert_eq!(view["actionLabel"], "Type ref");
        assert_eq!(view["outcome"]["errorCode"], "TARGET_GONE");
        assert_eq!(view["actionOutcome"], "unknown");
        assert_eq!(view["url"], "https://example.com/form");
        assert_eq!(view["controlEpoch"], 3);
    }

    #[test]
    fn egress_projection_renames_snake_case() {
        let row = json!({
            "sequence": 7,
            "tab_id": "tab-1",
            "method": "GET",
            "url": "https://example.com/path",
            "decision": "block",
            "policy": "private_address",
            "timestamp_ms": 1234
        });
        let view = egress_view(&row);
        assert_eq!(view["tabId"], "tab-1");
        assert_eq!(view["timestampMs"], 1234);
        assert_eq!(view["decision"], "block");
        assert!(view.get("tab_id").is_none(), "{view}");
        assert!(view.get("timestamp_ms").is_none(), "{view}");
    }

    #[test]
    fn procedure_redacts_typed_text_before_responding() {
        let procedure = json!({
            "procedure_id": "proc-1",
            "version": 1,
            "source_receipt_ids": ["rcpt-1"],
            "steps": [{
                "step": 1,
                "action": { "type": "type_ref", "snapshot_id": "s1", "generation": 1, "ref_id": "r1", "text": "hunter2" },
                "expected_outcome": "verified"
            }]
        });
        let view = procedure_view(&procedure);
        let rendered = view.to_string();
        assert!(!rendered.contains("hunter2"), "{rendered}");
        assert_eq!(
            view["steps"][0]["action"]["text"],
            json!({ "redacted": true, "length": 7 })
        );
        assert_eq!(view["steps"][0]["action"]["ref_id"], "r1", "replay fields stay");
        assert_eq!(view["steps"][0]["expectedOutcome"], "verified");
        assert_eq!(view["procedureId"], "proc-1");
    }

    #[test]
    fn evidence_routes_never_read_a_forwarded_org_header() {
        // The header name is split so this test does not name it as a literal.
        let source = include_str!("browser_evidence.rs");
        assert!(
            !source.contains(concat!("x-verevon", "-org-id")),
            "org comes from the verified owner, never a header"
        );
    }

    #[tokio::test]
    async fn receipts_route_is_owner_gated_404_for_foreign_session() {
        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/agent/runs/run-1/browser-session"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "error": { "code": "NOT_FOUND", "message": "agent run not found" }
            })))
            .mount(&quarry)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/agent/runs/run-1/receipts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
            .expect(0)
            .mount(&quarry)
            .await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();

        let (status, _) = send(
            evidence_app(state),
            "GET",
            "/api/v1/browser/sessions/run-1/receipts",
            "",
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn zdr_receipts_return_the_typed_error_without_asking_quarry() {
        let quarry = MockServer::start().await;
        mount_owned_run(&quarry, true).await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/agent/runs/run-1/receipts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
            .expect(0)
            .mount(&quarry)
            .await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();

        let (status, body) = send(
            evidence_app(state),
            "GET",
            "/api/v1/browser/sessions/run-1/receipts",
            "",
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "not a 404: {body}");
        assert_eq!(body["error"]["code"], "browser_receipts_unavailable_zdr");
    }

    #[tokio::test]
    async fn procedure_409_maps_to_typed_code() {
        let quarry = MockServer::start().await;
        mount_owned_run(&quarry, false).await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/agent/runs/run-1/procedure"))
            .respond_with(ResponseTemplate::new(409).set_body_json(json!({
                "error": { "code": "CONFLICT", "message": "unobserved effect" }
            })))
            .mount(&quarry)
            .await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();

        let (status, body) = send(
            evidence_app(state),
            "POST",
            "/api/v1/browser/sessions/run-1/procedure",
            "{}",
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "browser_procedure_not_compilable");
    }

    #[tokio::test]
    async fn compiled_procedure_response_carries_no_typed_text() {
        let quarry = MockServer::start().await;
        mount_owned_run(&quarry, false).await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/agent/runs/run-1/procedure"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "procedure_id": "proc-1",
                    "version": 1,
                    "source_receipt_ids": ["rcpt-1"],
                    "steps": [{
                        "step": 1,
                        "action": { "type": "type_ref", "snapshot_id": "s1", "generation": 1, "ref_id": "r1", "text": "hunter2" },
                        "expected_outcome": "verified"
                    }]
                }
            })))
            .mount(&quarry)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/agent/procedures/quality-check"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "procedure_id": "proc-1", "step_count": 1, "verified_step_count": 1, "promotion_allowed": true }
            })))
            .mount(&quarry)
            .await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();

        let (status, body) = send(
            evidence_app(state),
            "POST",
            "/api/v1/browser/sessions/run-1/procedure",
            "{}",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let rendered = body.to_string();
        assert!(!rendered.contains("hunter2"), "{rendered}");
        assert_eq!(body["data"]["quality"]["promotionAllowed"], true);
        assert_eq!(body["data"]["procedure"]["steps"][0]["action"]["text"]["length"], 7);
    }
}
