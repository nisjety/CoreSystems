//! Browser evidence proxies (WS9.C, D9 S3a): the receipts, egress-receipts and
//! procedure reads behind the BrowserChrome evidence panels, and the allowlist
//! that the devtools read passes through before Quarry sees it.
//!
//! E3.3-d adds the region and selection attach route behind `BROWSER_REGION_ATTACH`
//! (default off): the browser crops the streamed frame, and the gateway re-resolves
//! the tab and the region against Quarry before it hands back an ephemeral
//! attachment classified `browser-scraped`. A zero-data-retention session is refused.
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
    http::{header, HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{
    engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use hmac::{Hmac, KeyInit, Mac};
use reqwest::Method;
use sha2::{Digest, Sha256};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    domains::chat::shared,
    envelope::{error, error_with_details, ok, unwrap_data},
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

/// `BROWSER_REGION_ATTACH` (E3.3-d): mounts the region and selection attach route and
/// enables `browser_region.attach` on the action surface (QF-7). Default off. Read once
/// into `AppState::browser_region_attach` in `build_state`, never inside a handler.
const REGION_ATTACH_PATH: &str = "/api/v1/browser/sessions/{session_id}/attachments/region";
/// Decoded bytes of one cropped region image. The base64 body stays under the
/// router's default 2 MB request limit.
const REGION_IMAGE_MAX_BYTES: usize = 1024 * 1024;
/// The smallest region edge, in viewport CSS pixels, after clamping.
const REGION_MIN_EDGE_PX: u32 = 8;
/// A cropped image may be at most this many times the region's size (a high
/// device pixel ratio frame); a smaller (downscaled) image is fine.
const REGION_MAX_IMAGE_SCALE: u64 = 3;
/// Relative aspect ratio difference allowed between the image and the region.
const REGION_ASPECT_TOLERANCE: f64 = 0.05;
const SELECTION_MAX_CHARS: usize = 4000;
const TAB_LABEL_MAX_CHARS: usize = 200;
/// The wire name of `TrustClass::BrowserScraped` (mp-content-safety `trust.rs`).
const REGION_TRUST_CLASS: &str = "browser-scraped";
/// Lifetime of a region attach token (QF-8): the region must be sent on a chat turn
/// within ten minutes of the attach.
const REGION_ATTACH_TOKEN_TTL_SECS: u64 = 600;
/// Version tag of the attach token payload (QF-8).
const REGION_ATTACH_TOKEN_VERSION: &str = "v1";
/// Longest attach token accepted on a chat turn: the expiry, a dot and a base64url SHA-256 MAC.
const REGION_ATTACH_TOKEN_MAX_CHARS: usize = 96;

/// Saved recipes (Q4-E4.2-4a), mounted only with `AppState::browser_recipes` (`BROWSER_RECIPES`).
const SESSION_RECIPES_PATH: &str = "/api/v1/browser/sessions/{session_id}/recipes";
const RECIPES_PATH: &str = "/api/v1/browser/recipes";
/// capability-core's `procedures.MaxNameRunes` and `MaxDescriptionRunes`.
const RECIPE_NAME_MAX_CHARS: usize = 120;
const RECIPE_DESCRIPTION_MAX_CHARS: usize = 500;
/// capability-core's `procedures.MaxListLimit`.
const RECIPES_LIST_LIMIT_MAX: u64 = 100;
/// The longest recipe id the list cursor may carry.
const RECIPE_ID_MAX_CHARS: usize = 128;
/// Quality reasons a refusal carries at most, each at most this many characters.
const RECIPE_REASONS_MAX: usize = 32;
const RECIPE_REASON_MAX_CHARS: usize = 200;
/// The action id of the save on the generic action surface.
const SAVE_RECIPE_ACTION_ID: &str = "browser_evidence.save_recipe";

pub(crate) fn router(state: AppState) -> Router<AppState> {
    let region_attach = state.browser_region_attach;
    router_with(state, region_attach)
}

/// The `BROWSER_REGION_ATTACH` grammar; `build_state` parses the switch with the
/// same `crate::config::human_browse_enabled`.
#[cfg(test)]
fn region_attach_flag_on(value: Option<&str>) -> bool {
    crate::config::human_browse_enabled(value)
}

fn router_with(state: AppState, region_attach: bool) -> Router<AppState> {
    let mut routes = Router::new()
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
    if region_attach {
        routes = routes.route(REGION_ATTACH_PATH, post(attach_session_region));
    }
    if state.browser_recipes {
        routes = routes
            .route(SESSION_RECIPES_PATH, post(save_session_recipe))
            .route(RECIPES_PATH, get(list_saved_recipes));
    }
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
                    // Version 2 fields (Q4-E4.2-4a): the target keeps only its locator (the
                    // fingerprint identity stays in Quarry), and the slot is a name, never a value.
                    let target = step
                        .get("target")
                        .and_then(|target| target.get("locator"))
                        .filter(|locator| !locator.is_null())
                        .map(|locator| json!({ "locator": locator.clone() }));
                    without_nulls(json!({
                        "step": step.get("step").cloned(),
                        "action": redact_action(
                            step.get("action").cloned().unwrap_or(Value::Null),
                        ),
                        "expectedOutcome": step.get("expected_outcome").cloned(),
                        "target": target,
                        "slot": step.get("slot").filter(|slot| slot.is_string()).cloned(),
                        "expect": step.get("expect").cloned(),
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    let slots = slots_view(procedure);
    without_nulls(json!({
        "procedureId": procedure.get("procedure_id").cloned(),
        "version": procedure.get("version").cloned(),
        "sourceReceiptIds": procedure.get("source_receipt_ids").cloned(),
        "steps": steps,
        "slots": if slots.is_empty() { Value::Null } else { Value::Array(slots) },
    }))
}

/// A version 2 procedure's open inputs, by name and kind only. A slot never carries a value: the
/// replay supplies it. Absent (version 1) is an empty list.
fn slots_view(procedure: &Value) -> Vec<Value> {
    procedure
        .get("slots")
        .and_then(Value::as_array)
        .map(|slots| {
            slots
                .iter()
                .filter_map(|slot| {
                    let name = slot.get("name").and_then(Value::as_str)?;
                    // An unknown kind is shown as secret, never as plain text.
                    let kind = match slot.get("kind").and_then(Value::as_str) {
                        Some("text") => "text",
                        _ => "secret",
                    };
                    Some(json!({ "name": name, "kind": kind }))
                })
                .collect()
        })
        .unwrap_or_default()
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
    // With saved recipes on, the browser sees the canonical (version 2) compile it would save.
    let (procedure, quality) = compile_with_quality(
        state,
        user,
        headers,
        session_id,
        procedure_id,
        state.browser_recipes,
    )
    .await?;
    Ok(json!({
        "procedure": procedure_view(&procedure),
        "quality": quality_view(&quality),
    }))
}

/// Owner gate, zero-data-retention refusal, compile and quality check. Returns the procedure as
/// Quarry compiled it and Quarry's quality report, both unprojected: callers project them before
/// anything reaches the browser. `canonical` asks Quarry for a version 2 procedure (Q4-E4.2-2c).
async fn compile_with_quality(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    session_id: &str,
    procedure_id: Option<String>,
    canonical: bool,
) -> Result<(Value, Value), Response> {
    let (token, projection) = owner_gate(state, user, headers, session_id).await?;
    if receipts_refused(&projection) {
        return Err(zdr_receipts_unavailable());
    }
    let mut compile_body = match procedure_id {
        Some(id) => json!({ "procedure_id": id }),
        None => json!({}),
    };
    if canonical {
        compile_body["canonical"] = Value::Bool(true);
    }
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
    Ok((procedure, quality))
}

// ---------------------------------------------------------------------------
// Saved recipes (Q4-E4.2-4a)
// ---------------------------------------------------------------------------

/// The action surface's answer while `BROWSER_RECIPES` is off. The HTTP routes are absent then.
fn recipes_unavailable() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "browser_recipes_unavailable",
            "Saved browser recipes are not enabled.",
        )),
    )
        .into_response()
}

fn invalid_recipe(message: &'static str) -> Response {
    bad_request("invalid_browser_recipe", message)
}

/// The quality check refused promotion: the session's procedure cannot be saved as a recipe.
/// The reasons are Quarry's closed reason strings, capped.
fn recipe_not_promotable(quality: &Value) -> Response {
    let reasons: Vec<Value> = quality
        .get("reasons")
        .and_then(Value::as_array)
        .map(|reasons| {
            reasons
                .iter()
                .filter_map(Value::as_str)
                .take(RECIPE_REASONS_MAX)
                .map(|reason| Value::String(reason.chars().take(RECIPE_REASON_MAX_CHARS).collect()))
                .collect()
        })
        .unwrap_or_default();
    (
        StatusCode::CONFLICT,
        Json(error_with_details(
            "recipe_not_promotable",
            "This session's procedure did not pass the quality check and cannot be saved as a recipe.",
            without_nulls(json!({
                "reasons": reasons,
                "stepCount": quality.get("step_count").cloned(),
                "verifiedStepCount": quality.get("verified_step_count").cloned(),
            })),
        )),
    )
        .into_response()
}

/// A closed upstream reason: lower-case letters, digits and underscores, at most 64 characters.
fn closed_reason(raw: Option<&str>) -> Option<String> {
    raw.filter(|code| {
        (1..=64).contains(&code.len())
            && code
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    })
    .map(str::to_owned)
}

/// Maps a model-gateway / capability-core recipe failure to the gateway's own codes. Only the
/// upstream's closed error code passes, as `details.reason`; its message, and anything naming an
/// upstream URL or credential, never reaches the browser.
fn recipe_upstream_failure(status: StatusCode, body: &Value) -> Response {
    let reason = closed_reason(body.pointer("/error/code").and_then(Value::as_str));
    let (status, code, message): (StatusCode, &'static str, &'static str) = match status {
        StatusCode::NOT_FOUND if reason.as_deref() == Some("procedures_disabled") => (
            StatusCode::NOT_FOUND,
            "browser_recipes_unavailable",
            "Saved browser recipes are not enabled.",
        ),
        StatusCode::NOT_FOUND => (
            StatusCode::NOT_FOUND,
            "browser_recipe_not_found",
            "The recipe was not found.",
        ),
        StatusCode::BAD_REQUEST | StatusCode::PAYLOAD_TOO_LARGE | StatusCode::UNPROCESSABLE_ENTITY => (
            StatusCode::BAD_REQUEST,
            "browser_recipe_rejected",
            "The recipe store refused this recipe.",
        ),
        StatusCode::CONFLICT => (
            StatusCode::CONFLICT,
            "browser_recipe_limit_reached",
            "The recipe store has reached a limit for this recipe.",
        ),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => (
            StatusCode::FORBIDDEN,
            "browser_recipe_forbidden",
            "You may not use saved recipes here.",
        ),
        StatusCode::GATEWAY_TIMEOUT => (
            StatusCode::GATEWAY_TIMEOUT,
            "browser_recipe_timeout",
            "The recipe store did not answer in time.",
        ),
        _ => return (StatusCode::BAD_GATEWAY, Json(crate::envelope::upstream_unavailable())).into_response(),
    };
    match reason {
        Some(reason) => (
            status,
            Json(error_with_details(code, message, json!({ "reason": reason }))),
        )
            .into_response(),
        None => (status, Json(error(code, message))).into_response(),
    }
}

/// A recipe's name and optional description from a request object. `allowed` names the keys the
/// object may carry; any other key is refused, so nothing untyped reaches the recipe store.
fn recipe_meta_of(
    input: &Value,
    allowed: &[&str],
) -> Result<(String, Option<String>), &'static str> {
    let Some(object) = input.as_object() else {
        return Err("The recipe request must be a JSON object.");
    };
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("The recipe request may carry only a name and a description.");
    }
    let name = match object.get("name") {
        Some(Value::String(raw)) => raw.trim(),
        _ => return Err("A recipe needs a 'name'."),
    };
    let name_chars = name.chars().count();
    if name_chars == 0 || name_chars > RECIPE_NAME_MAX_CHARS || name.chars().any(char::is_control) {
        return Err("A recipe name is 1 to 120 characters without control characters.");
    }
    let description = match object.get("description") {
        None | Some(Value::Null) => None,
        Some(Value::String(raw)) => {
            let description = raw.trim();
            if description.chars().count() > RECIPE_DESCRIPTION_MAX_CHARS
                || description
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t')
            {
                return Err("A recipe description is at most 500 characters.");
            }
            (!description.is_empty()).then(|| description.to_owned())
        }
        Some(_) => return Err("A recipe description must be a string."),
    };
    Ok((name.to_owned(), description))
}

