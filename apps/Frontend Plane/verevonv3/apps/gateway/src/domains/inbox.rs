//! Inbox domain — unified support conversations.
//!
//! Proxies the SPA's `/api/v1/inbox/*` surface to conversation-core-go's
//! `/api/v1/conversations`, `/api/v1/inboxes`, and `/api/v1/ai-actions` (the HITL
//! review queue) API. conversation-core-go
//! authenticates with the internal API key + `x-org-id` + `x-user-*` actor
//! headers — exactly what `proxy_json` forwards — so these are thin proxies and
//! the SPA never talks to the Application Plane directly.

use axum::{
    extract::{Extension, Path, State},
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::error,
    middleware::{require_session, AuthenticatedUser},
    upstream::{proxy_conversation_json, proxy_json},
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/inbox/inboxes", get(list_inboxes))
        .route("/api/v1/inbox/conversations", get(list_conversations))
        .route(
            "/api/v1/inbox/outbound-intents",
            get(list_organization_outbound_intents),
        )
        .route("/api/v1/inbox/conversations/{id}", get(get_conversation))
        .route(
            "/api/v1/inbox/conversations/{id}/activity",
            get(list_conversation_activity),
        )
        .route(
            "/api/v1/inbox/conversations/{id}/outbound-intents",
            get(list_outbound_intents),
        )
        .route(
            "/api/v1/inbox/conversations/{id}/draft",
            get(get_conversation_draft)
                .put(upsert_conversation_draft)
                .delete(delete_conversation_draft),
        )
        .route(
            "/api/v1/inbox/conversations/{id}/draft-lease",
            get(get_draft_lease)
                .post(claim_draft_lease)
                .delete(release_draft_lease),
        )
        .route(
            "/api/v1/inbox/conversations/{id}/follow",
            get(get_conversation_follow)
                .post(follow_conversation)
                .delete(unfollow_conversation),
        )
        .route(
            "/api/v1/inbox/conversations/{id}/csat-preference",
            get(get_conversation_csat_preference).patch(patch_conversation_csat_preference),
        )
        .route(
            "/api/v1/inbox/conversations/{id}/messages",
            post(add_message),
        )
        .route("/api/v1/inbox/conversations/{id}/notes", post(add_note))
        .route(
            "/api/v1/inbox/conversations/{id}/status",
            patch(patch_status),
        )
        .route(
            "/api/v1/inbox/conversations/{id}/assignment",
            patch(patch_assignment),
        )
        .route("/api/v1/inbox/conversations/{id}/tags", post(add_tag))
        .route(
            "/api/v1/inbox/conversations/{id}/tags/{tag}",
            delete(remove_tag),
        )
        .route("/api/v1/inbox/feedback", post(submit_feedback))
        .route("/api/v1/inbox/workspace", get(get_workspace_state))
        .route("/api/v1/inbox/workspace/pins", post(update_workspace_pin))
        .route("/api/v1/inbox/workspace/read", post(update_workspace_read))
        .route(
            "/api/v1/inbox/ai-actions",
            get(list_ai_actions).post(create_ai_text_proposal),
        )
        .route(
            "/api/v1/inbox/ai-actions/{id}/approve",
            post(approve_ai_action),
        )
        .route(
            "/api/v1/inbox/ai-actions/{id}/reject",
            post(reject_ai_action),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn qs(uri: &Uri) -> String {
    uri.query()
        .filter(|q| !q.is_empty())
        .map(|q| format!("?{q}"))
        .unwrap_or_default()
}

fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

// ── handlers ────────────────────────────────────────────────────────────────

async fn list_inboxes(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let url = format!("{}/api/v1/inboxes", state.conversation_core_url);
    proxy_conversation_json(&state, Method::GET, &url, None, &user, None)
        .await
        .into_response()
}

async fn list_conversations(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    uri: Uri,
) -> Response {
    let url = format!(
        "{}/api/v1/conversations{}",
        state.conversation_core_url,
        qs(&uri)
    );
    proxy_conversation_json(&state, Method::GET, &url, None, &user, None)
        .await
        .into_response()
}

async fn list_organization_outbound_intents(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    uri: Uri,
) -> Response {
    let url = format!(
        "{}/api/v1/outbound-intents{}",
        state.conversation_core_url,
        qs(&uri)
    );
    proxy_conversation_json(&state, Method::GET, &url, None, &user, None)
        .await
        .into_response()
}

async fn get_conversation(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    let url = format!(
        "{}/api/v1/conversations/{}",
        state.conversation_core_url,
        urlencoding::encode(&id)
    );
    proxy_conversation_json(&state, Method::GET, &url, None, &user, None)
        .await
        .into_response()
}

async fn list_conversation_activity(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    uri: Uri,
) -> Response {
    let url = format!(
        "{}/api/v1/conversations/{}/activity{}",
        state.conversation_core_url,
        urlencoding::encode(&id),
        qs(&uri)
    );
    proxy_conversation_json(&state, Method::GET, &url, None, &user, None)
        .await
        .into_response()
}

async fn list_outbound_intents(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    forward_conversation_write(&state, &user, Method::GET, &id, "outbound-intents", None).await
}

async fn get_draft_lease(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    forward_conversation_write(&state, &user, Method::GET, &id, "draft-lease", None).await
}

async fn get_conversation_follow(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    forward_conversation_write(&state, &user, Method::GET, &id, "follow", None).await
}

async fn get_conversation_csat_preference(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    forward_conversation_write(&state, &user, Method::GET, &id, "csat-preference", None).await
}

async fn patch_conversation_csat_preference(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    forward_conversation_write(
        &state,
        &user,
        Method::PATCH,
        &id,
        "csat-preference",
        Some(body),
    )
    .await
}

// Personal drafts are the only Inbox text persisted before a message is sent.
// The gateway owns the retention decision so a browser cannot bypass Zero Data
// Retention by calling conversation-core directly (which only trusts delegation
// from this gateway).
async fn get_conversation_draft(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = require_draft_persistence(&state, &user).await {
        return response;
    }
    forward_conversation_write(&state, &user, Method::GET, &id, "draft", None).await
}

async fn upsert_conversation_draft(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if let Err(response) = require_draft_persistence(&state, &user).await {
        return response;
    }
    forward_conversation_write(&state, &user, Method::PUT, &id, "draft", Some(body)).await
}

async fn delete_conversation_draft(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = require_draft_persistence(&state, &user).await {
        return response;
    }
    forward_conversation_write(&state, &user, Method::DELETE, &id, "draft", None).await
}

async fn require_draft_persistence(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Result<(), Response> {
    let Some(membership) = user.authorized_membership.as_ref() else {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "active_organization_required",
                "Select an active organization first.",
            )),
        )
            .into_response());
    };
    let org_id = membership.organization_id.trim();
    if org_id.is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "active_organization_required",
                "Select an active organization first.",
            )),
        )
            .into_response());
    }

    let actor = actor_for(user);
    let (status, Json(organization)) = proxy_json(
        state,
        Method::GET,
        &format!(
            "{}/api/v1/organizations/{}",
            state.org_core_url,
            urlencoding::encode(org_id)
        ),
        None,
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "retention_posture_unavailable",
                "Draft persistence is unavailable until the organization retention policy can be verified.",
            )),
        )
            .into_response());
    }

    let zdr_enabled = organization
        .get("data")
        .unwrap_or(&organization)
        .get("metadata")
        .and_then(|metadata| metadata.get("interactiveRetention"))
        .and_then(|retention| retention.get("zdr"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if zdr_enabled {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(error(
                "zdr_draft_persistence_forbidden",
                "Personal drafts are not retained while Zero Data Retention is enabled.",
            )),
        )
            .into_response());
    }
    Ok(())
}

