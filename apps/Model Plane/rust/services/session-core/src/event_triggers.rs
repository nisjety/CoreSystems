//! `EventTriggerService`: the event trigger record store (Wave 4 EP-7, F-t3-047, slice W4-F T1).
//!
//! A trigger binds one integration connection, one Space and one Space member, and names the
//! connector event type that may wake a budgeted run for that member. This slice stores the record
//! and nothing fires. A stored trigger is not authority: every later fire is re-authorised by Control
//! through `FireAuthorizer` (the cron rule, `capability-core/internal/cron/sweeper.go`). The consumer
//! of `webhook_received` and the fire path are in capability-core (`internal/eventtrigger`, W4-F T2).
//! The clock arm comes in a later slice; the wake gate reads the wake filter stored here (W4-F T5).
//!
//! A trigger may name one cron schedule, the scheduled run a fire starts (W4-F T3). The binding is checked
//! here against that schedule's org, Space, creator and consent contract, and grants nothing: every fire is
//! re-authorised by Control, as before.
//!
//! A trigger may also carry a wake filter (W4-F T5): a closed, version 1 predicate over the webhook event a
//! fire is for. This store checks it against the closed schema and keeps it in canonical form. capability-core
//! evaluates it before a fire is decided, and it grants nothing either.
//!
//! The record is the member's own configuration. Only that member reads or changes it, a service
//! caller owns none and is refused, and a zero-retention or waived credential is refused on every RPC
//! (`DURABLE_MUTATION_FORBIDDEN`, DEC-32), so no ZDR content reaches the table. A member may pause,
//! resume or disable a trigger; "disabled" is final. The switch is `EVENT_TRIGGERS_ENABLED`, default
//! off; while it is off every RPC answers `unavailable` `event_triggers_disabled`.

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{
    self as pb, event_trigger_service_server::EventTriggerService,
};
use mp_ids::new_ulid;
use sqlx::PgPool;
use std::time::Instant;
use tonic::{Request, Response, Status};

use crate::auth::{authorize_operation, identity, VerifiedIdentity};
use crate::grpc::{record_metrics, to_proto_timestamp};

/// The switch. Only the literal `true` (case-insensitive) turns the store on.
pub(crate) const ENABLED_ENV: &str = "EVENT_TRIGGERS_ENABLED";
const DISABLED: &str = "event_triggers_disabled";
/// The refusal codes a caller can act on. Strings, as the other session-core refusals are.
pub(crate) const INVALID_TRIGGER: &str = "invalid_event_trigger";
pub(crate) const REVISION_CONFLICT: &str = "event_trigger_revision_conflict";
pub(crate) const TRIGGER_CLOSED: &str = "event_trigger_closed";
/// A schedule a trigger may not be bound to: not the member's own live schedule in the trigger's org and Space,
/// or one with no approval contract. Refused as `InvalidArgument` with this token.
pub(crate) const SCHEDULE_MISMATCH: &str = "trigger_schedule_mismatch";
/// The one connector event type a trigger may name in this slice (the table's CHECK).
pub(crate) const EVENT_TYPE_WEBHOOK_RECEIVED: &str = "webhook_received";
/// The longest label, in characters (the table's CHECK).
const MAX_NAME_CHARS: usize = 120;
/// The longest org, Space or connection id the store keeps, in characters (the table's CHECKs).
const MAX_REF_CHARS: usize = 128;
/// The most triggers one listing returns, newest first.
const LIST_LIMIT: i64 = 200;
/// The fields an update may name, in `UpdateEventTriggerRequest`'s words.
const UPDATE_PATHS: [&str; 5] = [
    "name",
    "clock_enabled",
    "status",
    "schedule_id",
    "wake_filter_json",
];
/// The longest wake filter, in bytes, as a member writes it. Postgres re-spaces the stored JSON, so the stored text can
/// be up to twice this long; the migration's CHECK bounds it at 16 KiB.
const MAX_WAKE_FILTER_BYTES: usize = 4096;
/// The one wake filter schema version this store writes and accepts.
const WAKE_FILTER_VERSION: u64 = 1;
/// The most entries `event_types` and `providers` may hold.
const MAX_WAKE_TOKENS: usize = 10;
/// The most entries `subject_contains` may hold.
const MAX_WAKE_SUBJECTS: usize = 5;
/// The most `field_equals` entries a filter may hold.
const MAX_WAKE_FIELDS: usize = 5;
/// The most segments in a `field_equals` path.
const MAX_WAKE_PATH_SEGMENTS: usize = 5;
/// The longest event type, provider or subject needle, in characters.
const MAX_WAKE_TOKEN_CHARS: usize = 100;
/// The longest `field_equals` value, in characters.
const MAX_WAKE_VALUE_CHARS: usize = 200;

