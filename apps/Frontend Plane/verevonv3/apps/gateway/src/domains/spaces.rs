//! Space authority reads exposed to Verevon clients.
//!
//! This intentionally exposes only the current Control membership fact. A
//! complete Space context needs recipient, privacy, entitlement, and
//! owner-resource decisions, so this module must not synthesize one from the
//! user's active organization or from a client-provided Space ID.

use std::sync::atomic::{AtomicU64, Ordering};

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::error,
    middleware::{require_session, AuthenticatedUser},
    upstream::proxy_json,
};

static THREAD_SCOPE_COUNTER: AtomicU64 = AtomicU64::new(1);

const FORGED_SPACE_AUTHORITY_FIELDS: &[&str] = &[
    "space_context",
    "space_append_context",
    "space_decision_token",
    "space_decision_ref",
    "recipient_audience_ref",
    "recipient_audience_revision",
    "recipient_audience_hash",
    "privacy_policy_ref",
    "authority_revision",
    "resource_authorization_ref",
    "action_schema_hash",
    "payload_digest",
];

/// Turns an untrusted personal-Space selection into a Control-issued thread
/// creation/append context. The browser may choose only `space_ref`; all
/// authority fields are removed before Control is called and are reconstructed
/// solely from its signed response. Existing scoped threads receive a fresh,
/// content-bound `thread:append` decision rather than a reusable create grant.
pub(crate) async fn inject_personal_thread_context(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    body: &mut Value,
) -> Result<(), (StatusCode, Json<Value>)> {
    let object = body.as_object_mut().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_chat_request",
                "Chat request must be an object.",
            )),
        )
    })?;
    for field in FORGED_SPACE_AUTHORITY_FIELDS {
        object.remove(*field);
    }
    // Remove both spellings even if one is malformed/empty. Leaving the second
    // spelling in the Model payload would create a future bypass when a
    // downstream deserializer grows an alias.
    let snake_space_ref = object.remove("space_ref");
    let camel_space_ref = object.remove("spaceRef");
    let normalized_space_ref = |value: Option<Value>| {
        value
            .and_then(|value| value.as_str().map(str::trim).map(str::to_owned))
            .filter(|value| !value.is_empty())
    };
    let snake_space_ref = normalized_space_ref(snake_space_ref);
    let camel_space_ref = normalized_space_ref(camel_space_ref);
    if snake_space_ref.is_some() && camel_space_ref.is_some() && snake_space_ref != camel_space_ref
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(error(
                "ambiguous_space_selection",
                "Conflicting Space references are not allowed.",
            )),
        ));
    }
    let space_ref = snake_space_ref.or(camel_space_ref);
    let Some(space_ref) = space_ref else {
        return Ok(());
    };
    if org_id.trim().is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership is required for a scoped thread.",
            )),
        ));
    }
    let existing_thread_id = object
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|thread_id| !thread_id.is_empty())
        .map(str::to_owned);

    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let counter = THREAD_SCOPE_COUNTER.fetch_add(1, Ordering::Relaxed);
    if let Some(thread_id) = existing_thread_id {
        let content = object
            .get("content")
            .and_then(Value::as_str)
            .filter(|content| !content.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(error(
                        "scoped_append_content_required",
                        "A scoped thread append requires message content.",
                    )),
                )
            })?;
        let idempotency_key =
            required_or_generated(object, "idempotency_key", "space-append", counter);
        let url = format!(
            "{}/api/v1/internal/spaces/thread-append-decision",
            state.user_core_url
        );
        let content_digest = format!("sha256:{:x}", Sha256::digest(content.as_bytes()));
        let (status, Json(response)) = proxy_json(
            state,
            Method::POST,
            &url,
            Some(json!({
                "space_ref": space_ref,
                "thread_id": thread_id,
                "content_digest": content_digest,
                "idempotency_key": idempotency_key,
            })),
            Some(org_id),
            Some(&actor),
            None,
        )
        .await;
        if !status.is_success() {
            return Err((status, Json(response)));
        }
        let data = crate::envelope::unwrap_data(&response);
        let decision = data
            .get("decision")
            .and_then(Value::as_object)
            .ok_or_else(invalid_decision)?;
        let token = data
            .get("token")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let context = json!({
            "space_id": required_decision_string(decision, "space_ref")?,
            "space_decision_ref": required_decision_string(decision, "decision_ref")?,
            "recipient_audience_ref": required_decision_string(decision, "recipient_audience_ref")?,
            "recipient_audience_revision": decision.get("recipient_audience_revision").and_then(Value::as_u64).ok_or_else(invalid_decision)?,
            "recipient_audience_hash": required_decision_string(decision, "recipient_audience_hash")?,
            "privacy_policy_ref": required_decision_string(decision, "privacy_policy_ref")?,
            "authority_revision": decision.get("authority_revision").and_then(Value::as_u64).ok_or_else(invalid_decision)?,
            "resource_authorization_ref": required_decision_string(decision, "resource_authorization_ref")?,
            "space_decision_token": token,
            "action_schema_hash": required_decision_string(decision, "action_schema_hash")?,
            "payload_digest": required_decision_string(decision, "payload_digest")?,
            "idempotency_key": required_decision_string(decision, "idempotency_key")?,
        });
        if token.trim().is_empty() {
            return Err(invalid_decision());
        }
        object.insert("space_append_context".to_owned(), context);
        return Ok(());
    }

    let session_key = required_or_generated(object, "session_key", "space-thread", counter);
    let idempotency_key = required_or_generated(object, "idempotency_key", "space-create", counter);
    let url = format!(
        "{}/api/v1/internal/spaces/thread-decision",
        state.user_core_url
    );
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_ref": space_ref,
            "session_key": session_key,
            "idempotency_key": idempotency_key,
        })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        return Err((status, Json(response)));
    }
    let data = crate::envelope::unwrap_data(&response);
    let decision = data
        .get("decision")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "invalid_space_decision",
                    "Control returned an invalid Space decision.",
                )),
            )
        })?;
    let token = data
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or_default();
    // Retrieval is a distinct Data-plane effect. Never reuse the scoped
    // thread:create bearer for it: Control must independently resolve the
    // current retrieval entitlement and issue the target-specific authority.
    let retrieval_url = format!(
        "{}/api/v1/internal/spaces/personal-retrieval-decision",
        state.user_core_url
    );
    let (retrieval_status, Json(retrieval_response)) = proxy_json(
        state,
        Method::POST,
        &retrieval_url,
        Some(json!({
            "space_ref": space_ref,
            "idempotency_key": format!("space-retrieval-{counter}"),
        })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !retrieval_status.is_success() {
        return Err((retrieval_status, Json(retrieval_response)));
    }
    let retrieval_token = crate::envelope::unwrap_data(&retrieval_response)
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let context = json!({
        "space_id": required_decision_string(decision, "space_ref")?,
        "space_decision_ref": required_decision_string(decision, "decision_ref")?,
        "recipient_audience_ref": required_decision_string(decision, "recipient_audience_ref")?,
        "recipient_audience_revision": decision.get("recipient_audience_revision").and_then(Value::as_u64).ok_or_else(invalid_decision)?,
        "recipient_audience_hash": required_decision_string(decision, "recipient_audience_hash")?,
        "privacy_policy_ref": required_decision_string(decision, "privacy_policy_ref")?,
        "authority_revision": decision.get("authority_revision").and_then(Value::as_u64).ok_or_else(invalid_decision)?,
        "resource_authorization_ref": required_decision_string(decision, "resource_authorization_ref")?,
        "space_decision_token": token,
        "retrieval_decision_token": &retrieval_token,
        "action_schema_hash": required_decision_string(decision, "action_schema_hash")?,
        "payload_digest": required_decision_string(decision, "payload_digest")?,
        "idempotency_key": required_decision_string(decision, "idempotency_key")?,
    });
    if token.trim().is_empty() || retrieval_token.trim().is_empty() {
        return Err(invalid_decision());
    }
    object.insert("space_context".to_owned(), context);
    Ok(())
}

/// Resolves a browser-selected personal Space into the initial, short-lived
/// import-ingress grant. The BFF holds the bearer only long enough to send it
/// to Imports Core; the browser never receives it and the worker later obtains
/// a separate Data-targeted decision for each durable write.
pub(crate) async fn personal_import_ingress_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    input: &mut Value,
    idempotency_key: &str,
) -> Result<Option<String>, (StatusCode, Json<Value>)> {
    let object = input.as_object_mut().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_action_input",
                "Action input must be an object.",
            )),
        )
    })?;
    for field in FORGED_SPACE_AUTHORITY_FIELDS {
        object.remove(*field);
    }
    object.remove("space_import_ingress_decision");
    let snake = object.remove("space_ref");
    let camel = object.remove("spaceRef");
    let normalize = |value: Option<Value>| {
        value
            .and_then(|value| value.as_str().map(str::trim).map(str::to_owned))
            .filter(|value| !value.is_empty())
    };
    let snake = normalize(snake);
    let camel = normalize(camel);
    if snake.is_some() && camel.is_some() && snake != camel {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(error(
                "ambiguous_space_selection",
                "Conflicting Space references are not allowed.",
            )),
        ));
    }
    let Some(space_ref) = snake.or(camel) else {
        return Ok(None);
    };
    let source_type = object
        .get("sourceType")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 64)
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(error(
                    "invalid_import_request",
                    "A source type is required for a Space import.",
                )),
            )
        })?;
    if org_id.trim().is_empty() || idempotency_key.trim().is_empty() || idempotency_key.len() > 200
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_import_request",
                "A bounded import idempotency key and active organization are required.",
            )),
        ));
    }
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!(
        "{}/api/v1/internal/spaces/personal-import-decision",
        state.user_core_url
    );
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({"space_ref": space_ref, "source_type": source_type, "idempotency_key": idempotency_key.trim()})),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        return Err((status, Json(response)));
    }
    let data = crate::envelope::unwrap_data(&response);
    let decision = data
        .get("decision")
        .and_then(Value::as_object)
        .ok_or_else(invalid_decision)?;
    let token = data
        .get("token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(invalid_decision)?;
    if required_decision_string(decision, "space_ref")? != space_ref
        || required_decision_string(decision, "service_audience")? != "ingestion-plane-import"
        || required_decision_string(decision, "action_id")? != "ingestion.import.write"
        || required_decision_string(decision, "action_schema_hash")? != "sha256:ingestion-import-v1"
        || required_decision_string(decision, "idempotency_key")? != idempotency_key.trim()
        || required_decision_string(decision, "import_source_type")? != source_type
    {
        return Err(invalid_decision());
    }
    Ok(Some(token.to_owned()))
}

