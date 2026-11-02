//! Room files: the BFF half of WS14-S7 (routes R1 to R6 of the WS14 plan).
//!
//! `POST /api/v1/spaces/{space_ref}/files` (R1) streams a multipart upload to Ingestion
//! (imports-core `space-upload`) under a Control-issued `space_upload` import decision.
//! `GET .../files/uploads/{upload_id}` (R2) reads the job back, `GET .../files` (R3) lists the
//! room's files, `GET .../files/{document_id}/text` (R4) reads one file's extracted text and
//! `DELETE .../files/{document_id}` (R5) removes one. `features_json` and `limits_json` are the
//! `room_files` blocks of `GET /api/v1/spaces/{space_ref}/context` (R6).
//!
//! This module stores nothing and decides nothing about authority. The bytes stream through (no
//! copy is kept here, in Dragonfly or in a log), the Control decision is requested per call and
//! relayed verbatim, and Data Plane (retrieval-engine, documents-api) and Ingestion enforce
//! ownership, quota, dedupe and the typed validation refusals. The flags are rollout controls,
//! never authorization inputs.
//!
//! Order of refusals on R1 (each stops before the next): flag, organization, ids, multipart
//! content type, idempotency key, Zero Data Retention (typed 412, before any Control call),
//! declared length, Space lifecycle, the Control import decision, then the upstream call.
//! Errors are `{error:{code,message,details}}`: 400 `invalid_space` / `invalid_document` /
//! `invalid_upload` / `idempotency_key_invalid` / `invalid_limit` / `invalid_cursor`, 403
//! `active_membership_required` / `space_files_not_authorized` (`details.reason_code`), 404
//! `room_files_disabled` / `space_not_found` / `file_not_found` / `upload_not_found`, 411
//! `upload_length_required`, 412 `zdr_durable_mutation_forbidden` (`details.surface:"room_files"`),
//! 413 `room_file_too_large`, 422 `upload_requires_multipart`, 503 `space_lifecycle_unavailable`
//! / `imports_auth_unavailable` / `room_files_unavailable`. Imports-core's own typed refusals
//! (`room_file_*`, `room_files_quota_exceeded`) pass through unchanged.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{
    extract::{Extension, Path, Query, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{
    denial::{self, Decision},
    invalid_decision, required_decision_string, space_lifecycle_by_ref,
    space_retrieval_decision_typed,
};
use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_bearer_json, proxy_json},
    zdr::durable_write_refusal,
};

type Reply = (StatusCode, Json<Value>);

/// The one import source type a room file carries (Control's shared allow-list).
const SOURCE_TYPE: &str = "space_upload";
const MAX_FILES: u32 = 10;
/// Per file, mirrors imports-core `SPACE_UPLOAD_MAX_FILE_MB` (25), which enforces it.
const MAX_FILE_BYTES: u64 = 25 * 1024 * 1024;
/// Whole request, equal to nginx `client_max_body_size 100m` (`nginx.conf`).
const MAX_REQUEST_BYTES: u64 = 100 * 1024 * 1024;
/// Mirrors imports-core `SPACE_UPLOAD_ALLOWED_TYPES`, which enforces it.
const ALLOWED_EXTENSIONS: [&str; 8] = ["pdf", "docx", "xlsx", "pptx", "txt", "md", "csv", "json"];
/// Per-turn inline text caps the composer applies (WS14-S8).
const TURN_INLINE_CHARS: u32 = 60_000;
const TURN_TOTAL_CHARS: u32 = 120_000;
const DEFAULT_TEXT_CHARS: usize = 100_000;
const MAX_TEXT_CHARS: usize = 1_000_000;
const LIST_DEFAULT_LIMIT: i64 = 50;
const LIST_MAX_LIMIT: i64 = 200;
const MAX_ID_LEN: usize = 200;
const MAX_CURSOR_LEN: usize = 256;
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/spaces/{space_ref}/files",
            post(upload_files).get(list_files),
        )
        .route(
            "/api/v1/spaces/{space_ref}/files/uploads/{upload_id}",
            get(get_upload),
        )
        .route(
            "/api/v1/spaces/{space_ref}/files/{document_id}/text",
            get(read_file_text),
        )
        .route(
            "/api/v1/spaces/{space_ref}/files/{document_id}",
            axum::routing::delete(delete_file),
        )
}

