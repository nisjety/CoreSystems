//! WS19-S4: the room canvas, the gateway side.
//!
//! Routes (merged by `module_routes()` in `spaces/mod.rs`, behind the session):
//! `GET /api/v1/spaces/{space_ref}/canvas?known_version_id=` (the head, `unchanged`, or no canvas yet),
//! `POST .../canvas/versions` (save), `GET .../canvas/versions?limit=&cursor=` (history),
//! `GET .../canvas/versions/{version_id}` (one version with its text) and
//! `POST .../canvas/versions/{version_id}/restore`. The two writes are also the actions
//! `spaces.save_canvas` and `spaces.restore_canvas_version` (dispatchers in `dispatchers_spaces/ws19.rs`).
//!
//! This module stores nothing and decides nothing about authority. Every call asks Control for its own
//! signed decision (`POST /api/v1/internal/spaces/canvas-decision`, under the caller's delegation) and
//! relays that decision to Data Plane (wiki-store `/v1/space-canvas/**`) in `X-Space-Decision`. Data Plane
//! verifies it and runs the head compare-and-swap. The decision token never reaches the browser.
//!
//! A write's decision names the canvas it changes, and this gateway keeps no state, so every write first
//! reads the head with a read decision and derives the operation exactly as wiki-store does: `create` with
//! an empty canvas id when the Space has no active canvas, `save` or `restore` with the active canvas id
//! otherwise. Read decisions carry an empty canvas id, and an absent expected version is JSON null on the
//! wire, never "". A mismatch makes wiki-store answer 403 `space_decision_digest_mismatch` (fail closed).
//!
//! Order of refusals on every route (each stops before the next): flag (404 `space_canvas_disabled`),
//! organization, Space reference, Zero Data Retention (typed 412 `zdr_durable_mutation_forbidden`, before
//! any Control call), request shape, Space lifecycle, the Control decisions, then the Data Plane call.
//! Control's refusals map to `canvas_read_not_authorized` or `canvas_write_not_authorized` (403) with
//! `details.reason`, to `canvas_not_entitled` or `active_membership_required`, or to the typed 409
//! `room_archived`; a Control outage or an unusable answer is 503 `space_decision_unavailable`. Data
//! Plane's typed refusals (`canvas_head_moved` with `details.head`, `canvas_too_large`, and so on) pass
//! through unchanged. Errors are `{error:{code,message,details}}`.
//!
//! The text is hashed here (`sha256:` over its bytes) so Control binds the content without seeing it.
//! Canvas text is never logged. `can_write` is informational: Control and Data Plane enforce the write
//! floor on every write.

use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use bytes::Bytes;
use reqwest::Method;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_json},
    zdr::{durable_write_refusal, zdr_refusal_body, ZDR_DURABLE_MUTATION_FORBIDDEN},
};

use super::{control_membership_url, denial, space_lifecycle_by_ref, unix_nanos};

type Reply = (StatusCode, Json<Value>);

/// wiki-store `canvas.MaxContentBytes`: one version's text, in bytes.
const MAX_CONTENT_BYTES: usize = 65_536;
/// wiki-store `canvas.MaxSummaryChars`, counted in characters after trimming.
const MAX_SUMMARY_CHARS: usize = 200;
/// wiki-store `canvas.MaxRequestBodyBytes` (128 KiB): the cap on one write body.
const MAX_BODY_BYTES: usize = 128 * 1024;
/// wiki-store `canvas.MaxVersionIDBytes` and `canvas.MaxSpaceRefBytes`.
const MAX_VERSION_ID_BYTES: usize = 128;
const MAX_SPACE_REF_BYTES: usize = 256;
/// The bounds of `client_save_id`, `^[A-Za-z0-9_-]{16,64}$`.
const CLIENT_SAVE_ID_MIN: usize = 16;
const CLIENT_SAVE_ID_MAX: usize = 64;
const ROOM_CANVAS_SURFACE: &str = "room_canvas";
const DELEGATED_AUTH_UNAVAILABLE: &str = "delegated_auth_unavailable";
/// The typed Data Plane answers the gateway relays unchanged. Any other failure is an outage.
const DATA_PLANE_CODES: &[&str] = &[
    "canvas_not_found",
    "canvas_version_not_found",
    "canvas_head_moved",
    "idempotency_key_conflict",
    "canvas_too_large",
    "invalid_canvas_request",
    "invalid_cursor",
    "canvas_rate_limited",
    "canvas_run_write_limit",
    "space_canvas_disabled",
    "zdr_persistence_forbidden",
    "space_decision_required",
    "space_decision_invalid",
    "space_decision_mismatch",
    "space_decision_digest_mismatch",
    "space_history_floor_unsupported",
];

