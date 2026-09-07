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
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
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
    //
    // Control now issues retrieval authority for both personal and shared
    // (room/project/case) Spaces via the unified retrieval-decision endpoint,
    // which dispatches on the Space's own registered kind server-side (the
    // same pattern thread-decision already uses for thread creation).
    //
    // retrieval_read_entitled is a separate, independently configured policy
    // bit from thread_create_entitled (migration 019: "a thread-create grant
    // never implies data access", default FALSE for every org). A FORBIDDEN
    // response here means Control considered the request and affirmatively
    // denied retrieval specifically — that must degrade this turn to
    // ungrounded chat, not fail the turn outright, or every org that has
    // chat enabled without separately opting into retrieval would lose chat
    // entirely rather than just grounding. Any other non-success status
    // (unreachable, 5xx, malformed response) still fails the whole call:
    // an outage must never be silently treated as "retrieval is fine, just
    // not entitled" — see scoped_retrieval_token's own contract in
    // model-gateway/src/retrieval.rs.
    let retrieval_url = format!(
        "{}/api/v1/internal/spaces/retrieval-decision",
        state.user_core_url
    );
    let (retrieval_status, Json(retrieval_response)) = proxy_json(
        state,
        Method::POST,
        &retrieval_url,
        Some(json!({
            "space_ref": space_ref,
            "idempotency_key": format!("space-retrieval-{}-{counter}", unix_nanos()),
        })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    let retrieval_token = if retrieval_status == StatusCode::FORBIDDEN {
        String::new()
    } else if !retrieval_status.is_success() {
        return Err((retrieval_status, Json(retrieval_response)));
    } else {
        let retrieval_token = crate::envelope::unwrap_data(&retrieval_response)
            .get("token")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if retrieval_token.trim().is_empty() {
            return Err(invalid_decision());
        }
        retrieval_token
    };
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
    if token.trim().is_empty() {
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

fn unix_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
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
    // Timestamp + per-process counter, never the counter alone: Session Core's
    // create-thread is get-or-create by session_key, so a bare process-local
    // counter reissued the same keys after every gateway restart and silently
    // resumed an unrelated old thread instead of creating a new one.
    let generated = format!("{prefix}-{}-{counter}", unix_nanos());
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
        // Static segment registered before the `{space_ref}` matchers so a
        // room literally named "organization-room" can never shadow it.
        .route(
            "/api/v1/spaces/organization-room",
            post(ensure_organization_room),
        )
        .route(
            "/api/v1/spaces/{space_ref}/membership",
            get(current_membership),
        )
        .route("/api/v1/spaces/{space_ref}/context", get(space_context))
        .route("/api/v1/spaces/{space_ref}/actions", get(space_actions))
		.route(
			"/api/v1/spaces/{space_ref}/conversations/{conversation_id}/agent-action-grants",
			post(create_agent_ticket_action_grant),
		)
		.route(
			"/api/v1/spaces/{space_ref}/conversations/{conversation_id}/agent-action-grants/{grant_id}",
			delete(revoke_agent_ticket_action_grant),
		)
        .route("/api/v1/spaces/{space_ref}/roster", get(space_roster))
        .route(
            "/api/v1/spaces/{space_ref}/members",
            post(add_space_member),
        )
        .route(
            "/api/v1/spaces/{space_ref}/members/{member_id}",
            delete(remove_space_member),
        )
        .route(
            "/api/v1/spaces/{space_ref}/instructions",
            get(get_space_instructions).patch(update_space_instructions),
        )
        .route(
            "/api/v1/agents/installations",
            get(list_org_agent_installations),
        )
        .route(
            "/api/v1/spaces/{space_ref}/agents",
            get(space_agents).post(create_space_agent),
        )
        .route(
            "/api/v1/spaces/{space_ref}/agents/available",
            get(list_installable_space_agents),
        )
        .route(
            "/api/v1/spaces/{space_ref}/agents/bind",
            post(bind_existing_space_agent),
        )
        // Registered AFTER `/agents/bind` so the literal segment still wins;
        // `{binding_ref}` would otherwise swallow it.
        .route(
            "/api/v1/spaces/{space_ref}/agents/{binding_ref}",
            patch(set_space_agent_state).delete(revoke_space_agent),
        )
        .route("/api/v1/spaces/{space_ref}/threads", get(list_space_threads))
        .route("/api/v1/spaces/{space_ref}/work", get(space_work))
        .route("/api/v1/spaces/{space_ref}/knowledge", get(space_knowledge))
        .route("/api/v1/spaces/{space_ref}/activity", get(space_activity))
        .route(
            "/api/v1/spaces/{space_ref}/threads/{thread_id}/transcript",
            get(space_thread_transcript),
        )
        .route(
            "/api/v1/spaces/{space_ref}/deletion-requests",
            post(request_personal_space_deletion),
        )
        .route(
            "/api/v1/spaces/deletion-requests/{request_id}",
            get(personal_space_deletion_receipt),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

#[derive(Debug, Deserialize)]
struct OwnerGrantMutationRequest {
    idempotency_key: String,
}

async fn create_agent_ticket_action_grant(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, conversation_id)): Path<(String, String)>,
    Json(request): Json<OwnerGrantMutationRequest>,
) -> Response {
    forward_owner_grant_mutation(
        &state,
        &user,
        &space_ref,
        &conversation_id,
        "create",
        None,
        request,
    )
    .await
}

async fn revoke_agent_ticket_action_grant(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, conversation_id, grant_id)): Path<(String, String, String)>,
    Json(request): Json<OwnerGrantMutationRequest>,
) -> Response {
    forward_owner_grant_mutation(
        &state,
        &user,
        &space_ref,
        &conversation_id,
        "revoke",
        Some(&grant_id),
        request,
    )
    .await
}

// The signed Control bearer never reaches the browser. The BFF receives an
// authenticated user's narrow mutation request, resolves current membership,
// obtains the bounded decision over its signed Control channel, checks the
// returned binding structurally, and immediately forwards it to the owner.
// This deliberately does not advertise or enable the Model action.
async fn forward_owner_grant_mutation(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    conversation_id: &str,
    operation: &str,
    grant_id: Option<&str>,
    request: OwnerGrantMutationRequest,
) -> Response {
    let space_ref = space_ref.trim();
    let conversation_id = conversation_id.trim();
    let grant_id = grant_id.map(str::trim);
    let idempotency_key = request.idempotency_key.trim();
    if space_ref.is_empty()
        || conversation_id.is_empty()
        || idempotency_key.is_empty()
        || space_ref.len() > 200
        || conversation_id.len() > 200
        || idempotency_key.len() > 200
        || grant_id.is_some_and(|id| id.is_empty() || id.len() > 200)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_owner_grant_request",
                "Space, conversation, grant selection, and idempotency key are invalid.",
            )),
        )
            .into_response();
    }
    if !matches!(operation, "create" | "revoke")
        || (operation == "create" && grant_id.is_some())
        || (operation == "revoke" && grant_id.is_none())
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_owner_grant_request",
                "Owner grant operation is invalid.",
            )),
        )
            .into_response();
    }
    let role = user.auth_role.as_deref().unwrap_or_default().trim();
    if role != "owner" && role != "admin" {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "owner_grant_role_required",
                "An organization owner or admin must manage agent action grants.",
            )),
        )
            .into_response();
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership is required to manage agent action grants.",
            )),
        )
            .into_response();
    }
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let control_url = format!(
        "{}/api/v1/internal/spaces/owner-grant-decision",
        state.user_core_url
    );
    let (control_status, Json(control_response)) = proxy_json(
        state,
        Method::POST,
        &control_url,
        Some(json!({
            "space_ref": space_ref,
            "conversation_id": conversation_id,
            "action_id": "tickets.create",
            "operation": operation,
            "grant_id": grant_id.unwrap_or_default(),
            "idempotency_key": idempotency_key,
        })),
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;
    if !control_status.is_success() {
        return (control_status, Json(control_response)).into_response();
    }
    let data = crate::envelope::unwrap_data(&control_response);
    let decision = match data.get("decision").and_then(Value::as_object) {
        Some(decision) => decision,
        None => return invalid_owner_grant_decision().into_response(),
    };
    let token = match data.get("token").and_then(Value::as_str).map(str::trim) {
        Some(token) if !token.is_empty() && token.len() <= 16_384 => token,
        _ => return invalid_owner_grant_decision().into_response(),
    };
    let decision_grant_id = decision
        .get("grant_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let expected_grant_id = grant_id.unwrap_or_default();
    let required_matches = [
        ("org_id", org_id.as_str()),
        ("conversation_id", conversation_id),
        ("space_ref", space_ref),
        ("subject_id", user.user_id.as_str()),
        ("service_audience", "application-plane-conversation-core"),
        ("action_id", "tickets.create"),
        ("operation", operation),
        ("idempotency_key", idempotency_key),
    ];
    if required_matches.into_iter().any(|(field, expected)| {
        decision.get(field).and_then(Value::as_str).map(str::trim) != Some(expected)
    }) || decision_grant_id != expected_grant_id
    {
        return invalid_owner_grant_decision().into_response();
    }
    let conversation_path = format!(
        "/api/v1/conversations/{}/agent-action-grants{}",
        urlencoding::encode(conversation_id),
        if operation == "revoke" {
            format!("/{}", urlencoding::encode(expected_grant_id))
        } else {
            String::new()
        }
    );
    let method = if operation == "revoke" {
        Method::DELETE
    } else {
        Method::POST
    };
    let url = format!("{}{}", state.conversation_core_url, conversation_path);
    let (status, Json(response)) = crate::upstream::proxy_conversation_json(
        state,
        method,
        &url,
        Some(json!({
            "control_decision_token": token,
            "idempotency_key": idempotency_key,
        })),
        user,
        None,
    )
    .await;
    (status, Json(response)).into_response()
}

fn invalid_owner_grant_decision() -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_GATEWAY,
        Json(error(
            "invalid_owner_grant_decision",
            "Control returned an invalid owner grant decision.",
        )),
    )
}

#[derive(Debug, Deserialize)]
struct SpaceThreadsQuery {
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DeleteSpaceRequest {
    pub(crate) idempotency_key: String,
}

pub(crate) async fn request_personal_space_deletion(
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
    let space = match space_lifecycle_by_ref(&state, &user, &org_id, space_ref).await {
        Ok(Some(space)) => space,
        Ok(None) => {
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
    // Without this the listing is owner-bound, so every member of one room sees
    // a different room. Control decides whether this caller may see the shared
    // record; a decline leaves the request exactly as it was.
    let read = match shared_thread_read_decision(&state, &user, &org_id, space_ref).await {
        Ok(read) => read,
        Err(response) => return response,
    };
    let read_query = read
        .as_ref()
        .map(|(decision_ref, token)| {
            format!(
                "&space_read_decision_ref={}&space_read_decision_token={}",
                urlencoding::encode(decision_ref),
                urlencoding::encode(token),
            )
        })
        .unwrap_or_default();
    let url = format!(
        "{}/v1/threads?limit={limit}&space_id={}{read_query}",
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

/// Ask Control whether this caller may read this shared Space's record, and
/// return the signed decision for Model Plane to verify.
///
/// `Ok(None)` means "no shared read available", which is a real answer rather
/// than a failure, and it has exactly two causes:
///
/// * **403** — Control considered the request and declined: the org has not
///   enabled `thread_read_entitled` (deny-by-default, migration 026), the
///   Space is personal, or the caller is not a current recipient.
/// * **404** — this Control does not have the endpoint at all, i.e. a gateway
///   newer than the Control it talks to. An absent capability is not an
///   outage, and failing here would mean a room that goes dark for the whole
///   window between deploying the two planes.
///
/// Everything else propagates. An unreachable or erroring Control must never
/// be read as "not entitled" — the same rule the retrieval decision follows
/// above, and the reason this returns a Result rather than an Option.
async fn shared_thread_read_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Result<Option<(String, String)>, (StatusCode, Json<Value>)> {
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!(
        "{}/api/v1/internal/spaces/thread-read-decision",
        state.user_core_url
    );
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_ref": space_ref,
            // Same shape as the retrieval decision's key above: a read is not
            // an idempotent effect to dedupe, it just needs a unique operation
            // name, and the clock gives one without a new dependency.
            "idempotency_key": format!("space-read-{}", unix_nanos()),
        })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        return Err((status, Json(response)));
    }
    let data = crate::envelope::unwrap_data(&response);
    let decision_ref = data
        .get("decision")
        .and_then(Value::as_object)
        .and_then(|decision| decision.get("decision_ref"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let token = data
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if decision_ref.trim().is_empty() || token.trim().is_empty() {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(error(
                "invalid_space_decision",
                "Control returned an invalid Space read decision.",
            )),
        ));
    }
    Ok(Some((decision_ref, token)))
}

/// `GET /api/v1/spaces/{space_ref}/activity` — what has happened here.
///
/// Activity and Work ask different questions of overlapping evidence. Work asks
/// "what is in flight"; Activity asks "what happened, on whose authority, and
/// with what outcome". So this reads the same run listing with a wider limit
/// and keeps terminal runs, and it adds the two things Work has no use for:
/// the owner effects performed under this Space's authority, and the grants
/// that authorized them.
///
/// # The owner receipts are correlated through the grant, not a Space column
///
/// `conversation_ticket_operations` has no `space_ref`, and deliberately does
/// not gain one. An operation is bound to the exact owner grant it committed
/// against, and that grant carries the `space_ref` Control decided — so
/// Conversation Core's new `/spaces/:ref/activity` joins the two. This closes
/// S2.3's slice 5 ("correlate the owner event into Application Space
/// Activity") without inventing an edge: an effect appears here only if it was
/// genuinely authorized for this room.
///
/// # Every section fails alone and says so
///
/// Runs need Control's `model.thread.read` decision; receipts need Conversation
/// Core; approvals need Model Plane per run. A reader must be able to tell "no
/// approvals were needed" from "we could not ask", so each gap is named with a
/// stable code beside whatever did resolve — the same contract Work and
/// Knowledge use.
async fn space_activity(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: axum::http::HeaderMap,
    Path(space_ref): Path<String>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let space_ref = space_ref.trim();
    if org_id.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership is required to read Space activity.",
            )),
        );
    }
    if space_ref.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_space", "A Space reference is required.")),
        );
    }
    let space = match space_lifecycle_by_ref(&state, &user, &org_id, space_ref).await {
        Ok(Some(space)) => space,
        Ok(None) => {
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

    let mut runs = Value::Array(vec![]);
    let mut operations = Value::Array(vec![]);
    let mut authority = Value::Array(vec![]);
    let mut approvals: Vec<Value> = Vec::new();
    let mut unavailable: Vec<Value> = Vec::new();

    // ---- runs, through the room's read authority -------------------------
    let read = match shared_thread_read_decision(&state, &user, &org_id, space_ref).await {
        Ok(read) => read,
        Err(response) => return response,
    };
    match read.as_ref() {
        Some((decision_ref, token)) => {
            let model_token =
                crate::domains::chat::shared::model_token(&state, &user, &headers).await;
            let session_token = match crate::domains::chat::shared::required_session_token(
                &state, &user, &headers,
            )
            .await
            {
                Ok(token) => Some(token),
                Err(_) => None,
            };
            match session_token {
                None => unavailable.push(json!({
                    "section": "runs",
                    "code": "runs_session_unavailable",
                    "reason": "This session cannot read this room's runs right now.",
                })),
                Some(session_token) => {
                    let url = format!(
                        "{}/v1/runs?limit=100&space_id={}&space_read_decision_ref={}&space_read_decision_token={}",
                        state.model_gateway_url,
                        urlencoding::encode(space_ref),
                        urlencoding::encode(decision_ref),
                        urlencoding::encode(token),
                    );
                    let (status, Json(payload)) =
                        crate::domains::chat::shared::proxy_model_json_with_session(
                            &state,
                            Method::GET,
                            &url,
                            None,
                            model_token.as_deref(),
                            Some(&session_token),
                            &user,
                        )
                        .await;
                    if status.is_success() {
                        runs = payload
                            .get("runs")
                            .filter(|value| value.is_array())
                            .cloned()
                            .unwrap_or(Value::Array(vec![]));
                        // Approvals are per-run upstream, so a room-wide fetch
                        // would be an unbounded fan-out. Ask only for the runs
                        // that are actually gated on a person — those are the
                        // rows whose decision the reader can still change, and
                        // the only ones an approval detail adds anything to.
                        let gated: Vec<String> = runs
                            .as_array()
                            .map(|rows| {
                                rows.iter()
                                    .filter(|row| {
                                        row.get("status").and_then(Value::as_str)
                                            == Some("awaiting_approval")
                                    })
                                    .filter_map(|row| {
                                        row.get("run_id")
                                            .and_then(Value::as_str)
                                            .map(str::to_owned)
                                    })
                                    .take(SPACE_ACTIVITY_APPROVAL_RUNS)
                                    .collect()
                            })
                            .unwrap_or_default();
                        let mut approval_read_failed = false;
                        for run_id in gated {
                            let url = format!(
                                "{}/v1/runs/{}/approvals",
                                state.model_gateway_url,
                                urlencoding::encode(&run_id),
                            );
                            let (status, Json(payload)) =
                                crate::domains::chat::shared::proxy_model_json_with_session(
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
                                approval_read_failed = true;
                                continue;
                            }
                            if let Some(rows) =
                                payload.get("approvals").and_then(Value::as_array)
                            {
                                for row in rows {
                                    let mut row = row.clone();
                                    // Carry the run so the timeline can put an
                                    // approval next to the work it gates.
                                    if let Some(object) = row.as_object_mut() {
                                        object
                                            .entry("run_id".to_owned())
                                            .or_insert_with(|| json!(run_id));
                                    }
                                    approvals.push(row);
                                }
                            }
                        }
                        if approval_read_failed {
                            unavailable.push(json!({
                                "section": "approvals",
                                "code": "approvals_upstream_unavailable",
                                "reason": "Model Plane could not return this room's approvals.",
                            }));
                        }
                    } else {
                        unavailable.push(json!({
                            "section": "runs",
                            "code": "runs_upstream_unavailable",
                            "reason": "Model Plane could not return this room's runs.",
                        }));
                    }
                }
            }
        }
        None => unavailable.push(json!({
            "section": "runs",
            "code": "runs_read_not_authorized",
            "reason": "Reading this Space's shared work is not authorized.",
        })),
    }

    // ---- owner receipts and the authority behind them --------------------
    let receipts_url = format!(
        "{}/api/v1/spaces/{}/activity",
        state.conversation_core_url,
        urlencoding::encode(space_ref),
    );
    let (receipts_status, Json(receipts)) = crate::upstream::proxy_conversation_json(
        &state,
        Method::GET,
        &receipts_url,
        None,
        &user,
        None,
    )
    .await;
    if receipts_status.is_success() {
        let data = crate::envelope::unwrap_data(&receipts);
        operations = data
            .get("operations")
            .filter(|value| value.is_array())
            .cloned()
            .unwrap_or(Value::Array(vec![]));
        authority = data
            .get("authority")
            .filter(|value| value.is_array())
            .cloned()
            .unwrap_or(Value::Array(vec![]));
    } else if receipts_status == StatusCode::NOT_FOUND {
        // A Conversation Core that predates the route. The room is still
        // readable; its owner effects simply are not, and saying which beats
        // an empty list that reads as "nothing has happened here".
        unavailable.push(json!({
            "section": "operations",
            "code": "operations_endpoint_unavailable",
            "reason": "This deployment cannot yet list this room's owner effects.",
        }));
    } else {
        unavailable.push(json!({
            "section": "operations",
            "code": "operations_upstream_unavailable",
            "reason": "Application Plane could not return this room's owner effects.",
        }));
    }

    // Evidence classes this room has no source for yet. Named rather than
    // omitted: the Activity tab's footnote has promised "other owner-plane
    // evidence joins when a correlated Space projection is published" since the
    // cockpit shipped, and a reader deserves to know which ones are still out.
    unavailable.push(json!({
        "section": "delivery",
        "code": "delivery_ledger_not_built",
        "reason": "Durable delivery state is not published yet (S4.4).",
    }));
    unavailable.push(json!({
        "section": "watches",
        "code": "watches_not_built",
        "reason": "Watches are not published yet (S4.3).",
    }));

    (
        StatusCode::OK,
        Json(json!({
            "data": {
                "space": public_space(&space),
                "membership": crate::envelope::unwrap_data(&membership),
                "runs": runs,
                "approvals": approvals,
                "operations": operations,
                "authority": authority,
                "unavailable": unavailable,
            }
        })),
    )
}

/// How many gated runs Activity will fetch approval detail for.
///
/// Approvals are a per-run read upstream, so this is a fan-out bound, not a
/// display limit. Only runs that are waiting on a person are asked about at
/// all; a room with more than this many simultaneous approvals has a bigger
/// problem than a truncated Activity list, and the Work tab is where that queue
/// belongs.
const SPACE_ACTIVITY_APPROVAL_RUNS: usize = 10;

/// Ask Control for this Space's `retrieval.read` authority.
///
/// Same decline semantics as `shared_thread_read_decision`: FORBIDDEN means
/// Control considered the request and said no (retrieval is a separately
/// entitled effect — `retrieval_read_entitled` is its own policy bit, and most
/// orgs have never turned it on), and NOT_FOUND means this Control predates the
/// endpoint. Both are answers, so both degrade the Knowledge tab to a named gap
/// rather than darkening it. Anything else is an outage and fails the call.
async fn space_retrieval_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Result<Option<String>, (StatusCode, Json<Value>)> {
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!(
        "{}/api/v1/internal/spaces/retrieval-decision",
        state.user_core_url
    );
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_ref": space_ref,
            "idempotency_key": format!("space-knowledge-{}", unix_nanos()),
        })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        return Err((status, Json(response)));
    }
    let token = crate::envelope::unwrap_data(&response)
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if token.trim().is_empty() {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(error(
                "invalid_space_decision",
                "Control returned an invalid Space retrieval decision.",
            )),
        ));
    }
    Ok(Some(token))
}

