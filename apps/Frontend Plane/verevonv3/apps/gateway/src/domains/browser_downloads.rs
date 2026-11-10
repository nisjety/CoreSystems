//! Q3-E3.4-7b: the Frontend Plane's door to a browser session's governed downloads.
//!
//! Quarry (Ingestion Plane) stores a file the browser downloaded as a tenant-owned artifact when the
//! download passes its type allow-list (E3.4-4/6), and exposes two owner routes (Q3-E3.4-7a):
//!
//!   GET  /v1/agent/runs/{run_id}/downloads
//!   POST /v1/agent/runs/{run_id}/downloads/{artifact_id}/knowledge
//!
//! This module is the BFF side of both:
//!
//! * `GET /api/v1/browser/sessions/{session_id}/downloads` lists the session's admitted downloads
//!   (artifact id, type, size, digest, hand-off state). It carries no URL and no file name.
//! * [`send_to_knowledge`] backs the `browser_download.send_to_knowledge` action: it hands one of
//!   those files to Data Plane through Quarry, which parses it and writes its text under the
//!   caller's org. The file itself never leaves Quarry.
//!
//! Identity and ownership: the session id is whatever the person's browser reported, so it is never
//! trusted on its own. Both paths first read Quarry's owner projection of the session
//! (`owner_browser_session_projection`), which answers 404 for an id the verified caller does not own.
//! The org and user come from the verified token Quarry is called with, never from a path, query or
//! body field. Quarry checks ownership again on its own routes.
//!
//! Zero data retention: a ZDR session keeps no download. The list answers an empty list and the
//! hand-off answers `403 zdr_feature_not_supported` naming the feature, before anything goes
//! upstream. A projection that does not say `zdr: false` is treated as ZDR (fail closed).
//!
//! Off by default: both are served only when `AppState.browser_downloads` is set, which is read once
//! from `BROWSER_DOWNLOADS` (exactly `true`) when the state is built. While it is off both answer
//! 404 `browser_downloads_unavailable` and nothing goes upstream. Quarry has its own switches
//! (`QUARRY_EDGE__DOWNLOADS`, `QUARRY_EDGE__DOWNLOAD_HANDOFF`).
//!
//! Upstream bodies are never relayed: every failure is re-keyed into the gateway's
//! `{ error: { code, message } }` envelope with a message written here.

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use super::browser::{
    cookie_header, is_valid_artifact_id, is_valid_path_segment, owner_browser_session_projection,
    quarry_call, quarry_token,
};
use crate::{
    config::AppState,
    envelope::{error, ok, unwrap_data},
    middleware::AuthenticatedUser,
};

/// The feature key the zero-retention refusal names.
pub(crate) const ZDR_FEATURE: &str = "browser_download";

/// The most downloads one list answer carries (the same cap Quarry applies).
const MAX_LISTED_DOWNLOADS: usize = 64;

/// The longest `idempotency_key` Quarry accepts, in characters.
const MAX_IDEMPOTENCY_KEY_CHARS: usize = 128;

/// The admitted types, as the sanitised extension Quarry reports them with.
const ADMITTED_TYPES: [&str; 4] = ["pdf", "txt", "md", "csv"];

/// The MIME types of the admitted types.
const ADMITTED_MIME_TYPES: [&str; 4] =
    ["application/pdf", "text/plain", "text/markdown", "text/csv"];

/// Where a listed download stands with the hand-off, as Quarry reports it.
const HANDOFF_STATES: [&str; 2] = ["not_handed_off", "handed_off"];

/// 404 while the BFF switch is off: the feature does not exist on this gateway.
pub(crate) fn unavailable_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "browser_downloads_unavailable",
            "Browser downloads are not enabled.",
        )),
    )
        .into_response()
}

/// The zero-retention refusal, in the catalogue shape the SPA reads (`feature` names what was
/// refused).
fn zdr_refusal_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": {
                "code": "zdr_feature_not_supported",
                "message": "This is not supported with zero data retention yet: sending a browser download to the knowledge base. The request was refused before anything was sent.",
                "retryable": false,
                "action": "turn_off_feature",
                "feature": ZDR_FEATURE,
            }
        })),
    )
        .into_response()
}

fn not_found_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "browser_download_not_found",
            "The browser download is not available.",
        )),
    )
        .into_response()
}

fn failed_response() -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(error(
            "browser_download_failed",
            "The browser download could not be processed.",
        )),
    )
        .into_response()
}

