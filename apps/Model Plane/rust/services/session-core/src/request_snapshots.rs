//! The request snapshot store (EP-14 S6): what each model call of a run was sent and offered.
//!
//! inference-core is the only writer. It calls `RunLedgerService.RecordRequestSnapshot`
//! ([`crate::run_ledger_grpc`]) behind the provider call with its own service token, minted for the call's
//! org. This module holds the write, the retention sweep and their switches:
//!
//! * the write checks the run against the caller: the run must belong to the token's org (a run of another
//!   org reads as not found), to the named user when one is named, and must be `persistent`. A ZDR run and a
//!   waived run (DEC-32, decision D13) are refused with `zdr_durable_mutation_forbidden` before anything is
//!   written, whatever inference-core decided;
//! * the system prompt and the tool catalogue are stored once per run, keyed by the sha256 of their full
//!   text (`ON CONFLICT DO NOTHING`), and every call references them by digest;
//! * a call with `call_seq` 0 is numbered next in its run, under the run's row lock;
//! * snapshots inference-core could not deliver are added to `runs.metadata.snapshots_dropped`, so a later
//!   reader can say the record is incomplete;
//! * every row expires after `SESSION_CORE_REQUEST_SNAPSHOT_TTL_DAYS` (default 7, clamped 1-30, decision D4),
//!   and the sweep deletes expired rows in batches.
//!
//! Nothing here logs any part of a snapshot or a blob.

use std::time::Duration;

use metrics::counter;
use mp_contracts::model_plane::v1 as pb;
use mp_contracts::request_snapshot::{self, sha256_hex, RequestSnapshotV1};
use sqlx::PgPool;
use tonic::Status;
use tracing::info;

use crate::retention_receipt::RetentionClass;

/// The writer switch. Off: `RecordRequestSnapshot` answers `FAILED_PRECONDITION`. Only the literal `true` enables it.
pub(crate) const ENABLED_ENV: &str = "SESSION_CORE_REQUEST_SNAPSHOTS_ENABLED";

/// The sweep switch. Off: the sweep task parks and reads nothing. Only the literal `true` enables it.
pub(crate) const SWEEP_ENABLED_ENV: &str = "SESSION_CORE_REQUEST_SNAPSHOTS_SWEEP_ENABLED";

/// Days a snapshot is kept.
pub(crate) const TTL_DAYS_ENV: &str = "SESSION_CORE_REQUEST_SNAPSHOT_TTL_DAYS";
pub(crate) const DEFAULT_TTL_DAYS: i32 = 7;
pub(crate) const MIN_TTL_DAYS: i32 = 1;
pub(crate) const MAX_TTL_DAYS: i32 = 30;

/// The only workload that may write snapshots, and the scope it needs. Both match `inference-core` in
/// `PLANE_SERVICE_PRINCIPALS_JSON`.
pub(crate) const WRITER_SERVICE: &str = "service:inference-core";
pub(crate) const WRITE_SCOPE: &str = "session:request-snapshots:write";

/// The largest snapshot JSON accepted. The manifest grows with the thread (about 120 bytes a message), so
/// this is far above a real call and still bounds a row.
const MAX_SNAPSHOT_BYTES: usize = 1024 * 1024;

/// How often the sweep runs, and how many rows one statement deletes.
const SWEEP_INTERVAL: Duration = Duration::from_secs(600);
pub(crate) const SWEEP_BATCH: i64 = 500;

/// The strict reading of a switch: only the literal `true` enables it.
fn switch_from(raw: Option<&str>) -> bool {
    raw.is_some_and(|value| value.trim() == "true")
}