/// AI-generated support work is retained review material. The Control Plane is
/// the sole authority for whether an organization permits that persistence:
/// ZDR forbids it entirely, Assist permits only transient model help, and
/// Review permits exact-text / bounded-field proposals for human review.
pub(crate) async fn require_support_ai_review(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Result<(), Response> {
    let Some(membership) = user.authorized_membership.as_ref() else {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "active_organization_required",
                "Select an active organization first.",
            )),
        )
            .into_response());
    };
    let org_id = membership.organization_id.trim();
    if org_id.is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "active_organization_required",
                "Select an active organization first.",
            )),
        )
            .into_response());
    }

    let actor = actor_for(user);
    let (status, Json(organization)) = proxy_json(
        state,
        Method::GET,
        &format!(
            "{}/api/v1/organizations/{}",
            state.org_core_url,
            urlencoding::encode(org_id)
        ),
        None,
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "support_ai_policy_unavailable",
                "AI review is unavailable until the organization policy can be verified.",
            )),
        )
            .into_response());
    }

    let metadata = organization
        .get("data")
        .unwrap_or(&organization)
        .get("metadata");
    let zdr_enabled = metadata
        .and_then(|value| value.get("interactiveRetention"))
        .and_then(|retention| retention.get("zdr"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if zdr_enabled {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(error(
                "zdr_ai_proposal_forbidden",
                "AI proposals are not retained while Zero Data Retention is enabled.",
            )),
        )
            .into_response());
    }

    let support_ai_mode = metadata
        .and_then(|value| value.get("supportAi"))
        .and_then(|support_ai| support_ai.get("mode"))
        .and_then(Value::as_str)
        .unwrap_or("review");
    if support_ai_mode != "review" {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(error(
                "ai_review_mode_required",
                "Retained AI proposals require the organization's Support AI review mode.",
            )),
        )
            .into_response());
    }
    Ok(())
}
async fn claim_draft_lease(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    forward_conversation_write(&state, &user, Method::POST, &id, "draft-lease", None).await
}
async fn release_draft_lease(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    forward_conversation_write(&state, &user, Method::DELETE, &id, "draft-lease", None).await
}