/// Whether the store answers. Read where it is used, with the parser the other switches use.
pub(crate) fn event_triggers_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The trigger RPCs. They hold the pool and nothing else; the rules are in this module.
pub struct EventTriggerGrpc {
    pub(crate) pool: PgPool,
}

#[tonic::async_trait]
impl EventTriggerService for EventTriggerGrpc {
    async fn create_event_trigger(
        &self,
        request: Request<pb::CreateEventTriggerRequest>,
    ) -> Result<Response<pb::EventTriggerResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            // `session:write` refuses a zero-retention credential before anything is read.
            authorize_operation(&caller, "session:write")?;
            refuse_retention_exempt(&caller)?;
            if !event_triggers_enabled() {
                return Err(Status::unavailable(DISABLED));
            }
            create_event_trigger(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("create_event_trigger", started, result.is_ok());
        result
    }

    async fn get_event_trigger(
        &self,
        request: Request<pb::GetEventTriggerRequest>,
    ) -> Result<Response<pb::EventTriggerResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            refuse_retention_exempt(&caller)?;
            authorize_operation(&caller, "session:read")?;
            if !event_triggers_enabled() {
                return Err(Status::unavailable(DISABLED));
            }
            get_event_trigger(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("get_event_trigger", started, result.is_ok());
        result
    }

    async fn list_event_triggers(
        &self,
        request: Request<pb::ListEventTriggersRequest>,
    ) -> Result<Response<pb::ListEventTriggersResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            refuse_retention_exempt(&caller)?;
            authorize_operation(&caller, "session:read")?;
            if !event_triggers_enabled() {
                return Err(Status::unavailable(DISABLED));
            }
            list_event_triggers(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("list_event_triggers", started, result.is_ok());
        result
    }

    async fn update_event_trigger(
        &self,
        request: Request<pb::UpdateEventTriggerRequest>,
    ) -> Result<Response<pb::EventTriggerResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            refuse_retention_exempt(&caller)?;
            if !event_triggers_enabled() {
                return Err(Status::unavailable(DISABLED));
            }
            update_event_trigger(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("update_event_trigger", started, result.is_ok());
        result
    }
}

/// A zero-retention or waived credential never reaches the store. `authorize_operation` lets a ZDR
/// credential read, and a waived bearer's `zdr` claim is false, so both are named here (DEC-32).
fn refuse_retention_exempt(caller: &VerifiedIdentity) -> Result<(), Status> {
    if caller.zdr() || caller.zdr_waiver().is_some() {
        return Err(
            mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN
                .grpc_status(tonic::Code::FailedPrecondition),
        );
    }
    Ok(())
}

/// The member a trigger belongs to: the human caller. A service principal owns no trigger, so it is
/// refused before anything is read.
fn member_of(caller: &VerifiedIdentity) -> Result<&str, Status> {
    caller
        .user_id()
        .filter(|user| !caller.is_service() && !user.trim().is_empty())
        .ok_or_else(|| Status::permission_denied("an event trigger belongs to its member"))
}

fn invalid(detail: &str) -> Status {
    Status::invalid_argument(format!("{INVALID_TRIGGER}: {detail}"))
}

/// A text field trimmed and checked to be 1 to `max_chars` characters.
fn bounded_text(raw: &str, max_chars: usize, field: &str) -> Result<String, Status> {
    let value = raw.trim();
    let chars = value.chars().count();
    if chars == 0 || chars > max_chars {
        return Err(invalid(&format!(
            "{field} must be 1 to {max_chars} characters"
        )));
    }
    Ok(value.to_owned())
}

/// An optional schedule id: empty means no binding, otherwise 1 to `MAX_REF_CHARS` characters after trimming.
fn optional_schedule(raw: &str) -> Result<Option<String>, Status> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    bounded_text(raw, MAX_REF_CHARS, "schedule_id").map(Some)
}

