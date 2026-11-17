//! Event trigger routes (Wave 4 EP-7, F-t3-047, slice W4-F T7).
//!
//! - `GET /v1/event-triggers?space_ref=`: the caller's own triggers, newest first, optionally of one Space.
//! - `POST /v1/event-triggers`: record a trigger for the caller, who becomes its member (201).
//! - `GET /v1/event-triggers/:trigger_id`: one trigger the caller owns, or 404 `event_trigger_not_found`.
//! - `PATCH /v1/event-triggers/:trigger_id`: change the fields the body names in `update_paths`, under a
//!   compare-and-set on the revision the caller read.
//! - `POST /v1/event-triggers/:trigger_id/dry-run`: would a fire of this trigger wake on one stored webhook event,
//!   and why. Proxied to capability-core (W4-T6) under the caller's capability bearer; nothing is recorded or fired.
//!
//! Session Core owns the record and decides who may read or change it, so the gateway forwards the verified session
//! bearer and the claims' org only, never an org, owner or member from a path or body. A key or service principal is
//! refused before anything is called, because a trigger belongs to one person. A zero-retention or waived caller is
//! refused before Session Core is asked: the record is a new durable store, and the dry run reads a stored payload.
//! A Session Core refusal that carries a zero-retention catalogue code is answered with that body and 403. Every route
//! answers 404 `event_triggers_disabled` while `MODEL_GATEWAY_EVENT_TRIGGERS` is not the literal `true`, so the SPA
//! hides the feature.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use mp_contracts::model_plane::v1::{
    CreateEventTriggerRequest, EventTrigger, EventTriggerResponse, GetEventTriggerRequest,
    ListEventTriggersRequest, UpdateEventTriggerRequest,
};
use mp_contracts::zdr_refusal::Refusal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::{Claims, VerifiedCapabilityBearer, VerifiedSessionBearer};
use crate::http_routes::{
    authenticated_session_request, proxy_to_capability_core, timestamp_to_rfc3339, HttpJsonError,
};
use crate::state::AppState;
use crate::zdr_posture::{Route, TurnPosture};

/// The gateway's switch for event triggers. Opt-in: only the literal `true` enables it. Off, every route answers 404.
pub const ENABLED_ENV: &str = "MODEL_GATEWAY_EVENT_TRIGGERS";

/// What this gateway and Session Core answer while the store is switched off.
const STORE_DISABLED: &str = "event_triggers_disabled";

/// Session Core's refusals a caller can act on, matched on the message the gRPC status carries.
const INVALID_TRIGGER: &str = "invalid_event_trigger";
const REVISION_CONFLICT: &str = "event_trigger_revision_conflict";
const TRIGGER_CLOSED: &str = "event_trigger_closed";
const SCHEDULE_MISMATCH: &str = "trigger_schedule_mismatch";

/// The dry run's refusal for a zero-retention or waived caller, named as capability-core names it.
const DRY_RUN_ZDR_CODE: &str = "event_trigger_dry_run_under_zdr_refused";

/// The longest id or space reference the gateway checks, in characters: the store's own bound for an id.
const MAX_REF_CHARS: usize = 128;

/// Whether event triggers are on in this process. Read at each call, as the other turn switches are.
#[must_use]
pub fn enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The routes, merged into the authed router beside `thread_goal_routes::router()`.
pub(crate) fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/v1/event-triggers",
            axum::routing::get(list_event_triggers).post(create_event_trigger),
        )
        .route(
            "/v1/event-triggers/:trigger_id",
            axum::routing::get(get_event_trigger).patch(update_event_trigger),
        )
        .route(
            "/v1/event-triggers/:trigger_id/dry-run",
            axum::routing::post(dry_run_event_trigger),
        )
}

/// The trigger as the caller reads it. The org and member ids are not sent: the member is the only reader.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TriggerJson {
    id: String,
    space_ref: String,
    connection_id: String,
    event_type: String,
    name: String,
    clock_enabled: bool,
    status: String,
    revision: u64,
    schedule_id: Option<String>,
    wake_filter_json: Option<String>,
    created_at: String,
    updated_at: String,
}

impl TriggerJson {
    fn from_proto(trigger: EventTrigger) -> Self {
        Self {
            id: trigger.id,
            space_ref: trigger.space_ref,
            connection_id: trigger.connection_id,
            event_type: trigger.event_type,
            name: trigger.name,
            clock_enabled: trigger.clock_enabled,
            status: trigger.status,
            revision: trigger.revision,
            schedule_id: non_empty(trigger.schedule_id),
            wake_filter_json: non_empty(trigger.wake_filter_json),
            created_at: timestamp_to_rfc3339(trigger.created_at),
            updated_at: timestamp_to_rfc3339(trigger.updated_at),
        }
    }
}