/// `GET /api/v1/spaces/{space_ref}/knowledge` — what this room knows about.
///
/// The Knowledge tab has read "Data Plane has not published a Space projection
/// for this yet" since the cockpit shipped, and the reason was further upstream
/// than a missing endpoint: `documents-api` verified a Space import decision on
/// every create and then discarded the Space, so no document row could say
/// which room it belonged to. `documents.space_ref` is that edge, and
/// retrieval-engine's `/v1/knowledge/space-sources` reads it under the same
/// Control `retrieval.read` authority a grounded room turn already uses.
///
/// # One authority, two sections, separate gaps
///
/// Documents resolve through `documents.space_ref`; wiki pages resolve through
/// the workspace this Space's `space_retrieval_bindings` row names. Those are
/// different mechanisms with different ways to be absent, so Data reports a
/// per-section gap and this endpoint relays it beside whatever did resolve.
///
/// # Why a decline is not an error
///
/// Retrieval is entitled separately from chat. A room whose org never opted
/// into `retrieval_read_entitled` is a working room that cannot list its
/// archive — saying that is the honest answer, and failing the request would
/// make an unconfigured entitlement look like a broken feature.
async fn space_knowledge(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: axum::http::HeaderMap,
    Path(space_ref): Path<String>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let space_ref = space_ref.trim();
    if org_id.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership is required to read Space knowledge.",
            )),
        );
    }
    if space_ref.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_space", "A Space reference is required.")),
        );
    }
    let space = match space_lifecycle_by_ref(&state, &user, &org_id, space_ref).await {
        Ok(Some(space)) => space,
        Ok(None) => {
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

    let mut documents = Value::Array(vec![]);
    let mut wiki_pages = Value::Array(vec![]);
    let mut binding = Value::Null;
    let mut documents_truncated = false;
    let mut unavailable: Vec<Value> = Vec::new();

    match space_retrieval_decision(&state, &user, &org_id, space_ref).await {
        Err(response) => return response,
        Ok(None) => unavailable.push(json!({
            "section": "knowledge",
            "code": "knowledge_read_not_authorized",
            "reason": "Reading this Space's knowledge is not authorized.",
        })),
        Ok(Some(decision)) => {
            let url = format!(
                "{}/v1/knowledge/space-sources",
                state.retrieval_engine_url
            );
            let (status, Json(payload)) =
                crate::domains::knowledge::shared::proxy_data_plane_json_with_space_decision(
                    &state,
                    &user,
                    &headers,
                    Method::POST,
                    &url,
                    Some(json!({ "limit": 50 })),
                    Some(&org_id),
                    &decision,
                )
                .await;
            if status.is_success() {
                // Same `null`-versus-`[]` care as the Work tab: keep only a
                // real array, so a room with nothing in it reaches the browser
                // as an empty list rather than a null the client must guess at.
                documents = payload
                    .get("documents")
                    .filter(|value| value.is_array())
                    .cloned()
                    .unwrap_or(Value::Array(vec![]));
                wiki_pages = payload
                    .get("wiki_pages")
                    .filter(|value| value.is_array())
                    .cloned()
                    .unwrap_or(Value::Array(vec![]));
                binding = payload.get("binding").cloned().unwrap_or(Value::Null);
                documents_truncated = payload
                    .get("documents_truncated")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                // Data's own per-section gaps, relayed rather than restated:
                // it knows which half of its answer is missing and why.
                if let Some(gaps) = payload.get("unavailable").and_then(Value::as_array) {
                    unavailable.extend(gaps.iter().cloned());
                }
            } else if status == StatusCode::FORBIDDEN {
                // Authority verified at Control and refused at Data means the
                // Space has no active binding to a Data target — a real,
                // nameable state, not an outage.
                unavailable.push(json!({
                    "section": "knowledge",
                    "code": "knowledge_binding_unavailable",
                    "reason": "This Space is not bound to a Data Plane knowledge target.",
                }));
            } else {
                unavailable.push(json!({
                    "section": "knowledge",
                    "code": "knowledge_upstream_unavailable",
                    "reason": "Data Plane could not return this room's knowledge.",
                }));
            }
        }
    }

    (
        StatusCode::OK,
        Json(json!({
            "data": {
                "space": public_space(&space),
                "membership": crate::envelope::unwrap_data(&membership),
                "binding": binding,
                "documents": documents,
                "documents_truncated": documents_truncated,
                "wiki_pages": wiki_pages,
                "unavailable": unavailable,
            }
        })),
    )
}

/// `GET /api/v1/spaces/{space_ref}/work` — what this room has running and
/// scheduled.
///
/// The Work tab has been rendering an honest "no plane has published a Space
/// projection for this" since the cockpit shipped, and it was right: Model
/// Plane's run listing was per-thread and owner-bound, and its schedule listing
/// was org-wide with no way to ask about one room. Both now take a Space, so
/// this composes them.
///
/// # Two upstreams, two different authorities, deliberately
///
/// Runs come back through the same `model.thread.read` decision the room's
/// transcript uses, and Session Core lists them THROUGH the threads that
/// decision admits — so Work reaches exactly as far as Chat and no further.
/// Schedules are filtered, not authorized, by `space_ref`: that listing has
/// always returned the verified organization's schedules to any member of it,
/// so narrowing to one room can only show less.
///
/// # Partial is reported, not hidden
///
/// Either upstream can fail on its own. Returning what did resolve with a named
/// gap is the honest answer for a tab whose whole point is "what needs me" —
/// silently dropping the schedules would make a room with pending work look
/// idle, and failing the whole call would hide the runs that did load.
async fn space_work(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: axum::http::HeaderMap,
    Path(space_ref): Path<String>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let space_ref = space_ref.trim();
    if org_id.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership is required to read Space work.",
            )),
        );
    }
    if space_ref.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_space", "A Space reference is required.")),
        );
    }
    // Lifecycle and membership first, exactly as the thread listing does: a
    // suspended room must not answer with its work.
    let space = match space_lifecycle_by_ref(&state, &user, &org_id, space_ref).await {
        Ok(Some(space)) => space,
        Ok(None) => {
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
    let model_token = crate::domains::chat::shared::model_token(&state, &user, &headers).await;

    // ---- runs, through the room's read authority -------------------------
    let read = match shared_thread_read_decision(&state, &user, &org_id, space_ref).await {
        Ok(read) => read,
        Err(response) => return response,
    };
    let mut runs = Value::Array(vec![]);
    let mut unavailable: Vec<Value> = Vec::new();
    match read.as_ref() {
        Some((decision_ref, token)) => {
            let url = format!(
                "{}/v1/runs?limit=50&space_id={}&space_read_decision_ref={}&space_read_decision_token={}",
                state.model_gateway_url,
                urlencoding::encode(space_ref),
                urlencoding::encode(decision_ref),
                urlencoding::encode(token),
            );
            let (status, Json(payload)) =
                crate::domains::chat::shared::proxy_model_json_with_session(
                    &state,
                    Method::GET,
                    &url,
                    None,
                    model_token.as_deref(),
                    Some(&session_token),
                    &user,
                )
                .await;
            if status.is_success() {
                runs = payload
                    .get("runs")
                    .filter(|value| value.is_array())
                    .cloned()
                    .unwrap_or(Value::Array(vec![]));
            } else {
                unavailable.push(json!({
                    "section": "runs",
                    "code": "runs_upstream_unavailable",
                    "reason": "Model Plane could not return this room's runs.",
                }));
            }
        }
        None => {
            // Control declined the shared read, or does not have the endpoint.
            // The room may still be usable; its OTHER members' work simply is
            // not visible, and saying which is better than an empty list.
            unavailable.push(json!({
                "section": "runs",
                "code": "runs_read_not_authorized",
                "reason": "Reading this Space's shared work is not authorized.",
            }));
        }
    }

    // ---- schedules, filtered by the room ---------------------------------
    let mut schedules = Value::Array(vec![]);
    match crate::domains::chat::shared::required_capability_token(&state, &user, &headers).await {
        Ok(capability) => {
            let url = format!(
                "{}/v1/cron?space_ref={}",
                state.model_gateway_url,
                urlencoding::encode(space_ref),
            );
            let (status, Json(payload)) =
                crate::domains::chat::shared::proxy_model_json_with_capability(
                    &state,
                    Method::GET,
                    &url,
                    None,
                    model_token.as_deref(),
                    Some(&capability),
                    &user,
                )
                .await;
            if status.is_success() {
                // Go marshals an empty slice as `null`, so `get("schedules")`
                // returns Some(Null) for a room with no schedules — which is
                // not the same as an array and would reach the browser as
                // `null`. Keep only a real array; anything else becomes the
                // empty list this endpoint promises.
                schedules = payload
                    .get("schedules")
                    .or_else(|| payload.get("data"))
                    .filter(|value| value.is_array())
                    .cloned()
                    .unwrap_or(Value::Array(vec![]));
            } else {
                unavailable.push(json!({
                    "section": "schedules",
                    "code": "schedules_upstream_unavailable",
                    "reason": "Model Plane could not return this room's schedules.",
                }));
            }
        }
        Err(_) => unavailable.push(json!({
            "section": "schedules",
            "code": "schedules_session_unavailable",
            "reason": "This session cannot read schedules right now.",
        })),
    }

    (
        StatusCode::OK,
        Json(json!({
            "data": {
                "space": public_space(&space),
                "membership": crate::envelope::unwrap_data(&membership),
                "runs": runs,
                "schedules": schedules,
                // Always present, empty when nothing is missing: a reader must
                // be able to tell "this room has no work" from "we could not
                // find out", and an absent key makes those look the same.
                "unavailable": unavailable,
            }
        })),
    )
}

/// One Space thread's transcript, read as the room rather than as its owner.
///
/// Deliberately a separate route from Chat's `/chat/threads/{id}/transcript`
/// instead of a flag on it. That one authorizes by finding the thread in the
/// caller's OWN durable list, which is the correct rule for a personal chat
/// history and exactly the wrong one for a shared room — a colleague's post is
/// not in your list, so the room could only ever render its preview. Splitting
/// the routes keeps each one's authority legible: Chat asks "is this yours",
/// the room asks Control "are you a current recipient here".
async fn space_thread_transcript(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: axum::http::HeaderMap,
    Path((space_ref, thread_id)): Path<(String, String)>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let space_ref = space_ref.trim();
    let thread_id = thread_id.trim();
    if org_id.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership is required to read a Space thread.",
            )),
        );
    }
    if space_ref.is_empty() || thread_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_space",
                "A Space reference and thread id are required.",
            )),
        );
    }
    // Lifecycle first: a suspended or deleting room must not serve content on
    // the strength of a membership row alone. Same read, and the same three
    // outcomes, as the sibling thread listing.
    match space_lifecycle_by_ref(&state, &user, &org_id, space_ref).await {
        Ok(Some(_)) => {}
        Ok(None) => {
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
    }
    let read = match shared_thread_read_decision(&state, &user, &org_id, space_ref).await {
        Ok(read) => read,
        Err(response) => return response,
    };
    let Some((decision_ref, token)) = read else {
        // Control declined to authorize a shared read. Say so plainly rather
        // than falling back to the owner-bound route: silently returning only
        // the caller's own turns is how a room starts lying about who said
        // what.
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "space_read_not_authorized",
                "Reading this Space's shared conversation is not authorized.",
            )),
        );
    };
    let session_token =
        match crate::domains::chat::shared::required_session_token(&state, &user, &headers).await {
            Ok(token) => token,
            Err(reason) => return crate::domains::chat::shared::delegated_auth_unavailable(reason),
        };
    let url = format!(
        "{}/v1/threads/{}/messages?space_id={}&space_read_decision_ref={}&space_read_decision_token={}",
        state.model_gateway_url,
        urlencoding::encode(thread_id),
        urlencoding::encode(space_ref),
        urlencoding::encode(&decision_ref),
        urlencoding::encode(&token),
    );
    let model_token = crate::domains::chat::shared::model_token(&state, &user, &headers).await;
    let (status, Json(payload)) = crate::domains::chat::shared::proxy_model_json_with_session(
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
        return (status, Json(payload));
    }
    let turns = payload
        .get("messages")
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    if !turns.is_array() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "invalid_model_gateway_response",
                "Model Gateway returned an invalid conversation.",
            )),
        );
    }
    (
        StatusCode::OK,
        Json(json!({
            "data": {
                "transcript": {
                    "threadId": thread_id,
                    "turns": turns,
                }
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
    // Control decides WHICH Spaces the caller may see; Application only names
    // them. Ask Control first and intersect, so a Space that exists in the
    // Application projection but is absent from Control's index — revoked,
    // never registered, or someone else's — can never appear here merely
    // because the projection still lists it.
    if let Some(index) = control_space_index(&state, &user, &org_id).await {
        let labels = organization_space_labels(&state, &user, &org_id).await;
        let spaces: Vec<Value> = index
            .iter()
            .filter_map(|entry| {
                let space_ref = entry.get("space_ref").and_then(Value::as_str)?;
                let label = labels.iter().find(|candidate| {
                    candidate.get("spaceRef").and_then(Value::as_str) == Some(space_ref)
                });
                Some(json!({
                    "space_ref": space_ref,
                    // A missing label is not a missing Space: Control authorized
                    // it, so it is listed under a neutral name rather than
                    // dropped, which would hide a room the caller is in.
                    "name": label
                        .and_then(|value| value.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or("Space"),
                    "kind": entry.get("kind").and_then(Value::as_str).unwrap_or("room"),
                    "lifecycle": label
                        .and_then(|value| value.get("lifecycle"))
                        .and_then(Value::as_str)
                        .unwrap_or("active"),
                    "role": entry.get("role").and_then(Value::as_str).unwrap_or_default(),
                    // Display fact from the Application labels (the same source
                    // as `name`): lets the surface render the org-wide channel
                    // distinctly. Absent label -> false, never a guess.
                    "is_organization_room": label
                        .and_then(|value| value.get("isOrganizationRoom"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                }))
            })
            .collect();
        return (StatusCode::OK, Json(json!({"data": {"spaces": spaces}})));
    }

    // Control's index is unavailable. Fall back to the Application projection of
    // the caller's own personal Space — the one Space whose membership needs no
    // Control lookup, because registration seeds its owner and there is exactly
    // one per principal. Shared rooms are deliberately NOT guessed at: without
    // Control there is nothing that can say who belongs to them.
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
pub(crate) async fn create_personal_space(
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

    // A named room is the general case of "create a Space"; a personal one is
    // the default because that is what this route has always made and what an
    // omitted `kind` used to mean. Anything else is refused rather than guessed
    // at — `project` and `case` are real Space kinds with owners and lifecycles
    // that nothing in the product creates yet.
    let kind = body
        .as_ref()
        .and_then(|Json(value)| value.get("kind"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
        .unwrap_or("personal")
        .to_owned();
    if kind != "personal" && kind != "room" {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "unsupported_space_kind",
                "Only a personal Space or a room can be created here.",
            )),
        );
    }
    if kind == "room" {
        let Some(name) = name.clone() else {
            return (
                StatusCode::BAD_REQUEST,
                Json(error("room_name_required", "A room needs a name.")),
            );
        };
        let Ok(created) = convex_gateway_call(
            &state,
            "mutation",
            "spaces:createRoomForGateway",
            json!({
                "externalAuthId": user.user_id,
                "externalOrgId": org_id,
                "name": name,
            }),
        )
        .await
        else {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(error(
                    "space_provisioning_unavailable",
                    "The room could not be created. Nothing was provisioned.",
                )),
            );
        };
        return (
            StatusCode::ACCEPTED,
            Json(json!({"data": {"space": public_space(&created)}})),
        );
    }

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

/// Provision the organization's shared room — the org-wide channel every
/// member lands in, Slack's "#general" shape.
///
/// Idempotent the same way the personal ensure is: an organization has at most
/// one org room (`spaces:ensureOrganizationRoomForGateway` enforces the
/// invariant), so a repeat call returns the existing record. The normal caller
/// is the onboarding create-organization action; this route exists so an
/// organization created BEFORE that hook shipped can self-heal from the Spaces
/// surface, and so a Convex outage during onboarding stays retryable.
///
/// Same 202 semantics as the personal create: the room exists but is
/// `pending_registration` until Control registers it, and the org roster is
/// converged onto it afterwards by the membership sync — a fresh room listing
/// only its registrar is a real intermediate state, not a failure.
pub(crate) async fn ensure_organization_room(
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
                "An active organization is required before its room can be created.",
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
        "spaces:ensureOrganizationRoomForGateway",
        args,
    )
    .await
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "space_provisioning_unavailable",
                "The organization room could not be created. Nothing was provisioned.",
            )),
        );
    };
    (
        StatusCode::ACCEPTED,
        Json(json!({"data": {"space": public_space(&created)}})),
    )
}

/// Who is in this Space, for a caller who is in it.
///
/// A straight proxy of Control's roster: membership is the authority and the
/// display identity comes with it, so there is nothing for this plane to
/// compose or decide. Control answers 404 to a non-member, which passes through
/// unchanged — "you cannot see this" must not be softened into an empty room.
async fn space_roster(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let space_ref = space_ref.trim();
    if org_id.is_empty() || space_ref.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership and Space are required.",
            )),
        );
    }
    let url = format!(
        "{}/api/v1/internal/spaces/{}/roster",
        state.user_core_url,
        urlencoding::encode(space_ref)
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

/// The active Space this ref names, at ANY kind — personal, room, project, case.
///
/// `space_context` and the thread projection used to resolve the caller's
/// *personal* Space and then require the requested ref to equal it. That held
/// while a personal Space was the only kind that existed. Once organization
/// rooms started appearing in the sidebar, every one of them answered 404, and
/// the room rendered as "your membership could not be confirmed" — a Space you
/// can navigate to but can never open, with an error blaming your membership
/// for what was really a personal-only lookup.
///
/// This answers "what and where is this room", nothing more. Whether the caller
/// may act in it stays Control's decision, checked separately by every caller.
async fn space_lifecycle_by_ref(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Result<Option<Value>, ()> {
    if org_id.trim().is_empty() || space_ref.trim().is_empty() {
        return Err(());
    }
    let value = convex_gateway_call(
        state,
        "query",
        "spaces:spacesForOrgForGateway",
        json!({ "externalAuthId": user.user_id, "externalOrgId": org_id }),
    )
    .await?;
    let Some(spaces) = value.as_array() else {
        return Err(());
    };
    Ok(spaces
        .iter()
        .find(|space| {
            space.get("spaceRef").and_then(Value::as_str) == Some(space_ref)
                && space.get("lifecycle").and_then(Value::as_str) == Some("active")
        })
        .cloned())
}

/// Space agent bindings from Application: identity for agents Control has
/// already authorized. Display only, and degrade-safe — a failed read costs the
/// names, never the roster.
async fn space_agent_bindings(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Vec<Value> {
    let Ok(value) = convex_gateway_call(
        state,
        "query",
        "spaceAgents:spaceAgentBindingsForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
        }),
    )
    .await
    else {
        return Vec::new();
    };
    value.as_array().cloned().unwrap_or_default()
}

/// The Space's agent participants.
///
/// Two planes answer two different questions here, and keeping them apart is
/// the entire contract (`docs/SPACE_AGENT_SCOPE_PLAN_2026-08-14.md` §3.2):
///
/// * **Control** answers *who may act in this room* — service-typed members of
///   the Space. It is authoritative, and a caller who cannot read the roster
///   gets an error rather than an empty list, because "I could not check" must
///   never render as "this room has no agents".
/// * **Application** answers *what that agent is called* — the binding
///   projection carrying name, title, lifecycle and published channels.
///
/// The join runs Control-first. An agent Control authorizes but Application has
/// not described still appears, flagged as having no published identity; the
/// reverse — a binding with no Control membership — is dropped entirely. That
/// asymmetry is deliberate: an unnamed participant is a gap in presentation,
/// while an unauthorized one rendered as present is the false membership claim
/// this whole split exists to prevent.
async fn space_agents(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let space_ref = space_ref.trim().to_owned();
    if org_id.is_empty() || space_ref.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership and Space are required.",
            )),
        );
    }
    match compose_space_agents(&state, &user, &org_id, &space_ref).await {
        Ok(agents) => (StatusCode::OK, Json(json!({"data": {"agents": agents}}))),
        Err(response) => response,
    }
}

