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

#[cfg(test)]
mod tests {
    use super::{
        delete_core, features_json, get_upload_core, limits_json, list_core, read_text_core,
        reason_code, relay_import_answer, upload_core, valid_idempotency_key, valid_uuid,
        zdr_refusal, MAX_REQUEST_BYTES,
    };
    use crate::config::AppState;
    use crate::middleware::{AuthenticatedUser, AuthorizedMembership};
    use axum::{
        body::Body,
        http::{header, HeaderMap, HeaderValue, Request, StatusCode},
        Json,
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SPACE: &str = "space-1";
    const KEY: &str = "idem-key-aaaaa";
    const UPLOAD_ID: &str = "11111111-2222-3333-4444-555555555555";
    const BOUNDARY: &str = "multipart/form-data; boundary=xyz";

    fn member(user_id: &str) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: user_id.to_owned(),
            user_email: format!("{user_id}@example.com"),
            user_name: "Caller".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    /// One mock server stands in for every upstream: the paths are disjoint.
    fn state_with(server: &MockServer, room_files: bool) -> AppState {
        let mut state = crate::tests::test_state(false);
        let uri = server.uri();
        state.auth_core_url = uri.clone();
        state.user_core_url = uri.clone();
        state.org_core_url = uri.clone();
        state.imports_api_url = uri.clone();
        state.retrieval_engine_url = uri.clone();
        state.documents_api_url = uri.clone();
        state.application_convex_url = uri;
        state.application_convex_service_key = "application-test-key".into();
        state.space_features.room_files_api = room_files;
        state
    }

    async fn mount(server: &MockServer, method: &str, route: &str, template: ResponseTemplate) {
        Mock::given(wm_method(method))
            .and(wm_path(route))
            .respond_with(template)
            .mount(server)
            .await;
    }

    fn json_ok(status: u16, body: Value) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_json(body)
    }

    async fn mount_basics(server: &MockServer, zdr: bool) {
        mount(
            server,
            "POST",
            "/api/query",
            json_ok(200, json!({ "value": [{ "spaceRef": SPACE, "lifecycle": "active" }] })),
        )
        .await;
        mount(
            server,
            "GET",
            "/api/v1/organizations/org-1",
            json_ok(
                200,
                json!({ "data": { "metadata": { "interactiveRetention": { "zdr": zdr } } } }),
            ),
        )
        .await;
        for audience in ["ingestion", "data-plane"] {
            mount(
                server,
                "GET",
                &format!("/api/{audience}/token"),
                json_ok(200, json!({ "token": format!("{audience}-token") })),
            )
            .await;
        }
    }

    fn import_decision() -> Value {
        json!({ "data": {
            "decision": {
                "space_ref": SPACE,
                "service_audience": "ingestion-plane-import",
                "action_id": "ingestion.import.write",
                "action_schema_hash": "sha256:ingestion-import-v1",
                "idempotency_key": KEY,
                "import_source_type": "space_upload",
            },
            "token": "import-decision-token",
        }})
    }

    async fn mount_import_decision(server: &MockServer) {
        mount(
            server,
            "POST",
            "/api/v1/internal/spaces/import-decision",
            json_ok(200, import_decision()),
        )
        .await;
    }

    async fn mount_read_decision(server: &MockServer) {
        mount(
            server,
            "POST",
            "/api/v1/internal/spaces/retrieval-decision",
            json_ok(200, json!({ "data": { "token": "retrieval-decision-token" } })),
        )
        .await;
    }

    async fn requests_to(server: &MockServer, needle: &str) -> usize {
        server
            .received_requests()
            .await
            .expect("requests")
            .iter()
            .filter(|request| request.url.path().contains(needle))
            .count()
    }