/// The closed wake filter schema, version 1, as a member writes it. `deny_unknown_fields` refuses every other key.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct WakeFilter {
    version: u64,
    #[serde(default)]
    event_types: Vec<String>,
    #[serde(default)]
    providers: Vec<String>,
    #[serde(default)]
    field_equals: Vec<FieldEquals>,
    #[serde(default)]
    subject_contains: Vec<String>,
}

/// One `field_equals` entry: the string at the dotted `path` must equal `value` exactly.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct FieldEquals {
    path: String,
    value: String,
}

/// A wake filter as the member wrote it, checked against the closed schema and returned as canonical JSON. An empty
/// value is no filter (NULL), as an empty schedule id is no binding. Every bound is checked here, so the gate only
/// ever reads a filter the schema allows.
fn optional_wake_filter(raw: &str) -> Result<Option<serde_json::Value>, Status> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    if raw.len() > MAX_WAKE_FILTER_BYTES {
        return Err(invalid("wake_filter is larger than 4 KiB"));
    }
    // The parser's own message is not returned: it can quote the member's text, and the refusal names the schema only.
    let filter: WakeFilter = serde_json::from_str(raw)
        .map_err(|_| invalid("wake_filter must be a JSON object in the closed version 1 schema"))?;
    let canonical = canonical_wake_filter(filter)?;
    serde_json::to_value(canonical)
        .map(Some)
        .map_err(|_| Status::internal("wake_filter could not be encoded"))
}

/// Checks each bound of the closed schema and rebuilds the filter in canonical form: strings trimmed, providers in
/// lowercase (the form the Ingestion Plane stores them in), and every list in the order the member wrote it.
fn canonical_wake_filter(filter: WakeFilter) -> Result<WakeFilter, Status> {
    if filter.version != WAKE_FILTER_VERSION {
        return Err(invalid("wake_filter version must be 1"));
    }
    if filter.field_equals.len() > MAX_WAKE_FIELDS {
        return Err(invalid(&format!(
            "wake_filter field_equals allows at most {MAX_WAKE_FIELDS} entries"
        )));
    }
    for entry in &filter.field_equals {
        check_wake_path(&entry.path)?;
        if entry.value.chars().count() > MAX_WAKE_VALUE_CHARS {
            return Err(invalid(&format!(
                "wake_filter field_equals values are at most {MAX_WAKE_VALUE_CHARS} characters"
            )));
        }
    }
    let providers: Vec<String> =
        canonical_wake_tokens(filter.providers, MAX_WAKE_TOKENS, "providers")?
            .into_iter()
            .map(|provider| provider.to_ascii_lowercase())
            .collect();
    Ok(WakeFilter {
        version: WAKE_FILTER_VERSION,
        event_types: canonical_wake_tokens(filter.event_types, MAX_WAKE_TOKENS, "event_types")?,
        providers,
        field_equals: filter.field_equals,
        subject_contains: canonical_wake_tokens(
            filter.subject_contains,
            MAX_WAKE_SUBJECTS,
            "subject_contains",
        )?,
    })
}

/// A `field_equals` path: one to five dot-separated segments, each made of lowercase letters, digits and underscores.
/// Only such segments can be named, so the reserved, camel-case keys the gate sets are never addressable.
fn check_wake_path(path: &str) -> Result<(), Status> {
    let segments: Vec<&str> = path.split('.').collect();
    let well_formed = segments.len() <= MAX_WAKE_PATH_SEGMENTS
        && segments.iter().all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        });
    if well_formed {
        Ok(())
    } else {
        Err(invalid(
            "wake_filter field_equals paths are 1 to 5 segments of lowercase letters, digits and underscores",
        ))
    }
}

/// A list of strings, each trimmed to 1 to [`MAX_WAKE_TOKEN_CHARS`] characters, of at most `limit` entries.
fn canonical_wake_tokens(
    values: Vec<String>,
    limit: usize,
    field: &str,
) -> Result<Vec<String>, Status> {
    if values.len() > limit {
        return Err(invalid(&format!(
            "wake_filter {field} allows at most {limit} entries"
        )));
    }
    values
        .into_iter()
        .map(|value| {
            let trimmed = value.trim();
            let chars = trimmed.chars().count();
            if chars == 0 || chars > MAX_WAKE_TOKEN_CHARS {
                return Err(invalid(&format!(
                    "wake_filter {field} entries are 1 to {MAX_WAKE_TOKEN_CHARS} characters"
                )));
            }
            Ok(trimmed.to_owned())
        })
        .collect()
}