/// Saves the session's canonical procedure as a recipe (Q4-E4.2-4a). Only the session's owner may
/// save; a zero-data-retention session is refused before Quarry is asked; a procedure the quality
/// check does not promote is refused with its reasons. The recipe store is reached through
/// model-gateway `/v1/recipes` with the caller's own capability token, so capability-core binds the
/// owner and the org from that token, never from this body.
async fn save_recipe_view(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    session_id: &str,
    name: String,
    description: Option<String>,
) -> Result<Value, Response> {
    let (procedure, quality) =
        compile_with_quality(state, user, headers, session_id, None, true).await?;
    if quality.get("promotion_allowed").and_then(Value::as_bool) != Some(true) {
        return Err(recipe_not_promotable(&quality));
    }
    let capability = match shared::required_capability_token(state, user, headers).await {
        Ok(token) => token,
        Err(cause) => return Err(shared::delegated_auth_unavailable(cause).into_response()),
    };
    let token = shared::model_token(state, user, headers).await;
    let mut create = json!({
        "name": name,
        "source_run_id": session_id,
        "procedure": procedure,
    });
    if let Some(description) = description {
        create["description"] = Value::String(description);
    }
    let (status, Json(body)) = shared::proxy_model_json_with_capability(
        state,
        Method::POST,
        &format!("{}/v1/recipes", state.model_gateway_url),
        Some(create),
        token.as_deref(),
        Some(&capability),
        user,
    )
    .await;
    if !status.is_success() {
        return Err(recipe_upstream_failure(status, &body));
    }
    let written = unwrap_data(&body);
    Ok(without_nulls(json!({
        "recipeId": written.get("procedure_id").filter(|id| id.is_string()).cloned(),
        "version": written.get("version").filter(|version| version.is_u64()).cloned(),
        "digest": written.get("digest").filter(|digest| digest.is_string()).cloned(),
        "slots": slots_view(&procedure),
    })))
}

