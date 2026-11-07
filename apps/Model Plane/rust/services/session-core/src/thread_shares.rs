//! Share snapshots: an owner's immutable, redacted copy of one thread (Wave 3 EP-12 S14, decision
//! D10; ADR `docs/decisions/conversation-share-model.md`, proposed until the owner accepts OD-5).
//!
//! A snapshot is the S9 export with the `REDACTED` profile, frozen at the moment it is taken and
//! stored in `thread_share_snapshots`. Later turns never reach it, and reading it calls no model,
//! connector or tool. Taking one follows the export's rules, because it is the export's read:
//!
//! * a zero-retention or waived caller is refused with `zdr_feature_not_supported`
//!   (`conversation_share`) before anything is read (DEC-32: a waived credential counts as
//!   zero-retention for every new durable store);
//! * a Space thread, or a thread holding any run that is not `persistent`, is refused with the
//!   export's `thread_not_exportable` and its reason on the trailer, so a shared transcript is
//!   never silently incomplete;
//! * another person's thread reads as not found, and a service principal is refused.
//!
//! Reads are owner-only. The ADR lets a member holding a Control grant on
//! `conversation_snapshot:<id>` read one too, but Control has no grant decision for that type that
//! session-core can verify (no owner service is registered for it and session-core holds no
//! authz-facade credential), so a reader who is not the owner reads not found, exactly as a
//! snapshot that does not exist.
//!
//! Revoking sets `revoked_at` and deletes the body and its digest in the same statement; a revoked
//! snapshot reads as not found to everyone. A snapshot is erased with its thread (`ON DELETE
//! CASCADE` and `delete_thread_rows`) and with the org purge. The switch is [`ENABLED_ENV`],
//! default off; while it is off every RPC answers `unavailable` `thread_shares_disabled`.

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{self as pb, thread_share_service_server::ThreadShareService};
use mp_contracts::zdr_refusal::Refusal;
use mp_ids::new_ulid;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use sqlx::PgPool;
use std::time::Instant;
use tonic::metadata::MetadataValue;
use tonic::{Request, Response, Status};

use crate::auth::{authorize_operation, authorize_owner_housekeeping, identity, VerifiedIdentity};
use crate::grpc::{record_metrics, to_proto_timestamp};
use crate::run_phase::ERROR_CODE_TRAILER;

/// The environment switch for the snapshot store. Default off, read by the strict parser the
/// other switches share, so only the literal `true` turns it on.
pub(crate) const ENABLED_ENV: &str = "SESSION_THREAD_SHARES_ENABLED";

/// What every RPC answers while the switch is off; the gateway maps it to its own 404.
pub(crate) const DISABLED: &str = "thread_shares_disabled";

/// The ZDR feature key a snapshot refusal names (`zdr-refusals.json`, EP-12 S3).
pub(crate) const ZDR_FEATURE: &str = "conversation_share";

/// The most live snapshots one thread holds. Each can be up to [`MAX_SNAPSHOT_BODY_BYTES`], so the
/// cap bounds what one thread can grow the store by; revoking one frees its place.
pub(crate) const MAX_LIVE_SNAPSHOTS_PER_THREAD: i64 = 20;

/// The largest stored body, matching the table's CHECK. The export caps its text at 8 MiB, but the
/// JSON around the text can carry a capped export past it, which is refused rather than cut.
pub(crate) const MAX_SNAPSHOT_BODY_BYTES: usize = 8 * 1024 * 1024;

/// The longest title a snapshot keeps, matching the table's CHECK.
const MAX_TITLE_CHARS: usize = 1_000;

/// The typed codes of the two store limits, on [`ERROR_CODE_TRAILER`].
pub(crate) const LIMIT_REACHED_CODE: &str = "snapshot_limit_reached";
pub(crate) const TOO_LARGE_CODE: &str = "snapshot_too_large";

/// The version of the stored body's shape. A body of any other version is not read.
const BODY_VERSION: u32 = 1;