/// R6: the `features.room_files` block of the Space context. Rollout controls only.
pub(super) fn features_json(state: &AppState) -> Value {
    let flags = &state.space_features;
    json!({
        "turn_attachments": flags.turn_attachments_ui,
        "durable": flags.room_files_api,
        "files_tab": flags.room_files_api && flags.room_files_ui,
    })
}

/// R6: the `limits.room_files` block of the Space context.
pub(super) fn limits_json() -> Value {
    json!({
        "max_files": MAX_FILES,
        "max_file_bytes": MAX_FILE_BYTES,
        "max_request_bytes": MAX_REQUEST_BYTES,
        "allowed_extensions": ALLOWED_EXTENSIONS,
        "turn_inline_chars": TURN_INLINE_CHARS,
        "turn_total_chars": TURN_TOTAL_CHARS,
    })
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn feature_off() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "room_files_disabled",
        "Room files are not enabled.",
    )
}

fn unavailable() -> Reply {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "room_files_unavailable",
        "Room files are temporarily unavailable. Nothing was changed.",
    )
}

fn file_not_found() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "file_not_found",
        "The file is not available.",
    )
}

fn no_store(reply: Reply) -> Response {
    let mut response = reply.into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_LEN
        && !value.chars().any(|c| c.is_control() || c == '/')
}

/// `reason_code` for `space_files_not_authorized`: a lowercase token from Control's `code`
/// (top level or inside `error`), never Control's message.
fn reason_code(body: &Value) -> String {
    let candidate = body
        .get("code")
        .and_then(Value::as_str)
        .or_else(|| body.pointer("/error/code").and_then(Value::as_str))
        .unwrap_or("not_authorized");
    if !candidate.is_empty()
        && candidate.len() <= 64
        && candidate
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        candidate.to_owned()
    } else {
        "not_authorized".to_owned()
    }
}

fn not_authorized(reason: &str) -> Reply {
    (
        StatusCode::FORBIDDEN,
        Json(error_with_details(
            "space_files_not_authorized",
            "This room's files are not available to you.",
            json!({ "reason_code": reason }),
        )),
    )
}

/// A declined Control decision as `space_files_not_authorized`.
fn declined(denial: Option<denial::SpaceDenial>) -> Reply {
    let reason = denial
        .as_ref()
        .and_then(|d| d.details_value().get("reason").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| "not_authorized".to_owned());
    not_authorized(&reason)
}

fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

/// Flag, organization and Space reference: everything that needs no upstream call.
fn pre_gate(
    state: &AppState,
    org_id: String,
    space_ref: &str,
) -> Result<(String, String), Reply> {
    if !state.space_features.room_files_api {
        return Err(feature_off());
    }
    if org_id.trim().is_empty() {
        return Err(refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to use room files.",
        ));
    }
    let space_ref = space_ref.trim();
    if !valid_id(space_ref) {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A Space reference is required.",
        ));
    }
    Ok((org_id, space_ref.to_owned()))
}

async fn lifecycle_gate(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Result<(), Reply> {
    match space_lifecycle_by_ref(state, user, org_id, space_ref).await {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(refuse(
            StatusCode::NOT_FOUND,
            "space_not_found",
            "Space is not available.",
        )),
        Err(()) => Err(refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_lifecycle_unavailable",
            "Space lifecycle is unavailable.",
        )),
    }
}

/// Flag, organization, reference and an active Space.
async fn gate(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
) -> Result<(String, String), Reply> {
    let org_id = authorized_org_id(state, user).await;
    let (org_id, space_ref) = pre_gate(state, org_id, space_ref)?;
    lifecycle_gate(state, user, &org_id, &space_ref).await?;
    Ok((org_id, space_ref))
}

/// Control's retrieval decision for a read. An outage is a typed 503, never Control's body.
async fn read_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Result<Decision<String>, Reply> {
    match space_retrieval_decision_typed(state, user, org_id, space_ref).await {
        Ok(decision) => Ok(decision),
        Err((status, _)) if status.is_server_error() => Err(unavailable()),
        Err((_, Json(body))) => Err(not_authorized(&reason_code(&body))),
    }
}