async fn follow_conversation(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    forward_conversation_write(&state, &user, Method::POST, &id, "follow", None).await
}

async fn unfollow_conversation(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    forward_conversation_write(&state, &user, Method::DELETE, &id, "follow", None).await
}

async fn add_message(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    forward_conversation_write(&state, &user, Method::POST, &id, "messages", Some(body)).await
}

async fn add_note(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    forward_conversation_write(&state, &user, Method::POST, &id, "notes", Some(body)).await
}

/// A signed-in org member's one-line friction report from the shell's
/// persistent "Send feedback" control. Proxies straight to
/// conversation-core-go's `/api/v1/feedback`, which lands it as a new,
/// `demo-feedback`-tagged conversation in the org's own Inbox — the SPA never
/// picks the conversation id or org scope itself.
async fn submit_feedback(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/v1/feedback", state.conversation_core_url);
    proxy_conversation_json(&state, Method::POST, &url, Some(body), &user, None)
        .await
        .into_response()
}

async fn get_workspace_state(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let actor = actor_for(&user);
    let (status, body) = proxy_json(
        &state,
        Method::GET,
        &format!("{}/api/v1/inbox-workspace", state.user_core_url),
        None,
        None,
        Some(&actor),
        None,
    )
    .await;
    (status, body).into_response()
}

async fn update_workspace_pin(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward_workspace_preference(&state, &user, "pins", body).await
}

async fn update_workspace_read(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward_workspace_preference(&state, &user, "read", body).await
}

async fn forward_workspace_preference(
    state: &AppState,
    user: &AuthenticatedUser,
    preference: &str,
    body: Value,
) -> Response {
    let actor = actor_for(user);
    let (status, body) = proxy_json(
        state,
        Method::POST,
        &format!(
            "{}/api/v1/inbox-workspace/{preference}",
            state.user_core_url
        ),
        Some(body),
        None,
        Some(&actor),
        None,
    )
    .await;
    (status, body).into_response()
}

async fn patch_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    forward_conversation_write(&state, &user, Method::PATCH, &id, "status", Some(body)).await
}

async fn patch_assignment(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    forward_conversation_write(&state, &user, Method::PATCH, &id, "assignment", Some(body)).await
}

async fn add_tag(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    forward_conversation_write(&state, &user, Method::POST, &id, "tags", Some(body)).await
}

async fn remove_tag(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, tag)): Path<(String, String)>,
) -> Response {
    let url = format!(
        "{}/api/v1/conversations/{}/tags/{}",
        state.conversation_core_url,
        urlencoding::encode(&id),
        urlencoding::encode(&tag)
    );
    proxy_conversation_json(&state, Method::DELETE, &url, None, &user, None)
        .await
        .into_response()
}

async fn forward_conversation_write(
    state: &AppState,
    user: &AuthenticatedUser,
    method: Method,
    id: &str,
    sub: &str,
    body: Option<Value>,
) -> Response {
    let url = format!(
        "{}/api/v1/conversations/{}/{}",
        state.conversation_core_url,
        urlencoding::encode(id),
        sub
    );
    proxy_conversation_json(state, method, &url, body, user, None)
        .await
        .into_response()
}