/// The one event type this slice accepts.
fn check_event_type(raw: &str) -> Result<&'static str, Status> {
    match raw.trim() {
        EVENT_TYPE_WEBHOOK_RECEIVED => Ok(EVENT_TYPE_WEBHOOK_RECEIVED),
        _ => Err(invalid("event_type must be webhook_received")),
    }
}

/// The status a member may write. The record never moves to any other state from a request.
fn check_status(raw: &str) -> Result<&'static str, Status> {
    match raw.trim() {
        "active" => Ok("active"),
        "paused" => Ok("paused"),
        "disabled" => Ok("disabled"),
        _ => Err(invalid("status must be active, paused or disabled")),
    }
}

fn check_update_paths(paths: &[String]) -> Result<(), Status> {
    if paths.is_empty() {
        return Err(invalid("update_paths is required"));
    }
    if let Some(unknown) = paths
        .iter()
        .find(|path| !UPDATE_PATHS.contains(&path.as_str()))
    {
        // The path is a field name the caller wrote; a bounded echo names the mistake.
        let name: String = unknown.chars().take(64).collect();
        return Err(invalid(&format!("unknown update path {name}")));
    }
    Ok(())
}

/// The compare-and-set: the revision the caller saw must be the revision now.
fn check_revision(current: i64, expected: u64) -> Result<(), Status> {
    if u64::try_from(current).ok() != Some(expected) {
        return Err(Status::aborted(REVISION_CONFLICT));
    }
    Ok(())
}

/// Whether a trigger of `member` in `org_id` and `space_ref` may be bound to `schedule_id`. The schedule must be
/// that member's own live schedule (not soft-deleted) in the same org and Space, and carry its approval contract
/// as a non-empty text value: the WS20 stored consent in the schedule's task template. The check is read in the
/// caller's transaction. It grants nothing: every fire is re-authorised by Control, so a schedule deleted just
/// after this check stops its triggers from firing.
async fn check_schedule_binding(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: &str,
    space_ref: &str,
    member: &str,
    schedule_id: &str,
) -> Result<(), Status> {
    let bound: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM cron_schedules
         WHERE id = $1 AND org_id = $2 AND space_ref = $3 AND creator_subject_id = $4
           AND deleted_at IS NULL
           AND jsonb_typeof(task_template -> 'approval_contract') = 'string'
           AND length(task_template ->> 'approval_contract') > 0",
    )
    .bind(schedule_id)
    .bind(org_id)
    .bind(space_ref)
    .bind(member)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)?;
    bound
        .map(|_| ())
        .ok_or_else(|| Status::invalid_argument(SCHEDULE_MISMATCH))
}

/// The columns a trigger is read with, in [`TriggerRow`]'s order.
const TRIGGER_COLUMNS: &str = "id, org_id, space_ref, connection_id, member_user_id, event_type, \
     name, clock_enabled, status, revision, created_at, updated_at, schedule_id, wake_filter";

#[derive(Debug, sqlx::FromRow)]
struct TriggerRow {
    id: String,
    org_id: String,
    space_ref: String,
    connection_id: String,
    member_user_id: String,
    event_type: String,
    name: String,
    clock_enabled: bool,
    status: String,
    revision: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    schedule_id: Option<String>,
    wake_filter: Option<serde_json::Value>,
}

impl TriggerRow {
    fn into_proto(self) -> pb::EventTrigger {
        pb::EventTrigger {
            id: self.id,
            org_id: self.org_id,
            space_ref: self.space_ref,
            connection_id: self.connection_id,
            member_user_id: self.member_user_id,
            event_type: self.event_type,
            name: self.name,
            clock_enabled: self.clock_enabled,
            status: self.status,
            revision: u64::try_from(self.revision).unwrap_or_default(),
            created_at: Some(to_proto_timestamp(self.created_at)),
            updated_at: Some(to_proto_timestamp(self.updated_at)),
            schedule_id: self.schedule_id.unwrap_or_default(),
            wake_filter_json: self
                .wake_filter
                .map(|filter| filter.to_string())
                .unwrap_or_default(),
        }
    }
}