/// Read where it is used, as the export's switch is.
pub(crate) fn thread_shares_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

fn switch_gate() -> Result<(), Status> {
    if thread_shares_enabled() {
        Ok(())
    } else {
        Err(Status::unavailable(DISABLED))
    }
}

/// The snapshot RPCs. They hold the pool and nothing else; the rules are in this module.
pub struct ThreadShareGrpc {
    pub(crate) pool: PgPool,
}

#[tonic::async_trait]
impl ThreadShareService for ThreadShareGrpc {
    async fn create_snapshot(
        &self,
        request: Request<pb::CreateSnapshotRequest>,
    ) -> Result<Response<pb::CreateSnapshotResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            // Named before the write scope, which would refuse a ZDR credential without the feature.
            refuse_retention_exempt(&caller)?;
            authorize_operation(&caller, "session:write")?;
            switch_gate()?;
            create_snapshot(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("create_share_snapshot", started, result.is_ok());
        result
    }

    async fn list_snapshots(
        &self,
        request: Request<pb::ListSnapshotsRequest>,
    ) -> Result<Response<pb::ListSnapshotsResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            switch_gate()?;
            list_snapshots(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("list_share_snapshots", started, result.is_ok());
        result
    }

    async fn get_snapshot(
        &self,
        request: Request<pb::GetSnapshotRequest>,
    ) -> Result<Response<pb::GetSnapshotResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            switch_gate()?;
            get_snapshot(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("get_share_snapshot", started, result.is_ok());
        result
    }

    async fn revoke_snapshot(
        &self,
        request: Request<pb::RevokeSnapshotRequest>,
    ) -> Result<Response<pb::RevokeSnapshotResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            // Revoking deletes content, so a zero-retention owner may do it (owner decision 17).
            authorize_owner_housekeeping(&caller, "session:write")?;
            switch_gate()?;
            revoke_snapshot(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("revoke_share_snapshot", started, result.is_ok());
        result
    }
}

/// A snapshot is a new durable copy of conversation text, which a zero-retention or waived
/// credential may never make.
fn refuse_retention_exempt(caller: &VerifiedIdentity) -> Result<(), Status> {
    if caller.zdr() || caller.zdr_waiver().is_some() {
        return Err(Refusal::feature_not_supported(ZDR_FEATURE)
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    Ok(())
}

/// The person acting. A snapshot is the owner's own act, and only its owner reads it, so a service
/// or a key principal is refused.
fn person(caller: &VerifiedIdentity) -> Result<&str, Status> {
    caller
        .user_id()
        .ok_or_else(|| Status::permission_denied("a snapshot belongs to the thread's owner"))
}

fn required<'a>(value: &'a str, name: &str) -> Result<&'a str, Status> {
    let value = value.trim();
    if value.is_empty() {
        return Err(Status::invalid_argument(format!("{name} is required")));
    }
    Ok(value)
}

/// A store limit: `failed_precondition` with its typed code on the trailer.
fn limit_status(code: &'static str, message: &str) -> Status {
    let mut status = Status::failed_precondition(message);
    status
        .metadata_mut()
        .insert(ERROR_CODE_TRAILER, MetadataValue::from_static(code));
    status
}