/// §UI-4: every published agent definition installed anywhere in the org,
/// grouped by definition with one row per Space it is bound in — ADR-0002
/// (`apps/CROSS_SPACE_AGENT_REGISTRY_ADR_2026-08-19.md`, "Application Plane
/// owns the cross-Space agent registry").
///
/// This narrow slice originally composed the per-Space calls the Agent tab
/// already used (`control_space_index` + `compose_space_agents`, one
/// Control-roster round trip per Space) because a new cross-Space Convex
/// query was an undecided backend contract at the time. ADR-0002 settles
/// that: Application already owns both source tables (`spaces`,
/// `spaceAgentBindings`/`agents`) in the same Convex database, so this is now
/// a single org-scoped read (`spaceAgents:agentInstallationsForOrgForGateway`)
/// instead of an O(N) fan-out over the org's Spaces.
///
/// # Presence, not authority
///
/// The registry is gated on the caller's org membership only (which
/// `agentInstallationsForOrgForGateway` itself verifies via
/// `requireGatewayMember`, same as `spacesForOrgForGateway`) — it does NOT
/// re-check each binding against Control's live per-Space roster the way
/// `compose_space_agents` does for the single-Space Agent tab. That is
/// intentional (ADR-0002, "the registry answers presence, not authority"):
/// this is a read-only, org-wide LABEL surface — name, title, kind,
/// lifecycle, which Space a binding lives in — never an implicit "and
/// therefore the caller may invoke it." Nothing on this response path lets a
/// caller act on a binding; any future surface that would must still resolve
/// Control's roster for that specific Space first, exactly as
/// `compose_space_agents` already does.
async fn list_org_agent_installations(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    if org_id.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership is required.",
            )),
        );
    }

    let Ok(value) = convex_gateway_call(
        &state,
        "query",
        "spaceAgents:agentInstallationsForOrgForGateway",
        json!({ "externalAuthId": user.user_id, "externalOrgId": org_id }),
    )
    .await
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "agent_registry_unavailable",
                "The organization's agent installations could not be resolved.",
            )),
        );
    };
    let bindings = value.as_array().cloned().unwrap_or_default();

    let mut by_definition: std::collections::BTreeMap<String, Value> =
        std::collections::BTreeMap::new();
    for binding in bindings {
        // Only identity-published bindings have a definition to group by —
        // the registry itself already drops any binding whose definition is
        // gone, so every row reaching here has one.
        let Some(agent_ref) = binding.get("agentRef").and_then(Value::as_str) else {
            continue;
        };
        let entry = by_definition.entry(agent_ref.to_owned()).or_insert_with(|| {
            json!({
                "agent_ref": agent_ref,
                "name": binding.get("name").cloned().unwrap_or(Value::Null),
                "description": binding.get("description").cloned().unwrap_or(Value::Null),
                "definition_status": binding.get("definitionStatus").cloned().unwrap_or(Value::Null),
                "installations": [],
            })
        });
        entry["installations"]
            .as_array_mut()
            .expect("literal array")
            .push(json!({
                "space_ref": binding.get("spaceRef").cloned().unwrap_or(Value::Null),
                "space_name": binding.get("spaceName").cloned().unwrap_or(json!("Space")),
                "space_kind": binding.get("spaceKind").cloned().unwrap_or(json!("room")),
                "status": binding.get("status").cloned().unwrap_or(Value::Null),
            }));
    }

    let definitions: Vec<Value> = by_definition.into_values().collect();
    (
        StatusCode::OK,
        Json(json!({"data": {"definitions": definitions}})),
    )
}

const CREATE_AGENT_ROLES: [&str; 2] = ["owner", "manager"];
const MAX_CREATE_AGENT_NAME_CHARS: usize = 60;
const MAX_CREATE_AGENT_INSTRUCTIONS_CHARS: usize = 4000;

/// Creates a simple agent from inside the room — the scope plan's §UI-3b
/// two-step, server-confirmed flow, sequenced here so the browser never holds
/// authority: (1) Application lands the definition and a `pending` binding in
/// one transaction; (2) Application declares the room's full service roster to
/// Control, and only Control's acceptance flips the binding `active`. The
/// creating human's room role is the authorizing decision (QM: authority for
/// future agent behavior comes from outside the agent) — resolved from
/// Control under the caller's own signed delegation, never from the body.
pub(crate) async fn create_space_agent(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let space_ref = space_ref.trim().to_owned();
    if org_id.is_empty() || space_ref.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership and Space are required.",
            )),
        );
    }

    let name = body
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    if name.chars().count() < 2 || name.chars().count() > MAX_CREATE_AGENT_NAME_CHARS {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_agent_name",
                "An agent name must be between 2 and 60 characters.",
            )),
        );
    }
    let instructions = body
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if instructions
        .as_deref()
        .is_some_and(|value| value.chars().count() > MAX_CREATE_AGENT_INSTRUCTIONS_CHARS)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_agent_instructions",
                "Agent instructions must stay under 4000 characters.",
            )),
        );
    }
    let avatar_color = body
        .get("avatar_color")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| {
            value.len() == 7
                && value.starts_with('#')
                && value[1..].chars().all(|ch| ch.is_ascii_hexdigit())
        })
        .map(str::to_owned);

    // Who may create is a Control fact about THIS room, not an org role: the
    // caller's own membership is resolved under their signed delegation.
    if let Err(response) = require_space_agent_grant_role(&state, &user, &org_id, &space_ref).await
    {
        return response;
    }

    // Convex `v.optional` accepts an absent key, never an explicit null —
    // optional fields are inserted only when present.
    let mut create_args = json!({
        "externalAuthId": user.user_id,
        "externalOrgId": org_id,
        "spaceRef": space_ref,
        "name": name,
    });
    if let Some(instructions) = instructions {
        create_args["instructions"] = json!(instructions);
    }
    if let Some(avatar_color) = avatar_color {
        create_args["avatarColor"] = json!(avatar_color);
    }
    let Ok(created) = convex_gateway_call(
        &state,
        "mutation",
        "spaceAgents:createSpaceAgentForGateway",
        create_args,
    )
    .await
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "agent_creation_unavailable",
                "The agent could not be created. Nothing was provisioned.",
            )),
        );
    };

    if !confirm_space_agent_membership(&state, &org_id, &space_ref).await {
        // The definition and its pending binding exist and render truthfully
        // as not-yet-a-member; nothing may invoke the agent until Control
        // accepts the roster. The caller learns exactly that.
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "agent_membership_unconfirmed",
                "The agent was created, but Control has not confirmed its room membership yet. It stays pending in the Agent tab.",
            )),
        );
    }

    (
        StatusCode::CREATED,
        Json(json!({"data": {
            "agent_ref": created.get("agentRef").and_then(Value::as_str).unwrap_or_default(),
            "subject_id": created.get("subjectId").and_then(Value::as_str).unwrap_or_default(),
            "status": "active",
        }})),
    )
}

/// Who may grant a new agent (created or bound) a place in this room: the
/// caller's own Control-resolved room role, checked under their signed
/// delegation — never an org-wide role, and never something the request body
/// could assert. Shared by §UI-3b (create new) and §UI-3 (bind existing) so
/// the two routes can never quietly diverge on who is allowed to grant.
/// Declare a managed room's people to Control after a grant or revocation.
///
/// Separate from `confirm_space_agent_membership` because the two converge
/// different subject types: this one says `["user"]`, that one says
/// `["service"]`. Merging them would let a human roster change revoke the
/// room's agents.
async fn sync_room_members(state: &AppState, org_id: &str, space_ref: &str) -> bool {
    convex_gateway_call(
        state,
        "action",
        "spaceMembers:syncRoomMembersForGateway",
        json!({
            "externalOrgId": org_id,
            "spaceRef": space_ref,
        }),
    )
    .await
    .map(|value| {
        value
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| status == "applied")
    })
    .unwrap_or(false)
}

/// Shared body of "change who is in this room".
///
/// Adding and removing differ only in the mutation called, so they share the
/// role gate, the Control convergence, and the refusal shapes. Both use the
/// same owner/manager floor as granting an agent: deciding who may read a
/// room's shared record is at least as consequential.
async fn apply_room_membership(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    member_id: &str,
    add: bool,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let space_ref = space_ref.trim();
    let member_id = member_id.trim();
    if org_id.is_empty() || space_ref.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership and Space are required.",
            )),
        );
    }
    if member_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("member_required", "A member is required.")),
        );
    }
    if let Err(response) = require_space_agent_grant_role(state, user, &org_id, space_ref).await {
        return response;
    }

    let mutation = if add {
        "spaceMembers:addSpaceMemberForGateway"
    } else {
        "spaceMembers:removeSpaceMemberForGateway"
    };
    let Ok(result) = convex_gateway_call(
        state,
        "mutation",
        mutation,
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
            "memberExternalAuthId": member_id,
        }),
    )
    .await
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "room_membership_unavailable",
                "The room's members could not be changed. Nothing was altered.",
            )),
        );
    };

    // Application's list is only an intent until Control accepts it. Say so
    // rather than reporting success on a list nobody is enforcing yet.
    if !sync_room_members(state, &org_id, space_ref).await {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "room_membership_unconfirmed",
                "The change was recorded, but Control has not confirmed the room's members yet.",
            )),
        );
    }

    (
        StatusCode::OK,
        Json(json!({"data": {
            "member_count": result.get("memberCount").and_then(Value::as_u64).unwrap_or(0),
            "changed": result
                .get(if add { "added" } else { "removed" })
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }})),
    )
}

/// `POST /api/v1/spaces/{space_ref}/members` — add one person to a named room.
async fn add_space_member(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let member_id = body
        .get("member_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    apply_room_membership(&state, &user, &space_ref, &member_id, true).await
}

/// `DELETE /api/v1/spaces/{space_ref}/members/{member_id}` — remove one person.
async fn remove_space_member(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, member_id)): Path<(String, String)>,
) -> (StatusCode, Json<Value>) {
    apply_room_membership(&state, &user, &space_ref, &member_id, false).await
}

/// Shared body of "change one binding's standing in this room".
///
/// Pause, resume and revoke differ only in the status asked for and in whether
/// Control has to be told afterwards, so they share one path: the same role
/// gate as adding an agent (governing an agent here is the same class of
/// decision as granting one), the same Convex mutation, and the same refusal
/// shapes.
async fn apply_space_agent_state(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    binding_ref: &str,
    status: &str,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let space_ref = space_ref.trim();
    let binding_ref = binding_ref.trim();
    if org_id.is_empty() || space_ref.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership and Space are required.",
            )),
        );
    }
    if binding_ref.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "binding_ref_required",
                "A binding reference is required.",
            )),
        );
    }

    // Role first, and against THIS room: the Convex mutation re-checks that the
    // binding belongs to the Space named here, so a manager of one room cannot
    // reach into another.
    if let Err(response) = require_space_agent_grant_role(state, user, &org_id, space_ref).await {
        return response;
    }

    let Ok(updated) = convex_gateway_call(
        state,
        "mutation",
        "spaceAgents:setSpaceAgentBindingStateForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
            "bindingRef": binding_ref,
            "status": status,
        }),
    )
    .await
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "agent_binding_state_unavailable",
                "The agent's standing in this room could not be changed. Nothing was altered.",
            )),
        );
    };

    // Only revocation changes who is in the room, so only revocation needs
    // Control to converge. Pausing keeps the agent a member that may not be
    // invoked, which the gateway's own invocation path already enforces.
    if status == "revoked" && !confirm_space_agent_membership(state, &org_id, space_ref).await {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "agent_revocation_unconfirmed",
                "The agent was revoked here, but Control has not confirmed the room's roster yet. It may still appear as a member until it does.",
            )),
        );
    }

    (
        StatusCode::OK,
        Json(json!({"data": {
            "binding_ref": updated.get("bindingRef").and_then(Value::as_str).unwrap_or(binding_ref),
            "status": updated.get("status").and_then(Value::as_str).unwrap_or(status),
            "changed": updated.get("changed").and_then(Value::as_bool).unwrap_or(false),
        }})),
    )
}

/// `PATCH /api/v1/spaces/{space_ref}/agents/{binding_ref}` — pause or resume.
///
/// Accepts only `active` and `paused`. Revocation is the DELETE below rather
/// than a third status here: it is the one irreversible option, it is the one
/// that changes Control's roster, and putting it behind its own verb keeps a
/// mistyped status from removing an agent.
async fn set_space_agent_state(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, binding_ref)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let status = body
        .get("status")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if status != "active" && status != "paused" {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_agent_binding_status",
                "An agent can only be paused or resumed here. Use DELETE to revoke it.",
            )),
        );
    }
    apply_space_agent_state(&state, &user, &space_ref, &binding_ref, status).await
}

/// `DELETE /api/v1/spaces/{space_ref}/agents/{binding_ref}` — revoke.
///
/// The binding row survives; `space-defenition.md` keeps a revoked binding for
/// audit history and never renders it as a participant. What is removed is the
/// agent's membership in Control's roster, which is what actually stops it
/// acting here.
async fn revoke_space_agent(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, binding_ref)): Path<(String, String)>,
) -> (StatusCode, Json<Value>) {
    apply_space_agent_state(&state, &user, &space_ref, &binding_ref, "revoked").await
}

async fn require_space_agent_grant_role(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
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
    let (membership_status, Json(membership_response)) = proxy_json(
        state,
        Method::GET,
        &membership_url,
        None,
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !membership_status.is_success() {
        return Err((membership_status, Json(membership_response)));
    }
    let role = crate::envelope::unwrap_data(&membership_response)
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if !CREATE_AGENT_ROLES.contains(&role.as_str()) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "space_role_cannot_create_agent",
                "Only a Space owner or manager can add an agent here.",
            )),
        ));
    }
    Ok(())
}

/// ADR-0003's Space-instructions role floor: `editor`/`manager`/`owner`, not
/// `viewer` — this is a durable write that affects every future turn in the
/// Space, not a read. Mirrors Control's own `ValidateForSharedThread` floor
/// (`user-core/internal/spaces/personal_thread_decision.go`).
const SPACE_INSTRUCTIONS_WRITE_ROLES: [&str; 3] = ["editor", "manager", "owner"];

/// Resolves the caller's live Control-issued role on `space_ref` and returns
/// it on success — same `GET .../membership` call as
/// [`require_space_agent_grant_role`], generalized to return the role instead
/// of hard-coding one allow-list, since the read path (any active member) and
/// the write path (`editor`/`manager`/`owner`) need different floors over the
/// same membership fact.
async fn resolve_space_role(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Result<String, (StatusCode, Json<Value>)> {
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
    let (membership_status, Json(membership_response)) = proxy_json(
        state,
        Method::GET,
        &membership_url,
        None,
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !membership_status.is_success() {
        return Err((membership_status, Json(membership_response)));
    }
    Ok(crate::envelope::unwrap_data(&membership_response)
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned())
}

/// GET `/api/v1/spaces/{space_ref}/instructions` — any active Space member
/// (viewer included) may read the Space's authored instructions: they affect
/// every turn a viewer takes part in too, not only an editor's.
async fn get_space_instructions(
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
    if let Err(response) = resolve_space_role(&state, &user, &org_id, space_ref).await {
        return response;
    }
    let instructions = fetch_space_instructions(&state, &org_id, space_ref).await;
    (
        StatusCode::OK,
        Json(json!({"data": {"instructions": instructions}})),
    )
}

#[derive(Deserialize)]
pub(crate) struct UpdateSpaceInstructionsRequest {
    #[serde(default)]
    pub(crate) instructions: Option<String>,
}

const MAX_SPACE_INSTRUCTIONS_LENGTH: usize = 4000;

/// PATCH `/api/v1/spaces/{space_ref}/instructions` — ADR-0003's Space-layer
/// authoring write, gated to `editor`/`manager`/`owner` (see
/// [`SPACE_INSTRUCTIONS_WRITE_ROLES`]).
pub(crate) async fn update_space_instructions(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    Json(body): Json<UpdateSpaceInstructionsRequest>,
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
    let role = match resolve_space_role(&state, &user, &org_id, space_ref).await {
        Ok(role) => role,
        Err(response) => return response,
    };
    if !SPACE_INSTRUCTIONS_WRITE_ROLES.contains(&role.as_str()) {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "space_role_cannot_edit_instructions",
                "Only a Space editor, manager, or owner can change its instructions.",
            )),
        );
    }
    let instructions = body
        .instructions
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    // Character count, not byte length — see orgs/instructions.rs's identical
    // check for why: Convex and the browser both count characters, and a
    // byte-length check would wrongly reject valid Norwegian text.
    if instructions.is_some_and(|value| value.chars().count() > MAX_SPACE_INSTRUCTIONS_LENGTH) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "instructions_too_long",
                "Space instructions must be 4000 characters or fewer.",
            )),
        );
    }
    let result = convex_gateway_call(
        &state,
        "mutation",
        "spaces:setInstructionsForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
            "instructions": instructions,
        }),
    )
    .await;
    match result {
        Ok(value) => (StatusCode::OK, Json(json!({"data": value}))),
        Err(()) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "instructions_unavailable",
                "Space instructions could not be saved.",
            )),
        ),
    }
}

/// Step 2 of the governed two-step, shared by create and bind: declare the
/// room's full service roster to Control and report whether it accepted.
async fn confirm_space_agent_membership(state: &AppState, org_id: &str, space_ref: &str) -> bool {
    let confirmation = convex_gateway_call(
        state,
        "action",
        "spaceAgents:confirmSpaceAgentMembershipForGateway",
        json!({
            "externalOrgId": org_id,
            "spaceRef": space_ref,
        }),
    )
    .await;
    confirmation
        .as_ref()
        .ok()
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        == Some("applied")
}

/// §UI-3: agent definitions this org has, for an owner/manager browsing what
/// they can add to THIS room. `already_bound` is reported, not filtered out —
/// hiding an already-added definition would look like it silently vanished
/// rather than telling the truth about its state.
async fn list_installable_space_agents(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let space_ref = space_ref.trim().to_owned();
    if org_id.is_empty() || space_ref.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership and Space are required.",
            )),
        );
    }
    if let Err(response) = require_space_agent_grant_role(&state, &user, &org_id, &space_ref).await
    {
        return response;
    }

    let Ok(definitions) = convex_gateway_call(
        &state,
        "query",
        "spaceAgents:listInstallableSpaceAgentsForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
        }),
    )
    .await
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "installable_agents_unavailable",
                "The organization's agent definitions could not be loaded.",
            )),
        );
    };
    let agents = definitions.as_array().cloned().unwrap_or_default();
    let agents: Vec<Value> = agents
        .into_iter()
        .map(|definition| {
            json!({
                "agent_ref": definition.get("agentRef").cloned().unwrap_or(Value::Null),
                "name": definition.get("name").cloned().unwrap_or(Value::Null),
                "description": definition.get("description").cloned().unwrap_or(Value::Null),
                "definition_status": definition.get("definitionStatus").cloned().unwrap_or(Value::Null),
                "already_bound": definition.get("alreadyBound").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect();
    (StatusCode::OK, Json(json!({"data": {"agents": agents}})))
}

/// §UI-3: bind an EXISTING agent definition to this room. Distinct from
/// `create_space_agent` (§UI-3b) only in step 1 — an `agent_ref` the caller
/// picked from `list_installable_space_agents`, not a freshly authored name.
pub(crate) async fn bind_existing_space_agent(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let space_ref = space_ref.trim().to_owned();
    if org_id.is_empty() || space_ref.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership and Space are required.",
            )),
        );
    }
    let agent_ref = body
        .get("agent_ref")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let Some(agent_ref) = agent_ref else {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "agent_ref_required",
                "An existing agent_ref is required to bind an agent.",
            )),
        );
    };

    if let Err(response) = require_space_agent_grant_role(&state, &user, &org_id, &space_ref).await
    {
        return response;
    }

    let Ok(bound) = convex_gateway_call(
        &state,
        "mutation",
        "spaceAgents:bindExistingSpaceAgentForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
            "agentId": agent_ref,
        }),
    )
    .await
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "agent_binding_unavailable",
                "The agent could not be bound. Nothing was provisioned.",
            )),
        );
    };

    if !confirm_space_agent_membership(&state, &org_id, &space_ref).await {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "agent_membership_unconfirmed",
                "The agent was bound, but Control has not confirmed its room membership yet. It stays pending in the Agent tab.",
            )),
        );
    }

    (
        StatusCode::CREATED,
        Json(json!({"data": {
            "subject_id": bound.get("subjectId").and_then(Value::as_str).unwrap_or_default(),
            "status": "active",
        }})),
    )
}