/// The list query the recipe store may receive: `limit` (1 to 100) and `afterId` (the previous
/// page's `nextAfterId`), each validated. Any other key is dropped. Returns `?query` or "".
fn recipes_list_query(uri: &Uri) -> Result<String, &'static str> {
    let query = query_map(uri);
    let mut pairs: Vec<String> = Vec::new();
    if let Some(raw) = query.get("limit").filter(|raw| !raw.is_empty()) {
        let limit: u64 = raw.parse().map_err(|_| "limit must be a whole number")?;
        if !(1..=RECIPES_LIST_LIMIT_MAX).contains(&limit) {
            return Err("limit must be between 1 and 100");
        }
        pairs.push(format!("limit={limit}"));
    }
    if let Some(raw) = query.get("afterId").filter(|raw| !raw.is_empty()) {
        if raw.len() > RECIPE_ID_MAX_CHARS
            || !raw
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err("afterId is not a recipe id");
        }
        pairs.push(format!("after_id={raw}"));
    }
    Ok(if pairs.is_empty() {
        String::new()
    } else {
        format!("?{}", pairs.join("&"))
    })
}

/// One saved recipe in the list, projected to the fields the browser reads.
fn recipe_list_item_view(item: &Value) -> Value {
    without_nulls(json!({
        "recipeId": item.get("id").cloned(),
        "name": item.get("name").cloned(),
        "description": item
            .get("description")
            .filter(|description| description.as_str().is_some_and(|text| !text.is_empty()))
            .cloned(),
        "sourceRunId": item
            .get("source_run_id")
            .filter(|run| run.as_str().is_some_and(|text| !text.is_empty()))
            .cloned(),
        "latestVersion": item.get("latest_version").cloned(),
        "createdAt": item.get("created_at").cloned(),
        "updatedAt": item.get("updated_at").cloned(),
    }))
}

/// The caller's saved recipes, one page at a time, through model-gateway with the caller's own
/// capability token: capability-core lists only the verified caller's recipes.
async fn recipes_list_view(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    query: &str,
) -> Result<Value, Response> {
    let capability = match shared::required_capability_token(state, user, headers).await {
        Ok(token) => token,
        Err(cause) => return Err(shared::delegated_auth_unavailable(cause).into_response()),
    };
    let token = shared::model_token(state, user, headers).await;
    let (status, Json(body)) = shared::proxy_model_json_with_capability(
        state,
        Method::GET,
        &format!("{}/v1/recipes{query}", state.model_gateway_url),
        None,
        token.as_deref(),
        Some(&capability),
        user,
    )
    .await;
    if !status.is_success() {
        return Err(recipe_upstream_failure(status, &body));
    }
    let data = unwrap_data(&body);
    let recipes: Vec<Value> = data
        .get("procedures")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(recipe_list_item_view).collect())
        .unwrap_or_default();
    Ok(without_nulls(json!({
        "recipes": recipes,
        "nextAfterId": data
            .get("next_after_id")
            .filter(|next| next.as_str().is_some_and(|text| !text.is_empty()))
            .cloned(),
    })))
}

// ---------------------------------------------------------------------------
// Region and selection attach (E3.3-d)
// ---------------------------------------------------------------------------

/// What the browser sends: the tab it believes it showed, the dragged region in
/// viewport CSS pixels, the image it cropped from the streamed frame, and an
/// optional selected text. None of it is trusted: the tab and the region are
/// re-resolved here and the image is checked against the resolved region.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RegionAttachBody {
    tab_id: String,
    region: RegionRectInput,
    image: RegionImageInput,
    #[serde(default)]
    selection_text: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegionRectInput {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RegionImageInput {
    mime_type: String,
    data_base64: String,
}

/// A region in whole viewport CSS pixels, inside the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RegionRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Debug)]
struct RegionImage {
    mime_type: &'static str,
    bytes: Vec<u8>,
    width: u32,
    height: u32,
}

fn invalid_attachment(message: &'static str) -> Response {
    bad_request("invalid_browser_attachment", message)
}

/// Page content from a zero-data-retention session is never attached.
fn zdr_attachment_refused() -> Response {
    (
        StatusCode::CONFLICT,
        Json(error(
            "browser_attachment_refused_zdr",
            "Page regions cannot be attached from a zero-data-retention browser session.",
        )),
    )
        .into_response()
}

fn attachment_unresolved(code: &'static str, message: &'static str, reason: &'static str) -> Response {
    (
        StatusCode::CONFLICT,
        Json(error_with_details(code, message, json!({ "reason": reason }))),
    )
        .into_response()
}

/// The session viewport from Quarry's projection, or None when it is not stated.
/// The region is never resolved against a guessed viewport.
fn projection_viewport(projection: &Value) -> Option<(u32, u32)> {
    let viewport = projection.get("viewport")?;
    let width = viewport
        .get("width")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)?;
    let height = viewport
        .get("height")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)?;
    Some((width, height))
}

/// Snap the requested region outward to whole pixels and clamp it to the
/// viewport. Refused when a value is not finite, an edge is not positive, or the
/// clamped region is smaller than the minimum edge (outside the viewport).
fn resolve_region(
    input: RegionRectInput,
    viewport_width: u32,
    viewport_height: u32,
) -> Result<RegionRect, &'static str> {
    let RegionRectInput {
        x,
        y,
        width,
        height,
    } = input;
    if ![x, y, width, height].iter().all(|value| value.is_finite()) {
        return Err("region values must be finite numbers");
    }
    if width <= 0.0 || height <= 0.0 {
        return Err("region width and height must be positive");
    }
    let viewport_w = f64::from(viewport_width);
    let viewport_h = f64::from(viewport_height);
    let left = x.floor().clamp(0.0, viewport_w);
    let top = y.floor().clamp(0.0, viewport_h);
    let right = (x + width).ceil().clamp(0.0, viewport_w);
    let bottom = (y + height).ceil().clamp(0.0, viewport_h);
    let min_edge = f64::from(REGION_MIN_EDGE_PX);
    if right - left < min_edge || bottom - top < min_edge {
        return Err("region is outside the viewport or smaller than 8 pixels");
    }
    // Every value is a whole number in [0, viewport], so the casts are exact.
    Ok(RegionRect {
        x: left as u32,
        y: top as u32,
        width: (right - left) as u32,
        height: (bottom - top) as u32,
    })
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// Width and height from a PNG's IHDR chunk.
fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    (width > 0 && height > 0).then_some((width, height))
}