/// Take a snapshot. The export decides whether the thread may leave and reads it in one
/// transaction; the store then writes that read in a second one, re-checking that the thread is
/// still the caller's. A turn that lands between the two is simply not in the snapshot.
pub(crate) async fn create_snapshot(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::CreateSnapshotRequest,
) -> Result<pb::CreateSnapshotResponse, Status> {
    refuse_retention_exempt(caller)?;
    let owner = person(caller)?;
    let thread_id = required(&req.thread_id, "thread_id")?;
    let export = crate::thread_export::export_thread(
        pool,
        caller,
        pb::ExportThreadRequest {
            thread_id: thread_id.to_owned(),
            profile: pb::ExportProfile::Redacted as i32,
        },
    )
    .await?;
    let title: String = export.title.chars().take(MAX_TITLE_CHARS).collect();
    let body = body_value(&export)?;
    let text = body.to_string();
    if text.len() > MAX_SNAPSHOT_BODY_BYTES {
        return Err(limit_status(
            TOO_LARGE_CODE,
            "the conversation is too large to share as one snapshot",
        ));
    }
    let digest = format!("sha256:{:x}", Sha256::digest(text.as_bytes()));
    let snapshot_id = format!("snap_{}", new_ulid());

    let mut tx = pool.begin().await.map_err(internal)?;
    // One writer per thread at a time, so the cap holds across replicas.
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('thread_share_snapshots:' || $1, 0))",
    )
    .bind(thread_id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let live: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM thread_share_snapshots WHERE thread_id = $1 AND revoked_at IS NULL",
    )
    .bind(thread_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    if live >= MAX_LIVE_SNAPSHOTS_PER_THREAD {
        return Err(limit_status(
            LIMIT_REACHED_CODE,
            "this conversation already has the most live snapshots; revoke one first",
        ));
    }
    // The owner, org and thread come from the thread row, never from the request, and the row must
    // still be the caller's: a thread deleted or moved since the export read inserts nothing.
    let row: Option<SnapshotRow> = sqlx::query_as(
        "INSERT INTO thread_share_snapshots (id, org_id, thread_id, owner_id, title, body, body_digest)
         SELECT $1, t.org_id, t.id, t.user_id, $5, $6, $7
           FROM threads t
          WHERE t.id = $2 AND t.org_id = $3 AND t.user_id = $4
         RETURNING id, thread_id, title, body_digest, created_at, revoked_at",
    )
    .bind(&snapshot_id)
    .bind(thread_id)
    .bind(caller.org_id())
    .bind(owner)
    .bind(&title)
    .bind(&body)
    .bind(&digest)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let row = row.ok_or_else(|| Status::not_found("thread not found"))?;
    tx.commit().await.map_err(internal)?;
    Ok(pb::CreateSnapshotResponse {
        snapshot: Some(row.into_proto()),
    })
}

/// The caller's live snapshots of one thread, newest first. Another person's thread lists none,
/// so the listing confirms nothing about it.
pub(crate) async fn list_snapshots(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::ListSnapshotsRequest,
) -> Result<pb::ListSnapshotsResponse, Status> {
    let owner = person(caller)?;
    let thread_id = required(&req.thread_id, "thread_id")?;
    let rows: Vec<SnapshotRow> = sqlx::query_as(
        "SELECT id, thread_id, title, body_digest, created_at, revoked_at
           FROM thread_share_snapshots
          WHERE thread_id = $1 AND org_id = $2 AND owner_id = $3 AND revoked_at IS NULL
          ORDER BY created_at DESC, id DESC
          LIMIT $4",
    )
    .bind(thread_id)
    .bind(caller.org_id())
    .bind(owner)
    .bind(MAX_LIVE_SNAPSHOTS_PER_THREAD)
    .fetch_all(pool)
    .await
    .map_err(internal)?;
    Ok(pb::ListSnapshotsResponse {
        snapshots: rows.into_iter().map(SnapshotRow::into_proto).collect(),
    })
}