// ── AI-action HITL review queue ───────────────────────────────────────────────
//
// Model-proposed actions awaiting a human decision. The org scope is always the
// authenticated session's org (never a client header/query/body), so a foreign
// action id simply does not match `org_id = $authenticated AND id = $id` upstream
// and resolves to 404 — never a cross-tenant read or a phantom review row.

async fn list_ai_actions(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    uri: Uri,
) -> Response {
    let url = format!(
        "{}/api/v1/ai-actions{}",
        state.conversation_core_url,
        qs(&uri)
    );
    proxy_conversation_json(&state, Method::GET, &url, None, &user, None)
        .await
        .into_response()
}

// The browser may create only allow-listed reviewable proposals. It cannot
// select an arbitrary action kind, actor, org, or executor payload; those
// powers remain in conversation-core and the authenticated session.
pub(crate) async fn create_ai_text_proposal(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    if let Err(response) = require_support_ai_review(&state, &user).await {
        return response;
    }
    let Some(conversation_id) = body
        .get("conversation_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return invalid_ai_text_proposal("conversation_id is required");
    };
    let kind = body
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("draft.reply");
    if kind == "ticket.update" {
        return create_ticket_update_proposal(&state, &user, conversation_id, &body).await;
    }
    if kind == "incident.create" {
        return create_incident_proposal(&state, &user, conversation_id, &body).await;
    }
    if kind == "problem.create" {
        return create_problem_proposal(&state, &user, conversation_id, &body).await;
    }
    let proposal_group_id = match proposal_group_id(&body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(body_text) = body
        .get("body_text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return invalid_ai_text_proposal("body_text is required");
    };
    if body_text.chars().count() > 8_000 {
        return invalid_ai_text_proposal("body_text must be 8,000 characters or fewer");
    }
    let kind = match kind {
        "draft.reply" | "internal.note" => kind,
        _ => return invalid_ai_text_proposal("unsupported AI proposal kind"),
    };

    let url = format!("{}/api/v1/ai-actions", state.conversation_core_url);
    proxy_conversation_json(
        &state,
        Method::POST,
        &url,
        Some(json!({
            "conversation_id": conversation_id,
            "proposal_group_id": proposal_group_id,
            "kind": kind,
            "payload": { "body_text": body_text },
        })),
        &user,
        None,
    )
    .await
    .into_response()
}

async fn create_ticket_update_proposal(
    state: &AppState,
    user: &AuthenticatedUser,
    conversation_id: &str,
    body: &Value,
) -> Response {
    let proposal_group_id = match proposal_group_id(body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(ticket_id) = body
        .get("ticket_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return invalid_ai_text_proposal("ticket_id is required for ticket.update");
    };
    let Some(fields) = body.get("suggested_fields").and_then(Value::as_object) else {
        return invalid_ai_text_proposal("suggested_fields are required for ticket.update");
    };
    let Some(confidence) = body
        .get("confidence")
        .and_then(Value::as_f64)
        .filter(|value| (0.0..=1.0).contains(value))
    else {
        return invalid_ai_text_proposal("ticket.update confidence must be between 0 and 1");
    };
    let Some(reason) = body
        .get("reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.chars().count() <= 500)
    else {
        return invalid_ai_text_proposal(
            "ticket.update reason is required and must be at most 500 characters",
        );
    };
    let evidence = body
        .get("evidence_message_ids")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if evidence.len() > 25
        || evidence.iter().any(|value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty() && value.chars().count() <= 120)
                .is_none()
        })
    {
        return invalid_ai_text_proposal(
            "ticket.update evidence_message_ids must contain at most 25 bounded identifiers",
        );
    }
    let normalized = match normalize_ticket_update_fields(fields) {
        Ok(fields) => fields,
        Err(message) => return invalid_ai_text_proposal(message),
    };
    let url = format!("{}/api/v1/ai-actions", state.conversation_core_url);
    proxy_conversation_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "conversation_id": conversation_id,
            "proposal_group_id": proposal_group_id,
            "kind": "ticket.update",
            "payload": {
                "ticket_id": ticket_id,
                "confidence": confidence,
                "reason": reason,
                "evidence_message_ids": evidence,
                "suggested_fields": normalized,
            },
        })),
        user,
        None,
    )
    .await
    .into_response()
}