/// Width and height from a JPEG's first start-of-frame segment.
fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut index = 2;
    while index + 1 < bytes.len() {
        if bytes[index] != 0xFF {
            return None;
        }
        let marker = bytes[index + 1];
        // Fill bytes before a marker.
        if marker == 0xFF {
            index += 1;
            continue;
        }
        // Markers without a length.
        if marker == 0x01 || (0xD0..=0xD8).contains(&marker) {
            index += 2;
            continue;
        }
        // Start of scan or end of image before a frame header: no size.
        if marker == 0xDA || marker == 0xD9 {
            return None;
        }
        if index + 4 > bytes.len() {
            return None;
        }
        let length = usize::from(u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]));
        if length < 2 {
            return None;
        }
        let is_frame_header =
            (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_frame_header {
            if index + 9 > bytes.len() {
                return None;
            }
            let height = u32::from(u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]));
            let width = u32::from(u16::from_be_bytes([bytes[index + 7], bytes[index + 8]]));
            return (width > 0 && height > 0).then_some((width, height));
        }
        index += 2 + length;
    }
    None
}

/// Decode and check the cropped image: PNG or JPEG only, at most 1 MiB, and
/// bytes that match the declared type.
fn decode_region_image(input: &RegionImageInput) -> Result<RegionImage, &'static str> {
    let mime_type: &'static str = match input.mime_type.trim().to_ascii_lowercase().as_str() {
        "image/png" => "image/png",
        "image/jpeg" | "image/jpg" => "image/jpeg",
        _ => return Err("image mimeType must be image/png or image/jpeg"),
    };
    let encoded = input.data_base64.trim();
    if encoded.is_empty() {
        return Err("image dataBase64 is empty");
    }
    if encoded.len() > (REGION_IMAGE_MAX_BYTES / 3 + 1) * 4 {
        return Err("image is larger than 1 MiB");
    }
    let bytes = BASE64_STANDARD
        .decode(encoded.as_bytes())
        .map_err(|_| "image dataBase64 is not valid base64")?;
    if bytes.len() > REGION_IMAGE_MAX_BYTES {
        return Err("image is larger than 1 MiB");
    }
    let dimensions = match mime_type {
        "image/png" => png_dimensions(&bytes),
        _ => jpeg_dimensions(&bytes),
    };
    let Some((width, height)) = dimensions else {
        return Err("image bytes do not match the declared type");
    };
    Ok(RegionImage {
        mime_type,
        bytes,
        width,
        height,
    })
}

/// The cropped image must have the region's shape (within 5 percent, or one
/// pixel of rounding) and must not be more than three times its size, so an
/// arbitrary image cannot be passed off as a crop of the resolved region.
fn image_matches_region(width: u32, height: u32, region: RegionRect) -> bool {
    if width == 0 || height == 0 || region.width == 0 || region.height == 0 {
        return false;
    }
    if u64::from(width) > u64::from(region.width) * REGION_MAX_IMAGE_SCALE
        || u64::from(height) > u64::from(region.height) * REGION_MAX_IMAGE_SCALE
    {
        return false;
    }
    let image_ratio = f64::from(width) / f64::from(height);
    let region_ratio = f64::from(region.width) / f64::from(region.height);
    if (image_ratio - region_ratio).abs() / region_ratio <= REGION_ASPECT_TOLERANCE {
        return true;
    }
    // One pixel of rounding in either dimension for small crops.
    let cross = (i64::from(width) * i64::from(region.height)
        - i64::from(height) * i64::from(region.width))
    .unsigned_abs();
    cross <= u64::from(region.width.max(region.height))
}

/// Selected text is page content: control characters other than newline and
/// tab become spaces, it is trimmed, and blank text is no selection.
fn sanitize_selection(raw: Option<&str>) -> Result<Option<String>, &'static str> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let cleaned: String = raw
        .chars()
        .map(|ch| {
            if ch == '\n' || ch == '\t' || !ch.is_control() {
                ch
            } else {
                ' '
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > SELECTION_MAX_CHARS {
        return Err("selectionText is longer than 4000 characters");
    }
    Ok(Some(trimmed.to_owned()))
}

/// Re-resolve the tab against Quarry's live tab list, failing closed. The streamed
/// frame only ever shows the active tab, so a known tab that is not active is
/// refused too.
fn resolve_attach_tab<'a>(tabs: &'a [Value], tab_id: &str) -> Result<&'a Value, &'static str> {
    if tabs.is_empty() {
        return Err("no-tabs");
    }
    let matches: Vec<&Value> = tabs
        .iter()
        .filter(|tab| tab.get("tabId").and_then(Value::as_str) == Some(tab_id))
        .collect();
    match matches.as_slice() {
        [] => Err("unknown-tab"),
        [tab] if tab.get("active").and_then(Value::as_bool) == Some(true) => Ok(*tab),
        [_] => Err("tab-not-active"),
        _ => Err("ambiguous-tab"),
    }
}

/// Origin and path of an http(s) URL, with credentials, query and fragment dropped.
fn safe_attach_target(raw: Option<&str>) -> Option<String> {
    let parsed = url::Url::parse(raw?.trim()).ok()?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }
    Some(format!(
        "{}{}",
        parsed.origin().ascii_serialization(),
        parsed.path()
    ))
}

/// Tab title without control characters, at most 200 characters.
fn safe_tab_title(raw: Option<&str>) -> Option<String> {
    let title: String = raw?
        .chars()
        .filter(|ch| !ch.is_control())
        .take(TAB_LABEL_MAX_CHARS)
        .collect();
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_owned())
}

/// The attachment the browser gets back. It is ephemeral (the gateway stores
/// nothing), grants nothing, and is classified browser-scraped wherever it goes.
fn region_attachment_view(
    session_id: &str,
    tab: &Value,
    region: RegionRect,
    viewport: (u32, u32),
    image: &RegionImage,
    selection: Option<String>,
) -> Value {
    let tab_id = tab.get("tabId").and_then(Value::as_str).unwrap_or_default();
    let target = safe_attach_target(tab.get("url").and_then(Value::as_str));
    let title = safe_tab_title(tab.get("title").and_then(Value::as_str));
    let label = title
        .clone()
        .or_else(|| target.clone())
        .unwrap_or_else(|| tab_id.to_owned());
    let selection = selection.map(|text| {
        let length = text.chars().count();
        json!({ "text": text, "length": length })
    });
    json!({
        "attachment": {
            "kind": "browser-region",
            "trustClass": REGION_TRUST_CLASS,
            "grant": "none",
            "retention": "ephemeral",
            "sessionId": session_id,
            "tabId": tab_id,
            "label": label,
            "title": title,
            "target": target,
            "region": {
                "x": region.x,
                "y": region.y,
                "width": region.width,
                "height": region.height,
            },
            "viewport": { "width": viewport.0, "height": viewport.1 },
            "image": {
                "mimeType": image.mime_type,
                "dataBase64": BASE64_STANDARD.encode(&image.bytes),
                "width": image.width,
                "height": image.height,
                "byteLength": image.bytes.len(),
            },
            "selection": selection,
        }
    })
}