/// One live snapshot with its body. Owner-only: see the module comment for the grant read that
/// does not exist yet.
pub(crate) async fn get_snapshot(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::GetSnapshotRequest,
) -> Result<pb::GetSnapshotResponse, Status> {
    let owner = person(caller)?;
    let snapshot_id = required(&req.snapshot_id, "snapshot_id")?;
    let row: Option<(String, String, String, Option<String>, DateTime<Utc>, Value)> =
        sqlx::query_as(
            "SELECT id, thread_id, title, body_digest, created_at, body
               FROM thread_share_snapshots
              WHERE id = $1 AND org_id = $2 AND owner_id = $3 AND revoked_at IS NULL",
        )
        .bind(snapshot_id)
        .bind(caller.org_id())
        .bind(owner)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    let (id, thread_id, title, body_digest, created_at, body) =
        row.ok_or_else(|| Status::not_found("snapshot not found"))?;
    let snapshot = SnapshotRow {
        id,
        thread_id,
        title,
        body_digest,
        created_at,
        revoked_at: None,
    };
    Ok(pb::GetSnapshotResponse {
        snapshot: Some(snapshot.into_proto()),
        body: Some(body_from_value(body)?),
    })
}

/// Revoke one of the caller's live snapshots: the body and its digest go in the same statement that
/// marks it revoked. Another person's snapshot, or one already revoked, is not found.
pub(crate) async fn revoke_snapshot(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::RevokeSnapshotRequest,
) -> Result<pb::RevokeSnapshotResponse, Status> {
    let owner = person(caller)?;
    let snapshot_id = required(&req.snapshot_id, "snapshot_id")?;
    let row: Option<SnapshotRow> = sqlx::query_as(
        "UPDATE thread_share_snapshots
            SET revoked_at = now(), body = NULL, body_digest = NULL
          WHERE id = $1 AND org_id = $2 AND owner_id = $3 AND revoked_at IS NULL
         RETURNING id, thread_id, title, body_digest, created_at, revoked_at",
    )
    .bind(snapshot_id)
    .bind(caller.org_id())
    .bind(owner)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let row = row.ok_or_else(|| Status::not_found("snapshot not found"))?;
    Ok(pb::RevokeSnapshotResponse {
        snapshot: Some(row.into_proto()),
    })
}