/// The canvas routes, merged by `module_routes()` in `spaces/mod.rs`.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/spaces/{space_ref}/canvas", get(get_canvas))
        .route(
            "/api/v1/spaces/{space_ref}/canvas/versions",
            get(list_versions).post(save_version),
        )
        .route(
            "/api/v1/spaces/{space_ref}/canvas/versions/{version_id}",
            get(get_version),
        )
        .route(
            "/api/v1/spaces/{space_ref}/canvas/versions/{version_id}/restore",
            post(restore_version),
        )
}

/// The body of `POST .../canvas/versions`. Unknown fields are refused, so no authority field can ride along.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SaveBody {
    pub(crate) expected_version_id: Option<String>,
    pub(crate) content: String,
    pub(crate) summary: Option<String>,
    pub(crate) client_save_id: String,
}

/// The body of `POST .../canvas/versions/{version_id}/restore`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RestoreBody {
    pub(crate) expected_version_id: Option<String>,
    pub(crate) client_save_id: String,
}

#[derive(Debug, Deserialize)]
struct KnownQuery {
    known_version_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    limit: Option<String>,
    cursor: Option<String>,
}

async fn get_canvas(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
    Query(query): Query<KnownQuery>,
) -> Reply {
    read_canvas(
        &state,
        &user,
        &headers,
        &space_ref,
        query.known_version_id.as_deref(),
    )
    .await
}

async fn save_version(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
    body: Bytes,
) -> Reply {
    if !state.space_features.canvas_api {
        return feature_off();
    }
    match parse_body::<SaveBody>(&body) {
        Ok(body) => save_version_core(&state, &user, &headers, &space_ref, body).await,
        Err(reply) => reply,
    }
}

async fn list_versions(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
    Query(query): Query<ListQuery>,
) -> Reply {
    list_versions_inner_reply(
        &state,
        &user,
        &headers,
        &space_ref,
        query.limit.as_deref(),
        query.cursor.as_deref(),
    )
    .await
}

async fn get_version(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, version_id)): Path<(String, String)>,
) -> Reply {
    get_version_reply(&state, &user, &headers, &space_ref, &version_id).await
}

async fn restore_version(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, version_id)): Path<(String, String)>,
    body: Bytes,
) -> Reply {
    if !state.space_features.canvas_api {
        return feature_off();
    }
    match parse_body::<RestoreBody>(&body) {
        Ok(body) => {
            restore_version_core(&state, &user, &headers, &space_ref, &version_id, body).await
        }
        Err(reply) => reply,
    }
}

/// `GET .../canvas`: the head with its text, `unchanged` for a known head, or `canvas: null` for a Space
/// with no canvas yet.
async fn read_canvas(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    known_version_id: Option<&str>,
) -> Reply {
    unify(read_canvas_inner(state, user, headers, space_ref, known_version_id).await)
}