// ---------------------------------------------------------------------------------------------
// R1: upload
// ---------------------------------------------------------------------------------------------

fn valid_idempotency_key(value: &str) -> bool {
    (8..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'~' | b'-'))
}

/// Control's `import-decision` for a `space_upload`, checked against the request before the token
/// is relayed (the same field checks `personal_import_ingress_decision` makes).
async fn shared_import_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    idempotency_key: &str,
) -> Result<String, Reply> {
    let actor = actor_for(user);
    let url = format!(
        "{}/api/v1/internal/spaces/import-decision",
        state.user_core_url
    );
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_ref": space_ref,
            "source_type": SOURCE_TYPE,
            "idempotency_key": idempotency_key,
        })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    let status = denial::refusal_status(status, &response);
    if status == StatusCode::FORBIDDEN {
        return Err(not_authorized(&reason_code(&response)));
    }
    if status == StatusCode::NOT_FOUND {
        // Control's shared import is off (or this Control predates it): the feature is not there.
        return Err(feature_off());
    }
    if !status.is_success() {
        return Err(unavailable());
    }
    let data = unwrap_data(&response);
    let fail = |_: Reply| not_authorized("invalid_decision");
    let decision = data
        .get("decision")
        .and_then(Value::as_object)
        .ok_or_else(|| fail(invalid_decision()))?;
    let token = data
        .get("token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(invalid_decision)?;
    let matches = required_decision_string(decision, "space_ref")? == space_ref
        && required_decision_string(decision, "service_audience")? == "ingestion-plane-import"
        && required_decision_string(decision, "action_id")? == "ingestion.import.write"
        && required_decision_string(decision, "action_schema_hash")? == "sha256:ingestion-import-v1"
        && required_decision_string(decision, "idempotency_key")? == idempotency_key
        && required_decision_string(decision, "import_source_type")? == SOURCE_TYPE;
    if !matches {
        return Err(invalid_decision());
    }
    Ok(token.to_owned())
}

/// Typed ZDR refusal with the gateway-local surface label (the catalogue code is feature-less).
fn zdr_refusal(reply: Reply) -> Reply {
    let (status, Json(mut body)) = reply;
    if status == StatusCode::PRECONDITION_FAILED {
        if let Some(object) = body.get_mut("error").and_then(Value::as_object_mut) {
            object.insert("details".to_owned(), json!({ "surface": "room_files" }));
        }
    }
    (status, Json(body))
}

async fn upload_files(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    request: Request,
) -> Response {
    let (parts, body) = request.into_parts();
    match upload_core(&state, &user, &space_ref, &parts.headers, body).await {
        Ok(response) => response,
        Err(reply) => reply.into_response(),
    }
}