/// `Some` of a text the store leaves empty for "none", so a client reads the absence as `null`.
fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

/// The query of `GET /v1/event-triggers`.
#[derive(Debug, Default, Deserialize)]
struct ListQuery {
    space_ref: Option<String>,
}

/// The body of `POST /v1/event-triggers`. Every field is optional on the wire; the store refuses what it cannot take.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CreateBody {
    #[serde(alias = "spaceRef")]
    space_ref: String,
    #[serde(alias = "connectionId")]
    connection_id: String,
    #[serde(alias = "eventType")]
    event_type: String,
    name: String,
    #[serde(alias = "clockEnabled")]
    clock_enabled: bool,
    #[serde(alias = "scheduleId")]
    schedule_id: String,
    #[serde(alias = "wakeFilterJson")]
    wake_filter_json: String,
}

impl CreateBody {
    fn into_request(self, org_id: String) -> CreateEventTriggerRequest {
        CreateEventTriggerRequest {
            org_id,
            space_ref: self.space_ref,
            connection_id: self.connection_id,
            event_type: self.event_type,
            name: self.name,
            clock_enabled: self.clock_enabled,
            schedule_id: self.schedule_id,
            wake_filter_json: self.wake_filter_json,
        }
    }
}

/// The body of `PATCH /v1/event-triggers/:trigger_id`: the revision the caller read, the fields it names and their
/// values. A value its `update_paths` leave out is ignored by Session Core.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UpdateBody {
    #[serde(alias = "expectedRevision")]
    expected_revision: u64,
    #[serde(alias = "updatePaths")]
    update_paths: Vec<String>,
    name: String,
    #[serde(alias = "clockEnabled")]
    clock_enabled: bool,
    status: String,
    #[serde(alias = "scheduleId")]
    schedule_id: String,
    #[serde(alias = "wakeFilterJson")]
    wake_filter_json: String,
}

impl UpdateBody {
    fn into_request(self, org_id: String, trigger_id: String) -> UpdateEventTriggerRequest {
        UpdateEventTriggerRequest {
            org_id,
            trigger_id,
            expected_revision: self.expected_revision,
            update_paths: self.update_paths,
            name: self.name,
            clock_enabled: self.clock_enabled,
            status: self.status,
            schedule_id: self.schedule_id,
            wake_filter_json: self.wake_filter_json,
        }
    }
}

/// The body of `POST .../dry-run`: the one stored webhook event to dry run against.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DryRunBody {
    #[serde(alias = "webhookEventId")]
    webhook_event_id: String,
}

/// A typed error body; `details` is always an object.
fn typed(status: StatusCode, code: &str, message: &str) -> HttpJsonError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": {} } })),
    )
}

/// A zero-retention catalogue refusal as its own body (the typed envelope the ZDR contract names), with 403.
fn refused(refusal: &Refusal) -> HttpJsonError {
    (StatusCode::FORBIDDEN, Json(refusal.json_body()))
}

/// The one answer a trigger the caller does not own gets, and a missing one too: the id confirms nothing.
fn not_found() -> HttpJsonError {
    typed(
        StatusCode::NOT_FOUND,
        "event_trigger_not_found",
        "the event trigger was not found",
    )
}

/// The switch first: a deployment without the feature answers the same for every caller.
fn switch_gate() -> Result<(), HttpJsonError> {
    if enabled() {
        Ok(())
    } else {
        Err(typed(
            StatusCode::NOT_FOUND,
            STORE_DISABLED,
            "event triggers are not enabled on this deployment",
        ))
    }
}

/// A trigger belongs to one person. A key or a service principal owns none, so it is refused before anything is read.
fn member_gate(claims: &Claims) -> Result<(), HttpJsonError> {
    let person = claims.principal_type.as_deref().unwrap_or("user") == "user"
        && claims.service_id.is_none()
        && claims.key_id.is_none()
        && !claims.user_id.is_empty();
    if person {
        Ok(())
    } else {
        Err(typed(
            StatusCode::FORBIDDEN,
            "event_trigger_member_only",
            "an event trigger belongs to its member",
        ))
    }
}