async fn read_canvas_inner(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    known_version_id: Option<&str>,
) -> Result<Reply, Reply> {
    let (org_id, space_ref) = pre_gate(state, user, space_ref).await?;
    zdr_gate(state, user, headers).await?;
    let known = match known_version_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => None,
        Some(value) if valid_version_id(value) => Some(value),
        Some(_) => return Err(invalid_request()),
    };
    lifecycle_gate(state, user, &org_id, &space_ref).await?;
    let read = read_decision(state, user, &org_id, &space_ref).await?;
    let mut url = format!(
        "{}/v1/space-canvas/spaces/{}",
        state.wiki_store_url,
        urlencoding::encode(&space_ref)
    );
    if let Some(known) = known {
        url.push_str("?known_version_id=");
        url.push_str(&urlencoding::encode(known));
    }
    let (status, Json(payload)) =
        proxy_wiki(state, user, headers, Method::GET, &url, None, &org_id, &read).await;
    if status == StatusCode::NOT_FOUND && data_plane_code(&payload) == Some("canvas_not_found") {
        let can_write = membership_can_write(state, user, &org_id, &space_ref).await;
        return Ok((
            StatusCode::OK,
            Json(json!({ "data": { "canvas": null, "head": null, "can_write": can_write } })),
        ));
    }
    if !status.is_success() {
        return Err(data_plane_refusal(status, payload));
    }
    let data = payload.get("data").cloned().unwrap_or(Value::Null);
    if data.get("unchanged").and_then(Value::as_bool) == Some(true) {
        return Ok((StatusCode::OK, Json(json!({ "data": data }))));
    }
    let can_write = membership_can_write(state, user, &org_id, &space_ref).await;
    Ok((
        StatusCode::OK,
        Json(json!({ "data": {
            "canvas": data.get("canvas").cloned().unwrap_or(Value::Null),
            "head": data.get("head").cloned().unwrap_or(Value::Null),
            "can_write": can_write,
        } })),
    ))
}

/// `POST .../canvas/versions` and `spaces.save_canvas`: a save over the active canvas, or the first
/// version of a Space that has none.
pub(crate) async fn save_version_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    body: SaveBody,
) -> Reply {
    unify(save_version_inner(state, user, headers, space_ref, body).await)
}

async fn save_version_inner(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    body: SaveBody,
) -> Result<Reply, Reply> {
    let (org_id, space_ref) = pre_gate(state, user, space_ref).await?;
    zdr_gate(state, user, headers).await?;
    let body = check_save(body)?;
    lifecycle_gate(state, user, &org_id, &space_ref).await?;
    let read = read_decision(state, user, &org_id, &space_ref).await?;
    let active = head_canvas_id(state, user, headers, &org_id, &space_ref, &read).await?;
    let idempotency_key = save_idempotency_key(&body.client_save_id);
    let (op, canvas_id, expected_version_id) = match active {
        Some(canvas_id) => ("save", Some(canvas_id), body.expected_version_id.clone()),
        None => ("create", None, None),
    };
    let write = canvas_decision(
        state,
        user,
        &org_id,
        &space_ref,
        Intent::Write,
        DecisionFields {
            op,
            canvas_id,
            expected_version_id,
            restore_from_version_id: None,
            content_digest: Some(content_digest(&body.content)),
            idempotency_key: idempotency_key.clone(),
        },
    )
    .await?;
    let url = format!(
        "{}/v1/space-canvas/spaces/{}/versions",
        state.wiki_store_url,
        urlencoding::encode(&space_ref)
    );
    let (status, Json(payload)) = proxy_wiki(
        state,
        user,
        headers,
        Method::POST,
        &url,
        Some(json!({
            "expected_version_id": body.expected_version_id,
            "content": body.content,
            "summary": body.summary,
            "idempotency_key": idempotency_key,
        })),
        &org_id,
        &write,
    )
    .await;
    relay(status, payload)
}

/// `POST .../canvas/versions/{version_id}/restore` and `spaces.restore_canvas_version`: copies that
/// version's text into a new head. A Space with no canvas has nothing to restore into.
pub(crate) async fn restore_version_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    version_id: &str,
    body: RestoreBody,
) -> Reply {
    unify(restore_version_inner(state, user, headers, space_ref, version_id, body).await)
}

async fn restore_version_inner(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    version_id: &str,
    body: RestoreBody,
) -> Result<Reply, Reply> {
    let (org_id, space_ref) = pre_gate(state, user, space_ref).await?;
    zdr_gate(state, user, headers).await?;
    if !valid_version_id(version_id) {
        return Err(invalid_request());
    }
    let body = check_restore(body)?;
    lifecycle_gate(state, user, &org_id, &space_ref).await?;
    let read = read_decision(state, user, &org_id, &space_ref).await?;
    let Some(canvas_id) = head_canvas_id(state, user, headers, &org_id, &space_ref, &read).await?
    else {
        return Err(canvas_not_found());
    };
    let idempotency_key = save_idempotency_key(&body.client_save_id);
    let write = canvas_decision(
        state,
        user,
        &org_id,
        &space_ref,
        Intent::Write,
        DecisionFields {
            op: "restore",
            canvas_id: Some(canvas_id),
            expected_version_id: body.expected_version_id.clone(),
            restore_from_version_id: Some(version_id.to_owned()),
            content_digest: None,
            idempotency_key: idempotency_key.clone(),
        },
    )
    .await?;
    let url = format!(
        "{}/v1/space-canvas/spaces/{}/versions/{}/restore",
        state.wiki_store_url,
        urlencoding::encode(&space_ref),
        urlencoding::encode(version_id)
    );
    let (status, Json(payload)) = proxy_wiki(
        state,
        user,
        headers,
        Method::POST,
        &url,
        Some(json!({
            "expected_version_id": body.expected_version_id,
            "idempotency_key": idempotency_key,
        })),
        &org_id,
        &write,
    )
    .await;
    relay(status, payload)
}