pub(crate) fn enabled() -> bool {
    switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

pub(crate) fn sweep_enabled() -> bool {
    switch_from(std::env::var(SWEEP_ENABLED_ENV).ok().as_deref())
}

/// The retention in days: the default when unset or unreadable, clamped to 1-30 otherwise.
pub(crate) fn ttl_days_from(raw: Option<&str>) -> i32 {
    raw.and_then(|value| value.trim().parse::<i64>().ok())
        .map_or(DEFAULT_TTL_DAYS, |days| {
            i32::try_from(days.clamp(i64::from(MIN_TTL_DAYS), i64::from(MAX_TTL_DAYS)))
                .unwrap_or(DEFAULT_TTL_DAYS)
        })
}

pub(crate) fn ttl_days() -> i32 {
    ttl_days_from(std::env::var(TTL_DAYS_ENV).ok().as_deref())
}

/// Only inference-core's service principal, holding the write scope and a persistent token, may write.
///
/// # Errors
///
/// `zdr_durable_mutation_forbidden` for a ZDR token, `permission_denied` for any other caller.
#[allow(clippy::result_large_err)]
pub(crate) fn authorize_writer(caller: &crate::auth::VerifiedIdentity) -> Result<(), Status> {
    crate::auth::authorize_operation(caller, WRITE_SCOPE)?;
    if !caller.is_service() || caller.principal_id() != WRITER_SERVICE {
        return Err(Status::permission_denied(
            "only inference-core may record request snapshots",
        ));
    }
    caller.require_service_scope(WRITE_SCOPE)
}

/// A blob after its checks: the digest it is stored under, and its body.
struct Blob<'a> {
    kind: &'static str,
    digest: &'a str,
    body: &'a str,
    truncated: bool,
}

/// Check a blob against the digest the snapshot names. A blob that was not cut must hash to that digest, so a
/// caller cannot file one text under another's digest. A cut blob is hashed by its sender over the full text,
/// which is not here to check.
#[allow(clippy::result_large_err)]
fn checked_blob<'a>(
    kind: &'static str,
    digest: &'a str,
    body: &'a str,
    truncated: bool,
    cap: usize,
) -> Result<Option<Blob<'a>>, Status> {
    if body.len() > cap {
        return Err(Status::invalid_argument(format!("the {kind} blob exceeds its cap")));
    }
    if digest.is_empty() {
        if !body.is_empty() {
            return Err(Status::invalid_argument(format!(
                "the {kind} blob has no digest in the snapshot"
            )));
        }
        return Ok(None);
    }
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Status::invalid_argument(format!("the {kind} digest is not a sha256")));
    }
    if body.is_empty() {
        return Err(Status::invalid_argument(format!("the {kind} blob is missing")));
    }
    if !truncated && sha256_hex(body.as_bytes()) != digest {
        return Err(Status::invalid_argument(format!(
            "the {kind} blob does not match its digest"
        )));
    }
    Ok(Some(Blob {
        kind,
        digest,
        body,
        truncated,
    }))
}

/// Parse and check the snapshot JSON.
#[allow(clippy::result_large_err)]
fn parse_snapshot(raw: &str) -> Result<(RequestSnapshotV1, serde_json::Value), Status> {
    if raw.len() > MAX_SNAPSHOT_BYTES {
        return Err(Status::invalid_argument("snapshot_json exceeds its cap"));
    }
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|_| Status::invalid_argument("snapshot_json must be valid JSON"))?;
    let snapshot: RequestSnapshotV1 = serde_json::from_value(value.clone())
        .map_err(|_| Status::invalid_argument("snapshot_json is not a request-snapshot-v1"))?;
    if snapshot.schema != request_snapshot::SCHEMA {
        return Err(Status::invalid_argument("snapshot_json is not a request-snapshot-v1"));
    }
    Ok((snapshot, value))
}