/// Create the only AI-originated operational incident action the browser may
/// submit. The gateway preserves the exact, bounded proposal data but does not
/// admit status, ownership, links beyond the supplied ticket, or lifecycle
/// controls. Conversation-core re-validates both tenancy and ticket ownership
/// before a reviewer can execute it.
async fn create_incident_proposal(
    state: &AppState,
    user: &AuthenticatedUser,
    conversation_id: &str,
    body: &Value,
) -> Response {
    let proposal_group_id = match proposal_group_id(body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(ticket_id) = bounded_required_string(body, "ticket_id", 120) else {
        return invalid_ai_text_proposal("ticket_id is required for incident.create");
    };
    let Some(title) = bounded_required_string(body, "title", 300) else {
        return invalid_ai_text_proposal(
            "incident.create title is required and must be at most 300 characters",
        );
    };
    let Some(severity) = bounded_required_string(body, "severity", 20) else {
        return invalid_ai_text_proposal("incident.create severity is required");
    };
    if !matches!(severity.as_str(), "low" | "medium" | "high" | "critical") {
        return invalid_ai_text_proposal("incident.create severity is invalid");
    }
    let Some(customer_impact) = bounded_required_string(body, "customer_impact", 2_000) else {
        return invalid_ai_text_proposal(
            "incident.create customer_impact is required and must be at most 2,000 characters",
        );
    };
    let Some(confidence) = body
        .get("confidence")
        .and_then(Value::as_f64)
        .filter(|value| (0.0..=1.0).contains(value))
    else {
        return invalid_ai_text_proposal("incident.create confidence must be between 0 and 1");
    };
    let Some(reason) = bounded_required_string(body, "reason", 500) else {
        return invalid_ai_text_proposal(
            "incident.create reason is required and must be at most 500 characters",
        );
    };
    let evidence = match bounded_evidence_message_ids(body) {
        Ok(value) => value,
        Err(response) => return response,
    };

    let url = format!("{}/api/v1/ai-actions", state.conversation_core_url);
    proxy_conversation_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "conversation_id": conversation_id,
            "proposal_group_id": proposal_group_id,
            "kind": "incident.create",
            "payload": {
                "ticket_id": ticket_id,
                "title": title,
                "severity": severity,
                "customer_impact": customer_impact,
                "confidence": confidence,
                "reason": reason,
                "evidence_message_ids": evidence,
            },
        })),
        user,
        None,
    )
    .await
    .into_response()
}

// Problem candidates remain their own reviewable object. In particular, the
// browser cannot smuggle an incident id, owner, status, or propagation choice
// into this path; only an approved core action may create a new investigating
// Problem from the bounded evidence available on this conversation.
async fn create_problem_proposal(
    state: &AppState,
    user: &AuthenticatedUser,
    conversation_id: &str,
    body: &Value,
) -> Response {
    let proposal_group_id = match proposal_group_id(body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(title) = bounded_required_string(body, "title", 300) else {
        return invalid_ai_text_proposal(
            "problem.create title is required and must be at most 300 characters",
        );
    };
    let Some(summary) = bounded_required_string(body, "summary", 2_000) else {
        return invalid_ai_text_proposal(
            "problem.create summary is required and must be at most 2,000 characters",
        );
    };
    let root_cause = match body.get("root_cause") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(_)) => match bounded_optional_string(body, "root_cause", 2_000) {
            Some(value) => value,
            None => {
                return invalid_ai_text_proposal(
                    "problem.create root_cause must be at most 2,000 characters when supplied",
                );
            }
        },
        _ => return invalid_ai_text_proposal("problem.create root_cause must be a string"),
    };
    let Some(confidence) = body
        .get("confidence")
        .and_then(Value::as_f64)
        .filter(|value| (0.0..=1.0).contains(value))
    else {
        return invalid_ai_text_proposal("problem.create confidence must be between 0 and 1");
    };
    let Some(reason) = bounded_required_string(body, "reason", 500) else {
        return invalid_ai_text_proposal(
            "problem.create reason is required and must be at most 500 characters",
        );
    };
    let evidence = match bounded_evidence_message_ids(body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let url = format!("{}/api/v1/ai-actions", state.conversation_core_url);
    proxy_conversation_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "conversation_id": conversation_id,
            "proposal_group_id": proposal_group_id,
            "kind": "problem.create",
            "payload": {
                "title": title,
                "summary": summary,
                "root_cause": root_cause,
                "confidence": confidence,
                "reason": reason,
                "evidence_message_ids": evidence,
            },
        })),
        user,
        None,
    )
    .await
    .into_response()
}