fn invalid_ids_response() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error(
            "invalid_browser_download",
            "The requested browser session or download id is invalid.",
        )),
    )
        .into_response()
}

/// A session projection says the session is not zero-retention only when it carries `zdr: false`.
fn projection_allows_content(projection: &Value) -> bool {
    projection.get("zdr").and_then(Value::as_bool) == Some(false)
}

fn lower_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// One listed download, re-keyed to the SPA's spelling. Every field is checked against a closed set
/// or a shape; an entry that does not pass is dropped, and nothing else Quarry sent is kept.
fn list_entry(raw: &Value) -> Option<Value> {
    let artifact_id = raw.get("artifact_id").and_then(Value::as_str)?;
    if !is_valid_artifact_id(artifact_id) {
        return None;
    }
    let mime_type = raw.get("mime_type").and_then(Value::as_str)?;
    if !ADMITTED_MIME_TYPES.contains(&mime_type) {
        return None;
    }
    let admitted_type = raw.get("admitted_type").and_then(Value::as_str)?;
    if !ADMITTED_TYPES.contains(&admitted_type) {
        return None;
    }
    let handoff_state = raw.get("handoff_state").and_then(Value::as_str)?;
    if !HANDOFF_STATES.contains(&handoff_state) {
        return None;
    }
    let sha256 = raw.get("sha256").and_then(Value::as_str)?;
    if !lower_hex_sha256(sha256) {
        return None;
    }
    let byte_size = raw.get("byte_size").and_then(Value::as_u64)?;
    Some(json!({
        "artifactId": artifact_id,
        "mimeType": mime_type,
        "sizeBytes": byte_size,
        "sha256": sha256,
        "admittedType": admitted_type,
        "handoffState": handoff_state,
    }))
}

/// The list answer's `data`, from Quarry's `data` array.
fn list_entries(body: &Value) -> Vec<Value> {
    unwrap_data(body)
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(list_entry)
                .take(MAX_LISTED_DOWNLOADS)
                .collect()
        })
        .unwrap_or_default()
}

/// `GET /api/v1/browser/sessions/{session_id}/downloads`: the admitted downloads of a session the
/// caller owns. 404 while `BROWSER_DOWNLOADS` is off; 404 for a session the caller does not own;
/// an empty list for a zero-retention session (nothing is kept) and when Quarry holds none.
pub(crate) async fn list_session_downloads(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Response {
    if !state.browser_downloads {
        return unavailable_response();
    }
    if !is_valid_path_segment(&session_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_session",
                "The requested browser session id is invalid.",
            )),
        )
            .into_response();
    }
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let projection = match owner_browser_session_projection(
        &state,
        &user,
        token.as_deref(),
        &session_id,
    )
    .await
    {
        Ok(projection) => projection,
        Err(response) => return response,
    };
    if !projection_allows_content(&projection) {
        return (StatusCode::OK, Json(ok(Vec::<Value>::new()))).into_response();
    }
    let (status, body) = quarry_call(
        &state,
        Method::GET,
        &format!(
            "/v1/agent/runs/{}/downloads",
            urlencoding::encode(&session_id)
        ),
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if status.is_success() {
        return (StatusCode::OK, Json(ok(list_entries(&body)))).into_response();
    }
    // Quarry knows no receipts for a session that has not downloaded anything: nothing to list.
    if status == StatusCode::NOT_FOUND {
        return (StatusCode::OK, Json(ok(Vec::<Value>::new()))).into_response();
    }
    if is_downloads_disabled(&body) {
        return unavailable_response();
    }
    failed_list_response()
}

fn failed_list_response() -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(error(
            "browser_downloads_failed",
            "The browser downloads could not be listed.",
        )),
    )
        .into_response()
}

/// Quarry answers `403` with the message `downloads_disabled` while `QUARRY_EDGE__DOWNLOADS` is off.
fn is_downloads_disabled(body: &Value) -> bool {
    body.pointer("/error/message").and_then(Value::as_str) == Some("downloads_disabled")
}

/// The stable reason Quarry writes into a hand-off refusal's details
/// (`DownloadHandoffRejection::reason`).
fn upstream_reason(body: &Value) -> Option<&str> {
    body.pointer("/error/details/reason")
        .and_then(Value::as_str)
}