async fn list_versions_inner_reply(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    limit: Option<&str>,
    cursor: Option<&str>,
) -> Reply {
    unify(list_versions_inner(state, user, headers, space_ref, limit, cursor).await)
}

/// `GET .../canvas/versions`: one page of history, newest first, without text.
async fn list_versions_inner(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    limit: Option<&str>,
    cursor: Option<&str>,
) -> Result<Reply, Reply> {
    let (org_id, space_ref) = pre_gate(state, user, space_ref).await?;
    zdr_gate(state, user, headers).await?;
    let mut query = Vec::new();
    if let Some(limit) = limit.map(str::trim).filter(|value| !value.is_empty()) {
        if !is_digits(limit, 6) {
            return Err(invalid_request());
        }
        query.push(format!("limit={limit}"));
    }
    if let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) {
        if !is_digits(cursor, 12) || cursor.bytes().all(|byte| byte == b'0') {
            return Err(invalid_cursor());
        }
        query.push(format!("cursor={cursor}"));
    }
    lifecycle_gate(state, user, &org_id, &space_ref).await?;
    let read = read_decision(state, user, &org_id, &space_ref).await?;
    let mut url = format!(
        "{}/v1/space-canvas/spaces/{}/versions",
        state.wiki_store_url,
        urlencoding::encode(&space_ref)
    );
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query.join("&"));
    }
    let (status, Json(payload)) =
        proxy_wiki(state, user, headers, Method::GET, &url, None, &org_id, &read).await;
    relay(status, payload)
}

/// `GET .../canvas/versions/{version_id}`: one version with its text.
async fn get_version_reply(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    version_id: &str,
) -> Reply {
    unify(get_version_inner(state, user, headers, space_ref, version_id).await)
}

async fn get_version_inner(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    version_id: &str,
) -> Result<Reply, Reply> {
    let (org_id, space_ref) = pre_gate(state, user, space_ref).await?;
    zdr_gate(state, user, headers).await?;
    if !valid_version_id(version_id) {
        return Err(version_not_found());
    }
    lifecycle_gate(state, user, &org_id, &space_ref).await?;
    let read = read_decision(state, user, &org_id, &space_ref).await?;
    let url = format!(
        "{}/v1/space-canvas/spaces/{}/versions/{}",
        state.wiki_store_url,
        urlencoding::encode(&space_ref),
        urlencoding::encode(version_id)
    );
    let (status, Json(payload)) =
        proxy_wiki(state, user, headers, Method::GET, &url, None, &org_id, &read).await;
    relay(status, payload)
}

// ---------------------------------------------------------------------------------------------
// Gates
// ---------------------------------------------------------------------------------------------

fn unify(result: Result<Reply, Reply>) -> Reply {
    match result {
        Ok(reply) | Err(reply) => reply,
    }
}

fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

/// Flag, organization and Space reference: everything that needs no upstream call but the org lookup.
async fn pre_gate(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
) -> Result<(String, String), Reply> {
    if !state.space_features.canvas_api {
        return Err(feature_off());
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err(refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to use the room canvas.",
        ));
    }
    let space_ref = space_ref.trim();
    if !valid_space_ref(space_ref) {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A Space reference is required.",
        ));
    }
    Ok((org_id, space_ref.to_owned()))
}