fn bounded_required_string(body: &Value, key: &str, max_chars: usize) -> Option<String> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.chars().count() <= max_chars)
        .map(str::to_owned)
}

/// Gateway-side defense in depth for the exact ticket-update payload that will
/// later be independently normalized by conversation-core. Status is limited
/// to active work states: closing, resolving, and snoozing are explicit human
/// decisions and never part of an AI proposal.
fn normalize_ticket_update_fields(
    fields: &serde_json::Map<String, Value>,
) -> Result<serde_json::Map<String, Value>, &'static str> {
    if fields.is_empty() || fields.len() > 8 {
        return Err("ticket.update needs one to eight suggested fields");
    }
    let mut normalized = serde_json::Map::new();
    for (key, value) in fields {
        if !matches!(
            key.as_str(),
            "category"
                | "intent"
                | "work_type"
                | "priority"
                | "severity"
                | "status"
                | "team_id"
                | "team_name"
        ) {
            return Err("ticket.update contains an unsupported field");
        }
        let Some(value) = value.as_str().map(str::trim).filter(|value| {
            !value.is_empty() && value.chars().count() <= if key == "team_name" { 160 } else { 120 }
        }) else {
            return Err("ticket.update fields must be non-empty and at most 120 characters");
        };
        if key == "priority" && !matches!(value, "low" | "normal" | "high" | "urgent") {
            return Err("ticket.update priority is invalid");
        }
        if key == "severity" && !matches!(value, "low" | "medium" | "high" | "critical") {
            return Err("ticket.update severity is invalid");
        }
        if key == "work_type" && !matches!(value, "customer_case" | "internal_work" | "incident") {
            return Err("ticket.update work_type is invalid");
        }
        if key == "status"
            && !matches!(
                value,
                "open" | "waiting_customer" | "waiting_team" | "escalated"
            )
        {
            return Err("ticket.update status is not reviewable");
        }
        if key == "category" && value.chars().count() > 80 {
            return Err("ticket.update category must be 80 characters or fewer");
        }
        normalized.insert(key.clone(), Value::String(value.to_string()));
    }
    if normalized.contains_key("team_id") != normalized.contains_key("team_name") {
        return Err("ticket.update routing needs both team_id and team_name");
    }
    Ok(normalized)
}

fn bounded_optional_string(body: &Value, key: &str, max_chars: usize) -> Option<String> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| value.chars().count() <= max_chars)
        .map(str::to_owned)
}

// `Response` as the error variant is this crate's idiomatic "fail with an
// HTTP response, propagate with `?`" pattern; boxing it here would ripple
// into every caller for no behavioral gain.
#[allow(clippy::result_large_err)]
fn bounded_evidence_message_ids(body: &Value) -> Result<Vec<Value>, Response> {
    let evidence = body
        .get("evidence_message_ids")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if evidence.len() > 25
        || evidence.iter().any(|value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty() && value.chars().count() <= 120)
                .is_none()
        })
    {
        return Err(invalid_ai_text_proposal(
            "evidence_message_ids must contain at most 25 bounded identifiers",
        ));
    }
    Ok(evidence)
}

/// Proposal grouping is a display/audit correlation only. It is not an
/// approval credential: each action remains independently scoped, reviewed,
/// and executed. Keep it opaque and bounded so it cannot become arbitrary
/// metadata or a query language.
#[allow(clippy::result_large_err)]
fn proposal_group_id(body: &Value) -> Result<String, Response> {
    let Some(value) = body.get("proposal_group_id") else {
        return Ok(String::new());
    };
    let Some(value) = value.as_str().map(str::trim) else {
        return Err(invalid_ai_text_proposal(
            "proposal_group_id must be a string",
        ));
    };
    if value.is_empty() {
        return Ok(String::new());
    }
    if value.chars().count() > 120
        || !value
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || matches!(char, '_' | '-'))
    {
        return Err(invalid_ai_text_proposal(
            "proposal_group_id must be a bounded opaque identifier",
        ));
    }
    Ok(value.to_owned())
}