// inject_personal_schedule_create_context turns the browser's Space selection
// and exact task-template value into a Control-issued one-create decision. The
// BFF generates the schedule ID before issuance, so neither a caller nor the
// Model Plane can substitute another durable schedule/template after Control
// authorizes it.
pub(crate) async fn inject_personal_schedule_create_context(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    input: &mut Value,
    schedule_id: &str,
    idempotency_key: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    let object = input.as_object_mut().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_cron_request",
                "Cron request must be an object.",
            )),
        )
    })?;
    for field in FORGED_SPACE_AUTHORITY_FIELDS {
        object.remove(*field);
    }
    object.remove("space_schedule_create_decision");
    object.remove("creator_subject_id");
    object.remove("idempotency_key");
    let browser_space_ref = object
        .get("space_ref")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    // The existing Settings schedule UI predates the Space picker. Until the
    // Work cockpit supplies an explicit selection, resolve exactly the active
    // user's personal Space from Application's lifecycle projection. This is
    // an authority lookup, not a browser default, and it never selects a room
    // or another member's Space.
    let space_ref = match browser_space_ref {
        Some(space_ref) => space_ref,
        None => personal_space_lifecycle(state, user, org_id)
            .await
            .map_err(|_| {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(error(
                        "space_lifecycle_unavailable",
                        "Personal Space lifecycle is unavailable.",
                    )),
                )
            })?
            .and_then(|space| {
                space
                    .get("spaceRef")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            })
            .ok_or_else(|| {
                (
                    StatusCode::FORBIDDEN,
                    Json(error(
                        "space_required",
                        "An active personal Space is required to create a schedule.",
                    )),
                )
            })?,
    };
    let template = object.get("task_template").cloned().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(error(
                "task_template_required",
                "A schedule requires an exact task template.",
            )),
        )
    })?;
    if !template.is_object()
        || org_id.trim().is_empty()
        || schedule_id.trim().is_empty()
        || idempotency_key.trim().is_empty()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_cron_request",
                "A bounded schedule identity, active organization, and object task template are required.",
            )),
        ));
    }
    let template_bytes = serde_json::to_vec(&template).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_cron_request",
                "Task template cannot be encoded.",
            )),
        )
    })?;
    let template_digest = format!("sha256:{:x}", Sha256::digest(template_bytes));
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!(
        "{}/api/v1/internal/spaces/schedule-create-decision",
        state.user_core_url
    );
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_ref": space_ref,
            "schedule_id": schedule_id,
            "template_digest": template_digest,
            "idempotency_key": idempotency_key,
        })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        return Err((status, Json(response)));
    }
    let data = crate::envelope::unwrap_data(&response);
    let decision = data
        .get("decision")
        .and_then(Value::as_object)
        .ok_or_else(invalid_decision)?;
    let token = data
        .get("token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(invalid_decision)?;
    if required_decision_string(decision, "space_ref")? != space_ref
        || required_decision_string(decision, "subject_id")? != user.user_id
        || required_decision_string(decision, "service_audience")? != "model-plane-capability-core"
        || required_decision_string(decision, "action_id")? != "model.cron.create"
        || required_decision_string(decision, "action_schema_hash")?
            != "sha256:space-cron-create-v1"
        || required_decision_string(decision, "idempotency_key")? != idempotency_key
    {
        return Err(invalid_decision());
    }
    object.insert("id".to_owned(), Value::String(schedule_id.to_owned()));
    object.insert("space_ref".to_owned(), Value::String(space_ref));
    object.insert(
        "creator_subject_id".to_owned(),
        Value::String(user.user_id.clone()),
    );
    object.insert(
        "space_schedule_create_decision".to_owned(),
        Value::String(token.to_owned()),
    );
    object.insert(
        "idempotency_key".to_owned(),
        Value::String(idempotency_key.to_owned()),
    );
    Ok(())
}