/// A zero-retention or waived caller is refused before Session Core is asked: the record is a new durable store, and
/// Session Core refuses such a credential on every trigger RPC.
fn retention_gate(claims: &Claims) -> Result<(), HttpJsonError> {
    match crate::zdr_posture::decide_for(claims, &Route::plain(false)) {
        Ok(TurnPosture::Persistent) => Ok(()),
        Ok(TurnPosture::Zdr { .. } | TurnPosture::Waived { .. }) => {
            Err(refused(&Refusal::DURABLE_MUTATION_FORBIDDEN))
        }
        Err(refusal) => Err(refused(&refusal)),
    }
}

/// The dry run reads a stored webhook payload, so only a persistent caller may make one. Any other is refused with
/// capability-core's own code for it, before any read.
fn dry_run_gate(claims: &Claims) -> Result<(), HttpJsonError> {
    match crate::zdr_posture::decide_for(claims, &Route::plain(false)) {
        Ok(TurnPosture::Persistent) => Ok(()),
        _ => Err(typed(
            StatusCode::FORBIDDEN,
            DRY_RUN_ZDR_CODE,
            "event trigger dry runs are not available under zero data retention",
        )),
    }
}

/// A trigger id as the store names it: `trg_` and a ULID, so letters, digits, `_` and `-` only. Any other text names no
/// trigger, and it is refused before it can reach an upstream path.
fn checked_trigger_id(raw: &str) -> Result<String, HttpJsonError> {
    let id = raw.trim();
    let url_safe = !id.is_empty()
        && id.len() <= MAX_REF_CHARS
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if url_safe {
        Ok(id.to_owned())
    } else {
        Err(not_found())
    }
}

/// The trigger a call answered, or the 404 a trigger the caller does not own gets.
fn found_trigger(response: EventTriggerResponse) -> Result<EventTrigger, HttpJsonError> {
    let EventTriggerResponse { trigger, found } = response;
    match trigger {
        Some(trigger) if found => Ok(trigger),
        _ => Err(not_found()),
    }
}

/// `{ data: { trigger } }`, the one shape every trigger route answers with a trigger.
fn trigger_body(trigger: EventTrigger) -> Value {
    json!({ "data": { "trigger": TriggerJson::from_proto(trigger) } })
}

/// Session Core's refusal as the caller reads it. A zero-retention refusal keeps its catalogue body; the store's own
/// refusals are typed by their message; anything else is a failure of the call, not a fact about the trigger.
fn trigger_error(error: &tonic::Status) -> HttpJsonError {
    if let Some(refusal) = Refusal::from_status(error) {
        return refused(&refusal);
    }
    let message = error.message();
    match error.code() {
        tonic::Code::Unavailable if message == STORE_DISABLED => typed(
            StatusCode::NOT_FOUND,
            STORE_DISABLED,
            "event triggers are not enabled on this deployment",
        ),
        tonic::Code::Unavailable => typed(
            StatusCode::SERVICE_UNAVAILABLE,
            "event_triggers_unavailable",
            "the event trigger store could not be reached now",
        ),
        tonic::Code::NotFound => not_found(),
        tonic::Code::PermissionDenied => typed(
            StatusCode::FORBIDDEN,
            "event_trigger_member_only",
            "an event trigger belongs to its member",
        ),
        tonic::Code::Unauthenticated => typed(
            StatusCode::UNAUTHORIZED,
            "unauthenticated",
            "the session credential was not accepted",
        ),
        tonic::Code::Aborted if message == REVISION_CONFLICT => typed(
            StatusCode::CONFLICT,
            "event_trigger_revision_conflict",
            "the trigger changed since it was read",
        ),
        tonic::Code::FailedPrecondition if message == TRIGGER_CLOSED => typed(
            StatusCode::CONFLICT,
            "event_trigger_closed",
            "a disabled trigger cannot be changed",
        ),
        tonic::Code::InvalidArgument if message == SCHEDULE_MISMATCH => typed(
            StatusCode::UNPROCESSABLE_ENTITY,
            "trigger_schedule_mismatch",
            "that schedule cannot be bound to this trigger",
        ),
        tonic::Code::InvalidArgument if message.starts_with(INVALID_TRIGGER) => typed(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_event_trigger",
            message
                .strip_prefix("invalid_event_trigger: ")
                .unwrap_or(message),
        ),
        _ => {
            tracing::warn!(code = ?error.code(), "event trigger call failed");
            typed(
                StatusCode::INTERNAL_SERVER_ERROR,
                "event_trigger_failed",
                "the event trigger could not be served",
            )
        }
    }
}