async fn upload_core(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    headers: &HeaderMap,
    body: axum::body::Body,
) -> Result<Response, Reply> {
    let org_id = authorized_org_id(state, user).await;
    let (org_id, space_ref) = pre_gate(state, org_id, space_ref)?;
    // ZDR first among the content checks: a Zero Data Retention organization never reaches
    // Control, Application or Ingestion with a file.
    if let Some(refusal) = durable_write_refusal(state, user, Some(headers), None).await {
        return Err(zdr_refusal(refusal));
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| value.to_ascii_lowercase().starts_with("multipart/form-data"))
        .ok_or_else(|| {
            refuse(
                StatusCode::UNPROCESSABLE_ENTITY,
                "upload_requires_multipart",
                "Files are uploaded as multipart form data.",
            )
        })?
        .to_owned();
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| valid_idempotency_key(value))
        .ok_or_else(|| {
            refuse(
                StatusCode::BAD_REQUEST,
                "idempotency_key_invalid",
                "An Idempotency-Key header of 8 to 128 characters is required.",
            )
        })?
        .to_owned();
    let length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .ok_or_else(|| {
            refuse(
                StatusCode::LENGTH_REQUIRED,
                "upload_length_required",
                "The upload must declare its length.",
            )
        })?;
    if length == 0 {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_upload",
            "The upload is empty.",
        ));
    }
    if length > MAX_REQUEST_BYTES {
        return Err(too_large());
    }
    lifecycle_gate(state, user, &org_id, &space_ref).await?;
    let decision = shared_import_decision(state, user, &org_id, &space_ref, &idempotency_key).await?;
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let Some(token) =
        crate::audience_tokens::get_audience_token(state, &user.user_id, cookie, "ingestion").await
    else {
        return Err(refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "imports_auth_unavailable",
            "Import authentication is temporarily unavailable.",
        ));
    };

    // The bytes stream through; the counter refuses a body that outgrows its declared cap.
    let exceeded = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&exceeded);
    let mut seen: u64 = 0;
    let stream = body.into_data_stream().map(move |chunk| match chunk {
        Ok(bytes) => {
            seen = seen.saturating_add(bytes.len() as u64);
            if seen > MAX_REQUEST_BYTES {
                flag.store(true, Ordering::SeqCst);
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "request too large",
                ))
            } else {
                Ok(bytes)
            }
        }
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "client body ended early",
        )),
    });
    let url = format!("{}/api/v1/import/jobs/space-upload", state.imports_api_url);
    let sent = state
        .client
        .post(&url)
        .timeout(UPLOAD_TIMEOUT)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, length)
        .bearer_auth(token)
        .header("x-org-id", &org_id)
        .header("x-user-id", &user.user_id)
        .header("x-space-import-ingress-decision", decision)
        .header("idempotency-key", &idempotency_key)
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await;
    let upstream = match sent {
        Ok(upstream) => upstream,
        Err(_) if exceeded.load(Ordering::SeqCst) => return Err(too_large()),
        Err(_) => {
            return Err((
                StatusCode::BAD_GATEWAY,
                Json(crate::envelope::upstream_unavailable()),
            ))
        }
    };
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let payload = upstream.json::<Value>().await.unwrap_or(Value::Null);
    Ok(no_store(relay_import_answer(status, payload)))
}

fn too_large() -> Reply {
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        Json(error_with_details(
            "room_file_too_large",
            "The upload is larger than the room file limit.",
            json!({ "max_bytes": MAX_REQUEST_BYTES, "scope": "request" }),
        )),
    )
}

/// Imports-core answers with `{data}` or its own typed `{error:{code,message,details}}`; both pass
/// through. Anything else (a framework validation body, a proxy page) becomes a generic typed error
/// so no upstream text reaches the browser.
fn relay_import_answer(status: StatusCode, payload: Value) -> Reply {
    if status.is_success() && payload.get("data").is_some() {
        return (status, Json(payload));
    }
    let typed = payload
        .pointer("/error/code")
        .and_then(Value::as_str)
        .is_some()
        && payload.pointer("/error/message").is_some();
    if typed {
        return (status, Json(payload));
    }
    if status.is_client_error() {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_upload",
            "The upload could not be accepted.",
        );
    }
    unavailable()
}

// ---------------------------------------------------------------------------------------------
// R2: upload progress
// ---------------------------------------------------------------------------------------------

fn valid_uuid(value: &str) -> bool {
    value.len() == 36
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() || b == b'-')
}

async fn get_upload(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, upload_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    match get_upload_core(&state, &user, &headers, &space_ref, &upload_id).await {
        Ok(reply) | Err(reply) => no_store(reply),
    }
}