fn required_or_generated(
    object: &mut Map<String, Value>,
    field: &str,
    prefix: &str,
    counter: u64,
) -> String {
    if let Some(value) = object
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return value.to_owned();
    }
    let generated = format!("{prefix}-{counter}");
    object.insert(field.to_owned(), Value::String(generated.clone()));
    generated
}

fn required_decision_string<'a>(
    decision: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, (StatusCode, Json<Value>)> {
    decision
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(invalid_decision)
}

fn invalid_decision() -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_GATEWAY,
        Json(error(
            "invalid_space_decision",
            "Control returned an invalid Space decision.",
        )),
    )
}

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/spaces",
            get(list_personal_spaces).post(create_personal_space),
        )
        .route(
            "/api/v1/spaces/:space_ref/membership",
            get(current_membership),
        )
        .route("/api/v1/spaces/:space_ref/context", get(space_context))
        .route("/api/v1/spaces/:space_ref/actions", get(space_actions))
        .route("/api/v1/spaces/:space_ref/threads", get(list_space_threads))
        .route(
            "/api/v1/spaces/:space_ref/deletion-requests",
            post(request_personal_space_deletion),
        )
        .route(
            "/api/v1/spaces/deletion-requests/:request_id",
            get(personal_space_deletion_receipt),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

#[derive(Debug, Deserialize)]
struct SpaceThreadsQuery {
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct DeleteSpaceRequest {
    idempotency_key: String,
}

async fn request_personal_space_deletion(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    Json(request): Json<DeleteSpaceRequest>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let space_ref = space_ref.trim();
    let idempotency_key = request.idempotency_key.trim();
    if org_id.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership is required to request deletion.",
            )),
        );
    }
    if space_ref.is_empty() || idempotency_key.is_empty() || idempotency_key.len() > 160 {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_deletion_request",
                "A Space reference and bounded idempotency key are required.",
            )),
        );
    }
    match personal_space_lifecycle(&state, &user, &org_id).await {
        Ok(Some(space)) if space.get("spaceRef").and_then(Value::as_str) == Some(space_ref) => {}
        Ok(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(error(
                    "space_not_found",
                    "Personal Space is not available for deletion.",
                )),
            )
        }
        Err(()) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(error(
                    "space_lifecycle_unavailable",
                    "Space lifecycle is unavailable.",
                )),
            )
        }
    }
    let args = json!({
        "externalAuthId": user.user_id,
        "externalOrgId": org_id,
        "spaceRef": space_ref,
        "idempotencyKey": idempotency_key,
    });
    match convex_gateway_call(
        &state,
        "mutation",
        "spaces:requestPersonalSpaceDeletionForGateway",
        args,
    )
    .await
    {
        Ok(value) => (StatusCode::ACCEPTED, Json(json!({"data": value}))),
        Err(()) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "space_deletion_unavailable",
                "Space deletion request is unavailable.",
            )),
        ),
    }
}

async fn personal_space_deletion_receipt(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(request_id): Path<String>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let request_id = request_id.trim();
    if org_id.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership is required to read deletion status.",
            )),
        );
    }
    if request_id.is_empty() || request_id.len() > 512 {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_deletion_request",
                "A deletion request reference is required.",
            )),
        );
    }
    let args = json!({
        "externalAuthId": user.user_id,
        "externalOrgId": org_id,
        "requestId": request_id,
    });
    match convex_gateway_call(
        &state,
        "query",
        "spaces:getPersonalSpaceDeletionReceiptForGateway",
        args,
    )
    .await
    {
        Ok(value) => (StatusCode::OK, Json(json!({"data": value}))),
        Err(()) => (
            StatusCode::NOT_FOUND,
            Json(error(
                "space_deletion_not_found",
                "Space deletion request is not available.",
            )),
        ),
    }
}