fn found(row: TriggerRow) -> pb::EventTriggerResponse {
    pb::EventTriggerResponse {
        trigger: Some(row.into_proto()),
        found: true,
    }
}

fn internal(error: sqlx::Error) -> Status {
    Status::internal(error.to_string())
}

/// Record a trigger for the caller, active. The org must be the caller's own; the Space and the
/// connection are stored as the caller names them (Control verifies them per fire in a later slice).
/// A named schedule is checked first against the caller's own schedule in this org and Space.
pub(crate) async fn create_event_trigger(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::CreateEventTriggerRequest,
) -> Result<pb::EventTriggerResponse, Status> {
    let member = member_of(caller)?;
    caller.authorize_org(&req.org_id)?;
    let space_ref = bounded_text(&req.space_ref, MAX_REF_CHARS, "space_ref")?;
    let connection_id = bounded_text(&req.connection_id, MAX_REF_CHARS, "connection_id")?;
    let event_type = check_event_type(&req.event_type)?;
    let name = bounded_text(&req.name, MAX_NAME_CHARS, "name")?;
    let schedule_id = optional_schedule(&req.schedule_id)?;
    let wake_filter = optional_wake_filter(&req.wake_filter_json)?;
    let mut tx = pool.begin().await.map_err(internal)?;
    if let Some(schedule) = &schedule_id {
        check_schedule_binding(&mut tx, caller.org_id(), &space_ref, member, schedule).await?;
    }
    let row: TriggerRow = sqlx::query_as(&format!(
        "INSERT INTO event_triggers
         (id, org_id, space_ref, connection_id, member_user_id, event_type, name, clock_enabled, status, schedule_id,
          wake_filter)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'active', $9, $10)
         RETURNING {TRIGGER_COLUMNS}"
    ))
    .bind(format!("trg_{}", new_ulid()))
    .bind(caller.org_id())
    .bind(&space_ref)
    .bind(&connection_id)
    .bind(member)
    .bind(event_type)
    .bind(&name)
    .bind(req.clock_enabled)
    .bind(&schedule_id)
    .bind(&wake_filter)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(found(row))
}

/// Read one trigger the caller owns. Another member's trigger answers `found: false`, the same as a
/// trigger that does not exist, so the id does not confirm anyone else's record.
pub(crate) async fn get_event_trigger(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::GetEventTriggerRequest,
) -> Result<pb::EventTriggerResponse, Status> {
    let member = member_of(caller)?;
    caller.authorize_org(&req.org_id)?;
    let row: Option<TriggerRow> = sqlx::query_as(&format!(
        "SELECT {TRIGGER_COLUMNS} FROM event_triggers
         WHERE id = $1 AND org_id = $2 AND member_user_id = $3"
    ))
    .bind(req.trigger_id.trim())
    .bind(caller.org_id())
    .bind(member)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    Ok(match row {
        Some(row) => found(row),
        None => pb::EventTriggerResponse {
            trigger: None,
            found: false,
        },
    })
}

/// The caller's own triggers, newest first, optionally of one Space. At most [`LIST_LIMIT`].
pub(crate) async fn list_event_triggers(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::ListEventTriggersRequest,
) -> Result<pb::ListEventTriggersResponse, Status> {
    let member = member_of(caller)?;
    caller.authorize_org(&req.org_id)?;
    let rows: Vec<TriggerRow> = sqlx::query_as(&format!(
        "SELECT {TRIGGER_COLUMNS} FROM event_triggers
         WHERE org_id = $1 AND member_user_id = $2 AND ($3 = '' OR space_ref = $3)
         ORDER BY created_at DESC, id DESC LIMIT $4"
    ))
    .bind(caller.org_id())
    .bind(member)
    .bind(req.space_ref.trim())
    .bind(LIST_LIMIT)
    .fetch_all(pool)
    .await
    .map_err(internal)?;
    Ok(pb::ListEventTriggersResponse {
        triggers: rows.into_iter().map(TriggerRow::into_proto).collect(),
    })
}