/// Zero Data Retention refuses every canvas route, reads included, before any Control call.
async fn zdr_gate(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Result<(), Reply> {
    match durable_write_refusal(state, user, Some(headers), None).await {
        None => Ok(()),
        Some((status, Json(body))) => Err(label_zdr(status, body)),
    }
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

// ---------------------------------------------------------------------------------------------
// Request shape (no authority is read here)
// ---------------------------------------------------------------------------------------------

/// Parses a write body under the 128 KiB cap. Unknown fields and malformed JSON are a typed 400.
fn parse_body<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Reply> {
    if bytes.len() > MAX_BODY_BYTES {
        return Err(too_large());
    }
    serde_json::from_slice(bytes).map_err(|_| invalid_request())
}

fn check_save(mut body: SaveBody) -> Result<SaveBody, Reply> {
    if !valid_client_save_id(&body.client_save_id) {
        return Err(invalid_request());
    }
    check_expected(body.expected_version_id.as_deref())?;
    check_content(&body.content)?;
    body.summary = normalize_summary(body.summary.as_deref())?;
    Ok(body)
}

fn check_restore(body: RestoreBody) -> Result<RestoreBody, Reply> {
    if !valid_client_save_id(&body.client_save_id) {
        return Err(invalid_request());
    }
    check_expected(body.expected_version_id.as_deref())?;
    Ok(body)
}

fn check_expected(expected: Option<&str>) -> Result<(), Reply> {
    match expected {
        Some(value) if !valid_version_id(value) => Err(invalid_request()),
        _ => Ok(()),
    }
}

/// wiki-store's `ValidateContent`: 1 to 65,536 bytes with no NUL byte. Oversize is the typed 413.
fn check_content(content: &str) -> Result<(), Reply> {
    if content.len() > MAX_CONTENT_BYTES {
        return Err(too_large());
    }
    if content.is_empty() || content.contains('\0') {
        return Err(invalid_request());
    }
    Ok(())
}

/// wiki-store's `NormalizeSummary`: trimmed, empty is absent, at most 200 characters, no NUL byte.
fn normalize_summary(summary: Option<&str>) -> Result<Option<String>, Reply> {
    let Some(raw) = summary else {
        return Ok(None);
    };
    if raw.contains('\0') {
        return Err(invalid_request());
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > MAX_SUMMARY_CHARS {
        return Err(invalid_request());
    }
    Ok(Some(trimmed.to_owned()))
}

fn valid_space_ref(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SPACE_REF_BYTES
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '?' | '#' | '\\'))
}

fn valid_version_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_VERSION_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn valid_client_save_id(value: &str) -> bool {
    (CLIENT_SAVE_ID_MIN..=CLIENT_SAVE_ID_MAX).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn is_digits(value: &str, max_len: usize) -> bool {
    !value.is_empty() && value.len() <= max_len && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// The `sha256:` commitment wiki-store recomputes from the body text: lowercase hex of its bytes.
fn content_digest(content: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(content.as_bytes())))
}

/// The decision's idempotency key for a write, shared by save and restore.
fn save_idempotency_key(client_save_id: &str) -> String {
    format!("canvas-save-{client_save_id}")
}

// ---------------------------------------------------------------------------------------------
// Control decisions
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Intent {
    Read,
    Write,
}

impl Intent {
    fn action_id(self) -> &'static str {
        match self {
            Self::Read => "data.canvas.read",
            Self::Write => "data.canvas.write",
        }
    }

    fn refusal_code(self) -> &'static str {
        match self {
            Self::Read => "canvas_read_not_authorized",
            Self::Write => "canvas_write_not_authorized",
        }
    }
}

/// The fields of one Control decision request. An absent optional field is JSON null (Control reads "").
struct DecisionFields {
    op: &'static str,
    canvas_id: Option<String>,
    expected_version_id: Option<String>,
    restore_from_version_id: Option<String>,
    content_digest: Option<String>,
    idempotency_key: String,
}

impl DecisionFields {
    fn read() -> Self {
        Self {
            op: "read",
            canvas_id: None,
            expected_version_id: None,
            restore_from_version_id: None,
            content_digest: None,
            idempotency_key: format!("canvas-read-{}", unix_nanos()),
        }
    }
}

async fn read_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Result<String, Reply> {
    canvas_decision(state, user, org_id, space_ref, Intent::Read, DecisionFields::read()).await
}