/// Returns only durable threads that Session Core filters by the canonical
/// Space inside its org/user-bound query. The BFF verifies the current
/// lifecycle projection and Control membership first, so a stale/forged Space
/// path never becomes a cross-scope Model query.
async fn list_space_threads(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: axum::http::HeaderMap,
    Path(space_ref): Path<String>,
    Query(query): Query<SpaceThreadsQuery>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let space_ref = space_ref.trim();
    if org_id.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership is required to read Space threads.",
            )),
        );
    }
    if space_ref.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_space", "A Space reference is required.")),
        );
    }
    let space = match personal_space_lifecycle(&state, &user, &org_id).await {
        Ok(Some(space)) if space.get("spaceRef").and_then(Value::as_str) == Some(space_ref) => {
            space
        }
        Ok(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(error("space_not_found", "Space is not available.")),
            )
        }
        Err(()) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(error(
                    "space_lifecycle_unavailable",
                    "Space lifecycle is unavailable.",
                )),
            )
        }
    };
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let membership_url = format!(
        "{}/api/v1/internal/spaces/{}/membership",
        state.user_core_url,
        urlencoding::encode(space_ref)
    );
    let (membership_status, Json(membership)) = proxy_json(
        &state,
        Method::GET,
        &membership_url,
        None,
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;
    if !membership_status.is_success() {
        return (membership_status, Json(membership));
    }
    let session_token =
        match crate::domains::chat::shared::required_session_token(&state, &user, &headers).await {
            Ok(token) => token,
            Err(reason) => return crate::domains::chat::shared::delegated_auth_unavailable(reason),
        };
    let limit = query.limit.unwrap_or(80).clamp(1, 200);
    let url = format!(
        "{}/v1/threads?limit={limit}&space_id={}",
        state.model_gateway_url,
        urlencoding::encode(space_ref),
    );
    let model_token = crate::domains::chat::shared::model_token(&state, &user, &headers).await;
    let (status, Json(threads)) = crate::domains::chat::shared::proxy_model_json_with_session(
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
        return (status, Json(threads));
    }
    let thread_items = threads
        .get("threads")
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    if !thread_items.is_array() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "invalid_model_gateway_response",
                "Model Gateway returned an invalid thread listing.",
            )),
        );
    }
    if !thread_items_match_space(&thread_items, space_ref) {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "invalid_model_gateway_response",
                "Model Gateway returned a thread outside the selected Space.",
            )),
        );
    }
    (
        StatusCode::OK,
        Json(json!({
            "data": {
                "space": public_space(&space),
                "membership": crate::envelope::unwrap_data(&membership),
                "threads": thread_items,
            }
        })),
    )
}

fn thread_items_match_space(items: &Value, space_ref: &str) -> bool {
    items.as_array().is_some_and(|items| {
        items.iter().all(|item| {
            item.get("space_id")
                .and_then(Value::as_str)
                .is_some_and(|item_space_ref| item_space_ref == space_ref)
        })
    })
}

/// Reads Application's lifecycle projection through its service-only Convex
/// query. The browser's session supplies the user/org; the Application service
/// key is resolved only inside this BFF and is never returned in the response.
async fn list_personal_spaces(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    // Unfiltered on purpose: a listing reports what exists. A Space awaiting
    // Control registration is still the caller's Space, and hiding it would
    // make "create a room" look like it silently failed. Every acting surface
    // keeps using `personal_space_lifecycle`, which still requires `active`.
    let Ok(space) = personal_space_record(&state, &user, &org_id).await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            )),
        );
    };
    let Some(space) = space else {
        return (StatusCode::OK, Json(json!({"data": {"spaces": []}})));
    };
    (
        StatusCode::OK,
        Json(json!({"data": {"spaces": [public_space(&space)]}})),
    )
}

/// Provision the caller's own personal Space.
///
/// Idempotent: an owner has at most one personal Space, so a repeat call
/// returns the existing record rather than creating a second — two would trip
/// the invariant guard every read applies. That also makes a double-clicked
/// button harmless.
///
/// The result is `pending_registration`, not active. Control registers it
/// afterwards, so this returns 202 rather than 201: the room now exists, but it
/// is not yet a room Control has authorized, and the acting surfaces will keep
/// refusing until it is. Reporting 201 would imply a readiness this does not
/// deliver.
///
/// The body is ignored beyond an optional name; owner and org come from the
/// verified session and the caller's active organization, never from the
/// browser.
async fn create_personal_space(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    body: Option<Json<Value>>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "organization_required",
                "An active organization is required before a Space can be created.",
            )),
        );
    }
    let name = body
        .as_ref()
        .and_then(|Json(value)| value.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned);

    // `convex_gateway_call` injects only the service key; identity is the
    // caller's responsibility, and it comes from the verified session and the
    // resolved active org — never from the request body.
    let mut args = json!({
        "externalAuthId": user.user_id,
        "externalOrgId": org_id,
    });
    if let Some(name) = name {
        args["name"] = json!(name);
    }
    let Ok(created) = convex_gateway_call(
        &state,
        "mutation",
        "spaces:ensurePersonalSpaceForGateway",
        args,
    )
    .await
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "space_provisioning_unavailable",
                "The Space could not be created. Nothing was provisioned.",
            )),
        );
    };
    (
        StatusCode::ACCEPTED,
        Json(json!({"data": {"space": public_space(&created)}})),
    )
}