/// One snapshot row without its body.
#[derive(sqlx::FromRow)]
struct SnapshotRow {
    id: String,
    thread_id: String,
    title: String,
    body_digest: Option<String>,
    created_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

impl SnapshotRow {
    fn into_proto(self) -> pb::ThreadShareSnapshot {
        pb::ThreadShareSnapshot {
            snapshot_id: self.id,
            thread_id: self.thread_id,
            title: self.title,
            created_at: Some(to_proto_timestamp(self.created_at)),
            body_digest: self.body_digest.unwrap_or_default(),
            revoked_at: self.revoked_at.map(to_proto_timestamp),
        }
    }
}

/// The stored body: the export message field for field, so a read returns exactly what was taken.
/// The JSON fields stay JSON text, as the export carries them, and timestamps keep their seconds
/// and nanoseconds, so nothing is reparsed or rounded on the way through the store.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotBody {
    version: u32,
    thread_id: String,
    title: String,
    created_at: Option<Stamp>,
    messages: Vec<BodyMessage>,
    runs: Vec<BodyRun>,
    truncated: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BodyMessage {
    message_id: String,
    role: String,
    content: String,
    created_at: Option<Stamp>,
    artifacts_json: String,
    citations_json: String,
    metadata_json: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BodyRun {
    run_id: String,
    status: String,
    origin: String,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    created_at: Option<Stamp>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stamp {
    seconds: i64,
    nanos: i32,
}

impl From<&prost_types::Timestamp> for Stamp {
    fn from(value: &prost_types::Timestamp) -> Self {
        Self {
            seconds: value.seconds,
            nanos: value.nanos,
        }
    }
}

impl From<Stamp> for prost_types::Timestamp {
    fn from(value: Stamp) -> Self {
        Self {
            seconds: value.seconds,
            nanos: value.nanos,
        }
    }
}

/// The export as the body the store keeps.
fn body_value(export: &pb::ExportThreadResponse) -> Result<Value, Status> {
    let body = SnapshotBody {
        version: BODY_VERSION,
        thread_id: export.thread_id.clone(),
        title: export.title.clone(),
        created_at: export.created_at.as_ref().map(Stamp::from),
        messages: export
            .messages
            .iter()
            .map(|message| BodyMessage {
                message_id: message.message_id.clone(),
                role: message.role.clone(),
                content: message.content.clone(),
                created_at: message.created_at.as_ref().map(Stamp::from),
                artifacts_json: message.artifacts_json.clone(),
                citations_json: message.citations_json.clone(),
                metadata_json: message.metadata_json.clone(),
            })
            .collect(),
        runs: export
            .runs
            .iter()
            .map(|run| BodyRun {
                run_id: run.run_id.clone(),
                status: run.status.clone(),
                origin: run.origin.clone(),
                model: run.model.clone(),
                input_tokens: run.input_tokens,
                output_tokens: run.output_tokens,
                created_at: run.created_at.as_ref().map(Stamp::from),
            })
            .collect(),
        truncated: export.truncated,
    };
    serde_json::to_value(body).map_err(|error| Status::internal(error.to_string()))
}

/// A stored body as the export message. A body of another version or shape is a store fault, not
/// something to show half of.
fn body_from_value(value: Value) -> Result<pb::ExportThreadResponse, Status> {
    let body: SnapshotBody = serde_json::from_value(value)
        .map_err(|_| Status::internal("snapshot body is unreadable"))?;
    if body.version != BODY_VERSION {
        return Err(Status::internal("snapshot body version is not supported"));
    }
    Ok(pb::ExportThreadResponse {
        thread_id: body.thread_id,
        title: body.title,
        created_at: body.created_at.map(Into::into),
        messages: body
            .messages
            .into_iter()
            .map(|message| pb::ExportedMessage {
                message_id: message.message_id,
                role: message.role,
                content: message.content,
                created_at: message.created_at.map(Into::into),
                artifacts_json: message.artifacts_json,
                citations_json: message.citations_json,
                metadata_json: message.metadata_json,
            })
            .collect(),
        runs: body
            .runs
            .into_iter()
            .map(|run| pb::ExportedRun {
                run_id: run.run_id,
                status: run.status,
                origin: run.origin,
                model: run.model,
                input_tokens: run.input_tokens,
                output_tokens: run.output_tokens,
                created_at: run.created_at.map(Into::into),
            })
            .collect(),
        truncated: body.truncated,
    })
}

fn internal(error: sqlx::Error) -> Status {
    Status::internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ORG: &str = "org-share";
    const OWNER: &str = "user-share-owner";
    const MEMBER: &str = "user-share-member";

    fn sample_export() -> pb::ExportThreadResponse {
        let at = prost_types::Timestamp {
            seconds: 1_793_000_000,
            nanos: 123_456_789,
        };
        pb::ExportThreadResponse {
            thread_id: "thread-1".to_owned(),
            title: "Quarterly notes".to_owned(),
            created_at: Some(at.clone()),
            messages: vec![pb::ExportedMessage {
                message_id: "m1".to_owned(),
                role: "assistant".to_owned(),
                content: "the answer".to_owned(),
                created_at: Some(at.clone()),
                artifacts_json: r#"[{"id":"a1","kind":"document","content":"text"}]"#.to_owned(),
                citations_json: String::new(),
                metadata_json: r#"{"attachments":[{"name":"attachment-1"}]}"#.to_owned(),
            }],
            runs: vec![pb::ExportedRun {
                run_id: "r1".to_owned(),
                status: "completed".to_owned(),
                origin: "chat".to_owned(),
                model: "m-1".to_owned(),
                input_tokens: 0,
                output_tokens: 0,
                created_at: Some(at),
            }],
            truncated: true,
        }
    }

    #[test]
    fn body_round_trips_exactly() {
        let export = sample_export();
        let stored = body_value(&export).unwrap();
        assert_eq!(stored["version"], BODY_VERSION);
        assert_eq!(
            stored["messages"][0]["artifacts_json"],
            r#"[{"id":"a1","kind":"document","content":"text"}]"#,
            "the JSON fields stay text, so the read returns them byte for byte"
        );
        assert_eq!(body_from_value(stored).unwrap(), export);
    }

    #[test]
    fn a_body_of_another_version_or_shape_is_not_read() {
        let mut stored = body_value(&sample_export()).unwrap();
        stored["version"] = json!(2);
        assert_eq!(
            body_from_value(stored).unwrap_err().code(),
            tonic::Code::Internal
        );

        let mut stored = body_value(&sample_export()).unwrap();
        stored["extra"] = json!("field");
        assert!(body_from_value(stored).is_err(), "an unknown field is refused");
        assert!(body_from_value(json!([])).is_err());
    }

    #[test]
    fn row_projects_digest_and_revocation() {
        let now = Utc::now();
        let live = SnapshotRow {
            id: "snap_1".to_owned(),
            thread_id: "thread-1".to_owned(),
            title: "t".to_owned(),
            body_digest: Some("sha256:00".to_owned()),
            created_at: now,
            revoked_at: None,
        }
        .into_proto();
        assert_eq!(live.body_digest, "sha256:00");
        assert!(live.revoked_at.is_none());

        let revoked = SnapshotRow {
            id: "snap_1".to_owned(),
            thread_id: "thread-1".to_owned(),
            title: "t".to_owned(),
            body_digest: None,
            created_at: now,
            revoked_at: Some(now),
        }
        .into_proto();
        assert!(revoked.body_digest.is_empty());
        assert_eq!(revoked.revoked_at, Some(to_proto_timestamp(now)));
    }

    #[test]
    fn zero_retention_and_waived_callers_are_refused_with_the_share_key() {
        for caller in [
            VerifiedIdentity::user_for_test_with_zdr(ORG, OWNER, true),
            VerifiedIdentity::user_for_test_with_waiver(ORG, OWNER, "conn-1", "ack-1"),
        ] {
            let refused = refuse_retention_exempt(&caller).unwrap_err();
            assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
            assert_eq!(
                Refusal::from_status(&refused),
                Some(Refusal::feature_not_supported(ZDR_FEATURE))
            );
        }
        assert!(refuse_retention_exempt(&VerifiedIdentity::user_for_test(ORG, OWNER)).is_ok());
    }

    #[test]
    fn a_service_principal_is_not_a_snapshot_owner() {
        let service = VerifiedIdentity::service_for_test(ORG, &["session:read"], false);
        assert_eq!(
            person(&service).unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            person(&VerifiedIdentity::user_for_test(ORG, OWNER)).unwrap(),
            OWNER
        );
    }

    #[test]
    fn limits_name_their_code_on_the_trailer() {
        for code in [LIMIT_REACHED_CODE, TOO_LARGE_CODE] {
            let status = limit_status(code, "limit");
            assert_eq!(status.code(), tonic::Code::FailedPrecondition);
            assert_eq!(
                status
                    .metadata()
                    .get(ERROR_CODE_TRAILER)
                    .and_then(|value| value.to_str().ok()),
                Some(code)
            );
        }
    }

    #[test]
    fn the_switch_is_off_unless_it_reads_true() {
        assert!(!mp_contracts::loop_fold::switch_from(None));
        assert!(!mp_contracts::loop_fold::switch_from(Some("1")));
        assert!(!mp_contracts::loop_fold::switch_from(Some("TRUE")));
        assert!(mp_contracts::loop_fold::switch_from(Some("true")));
    }

    #[test]
    fn blank_ids_are_refused() {
        assert_eq!(
            required("  ", "thread_id").unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(required(" t1 ", "thread_id").unwrap(), "t1");
    }

    // The database tests below run with `--ignored` against a disposable Postgres, as the other
    // session-core database tests do. They call the store functions directly, so the switch is not
    // needed; the switch is tested by `snapshots_are_refused_while_the_switch_is_off`.

    async fn connect() -> PgPool {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn seed_thread(pool: &PgPool, thread: &str) {
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)",
        )
        .bind(thread)
        .bind(ORG)
        .bind(OWNER)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn append(pool: &PgPool, thread: &str, role: &str, content: &str) {
        let mut tx = pool.begin().await.unwrap();
        let request = pb::AppendMessageRequest {
            thread_id: thread.to_owned(),
            role: role.to_owned(),
            content: content.to_owned(),
            ..Default::default()
        };
        crate::grpc::append_message_in_tx(&mut tx, &request, OWNER)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }

    async fn seed_run(pool: &PgPool, thread: &str, retention: &str) {
        let waived = retention == "zdr_waived";
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status, metadata,
                               retention_class, waiver_ack_id, waiver_connection_id)
             VALUES ($1, $2, 'g', $3, $4, 'completed', '{}'::jsonb, $5, $6, $7)",
        )
        .bind(new_ulid())
        .bind(thread)
        .bind(ORG)
        .bind(OWNER)
        .bind(retention)
        .bind(waived.then_some("ack-1"))
        .bind(waived.then_some("conn-1"))
        .execute(pool)
        .await
        .unwrap();
    }

    fn owner() -> VerifiedIdentity {
        VerifiedIdentity::user_for_test(ORG, OWNER)
    }

    async fn create(
        pool: &PgPool,
        caller: &VerifiedIdentity,
        thread: &str,
    ) -> Result<pb::ThreadShareSnapshot, Status> {
        create_snapshot(
            pool,
            caller,
            pb::CreateSnapshotRequest {
                thread_id: thread.to_owned(),
            },
        )
        .await
        .map(|created| created.snapshot.expect("a created snapshot"))
    }

    async fn get(
        pool: &PgPool,
        caller: &VerifiedIdentity,
        snapshot: &str,
    ) -> Result<pb::GetSnapshotResponse, Status> {
        get_snapshot(
            pool,
            caller,
            pb::GetSnapshotRequest {
                snapshot_id: snapshot.to_owned(),
            },
        )
        .await
    }

    async fn stored_body(pool: &PgPool, snapshot: &str) -> (Option<Value>, Option<String>) {
        sqlx::query_as("SELECT body, body_digest FROM thread_share_snapshots WHERE id = $1")
            .bind(snapshot)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn snapshot_of_waived_thread_refused() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread).await;
        append(&pool, &thread, "user", "question").await;
        seed_run(&pool, &thread, "persistent").await;
        seed_run(&pool, &thread, "zdr_waived").await;

        let refused = create(&pool, &owner(), &thread).await.unwrap_err();
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert_eq!(
            refused
                .metadata()
                .get(crate::thread_export::ERROR_REASON_TRAILER)
                .and_then(|value| value.to_str().ok()),
            Some("non_persistent_run")
        );
        let stored: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM thread_share_snapshots WHERE thread_id = $1")
                .bind(&thread)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, 0, "a refused snapshot writes nothing");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn revoked_snapshot_returns_not_found_and_has_no_body() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread).await;
        append(&pool, &thread, "user", "question").await;
        let snapshot = create(&pool, &owner(), &thread).await.unwrap();
        assert!(snapshot.body_digest.starts_with("sha256:"));