/// `GET /v1/event-triggers?space_ref=`: the caller's own triggers, newest first, optionally of one Space.
async fn list_event_triggers(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    switch_gate()?;
    member_gate(&claims)?;
    retention_gate(&claims)?;
    let space_ref = query.space_ref.unwrap_or_default().trim().to_owned();
    if space_ref.chars().count() > MAX_REF_CHARS {
        return Err(typed(
            StatusCode::UNPROCESSABLE_ENTITY,
            INVALID_TRIGGER,
            "space_ref is at most 128 characters",
        ));
    }
    let response = state
        .event_trigger_client
        .clone()
        .list_event_triggers(authenticated_session_request(
            ListEventTriggersRequest {
                org_id: claims.org_id.clone(),
                space_ref,
            },
            &bearer,
        )?)
        .await
        .map_err(|error| trigger_error(&error))?
        .into_inner();
    let triggers: Vec<TriggerJson> = response
        .triggers
        .into_iter()
        .map(TriggerJson::from_proto)
        .collect();
    Ok(Json(json!({ "data": { "triggers": triggers } })))
}

/// `POST /v1/event-triggers`: record a trigger for the caller, active. Answers 201 with the trigger.
async fn create_event_trigger(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Json(body): Json<CreateBody>,
) -> Result<(StatusCode, Json<Value>), HttpJsonError> {
    switch_gate()?;
    member_gate(&claims)?;
    retention_gate(&claims)?;
    let response = state
        .event_trigger_client
        .clone()
        .create_event_trigger(authenticated_session_request(
            body.into_request(claims.org_id.clone()),
            &bearer,
        )?)
        .await
        .map_err(|error| trigger_error(&error))?
        .into_inner();
    Ok((
        StatusCode::CREATED,
        Json(trigger_body(found_trigger(response)?)),
    ))
}

/// `GET /v1/event-triggers/:trigger_id`: one trigger the caller owns.
async fn get_event_trigger(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path(trigger_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    switch_gate()?;
    member_gate(&claims)?;
    retention_gate(&claims)?;
    let trigger_id = checked_trigger_id(&trigger_id)?;
    let response = state
        .event_trigger_client
        .clone()
        .get_event_trigger(authenticated_session_request(
            GetEventTriggerRequest {
                org_id: claims.org_id.clone(),
                trigger_id,
            },
            &bearer,
        )?)
        .await
        .map_err(|error| trigger_error(&error))?
        .into_inner();
    Ok(Json(trigger_body(found_trigger(response)?)))
}

/// `PATCH /v1/event-triggers/:trigger_id`: change the fields named in `update_paths`, under the revision the caller read.
async fn update_event_trigger(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path(trigger_id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<Value>, HttpJsonError> {
    switch_gate()?;
    member_gate(&claims)?;
    retention_gate(&claims)?;
    let trigger_id = checked_trigger_id(&trigger_id)?;
    if body.expected_revision == 0 {
        return Err(typed(
            StatusCode::UNPROCESSABLE_ENTITY,
            INVALID_TRIGGER,
            "expected_revision is the revision the trigger was read at, 1 or more",
        ));
    }
    if body.update_paths.is_empty() {
        return Err(typed(
            StatusCode::UNPROCESSABLE_ENTITY,
            INVALID_TRIGGER,
            "update_paths names no field",
        ));
    }
    let response = state
        .event_trigger_client
        .clone()
        .update_event_trigger(authenticated_session_request(
            body.into_request(claims.org_id.clone(), trigger_id),
            &bearer,
        )?)
        .await
        .map_err(|error| trigger_error(&error))?
        .into_inner();
    Ok(Json(trigger_body(found_trigger(response)?)))
}

/// `POST /v1/event-triggers/:trigger_id/dry-run`: would a fire of this trigger wake on one stored webhook event. Only the
/// webhook event id is forwarded, to capability-core under the caller's capability bearer; nothing is recorded or fired.
async fn dry_run_event_trigger(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedCapabilityBearer,
    Path(trigger_id): Path<String>,
    Json(body): Json<DryRunBody>,
) -> Result<Json<Value>, HttpJsonError> {
    switch_gate()?;
    member_gate(&claims)?;
    dry_run_gate(&claims)?;
    let trigger_id = checked_trigger_id(&trigger_id)?;
    let path = format!("event-triggers/{trigger_id}/dry-run");
    let upstream = json!({ "webhook_event_id": body.webhook_event_id.trim() });
    proxy_to_capability_core(&state, &claims, &bearer, &path, "POST", Some(&upstream)).await
}