/// The caller's personal Space as stored, at ANY lifecycle.
///
/// Used by surfaces that report existence rather than grant action. A Space
/// still `pending_registration` must be visible — otherwise creating one looks
/// like it did nothing, and the owner has no way to see that registration is
/// what they are waiting on.
async fn personal_space_record(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> Result<Option<Value>, ()> {
    let convex_url = std::env::var("APPLICATION_CONVEX_URL").unwrap_or_default();
    let service_key = std::env::var("APPLICATION_CONVEX_SERVICE_KEY").unwrap_or_default();
    if org_id.trim().is_empty() || convex_url.trim().is_empty() || service_key.trim().is_empty() {
        return Err(());
    }
    let url = format!("{}/api/query", convex_url.trim_end_matches('/'));
    let response = state
        .client
        .post(url)
        .json(&json!({
            "path": "spaces:getPersonalSpaceForGateway",
            "args": {
                "externalAuthId": user.user_id,
                "externalOrgId": org_id,
                "serviceKey": service_key,
            }
        }))
        .send()
        .await;
    let Ok(response) = response else {
        return Err(());
    };
    if !response.status().is_success() {
        return Err(());
    }
    let payload = response.json::<Value>().await.unwrap_or(Value::Null);
    let value = payload.get("value").cloned().unwrap_or(Value::Null);
    let Some(space) = value.as_object() else {
        return Ok(None);
    };
    let lifecycle = space
        .get("lifecycle")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let space_ref = space
        .get("spaceRef")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if space_ref.trim().is_empty() {
        return Ok(None);
    }
    let _ = lifecycle;
    Ok(Some(value))
}

/// The caller's personal Space **only when Control has authorized it**.
///
/// This is the resolver every acting surface uses — context, actions, threads,
/// deletion. A Space that exists is not yet a Space Control has registered, so
/// anything that grants an action must keep this gate.
///
/// [`personal_space_record`] is the unfiltered read, and is for surfaces whose
/// job is to report what exists rather than to act on it.
async fn personal_space_lifecycle(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> Result<Option<Value>, ()> {
    let Some(space) = personal_space_record(state, user, org_id).await? else {
        return Ok(None);
    };
    let active = space
        .get("lifecycle")
        .and_then(Value::as_str)
        .is_some_and(|lifecycle| lifecycle == "active");
    Ok(active.then_some(space))
}

// The Application service key stays exclusively in the BFF. Gateway callers
// supply only their authenticated session; these facades add the verified user
// and active-org facts at the server boundary rather than accepting them from a
// browser body.
async fn convex_gateway_call(
    state: &AppState,
    operation: &str,
    path: &str,
    mut args: Value,
) -> Result<Value, ()> {
    let convex_url = std::env::var("APPLICATION_CONVEX_URL").unwrap_or_default();
    let service_key = std::env::var("APPLICATION_CONVEX_SERVICE_KEY").unwrap_or_default();
    if convex_url.trim().is_empty() || service_key.trim().is_empty() {
        return Err(());
    }
    let object = args.as_object_mut().ok_or(())?;
    object.insert("serviceKey".to_owned(), Value::String(service_key));
    let response = state
        .client
        .post(format!(
            "{}/api/{operation}",
            convex_url.trim_end_matches('/')
        ))
        .json(&json!({"path": path, "args": args}))
        .send()
        .await
        .map_err(|_| ())?;
    if !response.status().is_success() {
        return Err(());
    }
    response
        .json::<Value>()
        .await
        .map_err(|_| ())?
        .get("value")
        .cloned()
        .ok_or(())
}

fn public_space(space: &Value) -> Value {
    json!({
        "space_ref": space.get("spaceRef").and_then(Value::as_str).unwrap_or_default(),
        "name": space.get("name").and_then(Value::as_str).unwrap_or("Personal Space"),
        "kind": space.get("kind").and_then(Value::as_str).unwrap_or("personal"),
        "lifecycle": space.get("lifecycle").and_then(Value::as_str).unwrap_or_default(),
    })
}

async fn space_context(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let space_ref = space_ref.trim();
    if org_id.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership is required to resolve a Space.",
            )),
        );
    }
    if space_ref.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_space", "A Space reference is required.")),
        );
    }
    let space = match personal_space_lifecycle(&state, &user, &org_id).await {
        Ok(Some(space)) if space.get("spaceRef").and_then(Value::as_str) == Some(space_ref) => {
            space
        }
        Ok(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(error("space_not_found", "Space is not available.")),
            )
        }
        Err(()) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(error(
                    "space_lifecycle_unavailable",
                    "Space lifecycle is unavailable.",
                )),
            )
        }
    };
    let actor = ActionActor {
        user_id: user.user_id,
        user_email: user.user_email,
        user_name: user.user_name,
        user_role: user.auth_role.unwrap_or_default(),
    };
    let url = format!(
        "{}/api/v1/internal/spaces/{}/membership",
        state.user_core_url,
        urlencoding::encode(space_ref)
    );
    let (status, Json(response)) = proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        return (status, Json(response));
    }
    (
        StatusCode::OK,
        Json(
            json!({"data": {"space": public_space(&space), "membership": crate::envelope::unwrap_data(&response)}}),
        ),
    )
}

async fn current_membership(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    if org_id.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership is required to resolve a Space.",
            )),
        );
    }

    let space_ref = space_ref.trim();
    if space_ref.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_space", "A Space reference is required.")),
        );
    }

    let url = format!(
        "{}/api/v1/internal/spaces/{}/membership",
        state.user_core_url,
        urlencoding::encode(space_ref),
    );
    let actor = ActionActor {
        user_id: user.user_id,
        user_email: user.user_email,
        user_name: user.user_name,
        user_role: user.auth_role.unwrap_or_default(),
    };
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await
}