/// Quarry's refusal of a hand-off, as the gateway's typed envelope. Nothing of the upstream body is
/// relayed: the code and message are the gateway's own.
fn handoff_failure(status: StatusCode, body: &Value) -> Response {
    match upstream_reason(body) {
        Some("download_handoff_zdr_refused") => return zdr_refusal_response(),
        Some("download_handoff_disabled") => {
            return (
                StatusCode::NOT_IMPLEMENTED,
                Json(error(
                    "browser_download_handoff_disabled",
                    "Sending a browser download to the knowledge base is not enabled.",
                )),
            )
                .into_response()
        }
        Some("download_handoff_unsupported_type") => {
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Json(error(
                    "browser_download_unsupported_type",
                    "This file type cannot be sent to the knowledge base.",
                )),
            )
                .into_response()
        }
        Some("download_handoff_no_text") => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(error(
                    "browser_download_no_text",
                    "The file has no text to send to the knowledge base.",
                )),
            )
                .into_response()
        }
        Some("download_handoff_too_large") => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(error(
                    "browser_download_too_large",
                    "The file's text is too large to send to the knowledge base.",
                )),
            )
                .into_response()
        }
        // Another org's artifact reads as not found: the route never confirms a foreign id.
        Some("download_handoff_org_mismatch") => return not_found_response(),
        Some(_) => return failed_response(),
        None => {}
    }
    if is_downloads_disabled(body) {
        return unavailable_response();
    }
    if status == StatusCode::NOT_FOUND {
        return not_found_response();
    }
    failed_response()
}

/// Quarry's `{ data: { document_id, created } }`, as the action's output. Quarry reports no
/// indexing state: the document is indexed after the write, so it is `pending` here.
fn handoff_success(body: &Value) -> Response {
    let data = unwrap_data(body);
    let Some(document_id) = data
        .get("document_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty() && id.len() <= 128)
    else {
        return failed_response();
    };
    let created = data.get("created").and_then(Value::as_bool).unwrap_or(true);
    (
        StatusCode::OK,
        Json(ok(json!({
            "documentId": document_id,
            "indexStatus": "pending",
            "created": created,
        }))),
    )
        .into_response()
}

/// The request key sent to Quarry: the caller's when it is usable, otherwise one derived from the
/// two ids, so a retry of the same file is the same request. Quarry dedupes on artifact and digest
/// either way; the key is a label.
fn handoff_key(session_id: &str, artifact_id: &str, supplied: &str) -> String {
    let supplied = supplied.trim();
    if !supplied.is_empty()
        && supplied.chars().count() <= MAX_IDEMPOTENCY_KEY_CHARS
        && !supplied.chars().any(char::is_control)
    {
        return supplied.to_owned();
    }
    format!("browser-download:{session_id}:{artifact_id}")
}

/// Hands one admitted download of the caller's own session to Data Plane (through Quarry).
///
/// Order: the switch (404), the id shapes (400), the owner projection of the session (404 for a
/// session the caller does not own), then zero retention (403 naming the feature) before anything
/// is sent, then Quarry's hand-off route with the user's Quarry token. Quarry's typed refusals
/// (`zdr_refused`, `disabled`, `unsupported_type`, `no_text`, `org_mismatch`, ...) are re-keyed by
/// [`handoff_failure`].
pub(crate) async fn send_to_knowledge(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    session_id: &str,
    artifact_id: &str,
    idempotency_key: &str,
) -> Response {
    if !state.browser_downloads {
        return unavailable_response();
    }
    if !is_valid_path_segment(session_id) || !is_valid_artifact_id(artifact_id) {
        return invalid_ids_response();
    }
    let cookie = cookie_header(headers);
    let token = quarry_token(state, user, &cookie).await;
    let projection =
        match owner_browser_session_projection(state, user, token.as_deref(), session_id).await {
            Ok(projection) => projection,
            Err(response) => return response,
        };
    if !projection_allows_content(&projection) {
        return zdr_refusal_response();
    }
    let (status, body) = quarry_call(
        state,
        Method::POST,
        &format!(
            "/v1/agent/runs/{}/downloads/{}/knowledge",
            urlencoding::encode(session_id),
            urlencoding::encode(artifact_id)
        ),
        Some(json!({
            "idempotency_key": handoff_key(session_id, artifact_id, idempotency_key),
        })),
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if status.is_success() {
        return handoff_success(&body);
    }
    handoff_failure(status, &body)
}
