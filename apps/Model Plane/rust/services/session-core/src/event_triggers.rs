//! `EventTriggerService`: the event trigger record store (Wave 4 EP-7, F-t3-047, slice W4-F T1).
//!
//! A trigger binds one integration connection, one Space and one Space member, and names the
//! connector event type that may wake a budgeted run for that member. This slice stores the record
//! and nothing fires. A stored trigger is not authority: every later fire is re-authorised by Control
//! through `FireAuthorizer` (the cron rule, `capability-core/internal/cron/sweeper.go`). The consumer
//! of `webhook_received` and the fire path are in capability-core (`internal/eventtrigger`, W4-F T2).
//! The wake gate and the clock arm come in later slices.
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
/// The one connector event type a trigger may name in this slice (the table's CHECK).
pub(crate) const EVENT_TYPE_WEBHOOK_RECEIVED: &str = "webhook_received";
/// The longest label, in characters (the table's CHECK).
const MAX_NAME_CHARS: usize = 120;
/// The longest org, Space or connection id the store keeps, in characters (the table's CHECKs).
const MAX_REF_CHARS: usize = 128;
/// The most triggers one listing returns, newest first.
const LIST_LIMIT: i64 = 200;
/// The fields an update may name, in `UpdateEventTriggerRequest`'s words.
const UPDATE_PATHS: [&str; 3] = ["name", "clock_enabled", "status"];

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

/// The columns a trigger is read with, in [`TriggerRow`]'s order.
const TRIGGER_COLUMNS: &str = "id, org_id, space_ref, connection_id, member_user_id, event_type, \
     name, clock_enabled, status, revision, created_at, updated_at";

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
    let row: TriggerRow = sqlx::query_as(&format!(
        "INSERT INTO event_triggers
         (id, org_id, space_ref, connection_id, member_user_id, event_type, name, clock_enabled, status)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'active')
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
    .fetch_one(pool)
    .await
    .map_err(internal)?;
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

    let row: TriggerRow = sqlx::query_as(&format!(
        "UPDATE event_triggers
         SET name = $2, clock_enabled = $3, status = $4, revision = revision + 1, updated_at = now()
         WHERE id = $1
         RETURNING {TRIGGER_COLUMNS}"
    ))
    .bind(&trigger.id)
    .bind(&trigger.name)
    .bind(trigger.clock_enabled)
    .bind(&trigger.status)
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
