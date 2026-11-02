//! Reminders: the BFF half of WS11-S11.
//!
//! * `POST /api/v1/spaces/{space_ref}/reminders {thread_id, message_id?, remind_at, idempotency_key?}`
//!   creates a reminder for the CALLER on one thread (and optionally one message) of a room they
//!   belong to: `201 {data:{reminder_id, remind_at}}`, or `200` with `deduplicated: true` when an
//!   identical pending reminder already exists.
//! * `GET /api/v1/spaces/reminders?state=pending` lists the caller's own pending reminders.
//! * `DELETE /api/v1/spaces/reminders/{reminder_id}` cancels one of the caller's own pending
//!   reminders. Another member's reminder answers exactly like one that does not exist.
//! * `spaces.create_reminder` and `spaces.cancel_reminder` ride [`create_reminder_core`] and
//!   [`cancel_reminder_core`], the same functions as the routes.
//!
//! Content. A reminder row holds ids, a closed state word and timestamps only; there is no note
//! field (a note would be user content). Creating one is still a durable write, so under Zero Data
//! Retention it is the typed 412 before any Application call. Cancelling removes data and listing
//! reads only the caller's own rows, so both stay allowed. Membership of the room is proved here
//! (Control's live role) before the write: Application only checks that the Space exists in the
//! caller's organization. At fire time Application re-checks the audience again.
//!
//! The flag (`SPACE_REMINDERS_API`, `space_reminders_api`) is a rollout control, never an
//! authorization input. Flag off answers 404 `space_reminders_disabled` on every route.
//!
//! Errors are `{error:{code,message,details}}`: 400 `invalid_request` / `invalid_space` /
//! `invalid_thread` / `invalid_message` / `invalid_state` / `invalid_reminder_time`, 403
//! `active_membership_required`, 404 `space_reminders_disabled` / `space_not_found` /
//! `reminder_not_found`, 412 `zdr_durable_mutation_forbidden`, 429 `too_many_reminders` (above 50
//! pending), 503 `reminders_unavailable` / `zdr_decision_unavailable`.

use std::collections::HashMap;

use axum::{
    extract::{rejection::JsonRejection, Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{
    convex_gateway_call, resolve_space_role, room_admin::convex_mutation_message,
};
use crate::{
    config::AppState,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
};

type Reply = (StatusCode, Json<Value>);

const MAX_ID_LEN: usize = 200;
/// Application caps pending reminders per person here too; the 429 detail reports it.
const MAX_PENDING: u32 = 50;

/// Registered through `module_routes()` behind `require_session`. The `reminders` segments are
/// static, which matchit ranks above `{space_ref}`, so a room can never shadow them.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/spaces/reminders", get(list_reminders))
        .route(
            "/api/v1/spaces/reminders/{reminder_id}",
            delete(cancel_reminder),
        )
        .route("/api/v1/spaces/{space_ref}/reminders", post(create_reminder))
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn disabled() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "space_reminders_disabled",
        "Space reminders are not enabled.",
    )
}

fn membership_required() -> Reply {
    refuse(
        StatusCode::FORBIDDEN,
        "active_membership_required",
        "An active organization and room membership is required to use reminders.",
    )
}

fn unavailable() -> Reply {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "reminders_unavailable",
        "Reminders are unavailable right now. Nothing was changed.",
    )
}

fn invalid_request() -> Reply {
    refuse(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "The request body is not valid.",
    )
}

fn invalid_time() -> Reply {
    refuse(
        StatusCode::BAD_REQUEST,
        "invalid_reminder_time",
        "remind_at must be a time between five seconds and 366 days from now.",
    )
}

fn not_found() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "reminder_not_found",
        "That reminder was not found.",
    )
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_ID_LEN && !value.chars().any(char::is_control)
}

/// Flag, then organization. Everything every route shares.
async fn gate(state: &AppState, user: &AuthenticatedUser) -> Result<String, Reply> {
    if !state.space_features.reminders_api {
        return Err(disabled());
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err(membership_required());
    }
    Ok(org_id)
}

/// The wire shape of one reminder.
fn reminder_json(value: &Value) -> Value {
    json!({
        "reminder_id": value.get("reminderId").cloned().unwrap_or(Value::Null),
        "space_ref": value.get("spaceRef").cloned().unwrap_or(Value::Null),
        "thread_id": value.get("threadId").cloned().unwrap_or(Value::Null),
        "message_id": value.get("messageId").cloned().unwrap_or(Value::Null),
        "remind_at": value.get("remindAt").cloned().unwrap_or(Value::Null),
        "state": value.get("state").cloned().unwrap_or(Value::Null),
        "created_at": value.get("createdAt").cloned().unwrap_or(Value::Null),
    })
}

// ── create ───────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateBody {
    thread_id: String,
    message_id: Option<String>,
    /// Epoch milliseconds; Application bounds it to five seconds..366 days ahead.
    remind_at: i64,
    /// Accepted for the contract; Application deduplicates on the natural key (room, thread,
    /// message, time), so an identical retry returns the same reminder either way.
    #[allow(dead_code)]
    idempotency_key: Option<String>,
}