fn invalid_ai_text_proposal(message: &'static str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error("invalid_ai_text_proposal", message)),
    )
        .into_response()
}

async fn approve_ai_action(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    body: Option<Json<Value>>,
) -> Response {
    forward_ai_action_review(&state, &user, &id, "approve", body.map(|Json(value)| value)).await
}

async fn reject_ai_action(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    body: Option<Json<Value>>,
) -> Response {
    forward_ai_action_review(&state, &user, &id, "reject", body.map(|Json(value)| value)).await
}

async fn forward_ai_action_review(
    state: &AppState,
    user: &AuthenticatedUser,
    id: &str,
    decision: &str,
    body: Option<Value>,
) -> Response {
    let url = format!(
        "{}/api/v1/ai-actions/{}/{}",
        state.conversation_core_url,
        urlencoding::encode(id),
        decision
    );
    proxy_conversation_json(state, Method::POST, &url, body, user, None)
        .await
        .into_response()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        bounded_evidence_message_ids, bounded_optional_string, bounded_required_string,
        normalize_ticket_update_fields, proposal_group_id,
    };

    #[test]
    fn proposal_group_id_allows_only_a_bounded_opaque_correlation() {
        assert_eq!(
            proposal_group_id(&json!({ "proposal_group_id": "resolution_abc-123" }))
                .expect("valid opaque correlation"),
            "resolution_abc-123"
        );
        assert_eq!(
            proposal_group_id(&json!({ "proposal_group_id": "   " }))
                .expect("blank correlation is optional"),
            ""
        );
        assert!(proposal_group_id(&json!({ "proposal_group_id": "resolution plan" })).is_err());
        assert!(proposal_group_id(&json!({ "proposal_group_id": 7 })).is_err());
    }

    #[test]
    fn incident_proposal_values_are_bounded_before_they_reach_core() {
        let proposal = json!({
            "title": " Checkout failures ",
            "evidence_message_ids": ["message-1", "message-2"],
        });
        assert_eq!(
            bounded_required_string(&proposal, "title", 300).as_deref(),
            Some("Checkout failures")
        );
        assert_eq!(
            bounded_evidence_message_ids(&proposal)
                .expect("bounded evidence")
                .len(),
            2
        );
        assert!(bounded_evidence_message_ids(&json!({
            "evidence_message_ids": [" "]
        }))
        .is_err());
    }

    #[test]
    fn problem_candidate_may_omit_root_cause_but_never_exceeds_its_boundary() {
        let proposal = json!({ "root_cause": "   " });
        assert_eq!(
            bounded_optional_string(&proposal, "root_cause", 2_000).as_deref(),
            Some("")
        );
        assert!(
            bounded_optional_string(&json!({ "root_cause": 7 }), "root_cause", 2_000).is_none()
        );
    }

    #[test]
    fn ticket_update_accepts_reviewable_status_but_rejects_terminal_lifecycle() {
        let fields = json!({
            "category": "delivery",
            "intent": "carrier_follow_up",
            "work_type": "customer_case",
            "priority": "high",
            "severity": "medium",
            "status": "waiting_customer"
        });
        let normalized = normalize_ticket_update_fields(fields.as_object().expect("object fields"))
            .expect("reviewable status is accepted");
        assert_eq!(normalized.get("status"), Some(&json!("waiting_customer")));

        let routed = json!({
            "priority": "high",
            "team_id": "team_delivery",
            "team_name": "Delivery"
        });
        let normalized = normalize_ticket_update_fields(routed.as_object().expect("object fields"))
            .expect("a paired routing proposal is accepted for review");
        assert_eq!(normalized.get("team_id"), Some(&json!("team_delivery")));

        let partial_route = json!({ "team_id": "team_delivery" });
        assert_eq!(
            normalize_ticket_update_fields(partial_route.as_object().expect("object fields")),
            Err("ticket.update routing needs both team_id and team_name")
        );

        let terminal = json!({ "status": "resolved" });
        assert_eq!(
            normalize_ticket_update_fields(terminal.as_object().expect("object fields")),
            Err("ticket.update status is not reviewable")
        );
    }
}