async fn region_attach_view(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    session_id: &str,
    input: RegionAttachBody,
) -> Result<Value, Response> {
    // Everything that needs no upstream is checked first.
    let tab_id = input.tab_id.trim();
    if !valid_tab_id(tab_id) {
        return Err(bad_request(
            "invalid_input",
            "tabId must be 1 to 64 letters, digits, '-' or '_'.",
        ));
    }
    let image = decode_region_image(&input.image).map_err(invalid_attachment)?;
    let selection =
        sanitize_selection(input.selection_text.as_deref()).map_err(invalid_attachment)?;

    let (token, projection) = owner_gate(state, user, headers, session_id).await?;
    // A projection that does not state the flag is refused like a ZDR session,
    // and Quarry's tab list is not read at all.
    if receipts_refused(&projection) {
        return Err(zdr_attachment_refused());
    }
    let Some(viewport) = projection_viewport(&projection) else {
        return Err(attachment_unresolved(
            "browser_attachment_region_unresolved",
            "The browser session's viewport is not known, so the region cannot be resolved.",
            "viewport-unknown",
        ));
    };
    let region = resolve_region(input.region, viewport.0, viewport.1).map_err(invalid_attachment)?;
    if !image_matches_region(image.width, image.height, region) {
        return Err(invalid_attachment(
            "the image does not match the shape of the resolved region",
        ));
    }

    let (status, body) = quarry_call(
        state,
        Method::GET,
        &format!("/v1/agent/runs/{}/tabs", urlencoding::encode(session_id)),
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return Err(forward_quarry_failure(status, body));
    }
    let tabs: Vec<Value> = unwrap_data(&body)
        .get("tabs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let tab = match resolve_attach_tab(&tabs, tab_id) {
        Ok(tab) => tab,
        Err(reason) => {
            return Err(attachment_unresolved(
                "browser_attachment_tab_unresolved",
                "The tab could not be resolved to the live tab the frame shows.",
                reason,
            ))
        }
    };
    let mut view = region_attachment_view(session_id, tab, region, viewport, &image, selection);
    // QF-8: a stateless token that lets the chat send path verify this exact region later.
    // Nothing is stored; with no key configured no token is issued and a send is refused.
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let resolved_tab_id = tab.get("tabId").and_then(Value::as_str).unwrap_or_default();
    let expires_unix = unix_now().saturating_add(REGION_ATTACH_TOKEN_TTL_SECS);
    if let Some(token) = issue_region_attach_token(
        &state.browser_region_attach_key,
        &RegionTokenClaims {
            user_id: &user.user_id,
            org_id: &org_id,
            session_id,
            tab_id: resolved_tab_id,
            region,
            image_sha256: &hex::encode(Sha256::digest(&image.bytes)),
        },
        expires_unix,
    ) {
        if let Some(attachment) = view.get_mut("attachment").and_then(Value::as_object_mut) {
            attachment.insert("attachToken".to_owned(), Value::String(token));
            attachment.insert("attachTokenExpiresAt".to_owned(), json!(expires_unix));
        }
    }
    Ok(view)
}

// ---------------------------------------------------------------------------
// Region attach token and chat-turn verification (QF-8)
// ---------------------------------------------------------------------------

type HmacSha256 = Hmac<Sha256>;

/// Seconds since the Unix epoch (0 if the clock is before it).
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// What an attach token binds: the verified caller and org, the session and tab the
/// gateway resolved, the resolved whole-pixel region and the SHA-256 of the image bytes.
struct RegionTokenClaims<'a> {
    user_id: &'a str,
    org_id: &'a str,
    session_id: &'a str,
    tab_id: &'a str,
    region: RegionRect,
    image_sha256: &'a str,
}

/// `v1|user_id|org_id|session_id|tab_id|x,y,w,h|sha256(image)|expires_unix`, or None when a
/// part could be confused with the separator.
fn region_token_payload(claims: &RegionTokenClaims<'_>, expires_unix: u64) -> Option<String> {
    let parts = [
        claims.user_id,
        claims.org_id,
        claims.session_id,
        claims.tab_id,
        claims.image_sha256,
    ];
    if parts.iter().any(|part| part.contains('|')) {
        return None;
    }
    let RegionRect {
        x,
        y,
        width,
        height,
    } = claims.region;
    Some(format!(
        "{REGION_ATTACH_TOKEN_VERSION}|{}|{}|{}|{}|{x},{y},{width},{height}|{}|{expires_unix}",
        claims.user_id, claims.org_id, claims.session_id, claims.tab_id, claims.image_sha256,
    ))
}

fn region_token_mac(key: &str, payload: &str) -> Option<HmacSha256> {
    if key.is_empty() {
        return None;
    }
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).ok()?;
    mac.update(payload.as_bytes());
    Some(mac)
}

/// `<expires_unix>.<base64url HMAC-SHA256 of the payload>`, or None when no key is configured.
fn issue_region_attach_token(
    key: &str,
    claims: &RegionTokenClaims<'_>,
    expires_unix: u64,
) -> Option<String> {
    let payload = region_token_payload(claims, expires_unix)?;
    let mac = region_token_mac(key, &payload)?;
    Some(format!(
        "{expires_unix}.{}",
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    ))
}

/// True only when the token was issued with `key` for exactly these claims and is unexpired.
/// The MAC comparison is constant time.
fn verify_region_attach_token(
    key: &str,
    token: &str,
    claims: &RegionTokenClaims<'_>,
    now_unix: u64,
) -> bool {
    if token.is_empty() || token.len() > REGION_ATTACH_TOKEN_MAX_CHARS {
        return false;
    }
    let Some((expires, signature)) = token.split_once('.') else {
        return false;
    };
    let Ok(expires_unix) = expires.parse::<u64>() else {
        return false;
    };
    // Expired, or further out than any token this gateway issues.
    if expires_unix <= now_unix
        || expires_unix > now_unix.saturating_add(REGION_ATTACH_TOKEN_TTL_SECS)
    {
        return false;
    }
    let Ok(signature) = URL_SAFE_NO_PAD.decode(signature.as_bytes()) else {
        return false;
    };
    let Some(payload) = region_token_payload(claims, expires_unix) else {
        return false;
    };
    let Some(mac) = region_token_mac(key, &payload) else {
        return false;
    };
    mac.verify_slice(&signature).is_ok()
}

/// One region as a chat turn carries it (`browser_regions[]`), in the chat body's snake_case
/// or the attach answer's camelCase. Other keys (labels, titles, selection text) are ignored:
/// nothing the token does not cover is forwarded.
#[derive(Debug, Deserialize)]
struct TurnRegionInput {
    #[serde(alias = "sessionId")]
    session_id: String,
    #[serde(alias = "tabId")]
    tab_id: String,
    region: TurnRegionRect,
    image: TurnRegionImage,
    #[serde(alias = "attachToken")]
    attach_token: String,
}