async fn create_reminder(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Reply {
    let Ok(Json(body)) = body else {
        return invalid_request();
    };
    create_reminder_core(&state, &user, Some(&headers), &space_ref, &body).await
}

/// The reminder write, shared by the route and the `spaces.create_reminder` action. Flag, then
/// organization, then ids, then the caller's LIVE membership of the room (Control), then the Zero
/// Data Retention refusal, then the Application call.
pub(crate) async fn create_reminder_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: Option<&HeaderMap>,
    space_ref: &str,
    body: &Value,
) -> Reply {
    let org_id = match gate(state, user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    let Ok(body) = serde_json::from_value::<CreateBody>(body.clone()) else {
        return invalid_request();
    };
    let space_ref = space_ref.trim();
    if !valid_id(space_ref) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A Space reference is required.",
        );
    }
    let thread_id = body.thread_id.trim();
    if !valid_id(thread_id) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_thread",
            "A thread id is required.",
        );
    }
    let message_id = body
        .message_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    if message_id.is_some_and(|id| !valid_id(id)) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_message",
            "The message id is not valid.",
        );
    }
    match resolve_space_role(state, user, &org_id, space_ref).await {
        Ok(role) if !role.trim().is_empty() => {}
        Ok(_) => return membership_required(),
        Err(reply) => return reply,
    }
    if let Some(refusal) = crate::zdr::durable_write_refusal(state, user, headers, None).await {
        return refusal;
    }
    let mut args = json!({
        "externalAuthId": user.user_id,
        "externalOrgId": org_id,
        "spaceRef": space_ref,
        "threadId": thread_id,
        "remindAt": body.remind_at,
    });
    if let (Some(id), Some(object)) = (message_id, args.as_object_mut()) {
        object.insert("messageId".to_owned(), json!(id));
    }
    match convex_mutation_message(state, "spaceReminders:createReminderForGateway", args).await {
        Ok(value) => {
            let deduplicated = value
                .get("deduplicated")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let status = if deduplicated {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            };
            (
                status,
                Json(json!({"data": {
                    "reminder_id": value.get("reminderId").cloned().unwrap_or(Value::Null),
                    "remind_at": value.get("remindAt").cloned().unwrap_or(Value::Null),
                    "deduplicated": deduplicated,
                }})),
            )
        }
        Err(Some(message)) => map_create_failure(&message),
        Err(None) => unavailable(),
    }
}

/// Maps Application's stable mutation messages to the typed browser errors.
fn map_create_failure(message: &str) -> Reply {
    if message.contains("invalid_reminder_time") {
        invalid_time()
    } else if message.contains("too_many_reminders") {
        (
            StatusCode::TOO_MANY_REQUESTS,
            Json(error_with_details(
                "too_many_reminders",
                "You have the most pending reminders allowed. Cancel one to add another.",
                json!({"max_pending": MAX_PENDING}),
            )),
        )
    } else if message.contains("space_not_found") {
        refuse(
            StatusCode::NOT_FOUND,
            "space_not_found",
            "That room was not found.",
        )
    } else if message.contains("invalid_space_ref") {
        refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A Space reference is required.",
        )
    } else if message.contains("invalid_thread_id") {
        refuse(
            StatusCode::BAD_REQUEST,
            "invalid_thread",
            "A thread id is required.",
        )
    } else if message.contains("invalid_message_id") {
        refuse(
            StatusCode::BAD_REQUEST,
            "invalid_message",
            "The message id is not valid.",
        )
    } else {
        unavailable()
    }
}

// ── list ─────────────────────────────────────────────────────────────────────────────────────────

async fn list_reminders(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Reply {
    let org_id = match gate(&state, &user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    // Only pending reminders are stored as live rows; settled ones are healed away.
    if params.get("state").is_some_and(|word| word != "pending") {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_state",
            "state must be pending.",
        );
    }
    match convex_gateway_call(
        &state,
        "query",
        "spaceReminders:listPendingRemindersForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
        }),
    )
    .await
    {
        Ok(value) => {
            let reminders: Vec<Value> = value
                .as_array()
                .map(|rows| rows.iter().map(reminder_json).collect())
                .unwrap_or_default();
            (StatusCode::OK, Json(json!({"data": {"reminders": reminders}})))
        }
        Err(()) => unavailable(),
    }
}

// ── cancel ───────────────────────────────────────────────────────────────────────────────────────

async fn cancel_reminder(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(reminder_id): Path<String>,
) -> Reply {
    cancel_reminder_core(&state, &user, &reminder_id).await
}

/// The cancel, shared by the route and the `spaces.cancel_reminder` action. Cancelling removes
/// data, so it is allowed under Zero Data Retention.
pub(crate) async fn cancel_reminder_core(
    state: &AppState,
    user: &AuthenticatedUser,
    reminder_id: &str,
) -> Reply {
    let org_id = match gate(state, user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    let reminder_id = reminder_id.trim();
    if !valid_id(reminder_id) {
        return not_found();
    }
    match convex_mutation_message(
        state,
        "spaceReminders:cancelReminderForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "reminderId": reminder_id,
        }),
    )
    .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"data": {"reminder_id": reminder_id, "cancelled": true}})),
        ),
        Err(Some(message)) if message.contains("reminder_not_found") => not_found(),
        Err(_) => unavailable(),
    }
}