/// Change one of the caller's triggers under a compare-and-set on its revision. A disabled trigger
/// takes no change: its status is final.
pub(crate) async fn update_event_trigger(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::UpdateEventTriggerRequest,
) -> Result<pb::EventTriggerResponse, Status> {
    let member = member_of(caller)?;
    caller.authorize_org(&req.org_id)?;
    check_update_paths(&req.update_paths)?;

    let mut tx = pool.begin().await.map_err(internal)?;
    let current: Option<TriggerRow> = sqlx::query_as(&format!(
        "SELECT {TRIGGER_COLUMNS} FROM event_triggers
         WHERE id = $1 AND org_id = $2 AND member_user_id = $3 FOR UPDATE"
    ))
    .bind(req.trigger_id.trim())
    .bind(caller.org_id())
    .bind(member)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let mut trigger = current.ok_or_else(|| Status::not_found("event trigger not found"))?;
    check_revision(trigger.revision, req.expected_revision)?;
    if trigger.status == "disabled" {
        return Err(Status::failed_precondition(TRIGGER_CLOSED));
    }

    let wants = |path: &str| req.update_paths.iter().any(|named| named == path);
    if wants("name") {
        trigger.name = bounded_text(&req.name, MAX_NAME_CHARS, "name")?;
    }
    if wants("clock_enabled") {
        trigger.clock_enabled = req.clock_enabled;
    }
    if wants("status") {
        trigger.status = check_status(&req.status)?.to_owned();
    }
    if wants("schedule_id") {
        trigger.schedule_id = optional_schedule(&req.schedule_id)?;
        if let Some(schedule) = &trigger.schedule_id {
            check_schedule_binding(
                &mut tx,
                &trigger.org_id,
                &trigger.space_ref,
                &trigger.member_user_id,
                schedule,
            )
            .await?;
        }
    }

    if wants("wake_filter_json") {
        trigger.wake_filter = optional_wake_filter(&req.wake_filter_json)?;
    }

    let row: TriggerRow = sqlx::query_as(&format!(
        "UPDATE event_triggers
         SET name = $2, clock_enabled = $3, status = $4, schedule_id = $5, wake_filter = $6,
             revision = revision + 1, updated_at = now()
         WHERE id = $1
         RETURNING {TRIGGER_COLUMNS}"
    ))
    .bind(&trigger.id)
    .bind(&trigger.name)
    .bind(trigger.clock_enabled)
    .bind(&trigger.status)
    .bind(&trigger.schedule_id)
    .bind(&trigger.wake_filter)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(found(row))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORG: &str = "org-event-triggers";
    const MEMBER: &str = "user-trigger-member";

    fn member() -> VerifiedIdentity {
        VerifiedIdentity::user_for_test(ORG, MEMBER)
    }

    fn create_request(org: &str, space: &str) -> pb::CreateEventTriggerRequest {
        pb::CreateEventTriggerRequest {
            org_id: org.to_owned(),
            space_ref: space.to_owned(),
            connection_id: "conn-inbox-1".to_owned(),
            event_type: EVENT_TYPE_WEBHOOK_RECEIVED.to_owned(),
            name: "Supplier mail".to_owned(),
            clock_enabled: false,
            schedule_id: String::new(),
            wake_filter_json: String::new(),
        }
    }

    #[test]
    fn event_trigger_text_limits_are_checked() {
        assert!(bounded_text("   ", MAX_NAME_CHARS, "name").is_err());
        assert_eq!(
            bounded_text(" Inbox ", MAX_NAME_CHARS, "name").unwrap(),
            "Inbox"
        );
        assert!(bounded_text(&"x".repeat(MAX_NAME_CHARS), MAX_NAME_CHARS, "name").is_ok());
        let long =
            bounded_text(&"x".repeat(MAX_NAME_CHARS + 1), MAX_NAME_CHARS, "name").unwrap_err();
        assert_eq!(long.code(), tonic::Code::InvalidArgument);
        assert!(long.message().starts_with(INVALID_TRIGGER));
    }

    #[test]
    fn event_trigger_closed_sets_refuse_everything_else() {
        assert_eq!(
            check_event_type("webhook_received").unwrap(),
            "webhook_received"
        );
        assert!(check_event_type("mail_received").is_err());
        assert!(check_event_type("").is_err());
        for status in ["active", "paused", "disabled"] {
            assert_eq!(check_status(status).unwrap(), status);
        }
        assert!(
            check_status("met").is_err(),
            "a stop state is never a member's to write"
        );
        assert!(check_update_paths(&[]).is_err());
        assert!(check_update_paths(&["org_id".to_owned()]).is_err());
        assert!(check_update_paths(&["status".to_owned(), "name".to_owned()]).is_ok());
    }

    #[test]
    fn event_trigger_revision_is_a_compare_and_set() {
        assert!(check_revision(3, 3).is_ok());
        let conflict = check_revision(3, 2).unwrap_err();
        assert_eq!(conflict.code(), tonic::Code::Aborted);
        assert_eq!(conflict.message(), REVISION_CONFLICT);
    }

    #[test]
    fn event_trigger_belongs_to_a_human_member_only() {
        assert_eq!(member_of(&member()).unwrap(), MEMBER);
        let service = VerifiedIdentity::service_for_test(ORG, &["session:write"], false);
        assert_eq!(
            member_of(&service).unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn event_trigger_refuses_zdr_and_waived_callers() {
        assert!(refuse_retention_exempt(&member()).is_ok());
        let zdr = VerifiedIdentity::user_for_test_with_zdr(ORG, MEMBER, true);
        assert_eq!(
            refuse_retention_exempt(&zdr).unwrap_err().code(),
            tonic::Code::FailedPrecondition
        );
        let waived = VerifiedIdentity::user_for_test_with_waiver(ORG, MEMBER, "conn-1", "ack-1");
        assert_eq!(
            refuse_retention_exempt(&waived).unwrap_err().code(),
            tonic::Code::FailedPrecondition
        );
    }

    #[test]
    fn event_trigger_switch_reads_literal_true_only() {
        assert!(!mp_contracts::loop_fold::switch_from(None));
        assert!(!mp_contracts::loop_fold::switch_from(Some("1")));
        assert!(!mp_contracts::loop_fold::switch_from(Some("yes")));
        assert!(!mp_contracts::loop_fold::switch_from(Some("on")));
        assert!(mp_contracts::loop_fold::switch_from(Some("true")));
    }

    #[test]
    fn event_trigger_row_maps_every_field() {
        let at = Utc::now();
        let row = TriggerRow {
            id: "trg_01".to_owned(),
            org_id: ORG.to_owned(),
            space_ref: "space-a".to_owned(),
            connection_id: "conn-1".to_owned(),
            member_user_id: MEMBER.to_owned(),
            event_type: "webhook_received".to_owned(),
            name: "Supplier mail".to_owned(),
            clock_enabled: true,
            status: "paused".to_owned(),
            revision: 4,
            created_at: at,
            updated_at: at,
            schedule_id: None,
            wake_filter: None,
        };
        let proto = row.into_proto();
        assert_eq!(proto.id, "trg_01");
        assert_eq!(proto.member_user_id, MEMBER);
        assert!(proto.clock_enabled);
        assert_eq!(proto.status, "paused");
        assert_eq!(proto.revision, 4);
        assert!(proto.created_at.is_some() && proto.updated_at.is_some());
    }

    #[tokio::test]
    async fn rpc_is_refused_while_the_switch_is_off() {
        if event_triggers_enabled() {
            return;
        }
        let service = EventTriggerGrpc {
            pool: PgPool::connect_lazy("postgres://unused@localhost/unused").unwrap(),
        };
        let mut request = Request::new(create_request(ORG, "space-a"));
        request.extensions_mut().insert(member());
        let status = service.create_event_trigger(request).await.unwrap_err();
        assert_eq!(status.code(), tonic::Code::Unavailable);
        assert_eq!(status.message(), DISABLED);
    }

    // The database tests below run with `--ignored` against a disposable Postgres, as the other
    // session-core database tests do. Each test takes its own org and member, so they can share a
    // database.

    async fn connect() -> PgPool {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    fn fresh_identity() -> (String, String) {
        (
            format!("org-trg-{}", new_ulid()),
            format!("user-trg-{}", new_ulid()),
        )
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn create_then_get_is_member_only() {
        let pool = connect().await;
        let (org, owner) = fresh_identity();
        let caller = VerifiedIdentity::user_for_test(&org, &owner);
        let created = create_event_trigger(&pool, &caller, create_request(&org, "space-a"))
            .await
            .unwrap()
            .trigger
            .unwrap();
        assert!(created.id.starts_with("trg_"));
        assert_eq!(created.status, "active");
        assert_eq!(created.revision, 1);
        assert_eq!(created.member_user_id, owner);

        let get = pb::GetEventTriggerRequest {
            org_id: org.clone(),
            trigger_id: created.id.clone(),
        };
        assert!(
            get_event_trigger(&pool, &caller, get.clone())
                .await
                .unwrap()
                .found
        );
        let stranger = VerifiedIdentity::user_for_test(&org, "user-someone-else");
        assert!(
            !get_event_trigger(&pool, &stranger, get)
                .await
                .unwrap()
                .found
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn list_filters_by_member_and_space() {
        let pool = connect().await;
        let (org, owner) = fresh_identity();
        let caller = VerifiedIdentity::user_for_test(&org, &owner);
        for space in ["space-a", "space-a", "space-b"] {
            create_event_trigger(&pool, &caller, create_request(&org, space))
                .await
                .unwrap();
        }
        let all = pb::ListEventTriggersRequest {
            org_id: org.clone(),
            space_ref: String::new(),
        };
        assert_eq!(
            list_event_triggers(&pool, &caller, all.clone())
                .await
                .unwrap()
                .triggers
                .len(),
            3
        );
        let one_space = pb::ListEventTriggersRequest {
            space_ref: "space-a".to_owned(),
            ..all.clone()
        };
        assert_eq!(
            list_event_triggers(&pool, &caller, one_space)
                .await
                .unwrap()
                .triggers
                .len(),
            2
        );
        let stranger = VerifiedIdentity::user_for_test(&org, "user-someone-else");
        assert!(list_event_triggers(&pool, &stranger, all)
            .await
            .unwrap()
            .triggers
            .is_empty());
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn update_is_compare_and_set_and_disabled_is_final() {
        let pool = connect().await;
        let (org, owner) = fresh_identity();
        let caller = VerifiedIdentity::user_for_test(&org, &owner);
        let created = create_event_trigger(&pool, &caller, create_request(&org, "space-a"))
            .await
            .unwrap()
            .trigger
            .unwrap();
        let change = |revision: u64, path: &str, status: &str| pb::UpdateEventTriggerRequest {
            org_id: org.clone(),
            trigger_id: created.id.clone(),
            expected_revision: revision,
            update_paths: vec![path.to_owned()],
            status: status.to_owned(),
            ..Default::default()
        };

        let stale = update_event_trigger(&pool, &caller, change(9, "status", "paused"))
            .await
            .unwrap_err();
        assert_eq!(stale.code(), tonic::Code::Aborted);

        let paused = update_event_trigger(&pool, &caller, change(1, "status", "paused"))
            .await
            .unwrap()
            .trigger
            .unwrap();
        assert_eq!((paused.status.as_str(), paused.revision), ("paused", 2));

        let disabled = update_event_trigger(&pool, &caller, change(2, "status", "disabled"))
            .await
            .unwrap()
            .trigger
            .unwrap();
        assert_eq!(
            (disabled.status.as_str(), disabled.revision),
            ("disabled", 3)
        );

        let closed = update_event_trigger(&pool, &caller, change(3, "status", "active"))
            .await
            .unwrap_err();
        assert_eq!(closed.code(), tonic::Code::FailedPrecondition);
        assert_eq!(closed.message(), TRIGGER_CLOSED);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn org_purge_removes_the_orgs_triggers_only() {
        let pool = connect().await;
        let (org, owner) = fresh_identity();
        let (other_org, other_owner) = fresh_identity();
        for (org, owner) in [(&org, &owner), (&other_org, &other_owner)] {
            let caller = VerifiedIdentity::user_for_test(org, owner);
            create_event_trigger(&pool, &caller, create_request(org, "space-a"))
                .await
                .unwrap();
        }
        crate::gdpr::purge_organization_data(&pool, &org, None)
            .await
            .unwrap();
        let left = |org: String| {
            let pool = pool.clone();
            async move {
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM event_triggers WHERE org_id = $1",
                )
                .bind(org)
                .fetch_one(&pool)
                .await
                .unwrap()
            }
        };
        assert_eq!(
            left(org.clone()).await,
            0,
            "the erased org's triggers are gone"
        );
        assert_eq!(
            left(other_org.clone()).await,
            1,
            "another org's trigger is untouched"
        );
        crate::gdpr::purge_organization_data(&pool, &other_org, None)
            .await
            .unwrap();
    }
}