/// Control's decision for one canvas operation, under the caller's own delegation. Returns the signed
/// token after checking that it answers this request. Refusals and outages are typed.
async fn canvas_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    intent: Intent,
    fields: DecisionFields,
) -> Result<String, Reply> {
    let actor = actor_for(user);
    let url = format!(
        "{}/api/v1/internal/spaces/canvas-decision",
        state.user_core_url
    );
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_ref": space_ref,
            "op": fields.op,
            "canvas_id": fields.canvas_id,
            "expected_version_id": fields.expected_version_id,
            "restore_from_version_id": fields.restore_from_version_id,
            "content_digest": fields.content_digest,
            "idempotency_key": fields.idempotency_key,
        })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    let status = denial::refusal_status(status, &response);
    if status == StatusCode::NOT_FOUND {
        return Err(refuse(
            StatusCode::NOT_FOUND,
            "space_canvas_disabled",
            "The room canvas is not enabled on this deployment.",
        ));
    }
    if status == StatusCode::CONFLICT
        && response.pointer("/error/code").and_then(Value::as_str) == Some("room_archived")
    {
        return Err(room_archived());
    }
    if status == StatusCode::FORBIDDEN {
        return Err(refused(intent, &response));
    }
    if !status.is_success() {
        return Err(decision_unavailable());
    }
    let data = unwrap_data(&response);
    let token = data
        .get("token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(decision_unavailable)?;
    let decision = data.get("decision").ok_or_else(decision_unavailable)?;
    let answers_request = decision.get("space_ref").and_then(Value::as_str) == Some(space_ref)
        && decision.get("action_id").and_then(Value::as_str) == Some(intent.action_id())
        && decision.get("idempotency_key").and_then(Value::as_str)
            == Some(fields.idempotency_key.as_str());
    if !answers_request {
        return Err(decision_unavailable());
    }
    Ok(token.to_owned())
}

/// Control's typed refusal, reduced to a browser code. The reason is Control's closed `space_*` token.
fn refused(intent: Intent, body: &Value) -> Reply {
    let reason = control_reason(body);
    match reason.as_str() {
        "space_zdr_forbids_effect" => zdr_reply(),
        "space_entitlement_off" | "space_policy_not_provisioned" => (
            StatusCode::FORBIDDEN,
            Json(error_with_details(
                "canvas_not_entitled",
                "The room canvas is not enabled for this organization or this room.",
                json!({ "reason": reason.clone() }),
            )),
        ),
        "space_membership_required" | "space_not_active" => (
            StatusCode::FORBIDDEN,
            Json(error_with_details(
                "active_membership_required",
                "An active room membership is required.",
                json!({ "reason": reason.clone() }),
            )),
        ),
        _ => (
            StatusCode::FORBIDDEN,
            Json(error_with_details(
                intent.refusal_code(),
                "The room canvas is not available to you.",
                json!({ "reason": reason.clone() }),
            )),
        ),
    }
}