/// Human-only catalog projection for a selected Space. Successful Control
/// membership is necessary to see it, but every action's owning plane still
/// authorizes its exact resource immediately before its effect. The response
/// contains no bearer, audience list, or model-eligible contract.
async fn space_actions(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
) -> (StatusCode, Json<Value>) {
    let space_ref = space_ref.trim().to_owned();
    if space_ref.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_space", "A Space reference is required.")),
        );
    }
    let (membership_status, Json(membership)) = current_membership(
        State(state.clone()),
        Extension(user.clone()),
        Path(space_ref.clone()),
    )
    .await;
    if !membership_status.is_success() {
        return (membership_status, Json(membership));
    }
    match crate::domains::actions::dispatchers::human_owner_action_catalog_for(&state, &user).await
    {
        Ok(catalog) => (
            StatusCode::OK,
            Json(json!({
                "data": {
                    "space_ref": space_ref,
                    "actor_type": "human",
                    "catalog": catalog,
                }
            })),
        ),
        Err((status, payload)) => (status, Json(payload)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        inject_personal_schedule_create_context, inject_personal_thread_context,
        personal_import_ingress_decision, space_actions, thread_items_match_space,
    };
    use axum::{
        body::Body,
        extract::{Extension, Path, State},
        http::Request,
    };
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::middleware::AuthenticatedUser;

    fn authenticated_user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "User One".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        }
    }

    #[test]
    fn membership_route_is_encoded_under_the_control_internal_namespace() {
        let base = "http://user-core:3012";
        let ref_id = "space/a?b";
        let url = format!(
            "{}/api/v1/internal/spaces/{}/membership",
            base,
            urlencoding::encode(ref_id),
        );
        assert_eq!(
            url,
            "http://user-core:3012/api/v1/internal/spaces/space%2Fa%3Fb/membership"
        );
    }

    #[test]
    fn thread_list_rejects_missing_or_cross_space_summary_metadata() {
        assert!(thread_items_match_space(
            &json!([{"thread_id": "thread-1", "space_id": "space-1"}]),
            "space-1"
        ));
        assert!(!thread_items_match_space(
            &json!([{"thread_id": "thread-1", "space_id": "space-2"}]),
            "space-1"
        ));
        assert!(!thread_items_match_space(
            &json!([{"thread_id": "thread-1"}]),
            "space-1"
        ));
    }

    #[tokio::test]
    async fn space_action_catalog_rejects_an_empty_space_before_any_owner_lookup() {
        let (status, _) = space_actions(
            State(crate::tests::test_state(false)),
            Extension(authenticated_user()),
            Path("   ".to_owned()),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn browser_cannot_smuggle_a_space_authority_without_a_space_selection() {
        let state = crate::tests::test_state(false);
        let mut outbound = json!({
            "content": "hello",
            "space_context": {"space_decision_token": "forged"},
            "space_decision_token": "forged", "payload_digest": "forged"
        });

        inject_personal_thread_context(&state, &authenticated_user(), "org-1", &mut outbound)
            .await
            .expect("unscoped request remains valid");

        assert!(outbound.get("space_context").is_none());
        assert!(outbound.get("space_decision_token").is_none());
        assert!(outbound.get("payload_digest").is_none());
    }

    #[tokio::test]
    async fn schedule_create_replaces_forged_authority_with_a_control_bound_token() {
        let user_core = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path(
                "/api/v1/internal/spaces/schedule-create-decision",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"token": "schedule-create-token", "decision": {
                    "space_ref": "personal-1", "subject_id": "user-1",
                    "service_audience": "model-plane-capability-core", "action_id": "model.cron.create",
                    "action_schema_hash": "sha256:space-cron-create-v1", "idempotency_key": "create-1"
                }}
            })))
            .mount(&user_core)
            .await;
        let mut state = crate::tests::test_state(false);
        state.user_core_url = user_core.uri();
        let mut outbound = json!({
            "space_ref": "personal-1", "name": "Nightly", "task_template": {"kind": "cron"},
            "creator_subject_id": "forged", "space_schedule_create_decision": "forged"
        });

        inject_personal_schedule_create_context(
            &state,
            &authenticated_user(),
            "org-1",
            &mut outbound,
            "cron-1",
            "create-1",
        )
        .await
        .expect("Control schedule create authority should be injected");

        assert_eq!(outbound["id"], "cron-1");
        assert_eq!(outbound["creator_subject_id"], "user-1");
        assert_eq!(
            outbound["space_schedule_create_decision"],
            "schedule-create-token"
        );
        let received = user_core
            .received_requests()
            .await
            .expect("Control request");
        let request = received
            .iter()
            .find(|request| {
                request.url.path() == "/api/v1/internal/spaces/schedule-create-decision"
            })
            .expect("schedule decision request");
        let body: Value = serde_json::from_slice(&request.body).expect("request JSON");
        assert_eq!(body["space_ref"], "personal-1");
        assert_eq!(body["schedule_id"], "cron-1");
        assert_eq!(body["idempotency_key"], "create-1");
        assert!(body["template_digest"]
            .as_str()
            .unwrap_or_default()
            .starts_with("sha256:"));
    }

    #[tokio::test]
    async fn control_issuance_replaces_forged_space_authority_before_model_dispatch() {
        let user_core = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/thread-decision"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "token": "control-signed-token",
                    "decision": {
                        "space_ref": "personal-1", "decision_ref": "decision-1",
                        "recipient_audience_ref": "audience-1", "privacy_policy_ref": "privacy-1",
                        "recipient_audience_revision": 3, "recipient_audience_hash": "sha256:audience-1", "authority_revision": 7, "resource_authorization_ref": "resource-1",
                        "action_schema_hash": "sha256:schema", "payload_digest": "sha256:payload",
                        "idempotency_key": "idem-1"
                    }
                }
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path(
                "/api/v1/internal/spaces/personal-retrieval-decision",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"token": "control-retrieval-token"}
            })))
            .mount(&user_core)
            .await;
        let mut state = crate::tests::test_state(false);
        state.user_core_url = user_core.uri();
        let mut outbound = json!({
            "content": "hello", "space_ref": "personal-1", "session_key": "session-1",
            "idempotency_key": "idem-1",
            "space_context": {"space_decision_token": "forged"},
            "payload_digest": "forged"
        });

        inject_personal_thread_context(&state, &authenticated_user(), "org-1", &mut outbound)
            .await
            .expect("Control decision should be injected");

        assert_eq!(
            outbound["space_context"]["space_decision_token"],
            "control-signed-token"
        );
        assert_eq!(
            outbound["space_context"]["payload_digest"],
            "sha256:payload"
        );
        assert_eq!(outbound["space_context"]["recipient_audience_revision"], 3);
        assert_eq!(
            outbound["space_context"]["recipient_audience_hash"],
            "sha256:audience-1"
        );
        assert_eq!(
            outbound["space_context"]["retrieval_decision_token"],
            "control-retrieval-token"
        );
        assert!(outbound.get("space_ref").is_none());
        assert!(outbound.get("payload_digest").is_none());
        let received = user_core
            .received_requests()
            .await
            .expect("Control request");
        let request = received
            .iter()
            .find(|request| request.url.path() == "/api/v1/internal/spaces/thread-decision")
            .expect("decision issuance request");
        let request_body: Value = serde_json::from_slice(&request.body).expect("request JSON");
        assert_eq!(request_body["space_ref"], "personal-1");
        assert_eq!(request_body["session_key"], "session-1");
        assert_eq!(request_body["idempotency_key"], "idem-1");
        assert!(request.headers.get("x-delegation-signature").is_some());
        let retrieval_request = received
            .iter()
            .find(|request| {
                request.url.path() == "/api/v1/internal/spaces/personal-retrieval-decision"
            })
            .expect("retrieval decision issuance request");
        let retrieval_body: Value =
            serde_json::from_slice(&retrieval_request.body).expect("retrieval JSON");
        assert_eq!(retrieval_body["space_ref"], "personal-1");
        assert_eq!(retrieval_body["idempotency_key"], "space-retrieval-1");
        assert!(retrieval_body.get("session_key").is_none());
    }

    #[tokio::test]
    async fn existing_scoped_thread_gets_fresh_content_bound_append_authority() {
        let user_core = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/thread-append-decision"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"token": "append-token", "decision": {
                    "space_ref": "room-1", "decision_ref": "append-decision-1",
                    "recipient_audience_ref": "audience-2", "recipient_audience_revision": 4,
                    "recipient_audience_hash": "sha256:audience-2", "privacy_policy_ref": "privacy-1",
                    "authority_revision": 8, "resource_authorization_ref": "resource-1",
                    "action_schema_hash": "sha256:thread-append-v1", "payload_digest": "sha256:append-payload",
                    "idempotency_key": "append-1"
                }}
            })))
            .mount(&user_core)
            .await;
        let mut state = crate::tests::test_state(false);
        state.user_core_url = user_core.uri();
        let mut outbound = json!({
            "content": "exact message", "thread_id": "thread-1", "space_ref": "room-1",
            "idempotency_key": "append-1", "space_append_context": {"space_decision_token": "forged"}
        });

        inject_personal_thread_context(&state, &authenticated_user(), "org-1", &mut outbound)
            .await
            .expect("fresh append authority should be injected");

        assert_eq!(
            outbound["space_append_context"]["space_decision_token"],
            "append-token"
        );
        assert_eq!(
            outbound["space_append_context"]["action_schema_hash"],
            "sha256:thread-append-v1"
        );
        let received = user_core
            .received_requests()
            .await
            .expect("Control request");
        let request = received
            .iter()
            .find(|request| request.url.path() == "/api/v1/internal/spaces/thread-append-decision")
            .expect("append decision request");
        let request_body: Value = serde_json::from_slice(&request.body).expect("request JSON");
        assert_eq!(request_body["space_ref"], "room-1");
        assert_eq!(request_body["thread_id"], "thread-1");
        assert_eq!(request_body["idempotency_key"], "append-1");
        assert_eq!(
            request_body["content_digest"],
            "sha256:9faee88762692caf9d3e783938a1f282defb0353e3edb9a1c7be26a1f095c174"
        );
        assert!(request.headers.get("x-delegation-signature").is_some());
        assert!(outbound.get("space_ref").is_none());
    }

    #[tokio::test]
    async fn import_ingress_uses_control_decision_and_never_leaves_a_bearer_in_action_input() {
        let user_core = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/personal-import-decision"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"token": "control-import-token", "decision": {
                    "space_ref": "personal-1", "service_audience": "ingestion-plane-import",
                    "action_id": "ingestion.import.write", "action_schema_hash": "sha256:ingestion-import-v1",
                    "idempotency_key": "import-1", "import_source_type": "notion"
                }}
            })))
            .mount(&user_core)
            .await;
        let mut state = crate::tests::test_state(false);
        state.user_core_url = user_core.uri();
        let mut input = json!({
            "sourceType": "notion", "spaceRef": "personal-1",
            "space_decision_token": "forged", "space_import_ingress_decision": "forged"
        });

        let token = personal_import_ingress_decision(
            &state,
            &authenticated_user(),
            "org-1",
            &mut input,
            "import-1",
        )
        .await
        .expect("Control decision should be accepted");

        assert_eq!(token.as_deref(), Some("control-import-token"));
        assert!(input.get("spaceRef").is_none());
        assert!(input.get("space_decision_token").is_none());
        assert!(input.get("space_import_ingress_decision").is_none());
        let received = user_core
            .received_requests()
            .await
            .expect("Control request");
        let request = received.first().expect("one decision request");
        let body: Value = serde_json::from_slice(&request.body).expect("request body");
        assert_eq!(
            body,
            json!({"space_ref": "personal-1", "source_type": "notion", "idempotency_key": "import-1"})
        );
        assert!(request.headers.get("x-delegation-signature").is_some());
    }

    #[tokio::test]
    async fn space_list_uses_server_derived_identity_and_never_returns_application_key() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId":"user-1", "orgId":"org-1", "role":"member", "onboardingStatus":"COMPLETED"
            })))
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": {
                "spaceRef":"space-personal", "name":"Personal Space", "kind":"personal", "lifecycle":"active"
            }})))
            .mount(&application).await;
        std::env::set_var("APPLICATION_CONVEX_URL", application.uri());
        std::env::set_var("APPLICATION_CONVEX_SERVICE_KEY", "application-test-key");
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/spaces")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["data"]["spaces"][0]["space_ref"], "space-personal");
        assert!(!body.to_string().contains("application-test-key"));
        let requests = application.received_requests().await.unwrap();
        let request = requests
            .iter()
            .find(|request| request.url.path() == "/api/query")
            .unwrap();
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["args"]["externalAuthId"], "user-1");
        assert_eq!(body["args"]["externalOrgId"], "org-1");
    }

    #[tokio::test]
    async fn deletion_request_uses_the_bff_service_facade_and_never_accepts_browser_identity() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId":"user-1", "orgId":"org-1", "role":"member", "onboardingStatus":"COMPLETED"
            })))
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": {
                "spaceRef":"space-personal", "name":"Personal Space", "kind":"personal", "lifecycle":"active"
            }})))
            .mount(&application)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": {
                "requestId":"delete-1", "idempotencyKey":"idem-1", "spaceRef":"space-personal",
                "externalOrgId":"org-1", "ownerExternalAuthId":"user-1", "state":"pending_authorization",
                "createdAt":1, "updatedAt":1
            }})))
            .mount(&application)
            .await;
        std::env::set_var("APPLICATION_CONVEX_URL", application.uri());
        std::env::set_var("APPLICATION_CONVEX_SERVICE_KEY", "application-test-key");
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/spaces/space-personal/deletion-requests")
                    .header("content-type", "application/json")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::from(
                        r#"{"idempotency_key":"idem-1","externalAuthId":"forged"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        assert_eq!(response.status(), axum::http::StatusCode::ACCEPTED);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["data"]["requestId"], "delete-1");
        assert!(!body.to_string().contains("application-test-key"));
        let requests = application.received_requests().await.unwrap();
        let mutation = requests
            .iter()
            .find(|request| request.url.path() == "/api/mutation")
            .unwrap();
        let payload: Value = serde_json::from_slice(&mutation.body).unwrap();
        assert_eq!(
            payload["path"],
            "spaces:requestPersonalSpaceDeletionForGateway"
        );
        assert_eq!(payload["args"]["externalAuthId"], "user-1");
        assert_eq!(payload["args"]["externalOrgId"], "org-1");
        assert_eq!(payload["args"]["spaceRef"], "space-personal");
        assert_eq!(payload["args"]["idempotencyKey"], "idem-1");
    }

    #[tokio::test]
    async fn space_context_composes_active_lifecycle_and_current_control_membership() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId":"user-1", "orgId":"org-1", "role":"member", "onboardingStatus":"COMPLETED"
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-personal/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
                "space_ref":"space-personal", "org_id":"org-1", "subject_id":"user-1", "role":"owner"
            }})))
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": {
                "spaceRef":"space-personal", "name":"Personal Space", "kind":"personal", "lifecycle":"active"
            }})))
            .mount(&application)
            .await;
        std::env::set_var("APPLICATION_CONVEX_URL", application.uri());
        std::env::set_var("APPLICATION_CONVEX_SERVICE_KEY", "application-test-key");
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/spaces/space-personal/context")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["data"]["space"]["space_ref"], "space-personal");
        assert_eq!(body["data"]["membership"]["subject_id"], "user-1");
        assert!(!body.to_string().contains("application-test-key"));
        let control = user_core.received_requests().await.unwrap();
        assert!(control.iter().any(
            |request| request.url.path() == "/api/v1/internal/spaces/space-personal/membership"
        ));
    }

    #[tokio::test]
    async fn space_threads_rechecks_lifecycle_and_membership_then_forwards_only_the_space_filter() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        for path in ["/api/model-plane/token", "/api/session-core/token"] {
            Mock::given(wm_method("GET"))
                .and(wm_path(path))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"token": "delegated-token"})),
                )
                .mount(&auth)
                .await;
        }
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId":"user-1", "orgId":"org-1", "role":"member", "onboardingStatus":"COMPLETED"
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-personal/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
                "space_ref":"space-personal", "org_id":"org-1", "subject_id":"user-1", "role":"owner"
            }})))
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": {
                "spaceRef":"space-personal", "name":"Personal Space", "kind":"personal", "lifecycle":"active"
            }})))
            .mount(&application)
            .await;
        let model = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/threads"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"threads": [{
                "thread_id":"thread-personal", "space_id":"space-personal", "title":"Scoped", "updated_at":"2026-08-13T00:00:00Z",
                "latest_run_id":"run-personal", "latest_run_status":"running", "latest_run_updated_at":"2026-08-13T00:00:01Z"
            }]})))
            .mount(&model)
            .await;
        std::env::set_var("APPLICATION_CONVEX_URL", application.uri());
        std::env::set_var("APPLICATION_CONVEX_SERVICE_KEY", "application-test-key");
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.model_gateway_url = model.uri();
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/spaces/space-personal/threads?limit=10")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["data"]["threads"][0]["thread_id"], "thread-personal");
        assert_eq!(body["data"]["threads"][0]["latest_run_status"], "running");
        let model_requests = model.received_requests().await.unwrap();
        let request = model_requests
            .iter()
            .find(|request| request.url.path() == "/v1/threads")
            .expect("Model thread listing request");
        assert_eq!(
            request.url.query(),
            Some("limit=10&space_id=space-personal")
        );
        assert!(request.headers.get("x-session-authorization").is_some());
    }

    #[tokio::test]
    async fn membership_read_uses_authenticated_actor_and_control_authority() {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
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
            .and(wm_path("/api/v1/internal/spaces/space-1/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "space_ref": "space-1", "org_id": "org-1", "subject_id": "user-1",
                    "kind": "personal", "role": "owner",
                    "revisions": {"authority": 1, "membership": 1, "privacy": 1, "recipient_audience": 1, "entitlement": 1}
                }
            })))
            .mount(&user_core)
            .await;

        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/spaces/space-1/membership")
                    .header("cookie", "better-auth.session_token=session-1")
                    .header("x-verevon-org-id", "forged-org")
                    .header("x-user-id", "forged-user")
                    .body(Body::empty())
                    .expect("membership request"),
            )
            .await
            .expect("gateway response");
        let received = user_core
            .received_requests()
            .await
            .expect("user-core requests");
        assert_eq!(
            response.status(),
            axum::http::StatusCode::OK,
            "user-core requests: {received:#?}"
        );
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .expect("membership response JSON");
        assert_eq!(body["data"]["subject_id"], "user-1");
        assert_eq!(body["data"]["org_id"], "org-1");

        let membership = received
            .iter()
            .find(|request| request.url.path() == "/api/v1/internal/spaces/space-1/membership")
            .expect("membership authority request");
        assert_eq!(
            membership
                .headers
                .get("x-user-id")
                .and_then(|v| v.to_str().ok()),
            Some("user-1")
        );
        assert_eq!(
            membership
                .headers
                .get("x-org-id")
                .and_then(|v| v.to_str().ok()),
            Some("org-1")
        );
        assert!(membership.headers.get("x-delegation-signature").is_some());
    }
}