        let revoked = revoke_snapshot(
            &pool,
            &owner(),
            pb::RevokeSnapshotRequest {
                snapshot_id: snapshot.snapshot_id.clone(),
            },
        )
        .await
        .unwrap()
        .snapshot
        .unwrap();
        assert!(revoked.revoked_at.is_some());
        assert!(revoked.body_digest.is_empty());
        assert_eq!(stored_body(&pool, &snapshot.snapshot_id).await, (None, None));

        let read = get(&pool, &owner(), &snapshot.snapshot_id).await.unwrap_err();
        assert_eq!(read.code(), tonic::Code::NotFound);
        let again = revoke_snapshot(
            &pool,
            &owner(),
            pb::RevokeSnapshotRequest {
                snapshot_id: snapshot.snapshot_id.clone(),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(again.code(), tonic::Code::NotFound, "a revoked snapshot is gone");
        let listed = list_snapshots(
            &pool,
            &owner(),
            pb::ListSnapshotsRequest {
                thread_id: thread.clone(),
            },
        )
        .await
        .unwrap();
        assert!(listed.snapshots.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn non_granted_member_cannot_read() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread).await;
        append(&pool, &thread, "user", "question").await;
        let snapshot = create(&pool, &owner(), &thread).await.unwrap();

        let member = VerifiedIdentity::user_for_test(ORG, MEMBER);
        let read = get(&pool, &member, &snapshot.snapshot_id).await.unwrap_err();
        assert_eq!(read.code(), tonic::Code::NotFound);
        let revoke = revoke_snapshot(
            &pool,
            &member,
            pb::RevokeSnapshotRequest {
                snapshot_id: snapshot.snapshot_id.clone(),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(revoke.code(), tonic::Code::NotFound);
        let listed = list_snapshots(
            &pool,
            &member,
            pb::ListSnapshotsRequest {
                thread_id: thread.clone(),
            },
        )
        .await
        .unwrap();
        assert!(listed.snapshots.is_empty(), "another person lists none");

        let foreign = VerifiedIdentity::user_for_test("org-elsewhere", OWNER);
        let read = get(&pool, &foreign, &snapshot.snapshot_id).await.unwrap_err();
        assert_eq!(read.code(), tonic::Code::NotFound, "another org reads nothing");
        let taken = create(&pool, &member, &thread).await.unwrap_err();
        assert_eq!(taken.code(), tonic::Code::NotFound, "only the owner takes one");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn snapshot_is_immutable_after_new_turns() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread).await;
        append(&pool, &thread, "user", "question").await;
        append(&pool, &thread, "assistant", "first answer").await;
        let snapshot = create(&pool, &owner(), &thread).await.unwrap();
        append(&pool, &thread, "user", "a later turn").await;

        let read = get(&pool, &owner(), &snapshot.snapshot_id).await.unwrap();
        let contents: Vec<&str> = read
            .body
            .as_ref()
            .unwrap()
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect();
        assert_eq!(contents, ["question", "first answer"]);
        assert_eq!(read.snapshot.unwrap().snapshot_id, snapshot.snapshot_id);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn thread_delete_cascades_snapshot() {
        let pool = connect().await;
        let thread = new_ulid();
        // A thread without turns or runs, so the bare thread delete below has no other child row.
        seed_thread(&pool, &thread).await;
        let snapshot = create(&pool, &owner(), &thread).await.unwrap();
        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(&thread)
            .execute(&pool)
            .await
            .unwrap();
        let left: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM thread_share_snapshots WHERE id = $1")
                .bind(&snapshot.snapshot_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(left, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn live_snapshots_per_thread_are_capped() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread).await;
        for _ in 0..MAX_LIVE_SNAPSHOTS_PER_THREAD {
            create(&pool, &owner(), &thread).await.unwrap();
        }
        let refused = create(&pool, &owner(), &thread).await.unwrap_err();
        assert_eq!(
            refused
                .metadata()
                .get(ERROR_CODE_TRAILER)
                .and_then(|value| value.to_str().ok()),
            Some(LIMIT_REACHED_CODE)
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn snapshots_are_refused_while_the_switch_is_off() {
        let pool = connect().await;
        // The switch is read from the environment; the suite runs with it unset, which is the
        // default this test proves.
        let mut request = Request::new(pb::GetSnapshotRequest {
            snapshot_id: "snap_missing".to_owned(),
        });
        request.extensions_mut().insert(owner());
        let refused = ThreadShareGrpc { pool }
            .get_snapshot(request)
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::Unavailable);
        assert_eq!(refused.message(), DISABLED);
    }
}