async fn get_upload_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    upload_id: &str,
) -> Result<Reply, Reply> {
    let (_org_id, _space_ref) = gate(state, user, space_ref).await?;
    let not_found = || {
        refuse(
            StatusCode::NOT_FOUND,
            "upload_not_found",
            "The upload is not available.",
        )
    };
    let upload_id = upload_id.trim();
    if !valid_uuid(upload_id) {
        return Err(not_found());
    }
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let Some(token) =
        crate::audience_tokens::get_audience_token(state, &user.user_id, cookie, "ingestion").await
    else {
        return Err(refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "imports_auth_unavailable",
            "Import authentication is temporarily unavailable.",
        ));
    };
    let url = format!(
        "{}/api/v1/import/jobs/{}",
        state.imports_api_url,
        urlencoding::encode(upload_id)
    );
    let (status, Json(job)) =
        proxy_bearer_json(state, Method::GET, &url, None, Some(&token), &user.user_id).await;
    if status == StatusCode::NOT_FOUND || status == StatusCode::FORBIDDEN {
        return Err(not_found());
    }
    if !status.is_success() {
        return Err(unavailable());
    }
    // The job read is scoped to the organization at Ingestion; this route additionally shows only
    // the caller's own room-file upload, never another import job that shares the table.
    let own = job.get("source_type").and_then(Value::as_str) == Some(SOURCE_TYPE)
        && job.get("user_id").and_then(Value::as_str) == Some(user.user_id.as_str());
    if !own {
        return Err(not_found());
    }
    let items: Vec<Value> = job
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    let mut view = serde_json::Map::new();
                    view.insert(
                        "file_name".to_owned(),
                        item.get("source_name").cloned().unwrap_or(Value::Null),
                    );
                    view.insert(
                        "status".to_owned(),
                        item.get("status").cloned().unwrap_or(Value::Null),
                    );
                    let meta = item.get("metadata");
                    for key in ["document_id", "error_code"] {
                        if let Some(value) = meta
                            .and_then(|m| m.get(key))
                            .and_then(Value::as_str)
                            .filter(|v| !v.is_empty())
                        {
                            view.insert(key.to_owned(), json!(value));
                        }
                    }
                    Value::Object(view)
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((
        StatusCode::OK,
        Json(json!({
            "data": {
                "upload_id": upload_id,
                "status": job.get("status").cloned().unwrap_or(Value::Null),
                "total": job.get("total_items").cloned().unwrap_or(json!(0)),
                "processed": job.get("processed_items").cloned().unwrap_or(json!(0)),
                "failed": job.get("failed_items").cloned().unwrap_or(json!(0)),
                "items": items,
            }
        })),
    ))
}

// ---------------------------------------------------------------------------------------------
// R3: list
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
struct ListQuery {
    limit: Option<String>,
    cursor: Option<String>,
}