/// The resolved whole-pixel region the attach answer returned.
#[derive(Debug, Deserialize)]
struct TurnRegionRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Debug, Deserialize)]
struct TurnRegionImage {
    #[serde(alias = "mimeType")]
    mime_type: String,
    #[serde(alias = "dataBase64")]
    data_base64: String,
}

/// Verify one region a chat turn carries (QF-8) and build the value the gateway forwards in
/// its place: the session, tab, region and image the token vouches for, classified
/// `browser-scraped` with no grant. Any failure (no key configured, bad shape, image that is
/// not the attached one, token for another user, org, session, tab or rect, expired token)
/// refuses the region. Nothing is stored.
pub(crate) fn verify_turn_region(
    key: &str,
    user_id: &str,
    org_id: &str,
    raw: &Value,
) -> Result<Value, &'static str> {
    if key.is_empty() {
        return Err("region attach tokens are not configured");
    }
    let input: TurnRegionInput = serde_json::from_value(raw.clone())
        .map_err(|_| "a region must carry session_id, tab_id, region, image and attach_token")?;
    let session_id = input.session_id.trim();
    if !is_valid_path_segment(session_id) {
        return Err("the region's session id is invalid");
    }
    let tab_id = input.tab_id.trim();
    if !valid_tab_id(tab_id) {
        return Err("the region's tab id is invalid");
    }
    let region = RegionRect {
        x: input.region.x,
        y: input.region.y,
        width: input.region.width,
        height: input.region.height,
    };
    if region.width < REGION_MIN_EDGE_PX || region.height < REGION_MIN_EDGE_PX {
        return Err("the region is smaller than 8 pixels");
    }
    let image = decode_region_image(&RegionImageInput {
        mime_type: input.image.mime_type,
        data_base64: input.image.data_base64,
    })?;
    if !image_matches_region(image.width, image.height, region) {
        return Err("the image does not match the shape of the region");
    }
    let image_sha256 = hex::encode(Sha256::digest(&image.bytes));
    let claims = RegionTokenClaims {
        user_id,
        org_id,
        session_id,
        tab_id,
        region,
        image_sha256: &image_sha256,
    };
    if !verify_region_attach_token(key, input.attach_token.trim(), &claims, unix_now()) {
        return Err("the region's attach token does not verify");
    }
    Ok(json!({
        "kind": "browser-region",
        "session_id": session_id,
        "tab_id": tab_id,
        "region": {
            "x": region.x,
            "y": region.y,
            "width": region.width,
            "height": region.height,
        },
        "image": {
            "mime_type": image.mime_type,
            "data_base64": BASE64_STANDARD.encode(&image.bytes),
            "width": image.width,
            "height": image.height,
            "byte_length": image.bytes.len(),
        },
        "trust_class": REGION_TRUST_CLASS,
        "grant": "none",
        "retention": "ephemeral",
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

/// `POST /api/v1/browser/sessions/{session_id}/attachments/region` with
/// `{ tabId, region: { x, y, width, height }, image: { mimeType, dataBase64 }, selectionText? }`
/// (E3.3-d, mounted only with `BROWSER_REGION_ATTACH`).
async fn attach_session_region(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    body: Bytes,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return invalid_session();
    }
    let input: RegionAttachBody = match serde_json::from_slice(&body) {
        Ok(input) => input,
        Err(_) => {
            return bad_request(
                "invalid_input",
                "The region attach body must be JSON with tabId, region and image only.",
            )
        }
    };
    match region_attach_view(&state, &user, &headers, &session_id, input).await {
        Ok(data) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "private, no-store")],
            Json(ok(data)),
        )
            .into_response(),
        Err(response) => response,
    }
}

/// `POST /api/v1/browser/sessions/{session_id}/recipes` with `{ name, description? }`
/// (Q4-E4.2-4a, mounted only with `BROWSER_RECIPES`). Answers `{ recipeId, version, digest, slots }`.
async fn save_session_recipe(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    body: Bytes,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return invalid_session();
    }
    let input: Value = match serde_json::from_slice(&body) {
        Ok(input) => input,
        Err(_) => return invalid_recipe("The recipe request body must be JSON."),
    };
    let (name, description) = match recipe_meta_of(&input, &["name", "description"]) {
        Ok(meta) => meta,
        Err(message) => return invalid_recipe(message),
    };
    match save_recipe_view(&state, &user, &headers, &session_id, name, description).await {
        Ok(data) => (
            StatusCode::CREATED,
            [(header::CACHE_CONTROL, "private, no-store")],
            Json(ok(data)),
        )
            .into_response(),
        Err(response) => response,
    }
}

/// `GET /api/v1/browser/recipes?limit=&afterId=` (Q4-E4.2-4a, mounted only with `BROWSER_RECIPES`).
async fn list_saved_recipes(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let query = match recipes_list_query(&uri) {
        Ok(query) => query,
        Err(message) => return bad_request("invalid_browser_recipe_query", message),
    };
    match recipes_list_view(&state, &user, &headers, &query).await {
        Ok(data) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "private, no-store")],
            Json(ok(data)),
        )
            .into_response(),
        Err(response) => response,
    }
}

/// `browser_evidence.save_recipe` with `{ sessionId, name, description? }` on the generic action
/// surface (Q4-E4.2-4a). Answers 404 `browser_recipes_unavailable` while `BROWSER_RECIPES` is off.
/// The envelope matches the other human actions.
pub(crate) async fn dispatch_save_recipe(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    if !state.browser_recipes {
        return recipes_unavailable();
    }
    let Some(session_id) = input
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return bad_request(
            "invalid_input",
            "browser_evidence.save_recipe requires a 'sessionId'",
        );
    };
    if !is_valid_path_segment(session_id) {
        return invalid_session();
    }
    let (name, description) =
        match recipe_meta_of(input, &["sessionId", "name", "description"]) {
            Ok(meta) => meta,
            Err(message) => return invalid_recipe(message),
        };
    match save_recipe_view(state, user, headers, session_id, name, description).await {
        Ok(result) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "private, no-store")],
            Json(ok(json!({
                "actionId": SAVE_RECIPE_ACTION_ID,
                "runId": format!("{}_{}", SAVE_RECIPE_ACTION_ID, user.user_id),
                "status": "completed",
                "auditId": format!("audit_{}_{}", SAVE_RECIPE_ACTION_ID, user.user_id),
                "result": result,
            }))),
        )
            .into_response(),
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

/// The action id of the region attach on the generic action surface (QF-7).
const REGION_ATTACH_ACTION_ID: &str = "browser_region.attach";

/// The action surface's answer while `BROWSER_REGION_ATTACH` is off. The HTTP route is absent then.
fn region_attach_unavailable() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "browser_region_attach_unavailable",
            "Attaching a browser page region is not enabled.",
        )),
    )
        .into_response()
}