/// Store one snapshot for `caller_org`, the org of the verified service token.
///
/// # Errors
///
/// `invalid_argument` for a malformed request, `not_found` for a run that is not the caller's org's,
/// `permission_denied` for a run of another user, `zdr_durable_mutation_forbidden` for a run that is not
/// persistent, and `internal` for a database error.
#[allow(clippy::result_large_err)]
pub(crate) async fn record(
    pool: &PgPool,
    caller_org: &str,
    ttl_days: i32,
    req: &pb::RecordRequestSnapshotRequest,
) -> Result<pb::RecordRequestSnapshotResponse, Status> {
    let run_id = req.run_id.trim();
    if run_id.is_empty() {
        return Err(Status::invalid_argument("run_id is required"));
    }
    if req.org_id != caller_org {
        return Err(Status::permission_denied(
            "the snapshot's org is not the token's org",
        ));
    }
    let (snapshot, snapshot_value) = parse_snapshot(&req.snapshot_json)?;
    let system = checked_blob(
        "system",
        &snapshot.system_digest,
        &req.system_text,
        req.system_truncated,
        request_snapshot::SYSTEM_CAP_BYTES,
    )?;
    let tools = checked_blob(
        "tools",
        &snapshot.tools_digest,
        &req.tools_json,
        req.tools_truncated,
        request_snapshot::TOOLS_CAP_BYTES,
    )?;
    let call_seq = i32::try_from(req.call_seq)
        .map_err(|_| Status::invalid_argument("call_seq is out of range"))?;

    let mut tx = pool.begin().await.map_err(db_error)?;
    // The row lock serializes the numbering of one run's calls.
    let run: Option<(String, String, String)> = sqlx::query_as(
        "SELECT org_id, user_id, retention_class FROM runs WHERE id = $1 FOR UPDATE",
    )
    .bind(run_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_error)?;
    let Some((run_org, run_user, retention_class)) = run else {
        return Err(Status::not_found("run not found"));
    };
    // Another org's run reads as absent, never as a refusal that confirms it exists.
    if run_org != caller_org {
        return Err(Status::not_found("run not found"));
    }
    if !req.user_id.is_empty() && req.user_id != run_user {
        return Err(Status::permission_denied(
            "the snapshot's user does not own the run",
        ));
    }
    // A ZDR run and a waived run keep nothing in a new store (DEC-32, D13).
    if RetentionClass::from_storage(&retention_class) != Some(RetentionClass::Persistent) {
        return Err(mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN
            .grpc_status(tonic::Code::FailedPrecondition));
    }

    let call_seq = if call_seq == 0 {
        sqlx::query_scalar::<_, i32>(
            "SELECT COALESCE(MAX(call_seq), 0) + 1 FROM run_request_snapshots WHERE run_id = $1",
        )
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?
    } else {
        call_seq
    };

    let mut deduplicated = [false, false];
    for (slot, blob) in [system, tools].into_iter().enumerate() {
        let Some(blob) = blob else {
            continue;
        };
        let inserted = sqlx::query(
            "INSERT INTO run_request_snapshot_blobs
                (run_id, digest, kind, org_id, body, bytes, truncated, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, now() + make_interval(days => $8))
             ON CONFLICT (run_id, digest) DO NOTHING",
        )
        .bind(run_id)
        .bind(blob.digest)
        .bind(blob.kind)
        .bind(&run_org)
        .bind(blob.body)
        .bind(i32::try_from(blob.body.len()).unwrap_or(i32::MAX))
        .bind(blob.truncated)
        .bind(ttl_days)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?
        .rows_affected();
        if let Some(flag) = deduplicated.get_mut(slot) {
            *flag = inserted == 0;
        }
    }

    let stored = sqlx::query(
        "INSERT INTO run_request_snapshots
            (id, run_id, org_id, user_id, call_seq, call_purpose, request_id, snapshot,
             system_digest, tools_digest, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, now() + make_interval(days => $11))
         ON CONFLICT (run_id, call_seq) DO NOTHING",
    )
    .bind(format!("rqs_{}", mp_ids::new_ulid()))
    .bind(run_id)
    .bind(&run_org)
    .bind(&run_user)
    .bind(call_seq)
    .bind(request_snapshot::purpose_name(req.purpose))
    .bind(&req.request_id)
    .bind(&snapshot_value)
    .bind(&snapshot.system_digest)
    .bind(&snapshot.tools_digest)
    .bind(ttl_days)
    .execute(&mut *tx)
    .await
    .map_err(db_error)?
    .rows_affected()
        > 0;

    if req.dropped_before > 0 {
        sqlx::query(
            "UPDATE runs SET metadata = jsonb_set(
                 COALESCE(metadata, '{}'::jsonb),
                 '{snapshots_dropped}',
                 to_jsonb(COALESCE((metadata->>'snapshots_dropped')::bigint, 0) + $2)
             )
             WHERE id = $1",
        )
        .bind(run_id)
        .bind(i64::from(req.dropped_before))
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    }
    tx.commit().await.map_err(db_error)?;

    let [deduplicated_system, deduplicated_tools] = deduplicated;
    Ok(pb::RecordRequestSnapshotResponse {
        stored,
        deduplicated_system,
        deduplicated_tools,
    })
}