/// Control roster ∩ Application binding, composed into the same agent shape
/// `space_agents` returns to the browser. Pulled out so the invocation path
/// (`resolve_mentioned_space_agent`, below) authorizes a mention against
/// exactly what the room's own Agent tab shows — one join, two callers,
/// rather than a second copy that could quietly drift from the first.
async fn compose_space_agents(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Result<Vec<Value>, (StatusCode, Json<Value>)> {
    let roster_url = format!(
        "{}/api/v1/internal/spaces/{}/roster",
        state.user_core_url,
        urlencoding::encode(space_ref)
    );
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let (status, Json(roster_response)) = proxy_json(
        state,
        Method::GET,
        &roster_url,
        None,
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        return Err((status, Json(roster_response)));
    }

    // Control's envelope is `{data: {members: [...], count: N}}` — not a bare
    // array. Reading it as one silently yields zero agents, which is the exact
    // "empty room" lie this handler is built to avoid.
    let members = crate::envelope::unwrap_data(&roster_response)
        .get("members")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let bindings = space_agent_bindings(state, user, org_id, space_ref).await;

    let agents = members
        .iter()
        .filter(|member| member.get("subject_type").and_then(Value::as_str) == Some("service"))
        .map(|member| {
            let subject_id = member
                .get("subject_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let binding = bindings.iter().find(|binding| {
                binding.get("subjectId").and_then(Value::as_str) == Some(subject_id)
            });

            let mut agent = json!({
                "subject_id": subject_id,
                "role": member.get("role").and_then(Value::as_str).unwrap_or_default(),
                "revision": member.get("revision").and_then(Value::as_i64).unwrap_or_default(),
                "identity_published": binding.is_some(),
            });
            let object = agent.as_object_mut().expect("literal object");

            match binding {
                Some(binding) => {
                    for (from, to) in [
                        ("bindingRef", "binding_ref"),
                        ("agentRef", "agent_ref"),
                        ("name", "name"),
                        ("title", "title"),
                        ("description", "description"),
                        ("status", "status"),
                        ("definitionStatus", "definition_status"),
                        ("updatedAt", "updated_at"),
                        // Binding policy — absent on legacy bindings, and the
                        // invocation path treats absence as the legacy default.
                        ("triggerModes", "trigger_modes"),
                        ("allowedTools", "allowed_tools"),
                        ("approvalMode", "approval_mode"),
                    ] {
                        if let Some(value) = binding.get(from) {
                            if !value.is_null() {
                                object.insert(to.to_owned(), value.clone());
                            }
                        }
                    }
                    object.insert(
                        "delivery_targets".to_owned(),
                        binding
                            .get("deliveryTargets")
                            .cloned()
                            .unwrap_or_else(|| json!([])),
                    );
                }
                None => {
                    // Control's own label for the subject, when it has one. Left
                    // absent otherwise so the browser renders its own honest
                    // "identity not published" state rather than a name we made up.
                    if let Some(display_name) = member
                        .get("display_name")
                        .and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                    {
                        object.insert("name".to_owned(), json!(display_name));
                    }
                    object.insert("delivery_targets".to_owned(), json!([]));
                }
            }

            agent
        })
        .collect();

    Ok(agents)
}

/// The one field the browser may name a mention by: the Control subject id
/// already visible on every `SpaceAgent` the room's roster returns. Never a
/// binding ref, an agent ref, or anything Convex-internal — those are gateway
/// implementation detail the browser has no business carrying.
const MENTIONED_AGENT_FIELDS: [&str; 2] = ["mentioned_agent_ref", "mentionedAgentRef"];

/// Exchange a browser-supplied agent mention for the two facts that actually
/// authorize and voice it: an active Control membership for that subject in
/// this Space, and the Application binding naming it. Only a hit on both
/// counts as "this agent may answer here" — the same rule `compose_space_agents`
/// draws for what to show, applied here to what to allow
/// (`docs/space-defenition.md`, "Invocation rule": a mention invokes, it never
/// grants — this is the check that keeps that true).
async fn resolve_mentioned_space_agent(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    subject_id: &str,
) -> Result<Option<Value>, (StatusCode, Json<Value>)> {
    let agents = compose_space_agents(state, user, org_id, space_ref).await?;
    Ok(agents.into_iter().find(|agent| {
        agent.get("subject_id").and_then(Value::as_str) == Some(subject_id)
            && agent.get("status").and_then(Value::as_str) == Some("active")
    }))
}

/// This agent's own instructions, resolved from its Convex definition. Never
/// forwarded to the browser — the mention UI only ever carries `subject_id`;
/// this is the one place `agent_ref` (a Convex id) is read, and it goes
/// straight into the outbound model turn.
async fn fetch_agent_persona(state: &AppState, org_id: &str, agent_ref: &str) -> Option<Value> {
    convex_gateway_call(
        state,
        "query",
        "spaceAgents:agentPersonaForGateway",
        json!({ "externalOrgId": org_id, "agentId": agent_ref }),
    )
    .await
    .ok()
    .filter(|value| !value.is_null())
}

/// Read (never consume) whichever spelling of the Space reference the request
/// carries. `inject_personal_thread_context` is the one that actually removes
/// it once it has issued its own decision — this only needs to know the ref is
/// there before that happens.
fn peek_space_ref(object: &serde_json::Map<String, Value>) -> Option<String> {
    object
        .get("space_ref")
        .or_else(|| object.get("spaceRef"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// ADR-0003's org layer: the org-admin-authored instruction, resolved from
/// Convex. `None` for an org with nothing authored — same absent-means-quiet
/// contract as `fetch_agent_persona`.
async fn fetch_org_instructions(state: &AppState, org_id: &str) -> Option<String> {
    let value = convex_gateway_call(
        state,
        "query",
        "organizations:instructionsForGateway",
        json!({ "externalOrgId": org_id }),
    )
    .await
    .ok()?;
    value
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// ADR-0003's Space layer: the Space owner/manager/editor-authored
/// instruction, resolved from Convex. `None` for a Space with nothing
/// authored, or one Convex cannot resolve for this org.
async fn fetch_space_instructions(
    state: &AppState,
    org_id: &str,
    space_ref: &str,
) -> Option<String> {
    let value = convex_gateway_call(
        state,
        "query",
        "spaces:instructionsForGateway",
        json!({ "externalOrgId": org_id, "spaceRef": space_ref }),
    )
    .await
    .ok()?;
    value
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// ADR-0003 — stamps the org (always, when `org_id` is known) and Space
/// (when the turn carries a `space_ref`) authored-instruction layers onto the
/// outbound turn as `org_instructions`/`space_instructions`, the two fields
/// `model-gateway`'s composed authored-instructions message reads. Fires on
/// every turn, unlike the mention-gated agent persona — a personal chat with
/// no Space still gets the org layer.
///
/// Any client-supplied value for either field is discarded first: these are
/// server-resolved, same provenance/trust class as `org_name`/`agent_name`,
/// never client-suppliable.
///
/// The Space layer is gated on the caller's own live Control membership
/// (`resolve_space_role`) before Convex is even asked — the same gate the
/// `GET/PATCH .../instructions` routes use. This does NOT rely on
/// `inject_personal_thread_context` independently rejecting a non-member's
/// `space_ref` afterward: that rejection happens to occur today, but nothing
/// ties its timing to this function, and a future change to either call site
/// could silently turn an unrelated ordering assumption into a real
/// cross-Space instructions disclosure.
///
/// Must run before [`inject_personal_thread_context`], which is the call that
/// removes the Space reference fields from the body — this only peeks at
/// `space_ref`, mirroring [`inject_mentioned_space_agent_persona`].
pub(crate) async fn inject_authored_instructions(
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
    object.remove("org_instructions");
    object.remove("space_instructions");

    let org_id = org_id.trim();
    if org_id.is_empty() {
        return Ok(());
    }
    if let Some(instructions) = fetch_org_instructions(state, org_id).await {
        object.insert("org_instructions".to_owned(), json!(instructions));
    }

    if let Some(space_ref) = peek_space_ref(object) {
        resolve_space_role(state, user, org_id, &space_ref).await?;
        if let Some(instructions) = fetch_space_instructions(state, org_id, &space_ref).await {
            object.insert("space_instructions".to_owned(), json!(instructions));
        }
    }
    Ok(())
}

/// Exchange a client-supplied `@` mention for a Control- and
/// Application-verified agent persona, injected into the outbound turn as
/// `agent_name`/`agent_system_prompt` — the two fields
/// `model-gateway`'s `agent_persona_message` reads. Absent on every turn
/// nobody addressed to an agent; unchanged behavior in that case.
///
/// Must run before [`inject_personal_thread_context`], which is the call that
/// actually removes the Space reference fields from the body.
pub(crate) async fn inject_mentioned_space_agent_persona(
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

    let mentioned = MENTIONED_AGENT_FIELDS
        .iter()
        .find_map(|field| object.remove(*field))
        .and_then(|value| value.as_str().map(str::trim).map(str::to_owned))
        .filter(|value| !value.is_empty());
    let Some(subject_id) = mentioned else {
        return Ok(());
    };

    let Some(space_ref) = peek_space_ref(object) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(error(
                "agent_mention_requires_space",
                "An agent can only be mentioned inside a Space.",
            )),
        ));
    };
    if org_id.trim().is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership is required to mention an agent.",
            )),
        ));
    }

    let agent = resolve_mentioned_space_agent(state, user, org_id, &space_ref, &subject_id)
        .await?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(error(
                    "mentioned_agent_not_bound",
                    "This agent is not bound to this Space.",
                )),
            )
        })?;

    // Binding policy enforcement (space-defenition.md "Binding model"). An
    // ABSENT field is the legacy default — bindings created before policy
    // fields existed keep behaving exactly as they did — while a PRESENT
    // field is a human decision and binds.
    apply_mention_binding_policy(object, &agent)?;

    let name = agent
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&subject_id)
        .to_owned();
    object.insert("agent_name".to_owned(), json!(name));

    // Best-effort: a persona missing its instructions still answers, in its
    // own name, on the framing message alone.
    if let Some(agent_ref) = agent.get("agent_ref").and_then(Value::as_str) {
        if let Some(persona) = fetch_agent_persona(state, org_id, agent_ref).await {
            if let Some(system_prompt) = persona.get("systemPrompt").and_then(Value::as_str) {
                object.insert("agent_system_prompt".to_owned(), json!(system_prompt));
            }
        }
    }

    Ok(())
}

/// Enforces the mentioned binding's policy on the outbound turn.
///
/// * `trigger_modes` — a present list without `"mention"` refuses the mention
///   itself: the binding says this agent is not invoked that way.
/// * `approval_mode: "blocked"` — refuses the invocation outright.
/// * `approval_mode: "require_confirmation"` (and the legacy absent case) —
///   the turn may never run autonomously: `plan_mode` is forced off and the
///   `agentic` feature is stripped, so a room mention cannot start an
///   unattended run. Only an explicit `"auto"` leaves them untouched.
/// * `allowed_tools: []` — strips the `tools` feature and any tool specs, so
///   the agent answers from its instructions and the conversation alone.
/// * `allowed_tools: [...]` (non-empty) — narrows `tools` to the specs whose
///   `name` is in the list; this is a filter, never a grant, so it can only
///   remove specs the caller already asked for, and if that leaves nothing
///   the `tools` feature is stripped too, exactly like the empty-list case.
///   Real authority still lives in the model plane's capability_scopes
///   (`CapabilityPolicy::evaluate`, checked per tool call regardless of
///   what this binding says) — this filter only trims what the model is
///   even offered, it is not the security boundary.
fn apply_mention_binding_policy(
    object: &mut Map<String, Value>,
    agent: &Value,
) -> Result<(), (StatusCode, Json<Value>)> {
    if let Some(trigger_modes) = agent.get("trigger_modes").and_then(Value::as_array) {
        let mentionable = trigger_modes
            .iter()
            .any(|mode| mode.as_str() == Some("mention"));
        if !mentionable {
            return Err((
                StatusCode::FORBIDDEN,
                Json(error(
                    "agent_mention_not_enabled",
                    "This agent's binding does not allow @-mention invocation.",
                )),
            ));
        }
    }

    let approval_mode = agent
        .get("approval_mode")
        .and_then(Value::as_str)
        .unwrap_or("require_confirmation");
    match approval_mode {
        "blocked" => {
            return Err((
                StatusCode::FORBIDDEN,
                Json(error(
                    "agent_invocation_blocked",
                    "This agent's binding blocks invocation in this Space.",
                )),
            ));
        }
        "auto" => {}
        _ => {
            object.insert("plan_mode".to_owned(), json!(false));
            if let Some(features) = object.get_mut("features").and_then(Value::as_array_mut) {
                features.retain(|feature| feature.as_str() != Some("agentic"));
            }
        }
    }

    if let Some(allowed_tools) = agent.get("allowed_tools").and_then(Value::as_array) {
        let remaining: Vec<Value> = if allowed_tools.is_empty() {
            Vec::new()
        } else {
            let allowed_names: std::collections::HashSet<&str> =
                allowed_tools.iter().filter_map(Value::as_str).collect();
            object
                .get("tools")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|spec| {
                    spec.get("name")
                        .and_then(Value::as_str)
                        .is_some_and(|name| allowed_names.contains(name))
                })
                .cloned()
                .collect()
        };
        if remaining.is_empty() {
            if let Some(features) = object.get_mut("features").and_then(Value::as_array_mut) {
                features.retain(|feature| feature.as_str() != Some("tools"));
            }
        }
        object.insert("tools".to_owned(), json!(remaining));
    }

    Ok(())
}