    fn upload_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(BOUNDARY));
        headers.insert("idempotency-key", HeaderValue::from_static(KEY));
        headers
    }

    fn with_length(mut headers: HeaderMap, length: u64) -> HeaderMap {
        headers.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&length.to_string()).unwrap(),
        );
        headers
    }

    fn flat(result: Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)>) -> (u16, Value) {
        match result {
            Ok((status, Json(body))) | Err((status, Json(body))) => (status.as_u16(), body),
        }
    }

    async fn upload(
        state: &AppState,
        headers: &HeaderMap,
        bytes: Vec<u8>,
    ) -> (u16, Value, Option<String>) {
        match upload_core(state, &member("files-user"), SPACE, headers, Body::from(bytes)).await {
            Ok(response) => {
                let status = response.status().as_u16();
                let cache = response
                    .headers()
                    .get(header::CACHE_CONTROL)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .unwrap()
                    .to_bytes();
                (status, serde_json::from_slice(&body).unwrap_or(Value::Null), cache)
            }
            Err((status, Json(body))) => (status.as_u16(), body, None),
        }
    }

    // ----- pure helpers ----------------------------------------------------------------------

    #[test]
    fn the_new_room_files_flags_default_off_and_the_context_blocks_mirror_them() {
        let state = crate::tests::test_state(false);
        let features = features_json(&state);
        assert_eq!(
            features,
            json!({ "turn_attachments": false, "durable": false, "files_tab": false })
        );
        let mut on = state.clone();
        on.space_features.room_files_api = true;
        assert_eq!(features_json(&on)["durable"], true);
        // The tab needs both the API and the UI flag.
        assert_eq!(features_json(&on)["files_tab"], false);
        on.space_features.room_files_ui = true;
        assert_eq!(features_json(&on)["files_tab"], true);
        on.space_features.room_files_api = false;
        assert_eq!(features_json(&on)["files_tab"], false);
        on.space_features.turn_attachments_ui = true;
        assert_eq!(features_json(&on)["turn_attachments"], true);

        let limits = limits_json();
        assert_eq!(limits["max_files"], 10);
        assert_eq!(limits["max_file_bytes"], 25 * 1024 * 1024);
        assert_eq!(limits["max_request_bytes"], 100 * 1024 * 1024);
        assert_eq!(limits["turn_inline_chars"], 60_000);
        assert_eq!(limits["turn_total_chars"], 120_000);
        assert_eq!(limits["allowed_extensions"].as_array().unwrap().len(), 8);
    }

    #[test]
    fn idempotency_key_uuid_and_reason_code_validation() {
        assert!(valid_idempotency_key("abcdefgh"));
        assert!(valid_idempotency_key(&"a".repeat(128)));
        assert!(!valid_idempotency_key("abcdefg"));
        assert!(!valid_idempotency_key(&"a".repeat(129)));
        assert!(!valid_idempotency_key("has space 123"));
        assert!(valid_uuid(UPLOAD_ID));
        assert!(!valid_uuid("not-a-uuid"));
        assert!(!valid_uuid(&format!("{UPLOAD_ID}x")));
        assert_eq!(reason_code(&json!({ "code": "space_files_off" })), "space_files_off");
        assert_eq!(reason_code(&json!({ "error": { "code": "role_forbidden" } })), "role_forbidden");
        // Anything that is not a lowercase token is the generic word, never Control's text.
        assert_eq!(reason_code(&json!({ "code": "Has Spaces!" })), "not_authorized");
        assert_eq!(reason_code(&json!({ "code": "UPPER" })), "not_authorized");
        assert_eq!(reason_code(&json!({})), "not_authorized");
    }

    #[test]
    fn zdr_refusal_adds_the_room_files_surface_only_to_the_412() {
        let body = json!({ "error": { "code": "zdr_durable_mutation_forbidden", "message": "m" } });
        let (status, Json(out)) =
            zdr_refusal((StatusCode::PRECONDITION_FAILED, Json(body.clone())));
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(out["error"]["details"]["surface"], "room_files");
        let (status, Json(out)) = zdr_refusal((StatusCode::SERVICE_UNAVAILABLE, Json(body)));
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(out["error"].get("details").is_none());
    }

    #[test]
    fn relay_import_answer_passes_data_and_typed_errors_and_hides_everything_else() {
        let (status, Json(body)) =
            relay_import_answer(StatusCode::ACCEPTED, json!({ "data": { "upload_id": "u" } }));
        assert_eq!((status.as_u16(), body["data"]["upload_id"].clone()), (202, json!("u")));
        let typed = json!({ "error": { "code": "room_file_type_not_allowed", "message": "no" } });
        let (status, Json(body)) = relay_import_answer(StatusCode::UNSUPPORTED_MEDIA_TYPE, typed.clone());
        assert_eq!(status.as_u16(), 415);
        assert_eq!(body, typed);
        let (status, Json(body)) = relay_import_answer(
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({ "detail": [{ "msg": "framework text" }] }),
        );
        assert_eq!(status.as_u16(), 400);
        assert_eq!(body["error"]["code"], "invalid_upload");
        assert!(!body.to_string().contains("framework text"));
        let (status, Json(body)) =
            relay_import_answer(StatusCode::INTERNAL_SERVER_ERROR, json!("<html>proxy</html>"));
        assert_eq!(status.as_u16(), 503);
        assert_eq!(body["error"]["code"], "room_files_unavailable");
    }

    // ----- R1: upload ------------------------------------------------------------------------

    #[tokio::test]
    async fn upload_flag_off_is_404_before_any_upstream_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        let state = state_with(&server, false);
        let (status, body, _) =
            upload(&state, &with_length(upload_headers(), 4), b"data".to_vec()).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "room_files_disabled");
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn upload_streams_to_imports_core_with_the_relayed_decision_and_answers_202_no_store() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount_import_decision(&server).await;
        mount(
            &server,
            "POST",
            "/api/v1/import/jobs/space-upload",
            json_ok(202, json!({ "data": { "upload_id": UPLOAD_ID, "status": "queued" } })),
        )
        .await;
        let state = state_with(&server, true);
        let payload = b"--xyz\r\nfile-bytes\r\n--xyz--\r\n".to_vec();
        let (status, body, cache) = upload(
            &state,
            &with_length(upload_headers(), payload.len() as u64),
            payload.clone(),
        )
        .await;
        assert_eq!(status, 202);
        assert_eq!(body["data"]["upload_id"], UPLOAD_ID);
        assert_eq!(cache.as_deref(), Some("no-store"));
        let sent = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|request| request.url.path() == "/api/v1/import/jobs/space-upload")
            .expect("upload call");
        assert_eq!(sent.body, payload, "the bytes reach Ingestion unchanged");
        let header_of = |name: &str| sent.headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
        assert_eq!(header_of("x-space-import-ingress-decision").as_deref(), Some("import-decision-token"));
        assert_eq!(header_of("idempotency-key").as_deref(), Some(KEY));
        assert_eq!(header_of("x-org-id").as_deref(), Some("org-1"));
        assert_eq!(header_of("x-user-id").as_deref(), Some("files-user"));
        assert_eq!(header_of("authorization").as_deref(), Some("Bearer ingestion-token"));
        // Control was asked for exactly the space_upload decision the caller needs.
        let decision_call = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|request| request.url.path().ends_with("/import-decision"))
            .unwrap();
        let asked: Value = serde_json::from_slice(&decision_call.body).unwrap();
        assert_eq!(
            asked,
            json!({ "space_ref": SPACE, "source_type": "space_upload", "idempotency_key": KEY })
        );
    }

    #[tokio::test]
    async fn upload_replay_200_is_relayed_as_200() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount_import_decision(&server).await;
        mount(
            &server,
            "POST",
            "/api/v1/import/jobs/space-upload",
            json_ok(200, json!({ "data": { "upload_id": UPLOAD_ID, "status": "completed" } })),
        )
        .await;
        let state = state_with(&server, true);
        let (status, body, _) =
            upload(&state, &with_length(upload_headers(), 4), b"data".to_vec()).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["status"], "completed");
    }

    #[tokio::test]
    async fn a_forged_or_mismatched_control_decision_is_refused_and_nothing_is_uploaded() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (field, forged) in [
            ("space_ref", "space-other"),
            ("service_audience", "documents-api"),
            ("action_id", "ingestion.import.read"),
            ("action_schema_hash", "sha256:other"),
            ("idempotency_key", "someone-elses-key"),
            ("import_source_type", "web_page"),
        ] {
            let server = MockServer::start().await;
            mount_basics(&server, false).await;
            let mut decision = import_decision();
            decision["data"]["decision"][field] = json!(forged);
            mount(
                &server,
                "POST",
                "/api/v1/internal/spaces/import-decision",
                json_ok(200, decision),
            )
            .await;
            let state = state_with(&server, true);
            let (status, body, _) =
                upload(&state, &with_length(upload_headers(), 4), b"data".to_vec()).await;
            assert_eq!(status, 502, "{field}");
            assert_eq!(body["error"]["code"], "invalid_space_decision", "{field}");
            assert_eq!(requests_to(&server, "space-upload").await, 0, "{field}");
        }
        // A missing token is refused too.
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        let mut decision = import_decision();
        decision["data"]["token"] = json!("  ");
        mount(&server, "POST", "/api/v1/internal/spaces/import-decision", json_ok(200, decision)).await;
        let state = state_with(&server, true);
        let (status, _, _) = upload(&state, &with_length(upload_headers(), 4), b"d".to_vec()).await;
        assert_eq!(status, 502);
        assert_eq!(requests_to(&server, "space-upload").await, 0);
    }

    #[tokio::test]
    async fn control_403_is_space_files_not_authorized_with_a_reason_code() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount(
            &server,
            "POST",
            "/api/v1/internal/spaces/import-decision",
            json_ok(403, json!({ "error": { "code": "space_role_below_floor", "message": "secret" } })),
        )
        .await;
        let state = state_with(&server, true);
        let (status, body, _) =
            upload(&state, &with_length(upload_headers(), 4), b"data".to_vec()).await;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "space_files_not_authorized");
        assert_eq!(body["error"]["details"]["reason_code"], "space_role_below_floor");
        assert!(!body.to_string().contains("secret"));
        assert_eq!(requests_to(&server, "space-upload").await, 0);
    }

    #[tokio::test]
    async fn control_404_is_room_files_disabled_and_a_control_outage_is_unavailable() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount(
            &server,
            "POST",
            "/api/v1/internal/spaces/import-decision",
            ResponseTemplate::new(404),
        )
        .await;
        let state = state_with(&server, true);
        let (status, body, _) =
            upload(&state, &with_length(upload_headers(), 4), b"data".to_vec()).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "room_files_disabled");

        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount(
            &server,
            "POST",
            "/api/v1/internal/spaces/import-decision",
            ResponseTemplate::new(500).set_body_string("down"),
        )
        .await;
        let state = state_with(&server, true);
        let (status, body, _) =
            upload(&state, &with_length(upload_headers(), 4), b"data".to_vec()).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "room_files_unavailable");
    }

    #[tokio::test]
    async fn zdr_org_gets_412_with_the_surface_and_control_is_never_called() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_basics(&server, true).await;
        mount_import_decision(&server).await;
        let state = state_with(&server, true);
        let (status, body, _) =
            upload(&state, &with_length(upload_headers(), 4), b"data".to_vec()).await;
        assert_eq!(status, 412);
        assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden");
        assert_eq!(body["error"]["details"]["surface"], "room_files");
        assert_eq!(requests_to(&server, "import-decision").await, 0);
        assert_eq!(requests_to(&server, "space-upload").await, 0);
        // The request-level flag alone is enough, even for an org that is not ZDR.
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount_import_decision(&server).await;
        let state = state_with(&server, true);
        let mut headers = with_length(upload_headers(), 4);
        headers.insert("x-zdr", HeaderValue::from_static("true"));
        let (status, _, _) = upload(&state, &headers, b"data".to_vec()).await;
        assert_eq!(status, 412);
        assert_eq!(requests_to(&server, "import-decision").await, 0);
    }

    #[tokio::test]
    async fn an_org_core_outage_is_503_zdr_decision_unavailable_and_nothing_else_is_called() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount(&server, "GET", "/api/v1/organizations/org-1", ResponseTemplate::new(500)).await;
        mount_import_decision(&server).await;
        let state = state_with(&server, true);
        let (status, body, _) =
            upload(&state, &with_length(upload_headers(), 4), b"data".to_vec()).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "zdr_decision_unavailable");
        assert_eq!(requests_to(&server, "import-decision").await, 0);
    }

    #[tokio::test]
    async fn upload_request_shape_is_refused_before_control_is_asked() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount_import_decision(&server).await;
        let state = state_with(&server, true);

        // Not multipart.
        let mut headers = with_length(upload_headers(), 4);
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let (status, body, _) = upload(&state, &headers, b"data".to_vec()).await;
        assert_eq!((status, body["error"]["code"].clone()), (422, json!("upload_requires_multipart")));
        headers.remove(header::CONTENT_TYPE);
        let (status, _, _) = upload(&state, &headers, b"data".to_vec()).await;
        assert_eq!(status, 422);

        // Bad or missing Idempotency-Key.
        for bad in [Some("short"), Some("has spaces in it"), None] {
            let mut headers = with_length(upload_headers(), 4);
            headers.remove("idempotency-key");
            if let Some(bad) = bad {
                headers.insert("idempotency-key", HeaderValue::from_str(bad).unwrap());
            }
            let (status, body, _) = upload(&state, &headers, b"data".to_vec()).await;
            assert_eq!(status, 400, "{bad:?}");
            assert_eq!(body["error"]["code"], "idempotency_key_invalid");
        }

        // No declared length is 411; an empty one is 400.
        let (status, body, _) = upload(&state, &upload_headers(), b"data".to_vec()).await;
        assert_eq!((status, body["error"]["code"].clone()), (411, json!("upload_length_required")));
        let (status, body, _) = upload(&state, &with_length(upload_headers(), 0), Vec::new()).await;
        assert_eq!((status, body["error"]["code"].clone()), (400, json!("invalid_upload")));

        // Over 100 MiB is refused from the header alone.
        let (status, body, _) = upload(
            &state,
            &with_length(upload_headers(), MAX_REQUEST_BYTES + 1),
            b"data".to_vec(),
        )
        .await;
        assert_eq!(status, 413);
        assert_eq!(body["error"]["code"], "room_file_too_large");
        assert_eq!(body["error"]["details"]["max_bytes"], MAX_REQUEST_BYTES);

        assert_eq!(requests_to(&server, "import-decision").await, 0);
        assert_eq!(requests_to(&server, "space-upload").await, 0);
    }

    #[tokio::test]
    async fn upload_for_a_missing_space_is_404_and_a_bad_reference_is_400() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount(&server, "GET", "/api/v1/organizations/org-1", json_ok(200, json!({ "data": {} }))).await;
        mount(&server, "POST", "/api/query", json_ok(200, json!({ "value": [] }))).await;
        mount_import_decision(&server).await;
        let state = state_with(&server, true);
        let headers = with_length(upload_headers(), 4);
        let (status, body, _) = upload(&state, &headers, b"data".to_vec()).await;
        assert_eq!((status, body["error"]["code"].clone()), (404, json!("space_not_found")));
        assert_eq!(requests_to(&server, "import-decision").await, 0);
        let result =
            upload_core(&state, &member("files-user"), "a/b", &headers, Body::from("data")).await;
        let (status, body) = flat(result.map(|_| unreachable!()));
        assert_eq!((status, body["error"]["code"].clone()), (400, json!("invalid_space")));
        let mut no_org = member("files-user");
        no_org.authorized_membership = None;
        let result = upload_core(&state, &no_org, SPACE, &headers, Body::from("data")).await;
        let (status, body) = flat(result.map(|_| unreachable!()));
        assert_eq!((status, body["error"]["code"].clone()), (403, json!("active_membership_required")));
    }

    /// Through the real router: a 10 MiB multipart body streams to Ingestion unchanged. The default
    /// axum body cap is 2 MiB, so this fails if any layer buffers the upload.
    #[tokio::test]
    async fn a_ten_mebibyte_upload_streams_through_the_router() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount_import_decision(&server).await;
        mount(
            &server,
            "GET",
            "/api/auth/get-session",
            json_ok(
                200,
                json!({
                    "user": { "id": "files-router-user", "email": "u@example.com", "emailVerified": true },
                    "session": { "activeOrganizationId": "org-1" }
                }),
            ),
        )
        .await;
        mount(
            &server,
            "GET",
            "/api/v1/me/session-context",
            json_ok(
                200,
                json!({ "userId": "files-router-user", "orgId": "org-1", "role": "member",
                        "onboardingStatus": "COMPLETED" }),
            ),
        )
        .await;
        mount(
            &server,
            "POST",
            "/api/v1/import/jobs/space-upload",
            json_ok(202, json!({ "data": { "upload_id": UPLOAD_ID, "status": "queued" } })),
        )
        .await;
        let state = state_with(&server, true);
        let size = 10 * 1024 * 1024usize;
        let payload = vec![b'a'; size];
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/spaces/{SPACE}/files"))
                    .header("cookie", "better-auth.session_token=session-1")
                    .header(header::CONTENT_TYPE, BOUNDARY)
                    .header(header::CONTENT_LENGTH, size.to_string())
                    .header("idempotency-key", KEY)
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 202);
        let sent = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|request| request.url.path() == "/api/v1/import/jobs/space-upload")
            .expect("upload reached Ingestion");
        assert_eq!(sent.body.len(), size);
    }

    // ----- R2: upload progress ---------------------------------------------------------------

    fn job(source_type: &str, user_id: &str) -> Value {
        json!({
            "id": UPLOAD_ID,
            "source_type": source_type,
            "user_id": user_id,
            "status": "completed",
            "total_items": 2,
            "processed_items": 1,
            "failed_items": 1,
            "items": [
                { "source_name": "a.pdf", "status": "completed",
                  "metadata": { "document_id": "doc-1" } },
                { "source_name": "b.exe", "status": "failed",
                  "metadata": { "error_code": "room_file_type_not_allowed" } }
            ]
        })
    }

    #[tokio::test]
    async fn upload_progress_shows_only_the_callers_own_space_upload_job() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let route = format!("/api/v1/import/jobs/{UPLOAD_ID}");
        let user = member("files-user");
        let headers = HeaderMap::new();

        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount(&server, "GET", &route, json_ok(200, job("space_upload", "files-user"))).await;
        let state = state_with(&server, true);
        let (status, body) = flat(get_upload_core(&state, &user, &headers, SPACE, UPLOAD_ID).await);
        assert_eq!(status, 200);
        assert_eq!(body["data"]["upload_id"], UPLOAD_ID);
        assert_eq!(body["data"]["total"], 2);
        assert_eq!(body["data"]["failed"], 1);
        let items = body["data"]["items"].as_array().unwrap();
        assert_eq!(items[0]["file_name"], "a.pdf");
        assert_eq!(items[0]["document_id"], "doc-1");
        assert_eq!(items[1]["error_code"], "room_file_type_not_allowed");
        assert!(items[1].get("document_id").is_none());

        for (source, owner) in [("space_upload", "someone-else"), ("web_page", "files-user")] {
            let server = MockServer::start().await;
            mount_basics(&server, false).await;
            mount(&server, "GET", &route, json_ok(200, job(source, owner))).await;
            let state = state_with(&server, true);
            let (status, body) =
                flat(get_upload_core(&state, &user, &headers, SPACE, UPLOAD_ID).await);
            assert_eq!(status, 404, "{source}/{owner}");
            assert_eq!(body["error"]["code"], "upload_not_found");
        }

        // Ingestion 404 and 403 read the same; an id that is not a uuid never reaches Ingestion.
        for upstream in [404u16, 403] {
            let server = MockServer::start().await;
            mount_basics(&server, false).await;
            mount(&server, "GET", &route, ResponseTemplate::new(upstream)).await;
            let state = state_with(&server, true);
            let (status, body) =
                flat(get_upload_core(&state, &user, &headers, SPACE, UPLOAD_ID).await);
            assert_eq!((status, body["error"]["code"].clone()), (404, json!("upload_not_found")));
        }
        let (status, body) = flat(get_upload_core(&state, &user, &headers, SPACE, "nope").await);
        assert_eq!((status, body["error"]["code"].clone()), (404, json!("upload_not_found")));
    }

    // ----- R3: list --------------------------------------------------------------------------

    #[tokio::test]
    async fn list_maps_space_sources_rows_to_files_and_forwards_the_decision() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount_read_decision(&server).await;
        mount(
            &server,
            "POST",
            "/v1/knowledge/space-sources",
            json_ok(
                200,
                json!({
                    "documents": [{
                        "document_id": "doc-1", "title": "Fallback title", "status": "ready",
                        "uploaded_by": "u-1", "created_at": "2026-11-01T10:00:00Z",
                        "metadata": { "filename": "plan.pdf", "content_type": "application/pdf",
                                      "size_bytes": 1234, "sha256": "abc", "extension": "pdf",
                                      "text_chars": 99 }
                    }, {
                        "document_id": "doc-2", "title": "Only a title", "status": "ready"
                    }],
                    "next_cursor": "next-1"
                }),
            ),
        )
        .await;
        let state = state_with(&server, true);
        let (status, body) = flat(
            list_core(&state, &member("files-user"), &HeaderMap::new(), SPACE, Some("5"), Some("c1"))
                .await,
        );
        assert_eq!(status, 200);
        let files = body["data"]["files"].as_array().unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0]["file_name"], "plan.pdf");
        assert_eq!(files[0]["mime_type"], "application/pdf");
        assert_eq!(files[0]["size_bytes"], 1234);
        assert_eq!(files[0]["uploaded_at"], "2026-11-01T10:00:00Z");
        assert_eq!(files[1]["file_name"], "Only a title");
        assert_eq!(body["data"]["next_cursor"], "next-1");
        assert_eq!(body["data"]["unavailable"], json!([]));
        let sent = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|request| request.url.path() == "/v1/knowledge/space-sources")
            .unwrap();
        let asked: Value = serde_json::from_slice(&sent.body).unwrap();
        assert_eq!(asked, json!({ "limit": 5, "types": ["space_upload"], "cursor": "c1" }));
        assert_eq!(
            sent.headers.get("x-space-decision").and_then(|v| v.to_str().ok()),
            Some("retrieval-decision-token")
        );
    }

    #[tokio::test]
    async fn a_declined_decision_lists_no_files_and_names_the_gap() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount(
            &server,
            "POST",
            "/api/v1/internal/spaces/retrieval-decision",
            json_ok(403, json!({ "error": { "code": "space_membership_required", "message": "m" } })),
        )
        .await;
        let state = state_with(&server, true);
        let (status, body) = flat(
            list_core(&state, &member("files-user"), &HeaderMap::new(), SPACE, None, None).await,
        );
        assert_eq!(status, 200);
        assert_eq!(body["data"]["files"], json!([]));
        let gaps = body["data"]["unavailable"].as_array().unwrap();
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0]["section"], "files");
        assert_eq!(gaps[0]["code"], "files_read_not_authorized");
        assert_eq!(requests_to(&server, "space-sources").await, 0);
    }

    #[tokio::test]
    async fn list_validates_limit_and_cursor_and_names_a_data_plane_gap() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount_read_decision(&server).await;
        mount(&server, "POST", "/v1/knowledge/space-sources", ResponseTemplate::new(502)).await;
        let state = state_with(&server, true);
        let user = member("files-user");
        let headers = HeaderMap::new();
        for bad in ["0", "201", "abc", "-1"] {
            let (status, body) =
                flat(list_core(&state, &user, &headers, SPACE, Some(bad), None).await);
            assert_eq!(status, 400, "{bad}");
            assert_eq!(body["error"]["code"], "invalid_limit");
        }
        let long = "c".repeat(257);
        for bad in [long.as_str(), "bad\ncursor"] {
            let (status, body) =
                flat(list_core(&state, &user, &headers, SPACE, None, Some(bad)).await);
            assert_eq!(status, 400);
            assert_eq!(body["error"]["code"], "invalid_cursor");
        }
        assert_eq!(requests_to(&server, "space-sources").await, 0);
        let (status, body) = flat(list_core(&state, &user, &headers, SPACE, Some("200"), None).await);
        assert_eq!(status, 200);
        assert_eq!(body["data"]["unavailable"][0]["code"], "files_upstream_unavailable");

        // 403 from Data Plane: the room has no knowledge binding. 400 with a cursor: bad cursor.
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount_read_decision(&server).await;
        mount(&server, "POST", "/v1/knowledge/space-sources", ResponseTemplate::new(403)).await;
        let state = state_with(&server, true);
        let (_, body) = flat(list_core(&state, &user, &headers, SPACE, None, None).await);
        assert_eq!(body["data"]["unavailable"][0]["code"], "files_binding_unavailable");
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount_read_decision(&server).await;
        mount(&server, "POST", "/v1/knowledge/space-sources", ResponseTemplate::new(400)).await;
        let state = state_with(&server, true);
        let (status, body) = flat(list_core(&state, &user, &headers, SPACE, None, Some("c")).await);
        assert_eq!((status, body["error"]["code"].clone()), (400, json!("invalid_cursor")));
    }

    #[tokio::test]
    async fn a_control_outage_on_the_read_decision_is_room_files_unavailable() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount(
            &server,
            "POST",
            "/api/v1/internal/spaces/retrieval-decision",
            ResponseTemplate::new(500).set_body_string("down"),
        )
        .await;
        let state = state_with(&server, true);
        let (status, body) = flat(
            list_core(&state, &member("files-user"), &HeaderMap::new(), SPACE, None, None).await,
        );
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "room_files_unavailable");
    }

    // ----- R4: text, R5: delete --------------------------------------------------------------

    #[tokio::test]
    async fn read_text_returns_the_text_and_caps_max_chars() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount_read_decision(&server).await;
        mount(
            &server,
            "POST",
            "/v1/knowledge/space-document",
            json_ok(
                200,
                json!({ "document_id": "doc-1", "file_name": "a.txt", "text": "hello",
                        "text_chars": 5, "truncated": false }),
            ),
        )
        .await;
        let state = state_with(&server, true);
        let user = member("files-user");
        let headers = HeaderMap::new();
        let (status, body) =
            flat(read_text_core(&state, &user, &headers, SPACE, "doc-1", Some("99999999")).await);
        assert_eq!(status, 200);
        assert_eq!(body["data"]["text"], "hello");
        assert_eq!(body["data"]["truncated"], false);
        let sent = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|request| request.url.path() == "/v1/knowledge/space-document")
            .unwrap();
        let asked: Value = serde_json::from_slice(&sent.body).unwrap();
        assert_eq!(asked, json!({ "document_id": "doc-1", "max_chars": 1_000_000 }));
        for bad in ["0", "abc", "-5"] {
            let (status, body) =
                flat(read_text_core(&state, &user, &headers, SPACE, "doc-1", Some(bad)).await);
            assert_eq!((status, body["error"]["code"].clone()), (400, json!("invalid_max_chars")));
        }
        let (status, body) = flat(read_text_core(&state, &user, &headers, SPACE, "a/b", None).await);
        assert_eq!((status, body["error"]["code"].clone()), (400, json!("invalid_document")));
    }

    #[tokio::test]
    async fn read_text_reads_data_plane_404_403_and_400_as_file_not_found_and_a_decline_as_403() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let user = member("files-user");
        let headers = HeaderMap::new();
        for upstream in [404u16, 403, 400] {
            let server = MockServer::start().await;
            mount_basics(&server, false).await;
            mount_read_decision(&server).await;
            mount(&server, "POST", "/v1/knowledge/space-document", ResponseTemplate::new(upstream)).await;
            let state = state_with(&server, true);
            let (status, body) =
                flat(read_text_core(&state, &user, &headers, SPACE, "doc-1", None).await);
            assert_eq!(status, 404, "{upstream}");
            assert_eq!(body["error"]["code"], "file_not_found");
        }
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount_read_decision(&server).await;
        mount(&server, "POST", "/v1/knowledge/space-document", ResponseTemplate::new(500)).await;
        let state = state_with(&server, true);
        let (status, body) =
            flat(read_text_core(&state, &user, &headers, SPACE, "doc-1", None).await);
        assert_eq!((status, body["error"]["code"].clone()), (503, json!("room_files_unavailable")));

        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount(
            &server,
            "POST",
            "/api/v1/internal/spaces/retrieval-decision",
            json_ok(403, json!({ "error": { "code": "space_membership_required", "message": "m" } })),
        )
        .await;
        let state = state_with(&server, true);
        let (status, body) =
            flat(read_text_core(&state, &user, &headers, SPACE, "doc-1", None).await);
        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "space_files_not_authorized");
        assert_eq!(requests_to(&server, "space-document").await, 0);
    }

    #[tokio::test]
    async fn delete_preflights_the_room_document_and_answers_204() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_basics(&server, false).await;
        mount_read_decision(&server).await;
        mount(
            &server,
            "POST",
            "/v1/knowledge/space-document",
            json_ok(200, json!({ "document_id": "doc-1", "text": "x" })),
        )
        .await;
        mount(&server, "DELETE", "/v1/documents/doc-1", ResponseTemplate::new(200)).await;
        let state = state_with(&server, true);
        let (status, body) = flat(
            delete_core(&state, &member("files-user"), &HeaderMap::new(), SPACE, "doc-1").await,
        );
        assert_eq!(status, 204);
        assert_eq!(body["data"]["deleted"], true);
        let sent = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|request| request.url.path() == "/v1/knowledge/space-document")
            .unwrap();
        let asked: Value = serde_json::from_slice(&sent.body).unwrap();
        assert_eq!(asked["max_chars"], 1, "the preflight reads one character");
        assert_eq!(requests_to(&server, "/v1/documents/doc-1").await, 1);
    }

    #[tokio::test]
    async fn delete_reads_a_foreign_hidden_or_not_yours_document_as_404_without_deleting() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let user = member("files-user");
        let headers = HeaderMap::new();
        // The preflight cannot see the document in this room.
        for upstream in [404u16, 403] {
            let server = MockServer::start().await;
            mount_basics(&server, false).await;
            mount_read_decision(&server).await;
            mount(&server, "POST", "/v1/knowledge/space-document", ResponseTemplate::new(upstream)).await;
            mount(&server, "DELETE", "/v1/documents/doc-1", ResponseTemplate::new(200)).await;
            let state = state_with(&server, true);
            let (status, body) = flat(delete_core(&state, &user, &headers, SPACE, "doc-1").await);
            assert_eq!(status, 404, "preflight {upstream}");
            assert_eq!(body["error"]["code"], "file_not_found");
            assert_eq!(requests_to(&server, "/v1/documents/").await, 0);
        }
        // documents-api applies the owner check: 403 and 404 both read as file_not_found.
        for upstream in [404u16, 403] {
            let server = MockServer::start().await;
            mount_basics(&server, false).await;
            mount_read_decision(&server).await;
            mount(
                &server,
                "POST",
                "/v1/knowledge/space-document",
                json_ok(200, json!({ "document_id": "doc-1", "text": "x" })),
            )
            .await;
            mount(&server, "DELETE", "/v1/documents/doc-1", ResponseTemplate::new(upstream)).await;
            let state = state_with(&server, true);
            let (status, body) = flat(delete_core(&state, &user, &headers, SPACE, "doc-1").await);
            assert_eq!(status, 404, "delete {upstream}");
            assert_eq!(body["error"]["code"], "file_not_found");
        }
    }
}