fn db_error(error: sqlx::Error) -> Status {
    Status::internal(error.to_string())
}

/// Counts of one person's request-snapshot erasure (D-ZDR-06). Counts only: no id, digest or text.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UserSnapshotErasure {
    /// `run_request_snapshots` rows of the person deleted.
    pub snapshots: u64,
    /// `run_request_snapshot_blobs` rows deleted because no remaining snapshot references them.
    pub blobs: u64,
}

/// Delete one person's request snapshots, and the blobs no remaining snapshot references, inside the
/// caller's erasure transaction (ZDR Q-ERASE, decision D-ZDR-06).
///
/// A snapshot row carries the `org_id` and `user_id` copied from its run, so the delete is by both. A
/// blob has no user: it is keyed `(run_id, digest)` and is referenced by digest from the snapshots of
/// its own run. After the snapshot delete, a blob of an affected run is removed only when no snapshot
/// that remains names its digest, so a blob another snapshot still uses is never removed. An empty
/// `user_id` matches nothing: rows written without a user are not any one person's.
///
/// Counts only are returned; nothing here logs or returns an id or any text.
///
/// # Errors
///
/// A database error; the caller's transaction then rolls back with the rest of the erasure.
pub(crate) async fn erase_user_snapshots_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: &str,
    user_id: &str,
) -> Result<UserSnapshotErasure, sqlx::Error> {
    if user_id.is_empty() {
        return Ok(UserSnapshotErasure::default());
    }
    let run_ids: Vec<String> = sqlx::query_scalar(
        "DELETE FROM run_request_snapshots WHERE org_id = $1 AND user_id = $2 RETURNING run_id",
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_all(&mut **tx)
    .await?;
    let snapshots = run_ids.len() as u64;
    if run_ids.is_empty() {
        return Ok(UserSnapshotErasure::default());
    }
    let mut affected_runs = run_ids;
    affected_runs.sort();
    affected_runs.dedup();
    let blobs = sqlx::query(
        "DELETE FROM run_request_snapshot_blobs b
         WHERE b.org_id = $1 AND b.run_id = ANY($2)
           AND NOT EXISTS (
               SELECT 1 FROM run_request_snapshots s
               WHERE s.run_id = b.run_id
                 AND (s.system_digest = b.digest OR s.tools_digest = b.digest)
           )",
    )
    .bind(org_id)
    .bind(&affected_runs)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    Ok(UserSnapshotErasure { snapshots, blobs })
}

/// Delete expired snapshot rows and blobs, `batch` rows per statement, until a statement deletes fewer than a
/// full batch. Returns the number deleted.
///
/// # Errors
///
/// A database error; rows already deleted stay deleted.
pub(crate) async fn sweep_once(pool: &PgPool, batch: i64) -> Result<u64, sqlx::Error> {
    let mut total = 0_u64;
    for statement in [
        "DELETE FROM run_request_snapshots
         WHERE id IN (
             SELECT id FROM run_request_snapshots
             WHERE expires_at < now()
             ORDER BY expires_at
             LIMIT $1
         )",
        "DELETE FROM run_request_snapshot_blobs
         WHERE (run_id, digest) IN (
             SELECT run_id, digest FROM run_request_snapshot_blobs
             WHERE expires_at < now()
             ORDER BY expires_at
             LIMIT $1
         )",
    ] {
        loop {
            let deleted = sqlx::query(statement)
                .bind(batch)
                .execute(pool)
                .await?
                .rows_affected();
            total += deleted;
            if i64::try_from(deleted).unwrap_or(i64::MAX) < batch {
                break;
            }
        }
    }
    Ok(total)
}

/// The retention sweep: every ten minutes, until the process stops.
///
/// # Errors
///
/// Never returns `Ok`; a database error ends the pass and the supervisor restarts it.
pub(crate) async fn run_sweeper(pool: PgPool) -> anyhow::Result<()> {
    info!("session-core request snapshot sweep started");
    loop {
        let deleted = sweep_once(&pool, SWEEP_BATCH).await?;
        if deleted > 0 {
            info!(deleted, "request snapshot sweep");
            counter!("mp_session_request_snapshots_swept_total").increment(deleted);
        }
        tokio::time::sleep(SWEEP_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loop_checkpoint::test_support::{scratch_pool, seed_run};
    use mp_contracts::request_snapshot::{MessageManifestEntry, SnapshotParams};

    const SYSTEM: &str = "You are Verevon.";

    fn snapshot_json(system: &str) -> String {
        RequestSnapshotV1 {
            schema: request_snapshot::SCHEMA.to_owned(),
            model: "gpt-5.1".to_owned(),
            provider: "azure-openai".to_owned(),
            params: SnapshotParams::default(),
            system_digest: sha256_hex(system.as_bytes()),
            messages: vec![MessageManifestEntry::of("system", "", false, system)],
            ..RequestSnapshotV1::default()
        }
        .to_canonical_json()
    }

    fn request(run_id: &str, org: &str, user: &str) -> pb::RecordRequestSnapshotRequest {
        pb::RecordRequestSnapshotRequest {
            run_id: run_id.to_owned(),
            purpose: pb::InferPurpose::AgentRound as i32,
            request_id: "req-1".to_owned(),
            snapshot_json: snapshot_json(SYSTEM),
            system_text: SYSTEM.to_owned(),
            org_id: org.to_owned(),
            user_id: user.to_owned(),
            ..pb::RecordRequestSnapshotRequest::default()
        }
    }

    async fn count(pool: &PgPool, table: &str, run_id: &str) -> i64 {
        // `table` is one of two constants named by the tests below, never input.
        sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE run_id = $1"))
            .bind(run_id)
            .fetch_one(pool)
            .await
            .expect("count")
    }

    #[test]
    fn switches_take_only_the_literal_true() {
        for raw in [None, Some(""), Some("1"), Some("on"), Some("TRUE"), Some("yes")] {
            assert!(!switch_from(raw), "{raw:?}");
        }
        assert!(switch_from(Some("true")));
        assert!(switch_from(Some(" true ")));
    }

    #[test]
    fn snapshot_ttl_defaults_to_seven_and_clamps_to_one_through_thirty() {
        assert_eq!(ttl_days_from(None), 7);
        assert_eq!(ttl_days_from(Some("nonsense")), 7);
        assert_eq!(ttl_days_from(Some("0")), 1);
        assert_eq!(ttl_days_from(Some("-3")), 1);
        assert_eq!(ttl_days_from(Some("14")), 14);
        assert_eq!(ttl_days_from(Some("31")), 30);
        assert_eq!(ttl_days_from(Some("99999999999")), 30);
    }

    #[test]
    fn snapshot_blob_must_match_its_digest_unless_cut() {
        let digest = sha256_hex(SYSTEM.as_bytes());
        assert!(checked_blob("system", &digest, SYSTEM, false, 1024).is_ok());
        assert!(checked_blob("system", &digest, "another text", false, 1024).is_err());
        // A cut blob was hashed over its full text by the sender.
        assert!(checked_blob("system", &digest, "You are", true, 1024).is_ok());
        assert!(checked_blob("system", "", "text without digest", false, 1024).is_err());
        assert!(checked_blob("system", "", "", false, 1024).expect("none").is_none());
        assert!(checked_blob("system", "short", SYSTEM, false, 1024).is_err());
        assert!(checked_blob("system", &digest, SYSTEM, false, 4).is_err(), "over the cap");
    }

    #[test]
    fn snapshot_json_must_be_request_snapshot_v1() {
        assert!(parse_snapshot(&snapshot_json(SYSTEM)).is_ok());
        assert!(parse_snapshot("not json").is_err());
        assert!(parse_snapshot(r#"{"schema":"request-snapshot-v2"}"#).is_err());
    }

    #[test]
    fn only_inference_core_may_write() {
        use crate::auth::VerifiedIdentity;
        let writer =
            VerifiedIdentity::service_for_test_as("org-a", WRITER_SERVICE, &[WRITE_SCOPE], false);
        assert!(authorize_writer(&writer).is_ok());
        let other = VerifiedIdentity::service_for_test_as(
            "org-a",
            "service:execution-core",
            &[WRITE_SCOPE],
            false,
        );
        assert_eq!(authorize_writer(&other).unwrap_err().code(), tonic::Code::PermissionDenied);
        let unscoped = VerifiedIdentity::service_for_test_as("org-a", WRITER_SERVICE, &[], false);
        assert!(authorize_writer(&unscoped).is_err());
        let zdr = VerifiedIdentity::service_for_test_as("org-a", WRITER_SERVICE, &[WRITE_SCOPE], true);
        assert_eq!(authorize_writer(&zdr).unwrap_err().code(), tonic::Code::FailedPrecondition);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn record_snapshot_dedups_system_per_run() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        let first = record(&pool, &org, 7, &request(&run, &org, &user)).await.expect("first");
        assert!(first.stored && !first.deduplicated_system);
        let second = record(&pool, &org, 7, &request(&run, &org, &user)).await.expect("second");
        assert!(second.stored && second.deduplicated_system, "the same prompt is stored once");
        assert_eq!(count(&pool, "run_request_snapshots", &run).await, 2);
        assert_eq!(count(&pool, "run_request_snapshot_blobs", &run).await, 1);
        let seqs: Vec<i32> = sqlx::query_scalar(
            "SELECT call_seq FROM run_request_snapshots WHERE run_id = $1 ORDER BY call_seq",
        )
        .bind(&run)
        .fetch_all(&pool)
        .await
        .expect("seqs");
        assert_eq!(seqs, vec![1, 2], "calls are numbered in their run");
        let (purpose, ttl_ok): (String, bool) = sqlx::query_as(
            "SELECT call_purpose, expires_at BETWEEN now() + interval '6 days 23 hours' AND now() + interval '7 days 1 hour'
             FROM run_request_snapshots WHERE run_id = $1 AND call_seq = 1",
        )
        .bind(&run)
        .fetch_one(&pool)
        .await
        .expect("row");
        assert_eq!(purpose, "agent_round");
        assert!(ttl_ok, "expires seven days out");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn record_snapshot_counts_dropped_writes_on_the_run() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        let mut req = request(&run, &org, &user);
        req.dropped_before = 3;
        record(&pool, &org, 7, &req).await.expect("record");
        record(&pool, &org, 7, &req).await.expect("record");
        let dropped: i64 = sqlx::query_scalar(
            "SELECT (metadata->>'snapshots_dropped')::bigint FROM runs WHERE id = $1",
        )
        .bind(&run)
        .fetch_one(&pool)
        .await
        .expect("dropped");
        assert_eq!(dropped, 6);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn record_snapshot_refuses_zdr_run() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "zdr").await;
        let error = record(&pool, &org, 7, &request(&run, &org, &user)).await.unwrap_err();
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        assert_eq!(count(&pool, "run_request_snapshots", &run).await, 0);
        assert_eq!(count(&pool, "run_request_snapshot_blobs", &run).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn record_snapshot_refuses_waived_run() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "zdr_waived").await;
        let error = record(&pool, &org, 7, &request(&run, &org, &user)).await.unwrap_err();
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        assert_eq!(count(&pool, "run_request_snapshots", &run).await, 0);
        assert_eq!(count(&pool, "run_request_snapshot_blobs", &run).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn record_snapshot_foreign_org_refused() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        // A token of another org naming this run: the run reads as absent.
        let foreign = record(&pool, "org-other", 7, &request(&run, "org-other", &user))
            .await
            .unwrap_err();
        assert_eq!(foreign.code(), tonic::Code::NotFound);
        // A request whose org is not the token's.
        let mismatched = record(&pool, "org-other", 7, &request(&run, &org, &user))
            .await
            .unwrap_err();
        assert_eq!(mismatched.code(), tonic::Code::PermissionDenied);
        // Another user of the right org.
        let other_user = record(&pool, &org, 7, &request(&run, &org, "u-other"))
            .await
            .unwrap_err();
        assert_eq!(other_user.code(), tonic::Code::PermissionDenied);
        assert_eq!(count(&pool, "run_request_snapshots", &run).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn snapshot_sweep_deletes_expired() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        record(&pool, &org, 1, &request(&run, &org, &user)).await.expect("record");
        let mut kept = request(&run, &org, &user);
        kept.snapshot_json = snapshot_json("Another prompt.");
        kept.system_text = "Another prompt.".to_owned();
        record(&pool, &org, 30, &kept).await.expect("record");
        for table in ["run_request_snapshots", "run_request_snapshot_blobs"] {
            sqlx::query(&format!(
                "UPDATE {table} SET expires_at = now() - interval '1 minute'
                 WHERE run_id = $1 AND expires_at < now() + interval '2 days'"
            ))
            .bind(&run)
            .execute(&pool)
            .await
            .expect("back-date");
        }
        let deleted = sweep_once(&pool, 1).await.expect("sweep");
        assert!(deleted >= 2, "one snapshot and one blob were expired");
        assert_eq!(count(&pool, "run_request_snapshots", &run).await, 1);
        assert_eq!(count(&pool, "run_request_snapshot_blobs", &run).await, 1);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn snapshots_cascade_on_thread_delete() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        record(&pool, &org, 7, &request(&run, &org, &user)).await.expect("record");
        // Thread delete removes the thread's runs; the snapshots go with them.
        sqlx::query("DELETE FROM runs WHERE id = $1")
            .bind(&run)
            .execute(&pool)
            .await
            .expect("delete run");
        assert_eq!(count(&pool, "run_request_snapshots", &run).await, 0);
        assert_eq!(count(&pool, "run_request_snapshot_blobs", &run).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn org_purge_deletes_snapshots() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        let (other_run, other_org, other_user) = seed_run(&pool, "persistent").await;
        record(&pool, &org, 7, &request(&run, &org, &user)).await.expect("record");
        record(&pool, &other_org, 7, &request(&other_run, &other_org, &other_user))
            .await
            .expect("record");
        let summary = crate::gdpr::purge_organization_data(&pool, &org, None)
            .await
            .expect("purge");
        assert_eq!(summary.run_request_snapshots, 1);
        assert_eq!(summary.run_request_snapshot_blobs, 1);
        assert_eq!(count(&pool, "run_request_snapshots", &run).await, 0);
        assert_eq!(count(&pool, "run_request_snapshots", &other_run).await, 1, "another org is untouched");
        crate::gdpr::purge_organization_data(&pool, &other_org, None)
            .await
            .expect("cleanup");
    }
}