async fn list_files(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Response {
    let reply = match list_core(
        &state,
        &user,
        &headers,
        &space_ref,
        query.limit.as_deref(),
        query.cursor.as_deref(),
    )
    .await
    {
        Ok(reply) | Err(reply) => reply,
    };
    no_store(reply)
}

/// One room file as the browser sees it, from retrieval-engine's listing row.
fn file_view(document: &Value) -> Value {
    let meta = document.get("metadata");
    let from_meta = |key: &str| meta.and_then(|m| m.get(key)).cloned().unwrap_or(Value::Null);
    let file_name = meta
        .and_then(|m| m.get("filename"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| document.get("title").and_then(Value::as_str).map(str::to_owned));
    json!({
        "document_id": document.get("document_id").cloned().unwrap_or(Value::Null),
        "file_name": file_name,
        "mime_type": from_meta("content_type"),
        "size_bytes": from_meta("size_bytes"),
        "sha256": from_meta("sha256"),
        "extension": from_meta("extension"),
        "status": document.get("status").cloned().unwrap_or(Value::Null),
        "text_chars": from_meta("text_chars"),
        "uploaded_by": document.get("uploaded_by").cloned().unwrap_or(Value::Null),
        "uploaded_at": document.get("created_at").cloned().unwrap_or(Value::Null),
    })
}

/// R3 and `space.files.list`. A declined Control decision is an answer, not an error: the list is
/// empty and `unavailable[]` names why.
pub(crate) async fn list_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    limit: Option<&str>,
    cursor: Option<&str>,
) -> Result<Reply, Reply> {
    let (org_id, space_ref) = gate(state, user, space_ref).await?;
    let limit = match limit.map(str::trim).filter(|v| !v.is_empty()) {
        None => LIST_DEFAULT_LIMIT,
        Some(raw) => match raw.parse::<i64>() {
            Ok(n) if (1..=LIST_MAX_LIMIT).contains(&n) => n,
            _ => {
                return Err(refuse(
                    StatusCode::BAD_REQUEST,
                    "invalid_limit",
                    "The limit must be between 1 and 200.",
                ))
            }
        },
    };
    let cursor = cursor.map(str::trim).filter(|v| !v.is_empty());
    if cursor.is_some_and(|c| c.len() > MAX_CURSOR_LEN || c.chars().any(char::is_control)) {
        return Err(invalid_cursor());
    }
    let mut unavailable_entries: Vec<Value> = Vec::new();
    let mut files: Vec<Value> = Vec::new();
    let mut next_cursor = Value::Null;
    match read_decision(state, user, &org_id, &space_ref).await? {
        Decision::Declined(denial) => {
            unavailable_entries.push(denial::with_gap_details(
                json!({
                    "section": "files",
                    "code": "files_read_not_authorized",
                    "reason": "Reading this room's files is not authorized.",
                }),
                denial.as_ref(),
            ));
        }
        Decision::Granted(decision) => {
            let mut body = json!({ "limit": limit, "types": [SOURCE_TYPE] });
            if let Some(cursor) = cursor {
                body["cursor"] = json!(cursor);
            }
            let url = format!("{}/v1/knowledge/space-sources", state.retrieval_engine_url);
            let (status, Json(payload)) =
                crate::domains::knowledge::shared::proxy_data_plane_json_with_space_decision(
                    state,
                    user,
                    headers,
                    Method::POST,
                    &url,
                    Some(body),
                    Some(&org_id),
                    &decision,
                )
                .await;
            if status.is_success() {
                files = payload
                    .get("documents")
                    .and_then(Value::as_array)
                    .map(|documents| documents.iter().map(file_view).collect())
                    .unwrap_or_default();
                next_cursor = payload.get("next_cursor").cloned().unwrap_or(Value::Null);
            } else if status == StatusCode::BAD_REQUEST && cursor.is_some() {
                return Err(invalid_cursor());
            } else if status == StatusCode::FORBIDDEN {
                unavailable_entries.push(json!({
                    "section": "files",
                    "code": "files_binding_unavailable",
                    "reason": "This room is not bound to a Data Plane knowledge target.",
                }));
            } else {
                unavailable_entries.push(json!({
                    "section": "files",
                    "code": "files_upstream_unavailable",
                    "reason": "Data Plane could not return this room's files.",
                }));
            }
        }
    }
    Ok((
        StatusCode::OK,
        Json(json!({
            "data": {
                "files": files,
                "next_cursor": next_cursor,
                "unavailable": unavailable_entries,
            }
        })),
    ))
}

fn invalid_cursor() -> Reply {
    refuse(
        StatusCode::BAD_REQUEST,
        "invalid_cursor",
        "The page cursor is not valid.",
    )
}

// ---------------------------------------------------------------------------------------------
// R4: text
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
struct TextQuery {
    max_chars: Option<String>,
}

async fn read_file_text(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, document_id)): Path<(String, String)>,
    headers: HeaderMap,
    Query(query): Query<TextQuery>,
) -> Response {
    let reply = match read_text_core(
        &state,
        &user,
        &headers,
        &space_ref,
        &document_id,
        query.max_chars.as_deref(),
    )
    .await
    {
        Ok(reply) | Err(reply) => reply,
    };
    no_store(reply)
}

fn check_document_id(document_id: &str) -> Result<String, Reply> {
    let document_id = document_id.trim();
    if valid_id(document_id) {
        Ok(document_id.to_owned())
    } else {
        Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_document",
            "A document id is required.",
        ))
    }
}

/// One room file's extracted text through retrieval-engine's `space-document`, under the caller's
/// own Control decision. Text only: there is no download and the answer is `no-store`.
async fn space_document(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    org_id: &str,
    space_ref: &str,
    document_id: &str,
    max_chars: usize,
) -> Result<Value, Reply> {
    let decision = match read_decision(state, user, org_id, space_ref).await? {
        Decision::Granted(decision) => decision,
        Decision::Declined(denial) => return Err(declined(denial)),
    };
    let url = format!("{}/v1/knowledge/space-document", state.retrieval_engine_url);
    let (status, Json(payload)) =
        crate::domains::knowledge::shared::proxy_data_plane_json_with_space_decision(
            state,
            user,
            headers,
            Method::POST,
            &url,
            Some(json!({ "document_id": document_id, "max_chars": max_chars })),
            Some(org_id),
            &decision,
        )
        .await;
    if status.is_success() {
        return Ok(payload);
    }
    // Missing, deleted, another room's and not-visible all read the same.
    if matches!(
        status,
        StatusCode::NOT_FOUND | StatusCode::FORBIDDEN | StatusCode::BAD_REQUEST
    ) {
        return Err(file_not_found());
    }
    Err(unavailable())
}

