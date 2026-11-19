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
    "zdr_durable_mutation_forbidden",
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
    let idempotency_key = restore_idempotency_key(&body.client_save_id);
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

/// The decision's idempotency key for a save.
fn save_idempotency_key(client_save_id: &str) -> String {
    format!("canvas-save-{client_save_id}")
}

/// The decision's idempotency key for a restore (W3-FIX-9): the prefix names the action, so a restore never
/// shares a key with a save that carries the same client id. wiki-store only bounds the key's length.
fn restore_idempotency_key(client_save_id: &str) -> String {
    format!("canvas-restore-{client_save_id}")
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

#[cfg(test)]
mod tests {
    //! WS19-S4, WS19-S6, W3-DEC-WIKIZDR and W3-FIX-9 through the real router. Session, Control (user-core), org-core,
    //! Convex and wiki-store are doubles. Control answers each canvas decision by echoing the request's own Space, action
    //! and idempotency key, so every derivation (create, save, restore and the read) is read off the wire.
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        Json,
    };
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, Request as WmRequest, Respond, ResponseTemplate};

    use super::*;

    const SPACE: &str = "space-room";
    const CANVAS: &str = "canvas-1";
    const TOKEN: &str = "decision-token-secret-xyz";
    const COOKIE: &str = "better-auth.session_token=session-1";
    const SAVE_ID: &str = "save-client-0000001";
    const RESTORE_ID: &str = "restore-client-0001";
    const CANVAS_URL: &str = "/api/v1/spaces/space-room/canvas";
    const VERSIONS_URL: &str = "/api/v1/spaces/space-room/canvas/versions";
    const RESTORE_URL: &str = "/api/v1/spaces/space-room/canvas/versions/ver-1/restore";
    const WIKI_HEAD: &str = "/v1/space-canvas/spaces/space-room";
    const WIKI_VERSIONS: &str = "/v1/space-canvas/spaces/space-room/versions";
    const WIKI_RESTORE: &str = "/v1/space-canvas/spaces/space-room/versions/ver-1/restore";
    const CONTROL_DECISION: &str = "/api/v1/internal/spaces/canvas-decision";

    /// Every double a canvas call reaches. `decision` is Control's answer to each canvas decision; `None` grants each
    /// one by echo. The canvas flag is on; a test that needs it off sets it.
    struct Fixture {
        state: AppState,
        user_core: MockServer,
        org_core: MockServer,
        wiki: MockServer,
        _others: Vec<MockServer>,
    }

    /// Control's answer to a canvas decision that matches its request: the request's own Space, action and key.
    struct EchoDecision;

    impl Respond for EchoDecision {
        fn respond(&self, request: &WmRequest) -> ResponseTemplate {
            let body: Value = request.body_json().unwrap_or(Value::Null);
            let action = if body["op"] == "read" {
                "data.canvas.read"
            } else {
                "data.canvas.write"
            };
            ResponseTemplate::new(200).set_body_json(json!({"data": {
                "token": TOKEN,
                "decision": {
                    "space_ref": body["space_ref"],
                    "action_id": action,
                    "idempotency_key": body["idempotency_key"],
                }
            }}))
        }
    }

    async fn fixture(org_zdr: bool, decision: Option<ResponseTemplate>) -> Fixture {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/data-plane/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"token": "data-plane-token"})))
            .mount(&auth)
            .await;
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": "member", "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"role": "manager"}})))
            .mount(&user_core)
            .await;
        let decisions = Mock::given(wm_method("POST")).and(wm_path(CONTROL_DECISION));
        match decision {
            Some(answer) => decisions.respond_with(answer).mount(&user_core).await,
            None => decisions.respond_with(EchoDecision).mount(&user_core).await,
        }
        let org_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(org_record(org_zdr))
            .mount(&org_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                {"spaceRef": SPACE, "name": "Room", "kind": "room", "lifecycle": "active"}
            ]})))
            .mount(&application)
            .await;
        let wiki = MockServer::start().await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.org_core_url = org_core.uri();
        state.application_convex_url = application.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.wiki_store_url = wiki.uri();
        state.space_features.canvas_api = true;
        Fixture {
            state,
            user_core,
            org_core,
            wiki,
            _others: vec![auth, application],
        }
    }

    fn org_record(zdr: bool) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_json(json!({"data": {"metadata": {"interactiveRetention": {"zdr": zdr}}}}))
    }

    /// wiki-store's head: a canvas and its current version.
    fn canvas_head() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"data": {
            "canvas": {"canvas_id": CANVAS, "space_ref": SPACE},
            "head": {"version_id": "ver-1"}
        }}))
    }

    /// wiki-store's answer for a Space with no canvas yet.
    fn no_canvas() -> ResponseTemplate {
        ResponseTemplate::new(404).set_body_json(
            json!({"error": {"code": "canvas_not_found", "message": "The Space has no canvas."}}),
        )
    }

    /// wiki-store's answer to a write that landed.
    fn saved() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"data": {"version": {"version_id": "ver-2"}}}))
    }

    async fn mount_wiki(wiki: &MockServer, verb: &str, route: &str, answer: ResponseTemplate) {
        Mock::given(wm_method(verb))
            .and(wm_path(route))
            .respond_with(answer)
            .mount(wiki)
            .await;
    }

    /// One request through the real router with the session cookie. Returns the status and the raw body.
    async fn call(
        state: AppState,
        method: &str,
        uri: &str,
        extra: &[(&str, &str)],
        body: Option<Value>,
    ) -> (u16, Vec<u8>) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", COOKIE);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        for (name, value) in extra {
            builder = builder.header(*name, *value);
        }
        let request = builder
            .body(Body::from(body.map(|value| value.to_string()).unwrap_or_default()))
            .expect("a request");
        let response = crate::build_router(state)
            .oneshot(request)
            .await
            .expect("the router answers");
        let status = response.status().as_u16();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("a body")
            .to_bytes()
            .to_vec();
        (status, bytes)
    }

    fn json_of(bytes: &[u8]) -> Value {
        serde_json::from_slice(bytes).unwrap_or(Value::Null)
    }

    /// The JSON bodies a server recorded for one method and one exact path.
    async fn bodies_of(server: &MockServer, verb: &str, path: &str) -> Vec<Value> {
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|request| request.method.as_str() == verb && request.url.path() == path)
            .map(|request| request.body_json::<Value>().unwrap_or(Value::Null))
            .collect()
    }

    /// Every canvas decision Control was asked for, reads and writes alike.
    async fn decision_bodies(fixture: &Fixture) -> Vec<Value> {
        bodies_of(&fixture.user_core, "POST", CONTROL_DECISION).await
    }

    /// The write decisions only: create, save and restore.
    async fn write_decisions(fixture: &Fixture) -> Vec<Value> {
        decision_bodies(fixture)
            .await
            .into_iter()
            .filter(|body| body["op"] != "read")
            .collect()
    }

    /// Calls Control receives for a Space, whatever the path under `/internal/spaces/`.
    async fn control_space_calls(fixture: &Fixture) -> usize {
        fixture
            .user_core
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|request| request.url.path().contains("/internal/spaces/"))
            .count()
    }

    async fn wiki_calls(fixture: &Fixture) -> usize {
        fixture.wiki.received_requests().await.unwrap_or_default().len()
    }

    fn save_body(content: &str) -> Value {
        json!({"content": content, "client_save_id": SAVE_ID})
    }

    fn restore_body() -> Value {
        json!({"client_save_id": RESTORE_ID})
    }

    /// The `sha256:` commitment wiki-store recomputes from the body text.
    fn sha256_of(content: &str) -> String {
        format!("sha256:{}", hex::encode(Sha256::digest(content.as_bytes())))
    }

    // ---- WS19-S4: the flag, the zero-retention gate and the refusals -------------------------------------

    #[tokio::test]
    async fn the_canvas_is_404_and_calls_nothing_while_its_flag_is_off() {
        let mut fixture = fixture(false, None).await;
        fixture.state.space_features.canvas_api = false;
        mount_wiki(&fixture.wiki, "GET", WIKI_HEAD, canvas_head()).await;
        for (method, uri, body) in [
            ("GET", CANVAS_URL, None),
            ("GET", VERSIONS_URL, None),
            ("POST", VERSIONS_URL, Some(save_body("text"))),
            ("POST", RESTORE_URL, Some(restore_body())),
        ] {
            let (status, bytes) = call(fixture.state.clone(), method, uri, &[], body).await;
            assert_eq!(status, 404, "{method} {uri}");
            assert_eq!(json_of(&bytes)["error"]["code"], "space_canvas_disabled", "{method} {uri}");
        }
        assert_eq!(control_space_calls(&fixture).await, 0);
        assert_eq!(wiki_calls(&fixture).await, 0);
        assert!(fixture.org_core.received_requests().await.unwrap_or_default().is_empty());
    }

    #[tokio::test]
    async fn a_request_or_an_org_under_zero_retention_is_412_before_any_control_call() {
        let cases: Vec<(bool, Vec<(&str, &str)>)> = vec![(true, vec![]), (false, vec![("x-zdr", "true")])];
        for (org_zdr, extra) in cases {
            let fixture = fixture(org_zdr, None).await;
            mount_wiki(&fixture.wiki, "GET", WIKI_HEAD, canvas_head()).await;
            for (method, uri, body) in [
                ("GET", CANVAS_URL, None),
                ("POST", VERSIONS_URL, Some(save_body("text"))),
            ] {
                let (status, bytes) = call(fixture.state.clone(), method, uri, &extra, body).await;
                let answer = json_of(&bytes);
                assert_eq!(status, 412, "org_zdr={org_zdr} {method} {uri}");
                assert_eq!(answer["error"]["code"], "zdr_durable_mutation_forbidden", "org_zdr={org_zdr} {method} {uri}");
                assert_eq!(answer["error"]["details"]["surface"], "room_canvas", "org_zdr={org_zdr} {method} {uri}");
            }
            assert_eq!(control_space_calls(&fixture).await, 0, "org_zdr={org_zdr}");
            assert_eq!(wiki_calls(&fixture).await, 0, "org_zdr={org_zdr}");
        }
    }

    #[tokio::test]
    async fn a_space_with_no_canvas_answers_200_with_a_null_canvas_and_head() {
        let fixture = fixture(false, None).await;
        mount_wiki(&fixture.wiki, "GET", WIKI_HEAD, no_canvas()).await;
        let (status, bytes) = call(fixture.state.clone(), "GET", CANVAS_URL, &[], None).await;
        assert_eq!(status, 200);
        let answer = json_of(&bytes);
        assert!(answer["data"]["canvas"].is_null());
        assert!(answer["data"]["head"].is_null());
        assert_eq!(answer["data"]["can_write"], true);
        // The read decision names the read action and carries no canvas id and no expected version.
        let reads = decision_bodies(&fixture).await;
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0]["op"], "read");
        assert!(reads[0]["canvas_id"].is_null());
        assert!(reads[0]["expected_version_id"].is_null());
    }

    #[tokio::test]
    async fn a_control_refusal_of_the_read_is_canvas_read_not_authorized_with_its_reason_and_no_token() {
        let refused = ResponseTemplate::new(403)
            .set_body_json(json!({"code": "space_read_denied", "error": "Not allowed."}));
        let fixture = fixture(false, Some(refused)).await;
        mount_wiki(&fixture.wiki, "GET", WIKI_HEAD, canvas_head()).await;
        let (status, bytes) = call(fixture.state.clone(), "GET", CANVAS_URL, &[], None).await;
        assert_eq!(status, 403);
        let answer = json_of(&bytes);
        assert_eq!(answer["error"]["code"], "canvas_read_not_authorized");
        assert_eq!(answer["error"]["details"]["reason"], "space_read_denied");
        assert!(answer.get("data").is_none());
        assert!(!String::from_utf8_lossy(&bytes).contains(TOKEN));
        // Refused at the read decision, so the Data Plane is never reached.
        assert_eq!(wiki_calls(&fixture).await, 0);
    }

    #[tokio::test]
    async fn a_control_outage_or_a_400_is_space_decision_unavailable() {
        for upstream in [400_u16, 500, 503] {
            let answer_body = json!({"error": {"code": "upstream_error", "message": "Down."}});
            let fixture = fixture(false, Some(ResponseTemplate::new(upstream).set_body_json(answer_body))).await;
            mount_wiki(&fixture.wiki, "GET", WIKI_HEAD, canvas_head()).await;
            let (status, bytes) = call(fixture.state.clone(), "GET", CANVAS_URL, &[], None).await;
            assert_eq!(status, 503, "Control answered {upstream}");
            assert_eq!(json_of(&bytes)["error"]["code"], "space_decision_unavailable", "Control answered {upstream}");
            assert_eq!(wiki_calls(&fixture).await, 0, "Control answered {upstream}");
        }
    }

    #[tokio::test]
    async fn a_body_field_outside_the_contract_is_400_invalid_canvas_request_and_nothing_is_sent() {
        let fixture = fixture(false, None).await;
        mount_wiki(&fixture.wiki, "GET", WIKI_HEAD, canvas_head()).await;
        let mut save = save_body("text");
        save["authority"] = json!("forged");
        let (status, bytes) = call(fixture.state.clone(), "POST", VERSIONS_URL, &[], Some(save)).await;
        assert_eq!(status, 400);
        assert_eq!(json_of(&bytes)["error"]["code"], "invalid_canvas_request");
        let mut restore = restore_body();
        restore["grant_id"] = json!("forged");
        let (status, bytes) = call(fixture.state.clone(), "POST", RESTORE_URL, &[], Some(restore)).await;
        assert_eq!(status, 400);
        assert_eq!(json_of(&bytes)["error"]["code"], "invalid_canvas_request");
        assert_eq!(control_space_calls(&fixture).await, 0);
        assert_eq!(wiki_calls(&fixture).await, 0);
    }

    #[tokio::test]
    async fn an_empty_expected_version_id_is_refused_before_any_control_call() {
        let fixture = fixture(false, None).await;
        mount_wiki(&fixture.wiki, "GET", WIKI_HEAD, canvas_head()).await;
        let mut save = save_body("text");
        save["expected_version_id"] = json!("");
        let (status, bytes) = call(fixture.state.clone(), "POST", VERSIONS_URL, &[], Some(save)).await;
        assert_eq!(status, 400);
        assert_eq!(json_of(&bytes)["error"]["code"], "invalid_canvas_request");
        let mut restore = restore_body();
        restore["expected_version_id"] = json!("");
        let (status, bytes) = call(fixture.state.clone(), "POST", RESTORE_URL, &[], Some(restore)).await;
        assert_eq!(status, 400);
        assert_eq!(json_of(&bytes)["error"]["code"], "invalid_canvas_request");
        assert_eq!(control_space_calls(&fixture).await, 0);
        assert_eq!(wiki_calls(&fixture).await, 0);
    }

    #[tokio::test]
    async fn a_client_save_id_outside_the_pattern_is_refused_before_any_control_call() {
        let fixture = fixture(false, None).await;
        mount_wiki(&fixture.wiki, "GET", WIKI_HEAD, canvas_head()).await;
        // Too short (15), too long (65) and a character outside `[A-Za-z0-9_-]`.
        for bad_id in ["a".repeat(15), "a".repeat(65), format!("{}!", "a".repeat(20))] {
            let save = json!({"content": "text", "client_save_id": bad_id});
            let (status, bytes) = call(fixture.state.clone(), "POST", VERSIONS_URL, &[], Some(save)).await;
            assert_eq!(status, 400, "save with client_save_id of {} characters", bad_id.len());
            assert_eq!(json_of(&bytes)["error"]["code"], "invalid_canvas_request");
            let restore = json!({"client_save_id": bad_id});
            let (status, bytes) = call(fixture.state.clone(), "POST", RESTORE_URL, &[], Some(restore)).await;
            assert_eq!(status, 400, "restore with client_save_id of {} characters", bad_id.len());
            assert_eq!(json_of(&bytes)["error"]["code"], "invalid_canvas_request");
        }
        assert_eq!(control_space_calls(&fixture).await, 0);
        assert_eq!(wiki_calls(&fixture).await, 0);
    }

    #[tokio::test]
    async fn a_version_of_65536_bytes_is_accepted_and_one_more_byte_is_413_with_its_limit() {
        let fixture = fixture(false, None).await;
        mount_wiki(&fixture.wiki, "GET", WIKI_HEAD, no_canvas()).await;
        mount_wiki(&fixture.wiki, "POST", WIKI_VERSIONS, saved()).await;
        let at_limit = "a".repeat(65_536);
        let (status, _) = call(fixture.state.clone(), "POST", VERSIONS_URL, &[], Some(save_body(&at_limit))).await;
        assert_eq!(status, 200);
        let sent = bodies_of(&fixture.wiki, "POST", WIKI_VERSIONS).await;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["content"].as_str().map(str::len), Some(65_536));

        let before = decision_bodies(&fixture).await.len();
        let over = "a".repeat(65_537);
        let (status, bytes) = call(fixture.state.clone(), "POST", VERSIONS_URL, &[], Some(save_body(&over))).await;
        let answer = json_of(&bytes);
        assert_eq!(status, 413);
        assert_eq!(answer["error"]["code"], "canvas_too_large");
        assert_eq!(answer["error"]["details"]["max_bytes"], 65_536);
        assert_eq!(decision_bodies(&fixture).await.len(), before, "an oversize version reaches no Control call");
        assert_eq!(bodies_of(&fixture.wiki, "POST", WIKI_VERSIONS).await.len(), 1);
    }

    // ---- WS19-S4: the derivation of each write, read off the decision on the wire ---------------------------

    #[tokio::test]
    async fn a_save_with_no_active_canvas_is_a_create_with_null_ids_and_the_content_digest() {
        let fixture = fixture(false, None).await;
        mount_wiki(&fixture.wiki, "GET", WIKI_HEAD, no_canvas()).await;
        mount_wiki(&fixture.wiki, "POST", WIKI_VERSIONS, saved()).await;
        let content = "The room canvas text.";
        let mut save = save_body(content);
        save["expected_version_id"] = json!("ver-old");
        let (status, _) = call(fixture.state.clone(), "POST", VERSIONS_URL, &[], Some(save)).await;
        assert_eq!(status, 200);
        let writes = write_decisions(&fixture).await;
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0]["op"], "create");
        assert!(writes[0]["canvas_id"].is_null());
        assert!(writes[0]["expected_version_id"].is_null());
        assert_eq!(writes[0]["content_digest"], sha256_of(content));
    }

    #[tokio::test]
    async fn a_save_over_an_active_canvas_is_a_save_with_its_id_and_the_expected_version() {
        let fixture = fixture(false, None).await;
        mount_wiki(&fixture.wiki, "GET", WIKI_HEAD, canvas_head()).await;
        mount_wiki(&fixture.wiki, "POST", WIKI_VERSIONS, saved()).await;
        let mut save = save_body("text");
        save["expected_version_id"] = json!("ver-1");
        let (status, _) = call(fixture.state.clone(), "POST", VERSIONS_URL, &[], Some(save)).await;
        assert_eq!(status, 200);
        let writes = write_decisions(&fixture).await;
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0]["op"], "save");
        assert_eq!(writes[0]["canvas_id"], CANVAS);
        assert_eq!(writes[0]["expected_version_id"], "ver-1");
        assert!(writes[0]["restore_from_version_id"].is_null());
    }

    #[tokio::test]
    async fn a_save_sends_canvas_save_as_its_idempotency_key_on_the_decision_and_the_wire_body() {
        let expected = format!("canvas-save-{SAVE_ID}");
        for head in [canvas_head(), no_canvas()] {
            let fixture = fixture(false, None).await;
            mount_wiki(&fixture.wiki, "GET", WIKI_HEAD, head).await;
            mount_wiki(&fixture.wiki, "POST", WIKI_VERSIONS, saved()).await;
            let (status, _) = call(fixture.state.clone(), "POST", VERSIONS_URL, &[], Some(save_body("text"))).await;
            assert_eq!(status, 200);
            let writes = write_decisions(&fixture).await;
            assert_eq!(writes.len(), 1);
            assert_eq!(writes[0]["idempotency_key"], expected);
            let wire = bodies_of(&fixture.wiki, "POST", WIKI_VERSIONS).await;
            assert_eq!(wire.len(), 1);
            assert_eq!(wire[0]["idempotency_key"], expected);
        }
    }

    #[tokio::test]
    async fn a_restore_with_no_canvas_is_404_canvas_not_found_and_sends_no_write_decision() {
        let fixture = fixture(false, None).await;
        mount_wiki(&fixture.wiki, "GET", WIKI_HEAD, no_canvas()).await;
        mount_wiki(&fixture.wiki, "POST", WIKI_RESTORE, saved()).await;
        let (status, bytes) = call(fixture.state.clone(), "POST", RESTORE_URL, &[], Some(restore_body())).await;
        assert_eq!(status, 404);
        assert_eq!(json_of(&bytes)["error"]["code"], "canvas_not_found");
        let sent = decision_bodies(&fixture).await;
        assert_eq!(sent.len(), 1, "the read decision only");
        assert_eq!(sent[0]["op"], "read");
        assert!(bodies_of(&fixture.wiki, "POST", WIKI_RESTORE).await.is_empty());
    }

    /// A Data Plane 409 is relayed with its head, and no answer the gateway gives carries the decision token.
    #[tokio::test]
    async fn a_data_plane_409_head_moved_is_relayed_with_its_head_and_no_answer_carries_the_decision_token() {
        let moved = ResponseTemplate::new(409).set_body_json(json!({"error": {
            "code": "canvas_head_moved",
            "message": "The canvas moved.",
            "details": {"head": {"version_id": "ver-9"}}
        }}));
        let conflict = fixture(false, None).await;
        mount_wiki(&conflict.wiki, "GET", WIKI_HEAD, canvas_head()).await;
        mount_wiki(&conflict.wiki, "POST", WIKI_VERSIONS, moved).await;
        let (status, bytes) = call(conflict.state.clone(), "POST", VERSIONS_URL, &[], Some(save_body("text"))).await;
        let answer = json_of(&bytes);
        assert_eq!(status, 409);
        assert_eq!(answer["error"]["code"], "canvas_head_moved");
        assert_eq!(answer["error"]["details"]["head"]["version_id"], "ver-9");
        assert!(!String::from_utf8_lossy(&bytes).contains(TOKEN));

        let success = fixture(false, None).await;
        mount_wiki(&success.wiki, "GET", WIKI_HEAD, canvas_head()).await;
        mount_wiki(&success.wiki, "POST", WIKI_VERSIONS, saved()).await;
        let (read_status, read) = call(success.state.clone(), "GET", CANVAS_URL, &[], None).await;
        let (written_status, written) =
            call(success.state.clone(), "POST", VERSIONS_URL, &[], Some(save_body("text"))).await;
        assert_eq!(read_status, 200);
        assert_eq!(written_status, 200);
        assert!(!String::from_utf8_lossy(&read).contains(TOKEN));
        assert!(!String::from_utf8_lossy(&written).contains(TOKEN));
    }

    // ---- WS19-S6 and W3-DEC-WIKIZDR: the Data Plane's typed refusals are relayed -----------------------------

    #[tokio::test]
    async fn a_data_plane_403_history_floor_is_relayed_with_its_code_and_not_as_canvas_unavailable() {
        let fixture = fixture(false, None).await;
        let floor = ResponseTemplate::new(403).set_body_json(json!({"error": {
            "code": "space_history_floor_unsupported",
            "message": "This canvas cannot show its history."
        }}));
        mount_wiki(&fixture.wiki, "GET", WIKI_VERSIONS, floor).await;
        let (status, bytes) = call(fixture.state.clone(), "GET", VERSIONS_URL, &[], None).await;
        assert_eq!(status, 403);
        assert_eq!(json_of(&bytes)["error"]["code"], "space_history_floor_unsupported");
    }

    #[test]
    fn a_data_plane_412_zdr_refusal_is_relayed_unchanged_and_an_unlisted_refusal_is_canvas_unavailable() {
        let refusal = json!({"error": {
            "code": "zdr_durable_mutation_forbidden",
            "message": "Zero Data Retention credentials cannot create, change, or delete durable state.",
            "retryable": false,
            "action": "none",
            "details": {"surface": "room_canvas"}
        }});
        let (status, Json(relayed)) = data_plane_refusal(StatusCode::PRECONDITION_FAILED, refusal.clone());
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(relayed, refusal);

        let (status, Json(answer)) =
            data_plane_refusal(StatusCode::FORBIDDEN, json!({"error": {"code": "not_a_canvas_code"}}));
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(answer["error"]["code"], "canvas_unavailable");
    }

    // ---- W3-FIX-9: a restore's key names the restore, a save's names the save -------------------------------

    #[tokio::test]
    async fn a_restore_sends_canvas_restore_as_its_idempotency_key_and_a_save_still_sends_canvas_save() {
        let fixture = fixture(false, None).await;
        mount_wiki(&fixture.wiki, "GET", WIKI_HEAD, canvas_head()).await;
        mount_wiki(&fixture.wiki, "POST", WIKI_RESTORE, saved()).await;
        mount_wiki(&fixture.wiki, "POST", WIKI_VERSIONS, saved()).await;

        let (status, _) = call(fixture.state.clone(), "POST", RESTORE_URL, &[], Some(restore_body())).await;
        assert_eq!(status, 200);
        let restore_key = format!("canvas-restore-{RESTORE_ID}");
        let writes = write_decisions(&fixture).await;
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0]["op"], "restore");
        assert_eq!(writes[0]["canvas_id"], CANVAS);
        assert_eq!(writes[0]["restore_from_version_id"], "ver-1");
        assert_eq!(writes[0]["idempotency_key"], restore_key);
        let wire = bodies_of(&fixture.wiki, "POST", WIKI_RESTORE).await;
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0]["idempotency_key"], restore_key);

        let (status, _) = call(fixture.state.clone(), "POST", VERSIONS_URL, &[], Some(save_body("text"))).await;
        assert_eq!(status, 200);
        let saves: Vec<Value> = write_decisions(&fixture)
            .await
            .into_iter()
            .filter(|body| body["op"] == "save")
            .collect();
        assert_eq!(saves.len(), 1);
        assert_eq!(saves[0]["idempotency_key"], format!("canvas-save-{SAVE_ID}"));
        let wire_saves = bodies_of(&fixture.wiki, "POST", WIKI_VERSIONS).await;
        assert_eq!(wire_saves.len(), 1);
        assert_eq!(wire_saves[0]["idempotency_key"], format!("canvas-save-{SAVE_ID}"));
    }
}