/// Control's denial code when the body carries a well-formed one (a lowercase token), else a fixed word.
fn control_reason(body: &Value) -> String {
    body.get("code")
        .and_then(Value::as_str)
        .filter(|code| {
            !code.is_empty()
                && code.len() <= 64
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
        .unwrap_or("not_authorized")
        .to_owned()
}

/// Whether the caller may write, from Control's current membership. Informational: any failure is `false`.
async fn membership_can_write(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> bool {
    let url = control_membership_url(state, space_ref, true);
    let actor = actor_for(user);
    let (status, Json(response)) =
        proxy_json(state, Method::GET, &url, None, Some(org_id), Some(&actor), None).await;
    if !status.is_success() {
        return false;
    }
    let data = unwrap_data(&response);
    matches!(
        data.get("role").and_then(Value::as_str),
        Some("editor" | "manager" | "owner")
    )
}

/// The active canvas id for this Space, or `None` when it has no canvas yet. Read with the caller's read
/// decision, so the id is derived from the same head wiki-store will check.
async fn head_canvas_id(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    org_id: &str,
    space_ref: &str,
    read: &str,
) -> Result<Option<String>, Reply> {
    let url = format!(
        "{}/v1/space-canvas/spaces/{}",
        state.wiki_store_url,
        urlencoding::encode(space_ref)
    );
    let (status, Json(payload)) =
        proxy_wiki(state, user, headers, Method::GET, &url, None, org_id, read).await;
    if status == StatusCode::NOT_FOUND && data_plane_code(&payload) == Some("canvas_not_found") {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(data_plane_refusal(status, payload));
    }
    match payload
        .pointer("/data/canvas/canvas_id")
        .and_then(Value::as_str)
    {
        Some(id) if !id.is_empty() => Ok(Some(id.to_owned())),
        _ => Err(canvas_unavailable()),
    }
}

// ---------------------------------------------------------------------------------------------
// Data Plane
// ---------------------------------------------------------------------------------------------

/// One Data Plane call under a Control decision. The decision travels in `X-Space-Decision` and never
/// reaches the browser.
#[allow(clippy::too_many_arguments)]
async fn proxy_wiki(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    method: Method,
    url: &str,
    body: Option<Value>,
    org_id: &str,
    decision: &str,
) -> (StatusCode, Json<Value>) {
    crate::domains::knowledge::shared::proxy_data_plane_json_with_space_decision(
        state,
        user,
        headers,
        method,
        url,
        body,
        Some(org_id),
        decision,
    )
    .await
}

fn data_plane_code(payload: &Value) -> Option<&str> {
    payload.pointer("/error/code").and_then(Value::as_str)
}

/// A success is relayed as the Data Plane answered it. A refusal is relayed when its code is a typed canvas
/// answer; anything else is an outage the browser cannot act on.
fn relay(status: StatusCode, payload: Value) -> Result<Reply, Reply> {
    if status.is_success() {
        Ok((status, Json(payload)))
    } else {
        Err(data_plane_refusal(status, payload))
    }
}

fn data_plane_refusal(status: StatusCode, payload: Value) -> Reply {
    let relayed = {
        let code = data_plane_code(&payload).unwrap_or_default();
        (status.is_client_error() && DATA_PLANE_CODES.contains(&code))
            || (status == StatusCode::SERVICE_UNAVAILABLE && code == DELEGATED_AUTH_UNAVAILABLE)
    };
    if relayed {
        (status, Json(payload))
    } else {
        canvas_unavailable()
    }
}

// ---------------------------------------------------------------------------------------------
// Typed answers
// ---------------------------------------------------------------------------------------------

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn feature_off() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "space_canvas_disabled",
        "The room canvas is not enabled.",
    )
}

fn invalid_request() -> Reply {
    refuse(
        StatusCode::BAD_REQUEST,
        "invalid_canvas_request",
        "The canvas request is not valid.",
    )
}

fn invalid_cursor() -> Reply {
    refuse(
        StatusCode::BAD_REQUEST,
        "invalid_cursor",
        "The cursor must be a version number.",
    )
}

fn version_not_found() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "canvas_version_not_found",
        "The version is not in this canvas.",
    )
}

fn canvas_not_found() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "canvas_not_found",
        "The Space has no canvas.",
    )
}

fn room_archived() -> Reply {
    refuse(StatusCode::CONFLICT, "room_archived", "This room is archived.")
}

fn decision_unavailable() -> Reply {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "space_decision_unavailable",
        "Room authority could not be confirmed right now. Nothing was changed.",
    )
}

fn canvas_unavailable() -> Reply {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "canvas_unavailable",
        "The room canvas is temporarily unavailable. Nothing was changed.",
    )
}

fn too_large() -> Reply {
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        Json(error_with_details(
            "canvas_too_large",
            "The content is larger than the canvas limit.",
            json!({ "max_bytes": MAX_CONTENT_BYTES }),
        )),
    )
}

/// The typed ZDR refusal, labelled with the room canvas surface (the catalogue code is feature-less).
fn label_zdr(status: StatusCode, mut body: Value) -> Reply {
    if status == StatusCode::PRECONDITION_FAILED {
        if let Some(object) = body.get_mut("error").and_then(Value::as_object_mut) {
            object.insert(
                "details".to_owned(),
                json!({ "surface": ROOM_CANVAS_SURFACE }),
            );
        }
    }
    (status, Json(body))
}

fn zdr_reply() -> Reply {
    label_zdr(
        StatusCode::PRECONDITION_FAILED,
        zdr_refusal_body(ZDR_DURABLE_MUTATION_FORBIDDEN),
    )
}