/// R4 and `space.files.read_text`.
pub(crate) async fn read_text_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    document_id: &str,
    max_chars: Option<&str>,
) -> Result<Reply, Reply> {
    let (org_id, space_ref) = gate(state, user, space_ref).await?;
    let document_id = check_document_id(document_id)?;
    let max_chars = match max_chars.map(str::trim).filter(|v| !v.is_empty()) {
        None => DEFAULT_TEXT_CHARS,
        Some(raw) => match raw.parse::<usize>() {
            Ok(n) if n >= 1 => n.min(MAX_TEXT_CHARS),
            _ => {
                return Err(refuse(
                    StatusCode::BAD_REQUEST,
                    "invalid_max_chars",
                    "max_chars must be a positive number.",
                ))
            }
        },
    };
    let payload =
        space_document(state, user, headers, &org_id, &space_ref, &document_id, max_chars).await?;
    Ok((
        StatusCode::OK,
        Json(json!({
            "data": {
                "document_id": payload.get("document_id").cloned().unwrap_or(json!(document_id)),
                "file_name": payload.get("file_name").cloned().unwrap_or(Value::Null),
                "text": payload.get("text").cloned().unwrap_or(json!("")),
                "text_chars": payload.get("text_chars").cloned().unwrap_or(Value::Null),
                "truncated": payload.get("truncated").and_then(Value::as_bool).unwrap_or(false),
            }
        })),
    ))
}

// ---------------------------------------------------------------------------------------------
// R5: delete
// ---------------------------------------------------------------------------------------------

async fn delete_file(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, document_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    match delete_core(&state, &user, &headers, &space_ref, &document_id).await {
        Ok((status, _)) if status == StatusCode::NO_CONTENT => {
            let mut response = StatusCode::NO_CONTENT.into_response();
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
        }
        Ok(reply) | Err(reply) => no_store(reply),
    }
}

/// R5 and `space.files.delete`. The document must be a file of THIS room that the caller can see
/// (proved by a one-character read under the caller's own decision) before documents-api is asked
/// to delete it; documents-api then applies its owner check, so only the uploader succeeds. Every
/// refusal, including "not yours", reads as 404 `file_not_found`. Success is 204.
///
/// ZDR: a delete persists nothing, so no gateway-side ZDR refusal applies; documents-api's own
/// rule for Zero Data Retention credentials decides.
pub(crate) async fn delete_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    document_id: &str,
) -> Result<Reply, Reply> {
    let (org_id, space_ref) = gate(state, user, space_ref).await?;
    let document_id = check_document_id(document_id)?;
    space_document(state, user, headers, &org_id, &space_ref, &document_id, 1)
        .await
        .map_err(|reply| {
            if reply.0 == StatusCode::FORBIDDEN {
                file_not_found()
            } else {
                reply
            }
        })?;
    let url = format!(
        "{}/v1/documents/{}",
        state.documents_api_url,
        urlencoding::encode(&document_id)
    );
    let (status, _) = crate::domains::knowledge::shared::proxy_data_plane_json(
        state,
        user,
        headers,
        Method::DELETE,
        &url,
        None,
        Some(&org_id),
        None,
    )
    .await;
    if status.is_success() {
        return Ok((
            StatusCode::NO_CONTENT,
            Json(json!({ "data": { "deleted": true, "document_id": document_id } })),
        ));
    }
    if matches!(status, StatusCode::NOT_FOUND | StatusCode::FORBIDDEN) {
        return Err(file_not_found());
    }
    Err(unavailable())
}

/// `space.files.attach` is multipart-only: a JSON action dispatch cannot carry bytes.
pub(crate) fn attach_requires_multipart() -> Reply {
    refuse(
        StatusCode::UNPROCESSABLE_ENTITY,
        "upload_requires_multipart",
        "Files are uploaded with POST /api/v1/spaces/{space_ref}/files as multipart form data.",
    )
}