/// `browser_region.attach` with `{ sessionId, tabId, region: { x, y, width, height },
/// image: { mimeType, dataBase64 }, selectionText? }` on the generic action surface (QF-7).
/// Human-only (the model-access map exempts it). Unknown keys are refused, and the rest of
/// the input goes through the same `region_attach_view` the route uses, so the owner gate,
/// the ZDR refusal and the tab and region re-resolution are identical. Nothing is stored.
/// Answers 404 `browser_region_attach_unavailable` while `BROWSER_REGION_ATTACH` is off.
pub(crate) async fn dispatch_region_attach(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    if !state.browser_region_attach {
        return region_attach_unavailable();
    }
    let Some(object) = input.as_object() else {
        return bad_request(
            "invalid_input",
            "browser_region.attach input must be an object",
        );
    };
    let Some(session_id) = object
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return bad_request(
            "invalid_input",
            "browser_region.attach requires a 'sessionId'",
        );
    };
    if !is_valid_path_segment(session_id) {
        return invalid_session();
    }
    let mut body = object.clone();
    body.remove("sessionId");
    let body: RegionAttachBody = match serde_json::from_value(Value::Object(body)) {
        Ok(body) => body,
        Err(_) => {
            return bad_request(
                "invalid_input",
                "browser_region.attach input must be sessionId, tabId, region, image and an optional selectionText only.",
            )
        }
    };
    match region_attach_view(state, user, headers, session_id, body).await {
        Ok(result) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "private, no-store")],
            Json(ok(json!({
                "actionId": REGION_ATTACH_ACTION_ID,
                "runId": format!("{}_{}", REGION_ATTACH_ACTION_ID, user.user_id),
                "status": "completed",
                "auditId": format!("audit_{}_{}", REGION_ATTACH_ACTION_ID, user.user_id),
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
            .route(REGION_ATTACH_PATH, post(attach_session_region))
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

    // -----------------------------------------------------------------------
    // Region and selection attach (E3.3-d)
    // -----------------------------------------------------------------------

    const REGION_URI: &str = "/api/v1/browser/sessions/run-1/attachments/region";

    /// The first 24 bytes of a PNG: signature and IHDR with the given size.
    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    /// SOI, an APP0 segment, then a baseline SOF0 with the given size.
    fn jpeg_bytes(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        bytes.extend_from_slice(b"JFIF\0");
        bytes.extend_from_slice(&[1, 1, 0, 0, 1, 0, 1, 0, 0]);
        bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        bytes
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> RegionRectInput {
        RegionRectInput {
            x,
            y,
            width,
            height,
        }
    }

    fn region_body(tab_id: &str, image: &[u8], extra: Option<(&str, Value)>) -> String {
        let mut body = json!({
            "tabId": tab_id,
            "region": { "x": 10.4, "y": 20, "width": 200, "height": 100 },
            "image": { "mimeType": "image/png", "dataBase64": BASE64_STANDARD.encode(image) },
            "selectionText": "Total due\u{0007}: 42 NOK"
        });
        if let Some((key, value)) = extra {
            body[key] = value;
        }
        body.to_string()
    }

    /// Quarry knows run-1 for the caller, with a 1280x720 viewport.
    async fn mount_owned_run_with_viewport(quarry: &MockServer, zdr: Value) {
        let mut data = json!({
            "runId": "run-1",
            "controlMode": "agent_control",
            "viewport": { "width": 1280, "height": 720 }
        });
        if !zdr.is_null() {
            data["zdr"] = zdr;
        }
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/agent/runs/run-1/browser-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": data })))
            .mount(quarry)
            .await;
    }

    async fn mount_tabs(quarry: &MockServer, tabs: Value, expected_calls: u64) {
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/agent/runs/run-1/tabs"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "data": { "tabs": tabs } })),
            )
            .expect(expected_calls)
            .mount(quarry)
            .await;
    }

    #[test]
    fn region_attach_flag_defaults_off() {
        assert!(!region_attach_flag_on(None));
        assert!(!region_attach_flag_on(Some("")));
        assert!(!region_attach_flag_on(Some("0")));
        assert!(!region_attach_flag_on(Some("false")));
        assert!(region_attach_flag_on(Some("1")));
        assert!(region_attach_flag_on(Some(" TRUE ")));
    }

    #[test]
    fn region_is_snapped_outward_and_clamped_to_the_viewport() {
        assert_eq!(
            resolve_region(rect(10.4, 20.0, 200.0, 100.0), 1280, 720),
            Ok(RegionRect {
                x: 10,
                y: 20,
                width: 201,
                height: 100
            })
        );
        assert_eq!(
            resolve_region(rect(-50.0, 700.0, 100.0, 100.0), 1280, 720),
            Ok(RegionRect {
                x: 0,
                y: 700,
                width: 50,
                height: 20
            })
        );
    }

    #[test]
    fn region_outside_the_viewport_or_degenerate_is_refused() {
        let cases = [
            rect(f64::NAN, 0.0, 10.0, 10.0),
            rect(0.0, 0.0, f64::INFINITY, 10.0),
            rect(0.0, 0.0, 0.0, 10.0),
            rect(0.0, 0.0, 10.0, -5.0),
            rect(1300.0, 10.0, 50.0, 50.0),
            rect(10.0, -100.0, 50.0, 50.0),
            rect(0.0, 0.0, 4.0, 40.0),
        ];
        for case in cases {
            assert!(resolve_region(case, 1280, 720).is_err(), "{case:?} is refused");
        }
    }

    #[test]
    fn image_dimensions_come_from_the_bytes_not_the_mime_type() {
        assert_eq!(png_dimensions(&png_bytes(201, 100)), Some((201, 100)));
        assert_eq!(jpeg_dimensions(&jpeg_bytes(402, 200)), Some((402, 200)));
        assert_eq!(png_dimensions(&jpeg_bytes(10, 10)), None);
        assert_eq!(jpeg_dimensions(&png_bytes(10, 10)), None);
        assert_eq!(png_dimensions(&png_bytes(0, 10)), None);
        assert_eq!(jpeg_dimensions(&[0xFF, 0xD8, 0xFF, 0xDA, 0, 2]), None);

        let mislabelled = RegionImageInput {
            mime_type: "image/jpeg".to_owned(),
            data_base64: BASE64_STANDARD.encode(png_bytes(10, 10)),
        };
        assert!(decode_region_image(&mislabelled).is_err());
        let svg = RegionImageInput {
            mime_type: "image/svg+xml".to_owned(),
            data_base64: BASE64_STANDARD.encode(b"<svg/>"),
        };
        assert!(decode_region_image(&svg).is_err());
        let too_large = RegionImageInput {
            mime_type: "image/png".to_owned(),
            data_base64: BASE64_STANDARD.encode(vec![0u8; REGION_IMAGE_MAX_BYTES + 1]),
        };
        assert!(decode_region_image(&too_large).is_err());
    }

    #[test]
    fn image_must_have_the_shape_of_the_resolved_region() {
        let region = RegionRect {
            x: 0,
            y: 0,
            width: 201,
            height: 100,
        };
        assert!(image_matches_region(201, 100, region));
        assert!(image_matches_region(402, 200, region), "a 2x frame");
        assert!(image_matches_region(100, 50, region), "a downscaled frame");
        assert!(!image_matches_region(100, 100, region), "wrong shape");
        assert!(!image_matches_region(1206, 600, region), "more than 3x");
        assert!(!image_matches_region(0, 100, region));
    }

    #[test]
    fn selection_text_is_cleaned_and_capped() {
        assert_eq!(sanitize_selection(None), Ok(None));
        assert_eq!(sanitize_selection(Some("  \u{0}  ")), Ok(None));
        assert_eq!(
            sanitize_selection(Some(" a\u{0007}b\nc\td ")),
            Ok(Some("a b\nc\td".to_owned()))
        );
        assert!(sanitize_selection(Some("x".repeat(SELECTION_MAX_CHARS + 1).as_str())).is_err());
    }

    #[test]
    fn attach_tab_resolution_fails_closed() {
        let tabs = vec![
            json!({ "tabId": "tab_a", "active": true }),
            json!({ "tabId": "tab_b", "active": false }),
            json!({ "tabId": "tab_c", "active": true }),
            json!({ "tabId": "tab_c", "active": true }),
        ];
        assert!(resolve_attach_tab(&tabs, "tab_a").is_ok());
        assert_eq!(resolve_attach_tab(&tabs, "tab_b").unwrap_err(), "tab-not-active");
        assert_eq!(resolve_attach_tab(&tabs, "tab_c").unwrap_err(), "ambiguous-tab");
        assert_eq!(resolve_attach_tab(&tabs, "tab_z").unwrap_err(), "unknown-tab");
        assert_eq!(resolve_attach_tab(&[], "tab_a").unwrap_err(), "no-tabs");
    }

    #[test]
    fn attach_target_drops_query_fragment_and_credentials() {
        assert_eq!(
            safe_attach_target(Some("https://user:pw@example.com/a/b?token=secret#frag")),
            Some("https://example.com/a/b".to_owned())
        );
        assert_eq!(safe_attach_target(Some("about:blank")), None);
        assert_eq!(safe_attach_target(Some("javascript:alert(1)")), None);
        assert_eq!(safe_attach_target(None), None);
    }

    #[tokio::test]
    async fn region_route_is_not_mounted_with_the_flag_off() {
        let state = test_app_state();
        let app = router_with(state.clone(), false).with_state(state);
        let (status, _) = send(app, "POST", REGION_URI, &region_body("tab_a", &png_bytes(201, 100), None)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn region_attach_resolves_the_live_tab_and_classifies_browser_scraped() {
        let quarry = MockServer::start().await;
        mount_owned_run_with_viewport(&quarry, json!(false)).await;
        mount_tabs(
            &quarry,
            json!([
                { "tabId": "tab_a", "title": "Invoice\u{0007} 42", "url": "https://example.com/inv?token=secret#x", "active": true },
                { "tabId": "tab_b", "title": null, "url": null, "active": false }
            ]),
            1,
        )
        .await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();

        let (status, body) = send(
            evidence_app(state),
            "POST",
            REGION_URI,
            &region_body("tab_a", &png_bytes(402, 200), None),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let attachment = &body["data"]["attachment"];
        assert_eq!(attachment["kind"], "browser-region");
        assert_eq!(attachment["trustClass"], "browser-scraped");
        assert_eq!(attachment["grant"], "none");
        assert_eq!(attachment["retention"], "ephemeral");
        assert_eq!(attachment["tabId"], "tab_a");
        assert_eq!(attachment["target"], "https://example.com/inv");
        assert_eq!(attachment["label"], "Invoice 42");
        assert_eq!(attachment["region"], json!({ "x": 10, "y": 20, "width": 201, "height": 100 }));
        assert_eq!(attachment["viewport"], json!({ "width": 1280, "height": 720 }));
        assert_eq!(attachment["image"]["width"], 402);
        assert_eq!(attachment["selection"]["text"], "Total due : 42 NOK");
        assert!(!body.to_string().contains("token=secret"), "{body}");
    }

    #[tokio::test]
    async fn region_attach_is_refused_for_zdr_without_reading_tabs() {
        for zdr in [json!(true), Value::Null] {
            let quarry = MockServer::start().await;
            mount_owned_run_with_viewport(&quarry, zdr.clone()).await;
            mount_tabs(&quarry, json!([{ "tabId": "tab_a", "active": true }]), 0).await;
            let mut state = test_app_state();
            state.quarry_edge_url = quarry.uri();

            let (status, body) = send(
                evidence_app(state),
                "POST",
                REGION_URI,
                &region_body("tab_a", &png_bytes(201, 100), None),
            )
            .await;
            assert_eq!(status, StatusCode::CONFLICT, "zdr {zdr}: {body}");
            assert_eq!(body["error"]["code"], "browser_attachment_refused_zdr");
        }
    }

    #[tokio::test]
    async fn region_attach_refuses_a_tab_the_frame_does_not_show() {
        let quarry = MockServer::start().await;
        mount_owned_run_with_viewport(&quarry, json!(false)).await;
        mount_tabs(
            &quarry,
            json!([
                { "tabId": "tab_a", "active": true },
                { "tabId": "tab_b", "active": false }
            ]),
            1,
        )
        .await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();

        let (status, body) = send(
            evidence_app(state),
            "POST",
            REGION_URI,
            &region_body("tab_b", &png_bytes(201, 100), None),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["error"]["code"], "browser_attachment_tab_unresolved");
        assert_eq!(body["error"]["details"]["reason"], "tab-not-active");
    }

    #[tokio::test]
    async fn region_attach_refuses_a_foreign_session_and_bad_input_before_tabs() {
        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/agent/runs/run-1/browser-session"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "error": { "code": "NOT_FOUND", "message": "agent run not found" }
            })))
            .mount(&quarry)
            .await;
        mount_tabs(&quarry, json!([{ "tabId": "tab_a", "active": true }]), 0).await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();

        let (status, _) = send(
            evidence_app(state.clone()),
            "POST",
            REGION_URI,
            &region_body("tab_a", &png_bytes(201, 100), None),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "owner gated");

        let (status, body) = send(
            evidence_app(state.clone()),
            "POST",
            REGION_URI,
            &region_body("tab_a", &png_bytes(201, 100), Some(("orgId", json!("org_b")))),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "unknown keys are refused: {body}");

        let (status, body) = send(
            evidence_app(state),
            "POST",
            REGION_URI,
            &region_body("tab/../a", &png_bytes(201, 100), None),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["code"], "invalid_input");
    }

    #[tokio::test]
    async fn region_attach_refuses_an_image_of_another_shape() {
        let quarry = MockServer::start().await;
        mount_owned_run_with_viewport(&quarry, json!(false)).await;
        mount_tabs(&quarry, json!([{ "tabId": "tab_a", "active": true }]), 0).await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();

        let (status, body) = send(
            evidence_app(state),
            "POST",
            REGION_URI,
            &region_body("tab_a", &png_bytes(300, 300), None),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["code"], "invalid_browser_attachment");
    }
}