/// Control's actor-filtered Space index: which Spaces this caller belongs to,
/// and the role held in each. `None` means Control could not answer — never
/// "no Spaces", because the caller above must not show those the same way.
async fn control_space_index(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> Option<Vec<Value>> {
    if org_id.trim().is_empty() {
        return None;
    }
    let url = format!("{}/api/v1/internal/spaces", state.user_core_url);
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let (status, Json(response)) = proxy_json(
        state,
        Method::GET,
        &url,
        None,
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        return None;
    }
    crate::envelope::unwrap_data(&response)
        .get("spaces")
        .and_then(Value::as_array)
        .cloned()
}

/// Names and lifecycles for the organization's Spaces. Display only — access
/// was already decided by [`control_space_index`], so a failed read degrades
/// the labels and never the membership.
async fn organization_space_labels(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> Vec<Value> {
    let Ok(value) = convex_gateway_call(
        state,
        "query",
        "spaces:spacesForOrgForGateway",
        json!({ "externalAuthId": user.user_id, "externalOrgId": org_id }),
    )
    .await
    else {
        return Vec::new();
    };
    value.as_array().cloned().unwrap_or_default()
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
//
// `pub(crate)` so `domains::orgs::instructions` can reuse it for the org
// layer's authoring write, rather than duplicating this Convex-call plumbing.
pub(crate) async fn convex_gateway_call(
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
    let mut projected = json!({
        "space_ref": space.get("spaceRef").and_then(Value::as_str).unwrap_or_default(),
        "name": space.get("name").and_then(Value::as_str).unwrap_or("Personal Space"),
        "kind": space.get("kind").and_then(Value::as_str).unwrap_or("personal"),
        "lifecycle": space.get("lifecycle").and_then(Value::as_str).unwrap_or_default(),
    });
    // Emitted for every Space this projects, true or false. The argument is a
    // real Convex record, and Convex stores the flag as `optional(literal(true))`
    // — so an absent field there IS a definite "not the organization room",
    // and saying so lets a caller distinguish that from a response which never
    // carried the field at all (the Control-outage fallback listing, which does
    // not come through here). A caller gating an action must still test
    // `=== false` rather than falsiness, so that silence stays unknown.
    projected["is_organization_room"] =
        json!(space.get("isOrganizationRoom").and_then(Value::as_bool) == Some(true));
    projected
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
    let space = match space_lifecycle_by_ref(&state, &user, &org_id, space_ref).await {
        Ok(Some(space)) => space,
        Ok(None) => {
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
        forward_owner_grant_mutation, inject_authored_instructions,
        inject_mentioned_space_agent_persona, inject_personal_schedule_create_context,
        inject_personal_thread_context, personal_import_ingress_decision, space_actions,
        thread_items_match_space, OwnerGrantMutationRequest, UpdateSpaceInstructionsRequest,
    };
    use crate::config::AppState;
    use axum::{
        body::Body,
        extract::{Extension, Path, State},
        http::Request,
    };
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{
        body_partial_json as wm_body_partial_json, method as wm_method, path as wm_path,
    };
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
    async fn owner_grant_mutation_rejects_a_member_before_control_or_owner_lookup() {
        let state = crate::tests::test_state(false);
        let response = forward_owner_grant_mutation(
            &state,
            &authenticated_user(),
            "space-1",
            "conversation-1",
            "create",
            None,
            OwnerGrantMutationRequest {
                idempotency_key: "owner-grant-1".to_owned(),
            },
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
    }

    /// Drive the real gateway router through session validation, live Control
    /// membership, Control decision issuance, and Conversation Core forwarding.
    /// The signed Control bearer is an internal hop credential: it may appear
    /// in the gateway -> Conversation Core request body, but must never be
    /// returned in the browser response (or be accepted from the browser).
    #[tokio::test]
    async fn owner_grant_control_token_stays_internal_to_conversation_core() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;

        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {
                    "id": "user-1",
                    "email": "owner@example.com",
                    "emailVerified": true
                },
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;

        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1",
                "orgId": "org-1",
                "role": "owner",
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/owner-grant-decision"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "token": "control-secret-token",
                    "decision": {
                        "grant_id": "",
                        "org_id": "org-1",
                        "conversation_id": "conversation-1",
                        "space_ref": "space-1",
                        "subject_id": "user-1",
                        "service_audience": "application-plane-conversation-core",
                        "action_id": "tickets.create",
                        "operation": "create",
                        "idempotency_key": "grant-1"
                    }
                }
            })))
            .mount(&user_core)
            .await;

        let conversation = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path(
                "/api/v1/conversations/conversation-1/agent-action-grants",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "grant_id": "grant-1",
                    "status": "active",
                    "receipt_id": "receipt-1"
                }
            })))
            .mount(&conversation)
            .await;

        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.conversation_core_url = conversation.uri();

        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/spaces/space-1/conversations/conversation-1/agent-action-grants")
                    .header("cookie", "better-auth.session_token=session-1")
                    .header("content-type", "application/json")
                    // Unknown authority fields are deliberately supplied here
                    // to prove the browser cannot smuggle a usable bearer.
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "idempotency_key": "grant-1",
                            "control_decision_token": "browser-forged-token"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let browser_body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(
            browser_body,
            json!({
                "data": {
                    "grant_id": "grant-1",
                    "status": "active",
                    "receipt_id": "receipt-1"
                }
            })
        );
        assert!(!browser_body.to_string().contains("control-secret-token"));
        assert!(!browser_body.to_string().contains("browser-forged-token"));

        let conversation_requests = conversation.received_requests().await.unwrap();
        assert_eq!(conversation_requests.len(), 1);
        let forwarded = &conversation_requests[0];
        let forwarded_body: Value = serde_json::from_slice(&forwarded.body).unwrap();
        assert_eq!(
            forwarded_body["control_decision_token"],
            "control-secret-token"
        );
        assert_eq!(forwarded_body["idempotency_key"], "grant-1");
        assert!(forwarded.headers.get("x-delegation-signature").is_some());
        assert!(forwarded.headers.get("x-service-token").is_none());

        let control_requests = user_core.received_requests().await.unwrap();
        let control_request = control_requests
            .iter()
            .find(|request| request.url.path() == "/api/v1/internal/spaces/owner-grant-decision")
            .expect("Control decision request");
        let control_body: Value = serde_json::from_slice(&control_request.body).unwrap();
        assert_eq!(control_body["action_id"], "tickets.create");
        assert_eq!(control_body["operation"], "create");
        assert_eq!(control_body["idempotency_key"], "grant-1");
        assert!(!control_body.to_string().contains("browser-forged-token"));
    }

    #[tokio::test]
    async fn space_action_catalog_rejects_an_empty_space_before_any_owner_lookup() {
        // Serialized: this reads process-global service URLs that sibling
        // tests mutate with `set_var`, so without the lock it asserts against
        // whichever mock scheduling happened to leave installed.
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
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
        // Serialized: this reads process-global service URLs that sibling
        // tests mutate with `set_var`, so without the lock it asserts against
        // whichever mock scheduling happened to leave installed.
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
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
        // Serialized: this reads process-global service URLs that sibling
        // tests mutate with `set_var`, so without the lock it asserts against
        // whichever mock scheduling happened to leave installed.
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
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
        // Serialized with the other Space tests: the handler under test reads
        // process-global service URLs, and a sibling test mutates them with
        // `set_var`. Without the lock this asserts on a payload captured from
        // whichever mock happened to be installed, which passes or fails by
        // scheduling. It went unnoticed while the request count was low enough
        // that the two rarely overlapped.
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
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
            .and(wm_path("/api/v1/internal/spaces/retrieval-decision"))
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
            .find(|request| request.url.path() == "/api/v1/internal/spaces/retrieval-decision")
            .expect("retrieval decision issuance request");
        let retrieval_body: Value =
            serde_json::from_slice(&retrieval_request.body).expect("retrieval JSON");
        assert_eq!(retrieval_body["space_ref"], "personal-1");
        // The suffix is a process-wide monotonic counter, so it must not be
        // coupled to test ordering. The scoped prefix is the contract: this
        // retrieval authority is a fresh effect, distinct from thread create.
        assert!(retrieval_body["idempotency_key"]
            .as_str()
            .is_some_and(|key| key.starts_with("space-retrieval-")));
        assert!(retrieval_body.get("session_key").is_none());
    }

    #[tokio::test]
    async fn a_room_thread_gets_real_retrieval_authority_via_the_unified_endpoint() {
        // Serialized: reads process-global service URLs mutated by siblings.
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let user_core = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/thread-decision"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "token": "control-signed-token",
                    "space_kind": "room",
                    "decision": {
                        "space_ref": "room-1", "decision_ref": "decision-1",
                        "recipient_audience_ref": "space:room-1:recipient-audience:1", "privacy_policy_ref": "privacy-1",
                        "recipient_audience_revision": 1, "recipient_audience_hash": "sha256:audience-1", "authority_revision": 7, "resource_authorization_ref": "resource-1",
                        "action_schema_hash": "sha256:schema", "payload_digest": "sha256:payload",
                        "idempotency_key": "idem-1"
                    }
                }
            })))
            .mount(&user_core)
            .await;
        // Control now issues retrieval authority for rooms too, via the same
        // unified endpoint a personal turn uses — the room kind is resolved
        // server-side from the registered Space, not from anything this
        // request claims.
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/retrieval-decision"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"token": "control-room-retrieval-token"}
            })))
            .mount(&user_core)
            .await;
        let mut state = crate::tests::test_state(false);
        state.user_core_url = user_core.uri();
        let mut outbound = json!({
            "content": "hello", "space_ref": "room-1", "session_key": "session-1",
            "idempotency_key": "idem-1"
        });

        inject_personal_thread_context(&state, &authenticated_user(), "org-1", &mut outbound)
            .await
            .expect("room thread authority should be injected");

        assert_eq!(
            outbound["space_context"]["space_decision_token"],
            "control-signed-token"
        );
        assert_eq!(
            outbound["space_context"]["retrieval_decision_token"],
            "control-room-retrieval-token"
        );
        let received = user_core
            .received_requests()
            .await
            .expect("Control request");
        assert!(
            received.iter().all(|request| {
                request.url.path() != "/api/v1/internal/spaces/personal-retrieval-decision"
            }),
            "a room turn must use the unified retrieval-decision endpoint, never the personal-only one"
        );
    }

    #[tokio::test]
    async fn a_denied_retrieval_entitlement_degrades_to_ungrounded_chat_not_a_failed_turn() {
        // Serialized: reads process-global service URLs mutated by siblings.
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let user_core = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/thread-decision"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "token": "control-signed-token",
                    "space_kind": "room",
                    "decision": {
                        "space_ref": "room-1", "decision_ref": "decision-1",
                        "recipient_audience_ref": "space:room-1:recipient-audience:1", "privacy_policy_ref": "privacy-1",
                        "recipient_audience_revision": 1, "recipient_audience_hash": "sha256:audience-1", "authority_revision": 7, "resource_authorization_ref": "resource-1",
                        "action_schema_hash": "sha256:schema", "payload_digest": "sha256:payload",
                        "idempotency_key": "idem-1"
                    }
                }
            })))
            .mount(&user_core)
            .await;
        // retrieval_read_entitled is a separate, independently configured
        // policy flag from thread_create_entitled (defaults to FALSE for
        // every org — migration 019). A FORBIDDEN response here means
        // Control considered and denied retrieval specifically, which must
        // degrade this turn to ungrounded chat, not fail it outright: an org
        // with chat enabled but not yet opted into retrieval must still be
        // able to chat.
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/retrieval-decision"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "error": "current retrieval authority required"
            })))
            .mount(&user_core)
            .await;
        let mut state = crate::tests::test_state(false);
        state.user_core_url = user_core.uri();
        let mut outbound = json!({
            "content": "hello", "space_ref": "room-1", "session_key": "session-1",
            "idempotency_key": "idem-1"
        });

        inject_personal_thread_context(&state, &authenticated_user(), "org-1", &mut outbound)
            .await
            .expect("a denied retrieval entitlement must not fail the whole turn");
        assert_eq!(
            outbound["space_context"]["space_decision_token"],
            "control-signed-token"
        );
        assert_eq!(outbound["space_context"]["retrieval_decision_token"], "");
    }

    #[tokio::test]
    async fn a_retrieval_decision_outage_still_fails_the_whole_turn() {
        // Serialized: reads process-global service URLs mutated by siblings.
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let user_core = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/thread-decision"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "token": "control-signed-token",
                    "space_kind": "personal",
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
        // Unlike a FORBIDDEN denial, an outage must never be silently treated
        // as "retrieval just isn't entitled" — that would let a deployment
        // gap or Control incident quietly downgrade every scoped chat to
        // unscoped-equivalent (no grounding, no error) instead of surfacing
        // the failure.
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/retrieval-decision"))
            .respond_with(ResponseTemplate::new(503).set_body_json(json!({
                "error": "Space authority repository unavailable"
            })))
            .mount(&user_core)
            .await;
        let mut state = crate::tests::test_state(false);
        state.user_core_url = user_core.uri();
        let mut outbound = json!({
            "content": "hello", "space_ref": "personal-1", "session_key": "session-1",
            "idempotency_key": "idem-1"
        });

        let result =
            inject_personal_thread_context(&state, &authenticated_user(), "org-1", &mut outbound)
                .await;
        assert!(
            result.is_err(),
            "a retrieval-decision outage must fail the whole turn, not degrade silently"
        );
    }

    #[tokio::test]
    async fn existing_scoped_thread_gets_fresh_content_bound_append_authority() {
        // Serialized: this reads process-global service URLs that sibling
        // tests mutate with `set_var`, so without the lock it asserts against
        // whichever mock scheduling happened to leave installed.
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
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
        // Serialized: this reads process-global service URLs that sibling
        // tests mutate with `set_var`, so without the lock it asserts against
        // whichever mock scheduling happened to leave installed.
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
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

    /// A shared organization room must open like any other Space.
    ///
    /// `space_context` used to resolve the caller's PERSONAL Space and then
    /// require the requested ref to match it, so every organization room in the
    /// sidebar answered 404 and the UI said "your membership could not be
    /// confirmed" — blaming membership for a personal-only lookup. Live UI
    /// testing found this; no unit test covered opening a non-personal room.
    #[tokio::test]
    async fn an_organization_room_opens_instead_of_reporting_unconfirmed_membership() {
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
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "role": "editor"}
            })))
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        // The org's Spaces: the caller's personal room AND a shared room. The
        // requested ref is the shared one, which the old lookup could not reach.
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                {"spaceRef":"space-personal","name":"Personal Space","kind":"personal","lifecycle":"active"},
                {"spaceRef":"space-room","name":"AQUATIQ AS","kind":"room","lifecycle":"active","isOrganizationRoom":true}
            ]})))
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
                    .uri("/api/v1/spaces/space-room/context")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");

        assert_eq!(
            response.status(),
            axum::http::StatusCode::OK,
            "a shared organization room must open, not 404"
        );
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["data"]["space"]["space_ref"], "space-room");
        assert_eq!(body["data"]["space"]["name"], "AQUATIQ AS");
        assert_eq!(body["data"]["space"]["kind"], "room");
    }

    /// Drive `GET /spaces/space-room/activity` with a given Control answer to
    /// the shared-read decision, a run listing, an approvals answer, and a
    /// Conversation Core receipts answer.
    async fn space_activity_response(
        read_decision: ResponseTemplate,
        runs: ResponseTemplate,
        approvals: ResponseTemplate,
        receipts: ResponseTemplate,
    ) -> (u16, Value, MockServer) {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        for slug in ["session-core", "capability-core"] {
            Mock::given(wm_method("GET"))
                .and(wm_path(format!("/api/{slug}/token")))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"token": "plane-token"})),
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
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": "editor"}
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/thread-read-decision"))
            .respond_with(read_decision)
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                {"spaceRef":"space-room","name":"AQUATIQ AS","kind":"room","lifecycle":"active"}
            ]})))
            .mount(&application)
            .await;
        let model_gateway = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/runs"))
            .respond_with(runs)
            .mount(&model_gateway)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/runs/run-gated/approvals"))
            .respond_with(approvals)
            .mount(&model_gateway)
            .await;
        let conversation = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/spaces/space-room/activity"))
            .respond_with(receipts)
            .mount(&conversation)
            .await;
        std::env::set_var("APPLICATION_CONVEX_URL", application.uri());
        std::env::set_var("APPLICATION_CONVEX_SERVICE_KEY", "application-test-key");
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.model_gateway_url = model_gateway.uri();
        state.conversation_core_url = conversation.uri();
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/spaces/space-room/activity")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        let status = response.status().as_u16();
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, body, model_gateway)
    }

    fn activity_read_decision() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "data": {"decision": {"decision_ref": "read-1"}, "token": "v2.a.b.c"}
        }))
    }

    fn activity_receipts() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "operations": [{
                    "operation_id": "op-1", "action_id": "tickets.create", "status": "completed",
                    "subject_id": "agent-7", "granted_by_user_id": "user-1",
                    "ticket_id": "ticket-9", "audit_event_id": "audit-3",
                }],
                "authority": [{
                    "grant_id": "grant-1", "action_id": "tickets.create",
                    "subject_id": "agent-7", "created_by_user_id": "user-1",
                }],
            }
        }))
    }

    /// S2.3's slice 5 and S2.5's core: the owner effect performed under this
    /// room's authority appears in the room's own record.
    #[tokio::test]
    async fn space_activity_correlates_owner_receipts_with_the_rooms_runs() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, _mg) = space_activity_response(
            activity_read_decision(),
            ResponseTemplate::new(200).set_body_json(json!({
                "runs": [{
                    "run_id": "run-1", "goal": "Opprette sak", "status": "completed",
                    "input_tokens": 900, "output_tokens": 120, "steps_completed": 4,
                }]
            })),
            ResponseTemplate::new(200).set_body_json(json!({"approvals": []})),
            activity_receipts(),
        )
        .await;

        assert_eq!(status, 200);
        assert_eq!(body["data"]["runs"][0]["goal"], "Opprette sak");
        // The cost evidence a run already carried and Work never used.
        assert_eq!(body["data"]["runs"][0]["input_tokens"], 900);
        assert_eq!(body["data"]["operations"][0]["ticket_id"], "ticket-9");
        assert_eq!(body["data"]["authority"][0]["grant_id"], "grant-1");
    }

    /// Approvals are a per-run read upstream. Only runs actually waiting on a
    /// person are asked about, or opening a busy room would fan out across
    /// every run it has ever had.
    #[tokio::test]
    async fn space_activity_asks_for_approvals_only_on_gated_runs() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, model_gateway) = space_activity_response(
            activity_read_decision(),
            ResponseTemplate::new(200).set_body_json(json!({
                "runs": [
                    {"run_id": "run-done", "goal": "Ferdig", "status": "completed"},
                    {"run_id": "run-gated", "goal": "Sende varsel", "status": "awaiting_approval"},
                ]
            })),
            ResponseTemplate::new(200).set_body_json(json!({
                "approvals": [{"id": "ap-1", "status": "APPROVAL_STATE_REQUESTED", "kind": "tool"}]
            })),
            activity_receipts(),
        )
        .await;

        assert_eq!(status, 200);
        assert_eq!(body["data"]["approvals"][0]["id"], "ap-1");
        // Carried so the timeline can put the approval beside the work it gates.
        assert_eq!(body["data"]["approvals"][0]["run_id"], "run-gated");

        let requests = model_gateway.received_requests().await.expect("requests");
        let approval_paths: Vec<_> = requests
            .iter()
            .map(|request| request.url.path().to_owned())
            .filter(|path| path.ends_with("/approvals"))
            .collect();
        assert_eq!(approval_paths, ["/v1/runs/run-gated/approvals"]);
    }

    /// Each section fails alone. A reader must be able to tell "no owner
    /// effects" from "we could not ask", and the runs that did load are still
    /// worth showing.
    #[tokio::test]
    async fn space_activity_names_a_missing_section_and_keeps_the_rest() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, _mg) = space_activity_response(
            activity_read_decision(),
            ResponseTemplate::new(200).set_body_json(json!({
                "runs": [{"run_id": "run-1", "goal": "Kjører", "status": "running"}]
            })),
            ResponseTemplate::new(200).set_body_json(json!({"approvals": []})),
            ResponseTemplate::new(503).set_body_json(json!({"error": "down"})),
        )
        .await;

        assert_eq!(status, 200);
        assert_eq!(body["data"]["runs"][0]["goal"], "Kjører");
        let codes: Vec<&str> = body["data"]["unavailable"]
            .as_array()
            .expect("gap list")
            .iter()
            .filter_map(|gap| gap["code"].as_str())
            .collect();
        assert!(codes.contains(&"operations_upstream_unavailable"), "codes = {codes:?}");
        assert_eq!(body["data"]["operations"], json!([]));
    }

    /// A Conversation Core that predates the route is a different fact from an
    /// outage, and neither is "nothing has happened in this room".
    #[tokio::test]
    async fn space_activity_separates_a_missing_endpoint_from_an_outage() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, _mg) = space_activity_response(
            activity_read_decision(),
            ResponseTemplate::new(200).set_body_json(json!({"runs": []})),
            ResponseTemplate::new(200).set_body_json(json!({"approvals": []})),
            ResponseTemplate::new(404).set_body_json(json!({"error": "not found"})),
        )
        .await;
        assert_eq!(status, 200);
        let codes: Vec<&str> = body["data"]["unavailable"]
            .as_array()
            .expect("gap list")
            .iter()
            .filter_map(|gap| gap["code"].as_str())
            .collect();
        assert!(codes.contains(&"operations_endpoint_unavailable"), "codes = {codes:?}");
    }

    /// The tab's footnote has promised since the cockpit shipped that other
    /// owner-plane evidence joins when its projection lands. Name the classes
    /// that are still out rather than leaving the reader to wonder.
    #[tokio::test]
    async fn space_activity_always_names_the_evidence_classes_that_do_not_exist_yet() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, _mg) = space_activity_response(
            activity_read_decision(),
            ResponseTemplate::new(200).set_body_json(json!({"runs": []})),
            ResponseTemplate::new(200).set_body_json(json!({"approvals": []})),
            activity_receipts(),
        )
        .await;
        assert_eq!(status, 200);
        let codes: Vec<&str> = body["data"]["unavailable"]
            .as_array()
            .expect("gap list")
            .iter()
            .filter_map(|gap| gap["code"].as_str())
            .collect();
        assert!(codes.contains(&"delivery_ledger_not_built"), "codes = {codes:?}");
        assert!(codes.contains(&"watches_not_built"), "codes = {codes:?}");
    }

    /// Control declining the shared read is a real answer: the caller can take
    /// part in the room without seeing what other members ran.
    #[tokio::test]
    async fn space_activity_reports_an_unauthorized_run_read_as_its_own_gap() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, model_gateway) = space_activity_response(
            ResponseTemplate::new(403).set_body_json(json!({
                "error": {"code": "forbidden", "message": "not entitled"}
            })),
            ResponseTemplate::new(200).set_body_json(json!({"runs": []})),
            ResponseTemplate::new(200).set_body_json(json!({"approvals": []})),
            activity_receipts(),
        )
        .await;

        assert_eq!(status, 200);
        let codes: Vec<&str> = body["data"]["unavailable"]
            .as_array()
            .expect("gap list")
            .iter()
            .filter_map(|gap| gap["code"].as_str())
            .collect();
        assert!(codes.contains(&"runs_read_not_authorized"), "codes = {codes:?}");
        // The owner receipts do NOT depend on that decision, so they still load.
        assert_eq!(body["data"]["operations"][0]["operation_id"], "op-1");
        assert!(
            model_gateway.received_requests().await.expect("requests").is_empty(),
            "Model Plane must not be asked without a decision to present"
        );
    }

    /// Drive `GET /spaces/space-room/knowledge` with a given Control answer to
    /// the retrieval decision and a given Data Plane listing outcome.
    async fn space_knowledge_response(
        retrieval_decision: ResponseTemplate,
        listing: ResponseTemplate,
    ) -> (u16, Value, MockServer) {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        // The Data Plane leg travels under a session-minted `aud=data-plane`
        // bearer, so the token mint must be mocked or the call fails closed.
        for slug in ["data-plane", "session-core", "capability-core"] {
            Mock::given(wm_method("GET"))
                .and(wm_path(format!("/api/{slug}/token")))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"token": "plane-token"})),
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
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": "editor"}
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/retrieval-decision"))
            .respond_with(retrieval_decision)
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                {"spaceRef":"space-room","name":"AQUATIQ AS","kind":"room","lifecycle":"active"}
            ]})))
            .mount(&application)
            .await;
        let retrieval = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/knowledge/space-sources"))
            .respond_with(listing)
            .mount(&retrieval)
            .await;
        std::env::set_var("APPLICATION_CONVEX_URL", application.uri());
        std::env::set_var("APPLICATION_CONVEX_SERVICE_KEY", "application-test-key");
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.retrieval_engine_url = retrieval.uri();
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/spaces/space-room/knowledge")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        let status = response.status().as_u16();
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, body, retrieval)
    }

    /// The Knowledge tab rendered "Data Plane has not published a Space
    /// projection for this yet" from the day the cockpit shipped. This is it.
    #[tokio::test]
    async fn space_knowledge_lists_the_rooms_documents_and_wiki_pages() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, retrieval) = space_knowledge_response(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": {"token": "v2.a.b.c"}})),
            ResponseTemplate::new(200).set_body_json(json!({
                "space_ref": "space-room",
                "binding": {"workspace_id": "ws-1", "collection_id": null},
                "documents": [{"document_id": "doc-1", "title": "Rutine for mottak"}],
                "documents_truncated": false,
                "wiki_pages": [{"page_id": "page-1", "title": "Onboarding", "path": "/onboarding"}],
                "unavailable": [],
            })),
        )
        .await;

        assert_eq!(status, 200);
        assert_eq!(body["data"]["documents"][0]["title"], "Rutine for mottak");
        assert_eq!(body["data"]["wiki_pages"][0]["title"], "Onboarding");
        assert_eq!(body["data"]["binding"]["workspace_id"], "ws-1");
        assert!(
            body["data"]["unavailable"].as_array().is_some_and(|gaps| gaps.is_empty()),
            "nothing was missing, so the gap list must be present and empty"
        );

        // The Space decision must reach Data as its own header. Without it the
        // listing has no authority to resolve a binding from, and Data refuses.
        let requests = retrieval.received_requests().await.expect("requests");
        let listing = requests
            .iter()
            .find(|request| request.url.path() == "/v1/knowledge/space-sources")
            .expect("listing request");
        assert_eq!(
            listing
                .headers
                .get("x-space-decision")
                .and_then(|value| value.to_str().ok()),
            Some("v2.a.b.c"),
        );
    }

    /// Retrieval is entitled separately from chat, so most orgs will answer
    /// exactly this way. A working room that cannot list its archive must say
    /// so — a failed request would make an unset entitlement look broken.
    #[tokio::test]
    async fn space_knowledge_names_an_unauthorized_read_instead_of_failing() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, retrieval) = space_knowledge_response(
            ResponseTemplate::new(403).set_body_json(json!({
                "error": {"code": "forbidden", "message": "retrieval is not entitled"}
            })),
            ResponseTemplate::new(200).set_body_json(json!({"documents": []})),
        )
        .await;

        assert_eq!(status, 200);
        assert_eq!(
            body["data"]["unavailable"][0]["code"],
            "knowledge_read_not_authorized"
        );
        assert!(
            body["data"]["documents"].as_array().is_some_and(|d| d.is_empty()),
            "an unauthorized read must still answer with an empty list, not null"
        );
        assert!(
            retrieval.received_requests().await.expect("requests").is_empty(),
            "Data must not be asked without a decision to present"
        );
    }

    /// Control issued the authority and Data refused it: the Space has no
    /// active binding to a Data target. That is a nameable state, not an
    /// outage, and it is the state the dev organization is actually in.
    #[tokio::test]
    async fn space_knowledge_separates_no_binding_from_an_outage() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, _retrieval) = space_knowledge_response(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": {"token": "v2.a.b.c"}})),
            ResponseTemplate::new(403).set_body_json(json!({
                "error": "Space retrieval binding is unavailable"
            })),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(
            body["data"]["unavailable"][0]["code"],
            "knowledge_binding_unavailable"
        );

        let (status, body, _retrieval) = space_knowledge_response(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": {"token": "v2.a.b.c"}})),
            ResponseTemplate::new(503).set_body_json(json!({"error": "down"})),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(
            body["data"]["unavailable"][0]["code"],
            "knowledge_upstream_unavailable"
        );
    }

    /// Data knows which half of its own answer is missing. Relay that rather
    /// than restating it: a collection-only binding has no wiki workspace, and
    /// an empty page list would read as "this room has no pages".
    #[tokio::test]
    async fn space_knowledge_relays_the_per_section_gap_data_reported() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, _retrieval) = space_knowledge_response(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": {"token": "v2.a.b.c"}})),
            ResponseTemplate::new(200).set_body_json(json!({
                "documents": [{"document_id": "doc-1", "title": "Rutine"}],
                // Go/Rust both marshal an absent list this way somewhere in
                // this chain; the browser must still receive an array.
                "wiki_pages": null,
                "unavailable": [{
                    "section": "wiki_pages",
                    "code": "space_binding_has_no_wiki_workspace",
                    "reason": "This Space's Data binding names no wiki workspace.",
                }],
            })),
        )
        .await;

        assert_eq!(status, 200);
        assert_eq!(body["data"]["documents"][0]["title"], "Rutine");
        assert_eq!(
            body["data"]["wiki_pages"],
            json!([]),
            "a null list must reach the browser as an empty array"
        );
        assert_eq!(
            body["data"]["unavailable"][0]["code"],
            "space_binding_has_no_wiki_workspace"
        );
    }

    /// A Control outage is not an answer. Degrading it to "not authorized"
    /// would tell the reader their room has no archive when the truth is that
    /// we could not ask.
    #[tokio::test]
    async fn space_knowledge_fails_the_call_on_a_control_outage() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, _body, retrieval) = space_knowledge_response(
            ResponseTemplate::new(503).set_body_json(json!({"error": "control down"})),
            ResponseTemplate::new(200).set_body_json(json!({"documents": []})),
        )
        .await;
        assert_eq!(status, 503);
        assert!(
            retrieval.received_requests().await.expect("requests").is_empty(),
            "Data must not be asked when Control could not be reached"
        );
    }

    /// Drive `GET /spaces/space-room/work` with a given Control answer to the
    /// shared-read decision and given upstream outcomes for runs and schedules.
    async fn space_work_response(
        read_decision: ResponseTemplate,
        runs: ResponseTemplate,
        schedules: ResponseTemplate,
    ) -> (u16, Value, MockServer) {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        for slug in ["session-core", "capability-core"] {
            Mock::given(wm_method("GET"))
                .and(wm_path(format!("/api/{slug}/token")))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"token": "plane-token"})),
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
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": "editor"}
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/thread-read-decision"))
            .respond_with(read_decision)
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                {"spaceRef":"space-room","name":"AQUATIQ AS","kind":"room","lifecycle":"active"}
            ]})))
            .mount(&application)
            .await;
        let model_gateway = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/runs"))
            .respond_with(runs)
            .mount(&model_gateway)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/cron"))
            .respond_with(schedules)
            .mount(&model_gateway)
            .await;
        std::env::set_var("APPLICATION_CONVEX_URL", application.uri());
        std::env::set_var("APPLICATION_CONVEX_SERVICE_KEY", "application-test-key");
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.model_gateway_url = model_gateway.uri();
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/spaces/space-room/work")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        let status = response.status().as_u16();
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, body, model_gateway)
    }

    /// The Work tab rendered an honest "not published yet" from the day the
    /// cockpit shipped. This is the projection it was waiting for.
    #[tokio::test]
    async fn space_work_composes_runs_and_schedules_for_the_room() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, model_gateway) = space_work_response(
            ResponseTemplate::new(200).set_body_json(json!({
                "data": {"decision": {"decision_ref": "read-1"}, "token": "v2.a.b.c"}
            })),
            ResponseTemplate::new(200).set_body_json(json!({
                "runs": [{"id": "r1", "goal": "Send varsel", "status": "awaiting_approval"}]
            })),
            ResponseTemplate::new(200).set_body_json(json!({
                "schedules": [{"id": "c1", "name": "Daglig rapport", "enabled": true}]
            })),
        )
        .await;

        assert_eq!(status, 200);
        assert_eq!(body["data"]["runs"][0]["goal"], "Send varsel");
        assert_eq!(body["data"]["schedules"][0]["name"], "Daglig rapport");
        assert!(
            body["data"]["unavailable"].as_array().is_some_and(|gaps| gaps.is_empty()),
            "nothing was missing, so the gap list must be present and empty"
        );

        // Runs must travel under the room's read decision, and schedules must
        // be narrowed to the room — otherwise Work shows the whole org.
        let requests = model_gateway.received_requests().await.expect("requests");
        let runs = requests
            .iter()
            .find(|request| request.url.path() == "/v1/runs")
            .expect("runs request");
        assert!(runs.url.query().unwrap_or_default().contains("space_read_decision_token=v2.a.b.c"));
        let cron = requests
            .iter()
            .find(|request| request.url.path() == "/v1/cron")
            .expect("cron request");
        assert!(cron.url.query().unwrap_or_default().contains("space_ref=space-room"));
    }

    /// Either upstream can fail alone. Showing what resolved with a named gap
    /// beats hiding the runs that loaded or claiming the room is idle.
    #[tokio::test]
    async fn space_work_reports_a_partial_answer_rather_than_hiding_it() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, _mg) = space_work_response(
            ResponseTemplate::new(200).set_body_json(json!({
                "data": {"decision": {"decision_ref": "read-1"}, "token": "v2.a.b.c"}
            })),
            ResponseTemplate::new(200).set_body_json(json!({
                "runs": [{"id": "r1", "goal": "Kjører", "status": "running"}]
            })),
            ResponseTemplate::new(503).set_body_json(json!({"error": "down"})),
        )
        .await;

        assert_eq!(status, 200, "a partial answer is still an answer");
        assert_eq!(body["data"]["runs"][0]["goal"], "Kjører");
        let gaps = body["data"]["unavailable"].as_array().expect("gaps");
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0]["section"], "schedules");
    }

    /// Control declining the shared read is a real answer: the caller can take
    /// part in the room without seeing other members' runs. It must be named,
    /// not rendered as an empty list.
    #[tokio::test]
    async fn space_work_names_an_unauthorized_shared_read_instead_of_showing_nothing() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, model_gateway) = space_work_response(
            ResponseTemplate::new(403).set_body_json(json!({"error": "not entitled"})),
            ResponseTemplate::new(200).set_body_json(json!({"runs": []})),
            ResponseTemplate::new(200).set_body_json(json!({"schedules": []})),
        )
        .await;

        assert_eq!(status, 200);
        let gaps = body["data"]["unavailable"].as_array().expect("gaps");
        assert!(gaps.iter().any(|gap| gap["section"] == "runs"));
        // And no run request was made at all — there was no authority to make it
        // under.
        let requests = model_gateway.received_requests().await.expect("requests");
        assert!(
            requests.iter().all(|request| request.url.path() != "/v1/runs"),
            "an unauthorized read must not reach Model Plane"
        );
    }

    /// Drive a room-membership request with a given Control-resolved room
    /// role and a given Convex outcome for the sync action.
    async fn room_membership_response(
        role: &str,
        method: &str,
        body: Option<Value>,
        sync: Value,
    ) -> (u16, Value, MockServer) {
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
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": role}
            })))
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": {"added": true, "removed": true, "memberCount": 2}
            })))
            .mount(&application)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/action"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": sync})))
            .mount(&application)
            .await;
        std::env::set_var("APPLICATION_CONVEX_URL", application.uri());
        std::env::set_var("APPLICATION_CONVEX_SERVICE_KEY", "application-test-key");
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        let uri = if method == "DELETE" {
            "/api/v1/spaces/space-room/members/user-2"
        } else {
            "/api/v1/spaces/space-room/members"
        };
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", "better-auth.session_token=session-1");
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        let response = crate::build_router(state)
            .oneshot(
                request
                    .body(match &body {
                        Some(value) => Body::from(value.to_string()),
                        None => Body::empty(),
                    })
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        let status = response.status().as_u16();
        let payload: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, payload, application)
    }

    /// Adding a person is only real once Control has the roster, so the route
    /// declares it and reports the result rather than trusting the write.
    #[tokio::test]
    async fn an_owner_adds_a_person_and_control_gets_the_roster() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, application) = room_membership_response(
            "owner",
            "POST",
            Some(json!({"member_id": "user-2"})),
            json!({"status": "applied"}),
        )
        .await;

        assert_eq!(status, 200);
        assert_eq!(body["data"]["changed"], true);
        let requests = application.received_requests().await.expect("requests");
        assert!(
            requests.iter().any(|request| request.url.path() == "/api/action"),
            "a membership change must be declared to Control"
        );
    }

    /// Application's list ahead of Control's roster is the safe direction, but
    /// the caller must be told: the person is not a member yet.
    #[tokio::test]
    async fn an_unconfirmed_roster_is_reported_rather_than_claimed_as_success() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, _application) = room_membership_response(
            "owner",
            "POST",
            Some(json!({"member_id": "user-2"})),
            json!({"status": "control_unavailable"}),
        )
        .await;

        assert_eq!(status, 502);
        assert_eq!(body["error"]["code"], "room_membership_unconfirmed");
    }

    /// Deciding who may read a room's shared record takes the same roles as
    /// granting an agent access to it.
    #[tokio::test]
    async fn a_viewer_cannot_add_or_remove_room_members() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (method, body) in [
            ("POST", Some(json!({"member_id": "user-2"}))),
            ("DELETE", None),
        ] {
            let (status, payload, application) =
                room_membership_response("viewer", method, body, json!({"status": "applied"})).await;
            assert_eq!(status, 403, "{method} must be refused for a viewer");
            assert_eq!(payload["error"]["code"], "space_role_cannot_create_agent");
            assert!(
                application
                    .received_requests()
                    .await
                    .expect("requests")
                    .is_empty(),
                "a refused {method} must never reach Application"
            );
        }
    }

    /// Drive `POST /api/v1/spaces` with a given body and return what came back
    /// plus the Application mock, so the chosen Convex mutation can be checked.
    async fn create_space_response(body: Value) -> (u16, Value, MockServer) {
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
            .and(wm_path("/api/mutation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": {"spaceRef": "space-new", "name": "Leveranse", "kind": "room", "lifecycle": "pending_registration"}
            })))
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
                    .uri("/api/v1/spaces")
                    .header("cookie", "better-auth.session_token=session-1")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        let status = response.status().as_u16();
        let payload: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, payload, application)
    }

    /// A named room is the general case; a personal Space stays the default so
    /// the route's existing callers are unaffected.
    #[tokio::test]
    async fn creating_a_room_uses_the_room_mutation_and_a_personal_space_stays_the_default() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, application) =
            create_space_response(json!({"kind": "room", "name": "Leveranse"})).await;
        assert_eq!(status, 202, "a room is created but not yet registered");
        assert_eq!(body["data"]["space"]["kind"], "room");
        let requests = application.received_requests().await.expect("requests");
        let sent = String::from_utf8_lossy(&requests[0].body).to_string();
        assert!(
            sent.contains("spaces:createRoomForGateway"),
            "a room must not be created through the personal-Space mutation: {sent}"
        );

        let (status, _body, application) = create_space_response(json!({})).await;
        assert_eq!(status, 202);
        let requests = application.received_requests().await.expect("requests");
        let sent = String::from_utf8_lossy(&requests[0].body).to_string();
        assert!(
            sent.contains("spaces:ensurePersonalSpaceForGateway"),
            "an omitted kind must still mean a personal Space: {sent}"
        );
    }

    /// A room with no name would appear in the sidebar as a blank row, and the
    /// other Space kinds have owners and lifecycles nothing here creates.
    #[tokio::test]
    async fn an_unnamed_room_or_unsupported_kind_is_refused_before_any_write() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (body, code) in [
            (json!({"kind": "room"}), "room_name_required"),
            (json!({"kind": "room", "name": "   "}), "room_name_required"),
            (json!({"kind": "project", "name": "P"}), "unsupported_space_kind"),
            (json!({"kind": "case", "name": "C"}), "unsupported_space_kind"),
        ] {
            let (status, payload, application) = create_space_response(body.clone()).await;
            assert_eq!(status, 400, "{body} must be refused");
            assert_eq!(payload["error"]["code"], code);
            assert!(
                application
                    .received_requests()
                    .await
                    .expect("requests")
                    .is_empty(),
                "a refused create must never reach Application"
            );
        }
    }

    /// Drive a binding-lifecycle request (PATCH or DELETE) with a given
    /// Control-resolved room role. Returns (status, body) plus the Application
    /// mock so the forwarded Convex call can be asserted.
    async fn space_agent_lifecycle_response(
        role: &str,
        method: &str,
        body: Option<Value>,
    ) -> (u16, Value, MockServer) {
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
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": role}
            })))
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .and(wm_body_partial_json(
                json!({"path": "spaceAgents:setSpaceAgentBindingStateForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": {"bindingRef": "sab_1", "subjectId": "agent-a1", "status": "paused", "changed": true}
            })))
            .mount(&application)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/action"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": {"status": "applied"}
            })))
            .mount(&application)
            .await;
        std::env::set_var("APPLICATION_CONVEX_URL", application.uri());
        std::env::set_var("APPLICATION_CONVEX_SERVICE_KEY", "application-test-key");
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        let mut request = Request::builder()
            .method(method)
            .uri("/api/v1/spaces/space-room/agents/sab_1")
            .header("cookie", "better-auth.session_token=session-1");
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        let response = crate::build_router(state)
            .oneshot(
                request
                    .body(match &body {
                        Some(value) => Body::from(value.to_string()),
                        None => Body::empty(),
                    })
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        let status = response.status().as_u16();
        let payload: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, payload, application)
    }

    /// The room could add an agent and never take one back. Pausing is the
    /// reversible half of closing that gap.
    #[tokio::test]
    async fn an_owner_can_pause_a_bound_agent_in_the_room() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, application) =
            space_agent_lifecycle_response("owner", "PATCH", Some(json!({"status": "paused"})))
                .await;

        assert_eq!(status, 200);
        assert_eq!(body["data"]["status"], "paused");
        let requests = application.received_requests().await.expect("requests");
        // Pausing keeps the agent a member that may not be invoked, so Control
        // has nothing to converge — the roster is unchanged.
        assert!(
            requests.iter().all(|request| request.url.path() != "/api/action"),
            "pausing must not re-declare the room's roster to Control"
        );
    }

    /// Governing a binding is the same class of decision as granting one, so
    /// it takes the same roles — and the refusal must happen before any write.
    #[tokio::test]
    async fn a_member_cannot_pause_or_revoke_an_agent() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (method, body) in [
            ("PATCH", Some(json!({"status": "paused"}))),
            ("DELETE", None),
        ] {
            let (status, payload, application) =
                space_agent_lifecycle_response("viewer", method, body).await;
            assert_eq!(status, 403, "{method} must be refused for a viewer");
            assert_eq!(payload["error"]["code"], "space_role_cannot_create_agent");
            assert!(
                application
                    .received_requests()
                    .await
                    .expect("requests")
                    .is_empty(),
                "a refused {method} must never reach Application"
            );
        }
    }

    /// Revocation is the one irreversible option, so it has its own verb. A
    /// status of "revoked" sent to PATCH is refused rather than honoured, and
    /// so is any status the binding contract does not define.
    #[tokio::test]
    async fn patch_refuses_revocation_and_unknown_states() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for status_value in ["revoked", "deleted", ""] {
            let (status, payload, application) = space_agent_lifecycle_response(
                "owner",
                "PATCH",
                Some(json!({"status": status_value})),
            )
            .await;
            assert_eq!(status, 400, "PATCH must refuse status {status_value:?}");
            assert_eq!(payload["error"]["code"], "invalid_agent_binding_status");
            assert!(
                application
                    .received_requests()
                    .await
                    .expect("requests")
                    .is_empty(),
                "a refused status must never reach Application"
            );
        }
    }

    /// Revoking changes who is in the room, so Control must be told. Without
    /// the roster convergence the binding would say "revoked" while Control
    /// still listed the agent as a member.
    #[tokio::test]
    async fn revoking_an_agent_reconverges_the_control_roster() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, _body, application) =
            space_agent_lifecycle_response("manager", "DELETE", None).await;

        assert_eq!(status, 200);
        let requests = application.received_requests().await.expect("requests");
        assert!(
            requests.iter().any(|request| request.url.path() == "/api/action"),
            "revocation must re-declare the room's roster to Control"
        );
    }

    /// Drive `GET /spaces/space-room/threads/{id}/transcript` with a given
    /// Control answer to the shared-read decision. Returns the response plus
    /// the Model Gateway mock so the forwarded query can be asserted.
    async fn space_thread_transcript_response(
        read_decision: ResponseTemplate,
    ) -> (u16, Value, MockServer) {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        // The room proxies to Model Plane under the caller delegation, so the
        // session-core audience token has to mint or the route 503s before it
        // ever asks Control anything.
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/session-core/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"token": "session-core-token"})))
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
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/thread-read-decision"))
            .respond_with(read_decision)
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                {"spaceRef":"space-room","name":"AQUATIQ AS","kind":"room","lifecycle":"active","isOrganizationRoom":true}
            ]})))
            .mount(&application)
            .await;
        let model_gateway = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/threads/thread-1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "thread_id": "thread-1",
                "messages": [
                    {"role":"user","content":"Når kommer leveransen?","author_subject_id":"user-2"},
                    {"role":"assistant","content":"I morgen.","agent_name":"Driftsassistent"}
                ]
            })))
            .mount(&model_gateway)
            .await;
        std::env::set_var("APPLICATION_CONVEX_URL", application.uri());
        std::env::set_var("APPLICATION_CONVEX_SERVICE_KEY", "application-test-key");
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.model_gateway_url = model_gateway.uri();
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/spaces/space-room/threads/thread-1/transcript")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        let status = response.status().as_u16();
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, body, model_gateway)
    }

    /// The room reads another member's turns, and the author travels with them.
    ///
    /// Before this route existed the room could only read threads in the
    /// caller's OWN durable list, so a colleague's post rendered as its preview
    /// and nothing else.
    #[tokio::test]
    async fn a_space_transcript_returns_another_members_turns_with_their_author() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, model_gateway) = space_thread_transcript_response(
            ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "decision": {"decision_ref": "read-decision-1"},
                    "token": "v2.a.b.c"
                }
            })),
        )
        .await;

        assert_eq!(status, 200, "a current member must be able to read the room");
        let turns = body["data"]["transcript"]["turns"]
            .as_array()
            .expect("turns");
        assert_eq!(turns.len(), 2);
        assert_eq!(
            turns[0]["author_subject_id"], "user-2",
            "another member's turn must carry that member's subject, not the reader's"
        );

        // The signed decision must actually reach Model Plane; without it
        // session-core stays owner-bound and the read silently narrows.
        let request = &model_gateway.received_requests().await.expect("requests")[0];
        let query = request.url.query().unwrap_or_default();
        assert!(
            query.contains("space_read_decision_token=v2.a.b.c"),
            "the Control read decision must be forwarded, got {query}"
        );
        assert!(query.contains("space_id=space-room"));
    }

    /// Control declining is a real answer, and the room must say so rather than
    /// quietly falling back to the owner-bound read — which would show the
    /// caller only their own turns while looking like the whole conversation.
    #[tokio::test]
    async fn a_declined_space_read_refuses_instead_of_narrowing_to_the_caller() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, model_gateway) = space_thread_transcript_response(
            ResponseTemplate::new(403).set_body_json(json!({"error": "not entitled"})),
        )
        .await;

        assert_eq!(status, 403, "an unauthorized shared read must refuse");
        assert_eq!(body["error"]["code"], "space_read_not_authorized");
        assert!(
            model_gateway
                .received_requests()
                .await
                .expect("requests")
                .is_empty(),
            "a declined read must never reach Model Plane"
        );
    }

    /// Drive `POST /spaces/space-room/agents` with a given Control-resolved
    /// room role and given Convex step outcomes; returns (status, body) plus
    /// the Application mock for request-shape assertions.
    async fn create_space_agent_response(
        role: &str,
        confirmation: Value,
        body: Value,
    ) -> (u16, Value, MockServer) {
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
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": role}
            })))
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .and(wm_body_partial_json(json!({"path": "spaceAgents:createSpaceAgentForGateway"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": {"agentRef": "agent123", "subjectId": "agent-agent123", "bindingRef": "sab_space-room_agent-agent123"}
            })))
            .mount(&application)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/action"))
            .and(wm_body_partial_json(
                json!({"path": "spaceAgents:confirmSpaceAgentMembershipForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": confirmation})))
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
                    .uri("/api/v1/spaces/space-room/agents")
                    .header("cookie", "better-auth.session_token=session-1")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        let status = response.status().as_u16();
        let parsed: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, parsed, application)
    }

    #[tokio::test]
    async fn an_owner_creates_a_room_agent_through_the_governed_two_step_flow() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, application) = create_space_agent_response(
            "owner",
            json!({"status": "applied"}),
            json!({"name": "Møtereferent", "instructions": "Skriv referat.", "avatar_color": "#2563eb"}),
        )
        .await;

        assert_eq!(
            status, 201,
            "owner-created agent should be confirmed: {body}"
        );
        assert_eq!(body["data"]["subject_id"], "agent-agent123");
        assert_eq!(body["data"]["status"], "active");

        let received = application.received_requests().await.expect("convex calls");
        let mutation = received
            .iter()
            .find(|request| request.url.path() == "/api/mutation")
            .expect("definition + pending binding request");
        let mutation_body: Value = serde_json::from_slice(&mutation.body).expect("mutation JSON");
        // Identity comes from the verified session and resolved org — the
        // browser body cannot supply it.
        assert_eq!(mutation_body["args"]["externalAuthId"], "user-1");
        assert_eq!(mutation_body["args"]["externalOrgId"], "org-1");
        assert_eq!(mutation_body["args"]["spaceRef"], "space-room");
        assert_eq!(mutation_body["args"]["name"], "Møtereferent");
        assert_eq!(mutation_body["args"]["instructions"], "Skriv referat.");
        assert_eq!(mutation_body["args"]["avatarColor"], "#2563eb");
        assert!(
            received
                .iter()
                .any(|request| request.url.path() == "/api/action"),
            "Control roster confirmation must run"
        );
    }

    #[tokio::test]
    async fn an_editor_cannot_create_a_room_agent() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, application) = create_space_agent_response(
            "editor",
            json!({"status": "applied"}),
            json!({"name": "Møtereferent"}),
        )
        .await;

        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "space_role_cannot_create_agent");
        let received = application.received_requests().await.expect("convex calls");
        assert!(
            received.is_empty(),
            "a denied role must reach no Application write at all"
        );
    }

    #[tokio::test]
    async fn an_unconfirmed_control_roster_leaves_the_agent_pending() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, _application) = create_space_agent_response(
            "owner",
            json!({"status": "rejected", "httpStatus": 403}),
            json!({"name": "Møtereferent"}),
        )
        .await;

        assert_eq!(status, 502);
        assert_eq!(body["error"]["code"], "agent_membership_unconfirmed");
    }

    /// Drive `GET /spaces/space-room/agents/available` with a given
    /// Control-resolved room role and a given Application definitions list.
    async fn list_installable_space_agents_response(
        role: &str,
        definitions: Value,
    ) -> (u16, Value, MockServer) {
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
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": role}
            })))
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(wm_body_partial_json(
                json!({"path": "spaceAgents:listInstallableSpaceAgentsForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": definitions})))
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
                    .uri("/api/v1/spaces/space-room/agents/available")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        let status = response.status().as_u16();
        let parsed: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, parsed, application)
    }

    #[tokio::test]
    async fn an_owner_browses_installable_agents_with_bound_state_reported_truthfully() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, _application) = list_installable_space_agents_response(
            "owner",
            json!([
                {"agentRef": "agent-1", "name": "Driftsassistent", "description": "", "definitionStatus": "active", "alreadyBound": true},
                {"agentRef": "agent-2", "name": "Møtereferent", "description": "Skriver referat", "definitionStatus": "active", "alreadyBound": false}
            ]),
        )
        .await;

        assert_eq!(status, 200);
        let agents = body["data"]["agents"].as_array().expect("agents array");
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0]["agent_ref"], "agent-1");
        assert_eq!(agents[0]["already_bound"], true);
        assert_eq!(agents[1]["agent_ref"], "agent-2");
        assert_eq!(
            agents[1]["already_bound"], false,
            "an unbound definition must be offered, not hidden"
        );
    }

    #[tokio::test]
    async fn an_editor_cannot_browse_installable_agents() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, application) =
            list_installable_space_agents_response("editor", json!([])).await;

        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "space_role_cannot_create_agent");
        let received = application.received_requests().await.expect("convex calls");
        assert!(
            received.is_empty(),
            "a denied role must reach no Application read at all"
        );
    }

    /// Drive `POST /spaces/space-room/agents/bind` with a given
    /// Control-resolved room role and given Convex step outcomes.
    async fn bind_existing_space_agent_response(
        role: &str,
        confirmation: Value,
        body: Value,
    ) -> (u16, Value, MockServer) {
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
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": role}
            })))
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .and(wm_body_partial_json(
                json!({"path": "spaceAgents:bindExistingSpaceAgentForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": {"subjectId": "agent-agent2", "bindingRef": "sab_space-room_agent-agent2"}
            })))
            .mount(&application)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/action"))
            .and(wm_body_partial_json(
                json!({"path": "spaceAgents:confirmSpaceAgentMembershipForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": confirmation})))
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
                    .uri("/api/v1/spaces/space-room/agents/bind")
                    .header("cookie", "better-auth.session_token=session-1")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        let status = response.status().as_u16();
        let parsed: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, parsed, application)
    }

    #[tokio::test]
    async fn an_owner_binds_an_existing_agent_through_the_governed_two_step_flow() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, application) = bind_existing_space_agent_response(
            "owner",
            json!({"status": "applied"}),
            json!({"agent_ref": "agent-2"}),
        )
        .await;

        assert_eq!(status, 201, "owner-bound agent should be confirmed: {body}");
        assert_eq!(body["data"]["subject_id"], "agent-agent2");
        assert_eq!(body["data"]["status"], "active");

        let received = application.received_requests().await.expect("convex calls");
        let mutation = received
            .iter()
            .find(|request| request.url.path() == "/api/mutation")
            .expect("bind-existing-definition request");
        let mutation_body: Value = serde_json::from_slice(&mutation.body).expect("mutation JSON");
        assert_eq!(mutation_body["args"]["externalAuthId"], "user-1");
        assert_eq!(mutation_body["args"]["externalOrgId"], "org-1");
        assert_eq!(mutation_body["args"]["spaceRef"], "space-room");
        assert_eq!(mutation_body["args"]["agentId"], "agent-2");
        assert!(
            received
                .iter()
                .any(|request| request.url.path() == "/api/action"),
            "Control roster confirmation must run"
        );
    }

    #[tokio::test]
    async fn an_editor_cannot_bind_an_existing_agent() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, application) = bind_existing_space_agent_response(
            "editor",
            json!({"status": "applied"}),
            json!({"agent_ref": "agent-2"}),
        )
        .await;

        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "space_role_cannot_create_agent");
        let received = application.received_requests().await.expect("convex calls");
        assert!(
            received.is_empty(),
            "a denied role must reach no Application write at all"
        );
    }

    #[tokio::test]
    async fn binding_without_an_agent_ref_is_rejected() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, application) =
            bind_existing_space_agent_response("owner", json!({"status": "applied"}), json!({}))
                .await;

        assert_eq!(status, 400);
        assert_eq!(body["error"]["code"], "agent_ref_required");
        let received = application.received_requests().await.expect("convex calls");
        assert!(
            received.is_empty(),
            "a malformed request must reach no Application call"
        );
    }

    #[tokio::test]
    async fn an_unconfirmed_control_roster_leaves_the_bound_agent_pending() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, _application) = bind_existing_space_agent_response(
            "owner",
            json!({"status": "rejected", "httpStatus": 403}),
            json!({"agent_ref": "agent-2"}),
        )
        .await;

        assert_eq!(status, 502);
        assert_eq!(body["error"]["code"], "agent_membership_unconfirmed");
    }

    /// Drive `GET /spaces/space-1/agents` with a given Control roster and a
    /// given Application binding projection, and return the parsed response.
    async fn space_agents_response(roster: Value, bindings: Value) -> (u16, Value) {
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
        // Control's real envelope, verified against user-core's `spaceRoster`
        // handler: `{data: {members, count}}`. Mocking a bare array here would
        // encode the caller's assumption instead of the contract, and the join
        // would pass its tests while returning nothing in production.
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-1/roster"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"members": roster, "count": roster.as_array().map(Vec::len).unwrap_or(0)}
            })))
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": bindings})))
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
                    .uri("/api/v1/spaces/space-1/agents")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        let status = response.status().as_u16();
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        (status, body)
    }

    /// Drive `GET /agents/installations` with a given
    /// `spaceAgents:agentInstallationsForOrgForGateway` registry response
    /// (ADR-0002). Unlike the narrow slice this replaced, there is no Control
    /// roster to mock at all — the registry is a single Application read.
    async fn list_org_agent_installations_response(registry_rows: Value) -> (u16, Value) {
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
            .and(wm_body_partial_json(json!({
                "path": "spaceAgents:agentInstallationsForOrgForGateway",
                "args": {"externalAuthId": "user-1", "externalOrgId": "org-1"},
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": registry_rows})))
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
                    .uri("/api/v1/agents/installations")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        let status = response.status().as_u16();
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        (status, body)
    }

    fn registry_row(space_ref: &str, space_name: &str, status: &str) -> Value {
        json!({
            "bindingRef": format!("sab_{space_ref}_svc-shared"),
            "agentRef": "agent-1",
            "subjectId": "svc-shared",
            "spaceRef": space_ref,
            "spaceName": space_name,
            "spaceKind": "room",
            "spaceLifecycle": "active",
            "name": "Shared Agent",
            "status": status,
            "definitionStatus": "active",
            "deliveryTargets": [],
        })
    }

    #[tokio::test]
    async fn an_agent_bound_in_two_rooms_reports_one_definition_with_two_installations() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = list_org_agent_installations_response(json!([
            registry_row("space-a", "Team Room", "active"),
            registry_row("space-b", "Ops Room", "pending"),
        ]))
        .await;

        assert_eq!(status, 200, "{body}");
        let definitions = body["data"]["definitions"]
            .as_array()
            .expect("definitions array");
        assert_eq!(
            definitions.len(),
            1,
            "the same definition must group, not duplicate: {body}"
        );
        let definition = &definitions[0];
        assert_eq!(definition["agent_ref"], "agent-1");
        assert_eq!(definition["name"], "Shared Agent");
        let installations = definition["installations"]
            .as_array()
            .expect("installations array");
        assert_eq!(installations.len(), 2);
        let by_space_ref = |space_ref: &str| {
            installations
                .iter()
                .find(|row| row["space_ref"] == space_ref)
                .unwrap()
        };
        assert_eq!(by_space_ref("space-a")["space_name"], "Team Room");
        assert_eq!(by_space_ref("space-a")["status"], "active");
        assert_eq!(by_space_ref("space-b")["space_name"], "Ops Room");
        assert_eq!(by_space_ref("space-b")["status"], "pending");
    }

    /// The registry itself already drops revoked bindings and bindings whose
    /// definition is gone (see `spaceAgents.ts`'s
    /// `projectAgentInstallationsForOrg` and its own unit tests) — so an empty
    /// registry response is the only shape the gateway needs to render as "no
    /// installations", and must not itself invent any.
    #[tokio::test]
    async fn an_empty_registry_response_reports_no_definitions() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = list_org_agent_installations_response(json!([])).await;

        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["definitions"], json!([]));
    }

    /// A row with no `agentRef` (should never happen — the registry only
    /// projects bindings it already joined to a live definition) is skipped
    /// defensively rather than panicking or fabricating a group key.
    #[tokio::test]
    async fn a_registry_row_with_no_agent_ref_is_skipped() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = list_org_agent_installations_response(json!([
            { "bindingRef": "sab_malformed", "spaceRef": "space-a", "status": "active" },
        ]))
        .await;

        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["definitions"], json!([]));
    }

    /// A failed registry read degrades to 503, never to a false "no
    /// installations" empty list — the same "could not check" vs. "there is
    /// nothing" distinction the single-Space roster path already enforces.
    #[tokio::test]
    async fn a_failed_registry_read_returns_service_unavailable() {
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
        // No APPLICATION_CONVEX_URL / APPLICATION_CONVEX_SERVICE_KEY set, so
        // `convex_gateway_call` fails closed with `Err(())` before ever
        // issuing an HTTP request.
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/installations")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status().as_u16();
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();

        assert_eq!(status, 503, "{body}");
        assert_eq!(body["error"]["code"], "agent_registry_unavailable");
    }

    fn service_member(subject_id: &str) -> Value {
        json!({
            "subject_type": "service",
            "subject_id": subject_id,
            "role": "editor",
            "revision": 4,
            "display_name": ""
        })
    }

    /// The happy path: Control authorizes the agent, Application names it, and
    /// the published channels reach the browser intact.
    #[tokio::test]
    async fn space_agents_join_control_authority_to_application_identity() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = space_agents_response(
            json!([service_member("svc-support")]),
            json!([{
                "bindingRef": "sab_space-1_svc-support",
                "agentRef": "agent-1",
                "subjectId": "svc-support",
                "name": "Kundestøtte",
                "title": "Support",
                "status": "active",
                "definitionStatus": "active",
                "deliveryTargets": [
                    {"channel": "teams", "label": "Drift", "status": "active"},
                    {"channel": "messenger", "label": "Verevon AS", "status": "pending"}
                ],
                "projectionVersion": 2
            }]),
        )
        .await;

        assert_eq!(status, 200);
        let agent = &body["data"]["agents"][0];
        assert_eq!(agent["subject_id"], "svc-support");
        assert_eq!(agent["name"], "Kundestøtte");
        assert_eq!(
            agent["role"], "editor",
            "role comes from Control, not the binding"
        );
        assert_eq!(agent["status"], "active");
        assert_eq!(agent["identity_published"], true);
        assert_eq!(agent["delivery_targets"][0]["channel"], "teams");
        assert_eq!(agent["delivery_targets"][1]["channel"], "messenger");
        assert_eq!(agent["delivery_targets"][1]["status"], "pending");
    }

    /// Control authorized an agent that Application has not described. It must
    /// still appear — dropping it would hide a participant that can genuinely
    /// act in the room — but nothing may invent a name for it.
    #[tokio::test]
    async fn an_authorized_agent_without_a_published_identity_is_still_listed() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) =
            space_agents_response(json!([service_member("svc-orphan")]), json!([])).await;

        assert_eq!(status, 200);
        let agent = &body["data"]["agents"][0];
        assert_eq!(agent["subject_id"], "svc-orphan");
        assert_eq!(agent["identity_published"], false);
        assert!(
            agent.get("name").is_none(),
            "an unnamed agent must not be given a manufactured name"
        );
        assert_eq!(agent["delivery_targets"], json!([]));
    }

    /// The asymmetry that matters: a binding Control does not back is not a
    /// participant. Rendering it would be the false membership claim the whole
    /// definition/binding split exists to prevent.
    #[tokio::test]
    async fn a_binding_without_control_membership_is_never_a_participant() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = space_agents_response(
            json!([]),
            json!([{
                "bindingRef": "sab_space-1_svc-ghost",
                "agentRef": "agent-9",
                "subjectId": "svc-ghost",
                "name": "Spøkelse",
                "status": "active",
                "deliveryTargets": []
            }]),
        )
        .await;

        assert_eq!(status, 200);
        assert_eq!(body["data"]["agents"], json!([]));
        assert!(!body.to_string().contains("Spøkelse"));
    }

    /// People are not agents. The Agent view reads the same roster as Members,
    /// so the service filter is what keeps the two views distinct.
    #[tokio::test]
    async fn human_members_never_appear_as_agents() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = space_agents_response(
            json!([
                {"subject_type": "user", "subject_id": "user-1", "role": "owner", "revision": 1, "display_name": "Ima"},
                service_member("svc-support")
            ]),
            json!([]),
        )
        .await;

        assert_eq!(status, 200);
        assert_eq!(body["data"]["agents"].as_array().unwrap().len(), 1);
        assert_eq!(body["data"]["agents"][0]["subject_id"], "svc-support");
    }

    /// A roster we could not read must not render as "this room has no agents".
    #[tokio::test]
    async fn an_unreadable_roster_fails_rather_than_reporting_no_agents() {
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
            .and(wm_path("/api/v1/internal/spaces/space-1/roster"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&user_core)
            .await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/spaces/space-1/agents")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(
            response.status(),
            axum::http::StatusCode::OK,
            "an unreadable roster must not answer 200 with an empty agent list"
        );
    }

    /// Common upstream wiring for `inject_mentioned_space_agent_persona` tests:
    /// a Control roster and an Application binding, both mountable per case.
    /// Returns the state plus the Convex mock so a test can additionally stub
    /// `agentPersonaForGateway` and later inspect what it received.
    // Returns the user-core MockServer too: wiremock shuts a server down on
    // drop, so leaving it as a helper-local made the roster endpoint die at
    // return and survive only by socket-linger luck — a scheduling flake that
    // surfaced as "mentioned_agent_not_bound" 404s under parallel test load.
    async fn mention_test_state(
        roster: Value,
        bindings: Value,
    ) -> (AppState, MockServer, MockServer) {
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-1/roster"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"members": roster, "count": roster.as_array().map(Vec::len).unwrap_or(0)}
            })))
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(wm_body_partial_json(
                json!({"path": "spaceAgents:spaceAgentBindingsForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": bindings})))
            .mount(&application)
            .await;
        std::env::set_var("APPLICATION_CONVEX_URL", application.uri());
        std::env::set_var("APPLICATION_CONVEX_SERVICE_KEY", "application-test-key");
        let mut state = crate::tests::test_state(false);
        state.user_core_url = user_core.uri();
        (state, application, user_core)
    }

    fn mention_service_member(subject_id: &str) -> Value {
        json!({
            "subject_type": "service",
            "subject_id": subject_id,
            "role": "editor",
            "revision": 1,
            "display_name": ""
        })
    }

    /// The full happy path: Control authorizes the agent, Application names
    /// it, Convex supplies its own instructions, and both land in the outbound
    /// body — while the client's mention field itself does not survive, since
    /// it was never authority, only a hint.
    #[tokio::test]
    async fn a_bound_active_agent_mention_injects_its_persona() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (application_state, application, _user_core) = mention_test_state(
            json!([mention_service_member("svc-support")]),
            json!([{
                "bindingRef": "sab_1",
                "agentRef": "agent-1",
                "subjectId": "svc-support",
                "name": "Kundestøtte",
                "status": "active",
                "deliveryTargets": []
            }]),
        )
        .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(wm_body_partial_json(
                json!({"path": "spaceAgents:agentPersonaForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": {"name": "Kundestøtte", "systemPrompt": "Answer in bullet points."}
            })))
            .mount(&application)
            .await;

        let mut body = json!({
            "content": "hello",
            "space_ref": "space-1",
            "mentioned_agent_ref": "svc-support",
        });
        let result = inject_mentioned_space_agent_persona(
            &application_state,
            &authenticated_user(),
            "org-1",
            &mut body,
        )
        .await;
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");

        assert!(result.is_ok(), "an active bound agent must be authorized");
        assert_eq!(body["agent_name"], "Kundestøtte");
        assert_eq!(body["agent_system_prompt"], "Answer in bullet points.");
        assert!(
            body.get("mentioned_agent_ref").is_none(),
            "the client's mention hint must not survive into the outbound turn"
        );
        // The Space reference is left untouched here: that field belongs to
        // `inject_personal_thread_context`, which runs next and removes it.
        assert_eq!(body["space_ref"], "space-1");
    }

    /// A binding whose policy says `approval_mode: blocked` refuses the
    /// invocation outright — the agent stays visible in the room, but a
    /// mention may not reach the model.
    #[tokio::test]
    async fn a_blocked_binding_refuses_the_mention() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (application_state, _application, _user_core) = mention_test_state(
            json!([mention_service_member("svc-support")]),
            json!([{
                "bindingRef": "sab_1",
                "agentRef": "agent-1",
                "subjectId": "svc-support",
                "name": "Kundestøtte",
                "status": "active",
                "approvalMode": "blocked",
                "deliveryTargets": []
            }]),
        )
        .await;

        let mut body = json!({
            "content": "hello",
            "space_ref": "space-1",
            "mentioned_agent_ref": "svc-support",
        });
        let result = inject_mentioned_space_agent_persona(
            &application_state,
            &authenticated_user(),
            "org-1",
            &mut body,
        )
        .await;
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");

        let (status, payload) = result.expect_err("a blocked binding must refuse");
        assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
        assert_eq!(payload.0["error"]["code"], "agent_invocation_blocked");
        assert!(
            body.get("agent_name").is_none(),
            "no persona may be injected"
        );
    }

    /// A present trigger list without "mention" refuses the mention: the
    /// binding says this agent is not invoked that way.
    #[tokio::test]
    async fn a_binding_without_the_mention_trigger_refuses_the_mention() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (application_state, _application, _user_core) = mention_test_state(
            json!([mention_service_member("svc-support")]),
            json!([{
                "bindingRef": "sab_1",
                "agentRef": "agent-1",
                "subjectId": "svc-support",
                "name": "Kundestøtte",
                "status": "active",
                "triggerModes": ["group"],
                "deliveryTargets": []
            }]),
        )
        .await;

        let mut body = json!({
            "content": "hello",
            "space_ref": "space-1",
            "mentioned_agent_ref": "svc-support",
        });
        let result = inject_mentioned_space_agent_persona(
            &application_state,
            &authenticated_user(),
            "org-1",
            &mut body,
        )
        .await;
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");

        let (status, payload) = result.expect_err("a non-mention binding must refuse");
        assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
        assert_eq!(payload.0["error"]["code"], "agent_mention_not_enabled");
    }

    /// The room-born policy (`mention`, no tools, require_confirmation) binds
    /// the turn itself: autonomy and the tool surface are stripped before the
    /// request leaves the gateway, while the persona still lands.
    #[tokio::test]
    async fn a_room_born_binding_strips_autonomy_and_tools_from_the_turn() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (application_state, application, _user_core) = mention_test_state(
            json!([mention_service_member("svc-support")]),
            json!([{
                "bindingRef": "sab_1",
                "agentRef": "agent-1",
                "subjectId": "svc-support",
                "name": "Kundestøtte",
                "status": "active",
                "triggerModes": ["mention"],
                "allowedTools": [],
                "approvalMode": "require_confirmation",
                "deliveryTargets": []
            }]),
        )
        .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(wm_body_partial_json(
                json!({"path": "spaceAgents:agentPersonaForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": {"name": "Kundestøtte", "systemPrompt": "Answer briefly."}
            })))
            .mount(&application)
            .await;

        let mut body = json!({
            "content": "hello",
            "space_ref": "space-1",
            "mentioned_agent_ref": "svc-support",
            "plan_mode": true,
            "features": ["tools", "agentic", "memory"],
            "tools": [{"name": "web_search"}],
        });
        let result = inject_mentioned_space_agent_persona(
            &application_state,
            &authenticated_user(),
            "org-1",
            &mut body,
        )
        .await;
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");

        assert!(
            result.is_ok(),
            "a mention-enabled binding must be authorized"
        );
        assert_eq!(body["agent_name"], "Kundestøtte");
        assert_eq!(
            body["plan_mode"], false,
            "confirmation policy forbids autonomous runs"
        );
        assert_eq!(
            body["features"],
            json!(["memory"]),
            "agentic and tools features must both be stripped"
        );
        assert_eq!(body["tools"], json!([]), "tool specs must be cleared");
    }

    /// A non-empty `allowedTools` narrows the turn's tool specs to the
    /// matching names instead of the old all-or-nothing behavior -- it must
    /// filter, never grant: a spec the request never asked for cannot appear
    /// just because the binding's allowlist happens to name it.
    #[tokio::test]
    async fn a_nonempty_allowlist_narrows_tool_specs_to_matching_names() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (application_state, application, _user_core) = mention_test_state(
            json!([mention_service_member("svc-support")]),
            json!([{
                "bindingRef": "sab_1",
                "agentRef": "agent-1",
                "subjectId": "svc-support",
                "name": "Kundestøtte",
                "status": "active",
                "triggerModes": ["mention"],
                "allowedTools": ["web_search"],
                "approvalMode": "auto",
                "deliveryTargets": []
            }]),
        )
        .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(wm_body_partial_json(
                json!({"path": "spaceAgents:agentPersonaForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": {"name": "Kundestøtte", "systemPrompt": "Answer briefly."}
            })))
            .mount(&application)
            .await;

        let mut body = json!({
            "content": "hello",
            "space_ref": "space-1",
            "mentioned_agent_ref": "svc-support",
            "plan_mode": true,
            "features": ["tools", "agentic", "memory"],
            "tools": [{"name": "web_search"}, {"name": "delete_file"}],
        });
        let result = inject_mentioned_space_agent_persona(
            &application_state,
            &authenticated_user(),
            "org-1",
            &mut body,
        )
        .await;
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");

        assert!(
            result.is_ok(),
            "a mention-enabled binding must be authorized"
        );
        assert_eq!(
            body["tools"],
            json!([{"name": "web_search"}]),
            "only the allow-listed spec survives"
        );
        assert_eq!(
            body["features"],
            json!(["tools", "agentic", "memory"]),
            "a non-empty surviving tool set keeps the tools feature on"
        );
    }

    /// If none of the request's tool specs match the allowlist, narrowing
    /// leaves nothing -- which must strip the `tools` feature too, exactly
    /// like an explicit empty `allowedTools`, rather than leaving the
    /// feature on with no tools behind it.
    #[tokio::test]
    async fn a_nonempty_allowlist_matching_nothing_strips_the_tools_feature() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (application_state, application, _user_core) = mention_test_state(
            json!([mention_service_member("svc-support")]),
            json!([{
                "bindingRef": "sab_1",
                "agentRef": "agent-1",
                "subjectId": "svc-support",
                "name": "Kundestøtte",
                "status": "active",
                "triggerModes": ["mention"],
                "allowedTools": ["records.read"],
                "approvalMode": "auto",
                "deliveryTargets": []
            }]),
        )
        .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(wm_body_partial_json(
                json!({"path": "spaceAgents:agentPersonaForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": {"name": "Kundestøtte", "systemPrompt": "Answer briefly."}
            })))
            .mount(&application)
            .await;

        let mut body = json!({
            "content": "hello",
            "space_ref": "space-1",
            "mentioned_agent_ref": "svc-support",
            "plan_mode": true,
            "features": ["tools", "agentic", "memory"],
            "tools": [{"name": "web_search"}],
        });
        let result = inject_mentioned_space_agent_persona(
            &application_state,
            &authenticated_user(),
            "org-1",
            &mut body,
        )
        .await;
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");

        assert!(
            result.is_ok(),
            "a mention-enabled binding must be authorized"
        );
        assert_eq!(body["tools"], json!([]), "nothing matched the allowlist");
        assert_eq!(
            body["features"],
            json!(["agentic", "memory"]),
            "tools feature must be stripped when narrowing leaves nothing"
        );
    }

    /// A mention naming an agent with no Control membership, or no Application
    /// binding, is rejected rather than silently answered by an unauthorized
    /// agent or silently ignored — either would let a mention grant reach
    /// (`docs/space-defenition.md`: "A mention never grants access").
    #[tokio::test]
    async fn mentioning_an_agent_not_bound_to_the_space_is_rejected() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (state, _application, _user_core) = mention_test_state(json!([]), json!([])).await;

        let mut body = json!({
            "content": "hello",
            "space_ref": "space-1",
            "mentioned_agent_ref": "svc-ghost",
        });
        let result =
            inject_mentioned_space_agent_persona(&state, &authenticated_user(), "org-1", &mut body)
                .await;
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");

        let (status, response) = result.expect_err("an unbound mention must be rejected");
        assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
        assert_eq!(response.0["error"]["code"], "mentioned_agent_not_bound");
        assert!(
            body.get("agent_name").is_none(),
            "a rejected mention must never inject a persona"
        );
    }

    /// A binding that exists but has moved to `paused`/`revoked`/`failed` must
    /// be rejected exactly like an absent one — `compose_space_agents` only
    /// calls a binding `active` when its own status says so, and this is that
    /// filter applied to the invocation path.
    #[tokio::test]
    async fn mentioning_a_paused_agent_binding_is_rejected() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (state, _application, _user_core) = mention_test_state(
            json!([mention_service_member("svc-support")]),
            json!([{
                "bindingRef": "sab_1",
                "agentRef": "agent-1",
                "subjectId": "svc-support",
                "name": "Kundestøtte",
                "status": "paused",
                "deliveryTargets": []
            }]),
        )
        .await;

        let mut body = json!({
            "content": "hello",
            "space_ref": "space-1",
            "mentioned_agent_ref": "svc-support",
        });
        let result =
            inject_mentioned_space_agent_persona(&state, &authenticated_user(), "org-1", &mut body)
                .await;
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");

        let (status, _) = result.expect_err("a paused binding must not answer");
        assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
    }

    /// An ordinary turn nobody addressed to an agent must be a complete
    /// no-op: no upstream call, no error, no injected field. This is what
    /// keeps every non-Space, non-mention Chat turn behaving exactly as it did
    /// before this feature existed.
    #[tokio::test]
    async fn a_turn_with_no_mention_is_untouched() {
        let mut body = json!({ "content": "hello", "space_ref": "space-1" });
        let original = body.clone();
        let result = inject_mentioned_space_agent_persona(
            &crate::tests::test_state(false),
            &authenticated_user(),
            "org-1",
            &mut body,
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(
            body, original,
            "an unmentioned turn must not be modified at all"
        );
    }

    /// A mention outside a Space is nonsensical — there is no roster to check
    /// it against — and must fail closed rather than silently answering as the
    /// plain Verevon voice with an unauthorized name attached.
    #[tokio::test]
    async fn mentioning_an_agent_with_no_space_ref_is_rejected() {
        let mut body = json!({ "content": "hello", "mentioned_agent_ref": "svc-support" });
        let result = inject_mentioned_space_agent_persona(
            &crate::tests::test_state(false),
            &authenticated_user(),
            "org-1",
            &mut body,
        )
        .await;

        let (status, response) = result.expect_err("a mention with no Space must be rejected");
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(response.0["error"]["code"], "agent_mention_requires_space");
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
        // The org's Space list, which is what the ref lookup now reads. It was
        // a single personal-Space object before; resolving any room by ref made
        // it a list.
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                {"spaceRef":"space-personal", "name":"Personal Space", "kind":"personal", "lifecycle":"active"}
            ]})))
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
        // The org's Space list — see the note in the context test above.
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                {"spaceRef":"space-personal", "name":"Personal Space", "kind":"personal", "lifecycle":"active"}
            ]})))
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

    // ── ADR-0003: authored-instruction hierarchy ────────────────────────────

    async fn authored_instructions_test_state(
        org_response: Value,
        space_response: Value,
    ) -> (AppState, MockServer, MockServer) {
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(wm_body_partial_json(
                json!({"path": "organizations:instructionsForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": org_response})))
            .mount(&application)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(wm_body_partial_json(
                json!({"path": "spaces:instructionsForGateway"}),
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"value": space_response})),
            )
            .mount(&application)
            .await;
        std::env::set_var("APPLICATION_CONVEX_URL", application.uri());
        std::env::set_var("APPLICATION_CONVEX_SERVICE_KEY", "application-test-key");
        // Any turn carrying a `space_ref` now has its Space layer gated on a
        // live Control membership check (`resolve_space_role`) before Convex
        // is even asked — this default "viewer" response lets a scoped test
        // reach the Convex mocks above without needing its own membership
        // stub, unless it specifically wants to test a non-member rejection.
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-1/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-1", "kind": "room", "role": "viewer"}
            })))
            .mount(&user_core)
            .await;
        let mut state = crate::tests::test_state(false);
        state.user_core_url = user_core.uri();
        (state, application, user_core)
    }

    #[tokio::test]
    async fn org_instructions_are_injected_on_a_personal_turn_with_no_space() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (state, _application, _user_core) = authored_instructions_test_state(
            json!({"instructions": "Always answer in Norwegian."}),
            Value::Null,
        )
        .await;
        let mut body = json!({"content": "hello"});
        let result =
            inject_authored_instructions(&state, &authenticated_user(), "org-1", &mut body).await;
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");

        assert!(result.is_ok());
        assert_eq!(body["org_instructions"], "Always answer in Norwegian.");
        assert!(
            body.get("space_instructions").is_none(),
            "no space_ref was carried, so no Space layer should be looked up at all"
        );
    }

    #[tokio::test]
    async fn space_instructions_are_injected_alongside_org_instructions_when_scoped() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (state, _application, _user_core) = authored_instructions_test_state(
            json!({"instructions": "Org rule."}),
            json!({"instructions": "Space rule."}),
        )
        .await;
        let mut body = json!({"content": "hello", "space_ref": "space-1"});
        let result =
            inject_authored_instructions(&state, &authenticated_user(), "org-1", &mut body).await;
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");

        assert!(result.is_ok());
        assert_eq!(body["org_instructions"], "Org rule.");
        assert_eq!(body["space_instructions"], "Space rule.");
        // Peeked, not consumed: `inject_personal_thread_context` still needs it.
        assert_eq!(body["space_ref"], "space-1");
    }

    #[tokio::test]
    async fn a_non_member_never_receives_that_spaces_instructions() {
        // The membership stub `authored_instructions_test_state` mounts only
        // answers for "space-1"; a `space_ref` it has never heard of gets
        // Control's genuine 404, exactly like a real non-member would.
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (state, _application, _user_core) = authored_instructions_test_state(
            json!({"instructions": "Org rule."}),
            json!({"instructions": "Space rule."}),
        )
        .await;
        let mut body = json!({"content": "hello", "space_ref": "someone-elses-space"});
        let result =
            inject_authored_instructions(&state, &authenticated_user(), "org-1", &mut body).await;
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");

        assert!(
            result.is_err(),
            "a non-member's Space read must fail closed"
        );
        assert!(
            body.get("space_instructions").is_none(),
            "no Space content may leak to a caller Control never confirmed as a member"
        );
    }

    #[tokio::test]
    async fn a_client_supplied_instructions_field_never_survives_uninspected() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (state, _application, _user_core) =
            authored_instructions_test_state(Value::Null, Value::Null).await;
        let mut body = json!({
            "content": "hello",
            "org_instructions": "forged org instructions",
            "space_instructions": "forged space instructions",
        });
        let result =
            inject_authored_instructions(&state, &authenticated_user(), "org-1", &mut body).await;
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");

        assert!(result.is_ok());
        assert!(
            body.get("org_instructions").is_none(),
            "a forged value must not survive just because Convex had nothing authored"
        );
        assert!(body.get("space_instructions").is_none());
    }

    #[tokio::test]
    async fn empty_org_id_injects_nothing() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (state, _application, _user_core) =
            authored_instructions_test_state(json!({"instructions": "Org rule."}), Value::Null)
                .await;
        let mut body = json!({"content": "hello"});
        let result =
            inject_authored_instructions(&state, &authenticated_user(), "", &mut body).await;
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");

        assert!(result.is_ok());
        assert!(body.get("org_instructions").is_none());
    }

    async fn space_instructions_response(
        role: &str,
        method: &str,
        path: &str,
        write_body: Option<Value>,
    ) -> (u16, Value) {
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
            .and(wm_path("/api/v1/internal/spaces/space-1/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-1", "kind": "room", "role": role}
            })))
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(wm_body_partial_json(
                json!({"path": "spaces:instructionsForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": {"instructions": "Existing rule."}
            })))
            .mount(&application)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .and(wm_body_partial_json(
                json!({"path": "spaces:setInstructionsForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": {"instructions": "New rule."}
            })))
            .mount(&application)
            .await;
        std::env::set_var("APPLICATION_CONVEX_URL", application.uri());
        std::env::set_var("APPLICATION_CONVEX_SERVICE_KEY", "application-test-key");
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();

        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("cookie", "better-auth.session_token=session-1");
        let request = if let Some(body) = write_body {
            request
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap()
        } else {
            request.body(Body::empty()).unwrap()
        };
        let response = crate::build_router(state).oneshot(request).await.unwrap();
        std::env::remove_var("APPLICATION_CONVEX_URL");
        std::env::remove_var("APPLICATION_CONVEX_SERVICE_KEY");
        let status = response.status().as_u16();
        let parsed: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, parsed)
    }

    #[tokio::test]
    async fn a_viewer_can_read_space_instructions() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = space_instructions_response(
            "viewer",
            "GET",
            "/api/v1/spaces/space-1/instructions",
            None,
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["instructions"], "Existing rule.");
    }

    #[tokio::test]
    async fn a_viewer_cannot_write_space_instructions() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = space_instructions_response(
            "viewer",
            "PATCH",
            "/api/v1/spaces/space-1/instructions",
            Some(json!({"instructions": "New rule."})),
        )
        .await;
        assert_eq!(status, 403, "{body}");
    }

    #[tokio::test]
    async fn an_editor_can_write_space_instructions() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = space_instructions_response(
            "editor",
            "PATCH",
            "/api/v1/spaces/space-1/instructions",
            Some(json!({"instructions": "New rule."})),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["instructions"], "New rule.");
    }

    #[tokio::test]
    async fn an_owner_can_write_space_instructions() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = space_instructions_response(
            "owner",
            "PATCH",
            "/api/v1/spaces/space-1/instructions",
            Some(json!({"instructions": "New rule."})),
        )
        .await;
        assert_eq!(status, 200, "{body}");
    }

    #[tokio::test]
    async fn overlong_space_instructions_are_rejected_before_reaching_convex() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = space_instructions_response(
            "owner",
            "PATCH",
            "/api/v1/spaces/space-1/instructions",
            Some(json!({"instructions": "x".repeat(4001)})),
        )
        .await;
        assert_eq!(status, 400, "{body}");
    }

    /// 'æ' is 2 bytes in UTF-8. 4000 of them is exactly at the character
    /// limit but 8000 bytes — a byte-length check would wrongly reject this
    /// valid Norwegian text.
    #[tokio::test]
    async fn four_thousand_norwegian_characters_are_accepted() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = space_instructions_response(
            "owner",
            "PATCH",
            "/api/v1/spaces/space-1/instructions",
            Some(json!({"instructions": "æ".repeat(4000)})),
        )
        .await;
        assert_eq!(status, 200, "{body}");
    }

    #[test]
    fn update_space_instructions_request_deserializes_a_missing_instructions_field() {
        let parsed: UpdateSpaceInstructionsRequest = serde_json::from_value(json!({})).unwrap();
        assert!(parsed.instructions.is_none());
    }
}
