//! gRPC handler for `RunLedgerService` (EP-14): what a run did and what its model calls were sent.
//!
//! It serves the request snapshot write (S6), and the run timing write and read (S5). The snapshot store, its
//! checks and its sweep live in [`crate::request_snapshots`]; this module authorizes the caller and applies the
//! switches, and holds the timing store, which is two plain statements over tables that already exist:
//!
//! * a timing record is one `RUN_TIMING_RECORDED` row in `events`, so thread delete and org purge erase it with
//!   the run's other events. Only the run's owner writes it (model-gateway forwards the user's bearer), only for
//!   a persistent run: a ZDR or waived run is refused with `zdr_durable_mutation_forbidden` (DEC-32, D13);
//! * a tool call's times are read from `session_tool_audit_intents`, content-free columns only (never the
//!   argument digest), at most [`MAX_TIMING_ACTIONS`] per read.
//!
//! The read is owner only and answers UNIMPLEMENTED unless `SESSION_CORE_RUN_LEDGER_READ` is on.
//!
//! The request inspector read (S7) returns one stored snapshot to the user who owns the run, never to an org
//! admin, a service or a reader of a system-owned run. Decision D3 is applied here, not in the client: the
//! system prompt is split back into its system messages by the snapshot's manifest, and a platform section
//! keeps only its label, digest and length. It answers UNIMPLEMENTED unless `SESSION_CORE_REQUEST_INSPECTOR_READ`
//! is on.

// tonic::Status is the unavoidable large Err for gRPC; boxing breaks the service-trait contract.
#![allow(clippy::result_large_err)]

use std::time::Instant;

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{
    self as pb,
    run_ledger_service_server::{RunLedgerService, RunLedgerServiceServer},
};
use mp_contracts::zdr_refusal::Refusal;
use mp_events::idempotency::derive_idempotency_hash;
use sqlx::PgPool;
use tonic::{Request, Response, Status};

use crate::auth::{OwnerIntent, VerifiedIdentity};
use crate::retention_receipt::RetentionClass;

/// The read switch (EP-14 S5). Off: `GetRunTiming` answers UNIMPLEMENTED. Only the literal `true` enables it.
pub(crate) const RUN_LEDGER_READ_ENV: &str = "SESSION_CORE_RUN_LEDGER_READ";

/// The inspector read switch (EP-14 S7). Off: `GetRequestSnapshot` answers UNIMPLEMENTED. Separate from the
/// timing read because it returns prompt text, so it waits for decision D3. Only the literal `true` enables it.
pub(crate) const REQUEST_INSPECTOR_READ_ENV: &str = "SESSION_CORE_REQUEST_INSPECTOR_READ";

/// The `events.event_type` of a timing record (`EventType::RunTimingRecorded`).
const RUN_TIMING_EVENT_TYPE: &str = "RUN_TIMING_RECORDED";
const RUN_TIMING_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.RunTimingRecorded";

/// The most tool calls one read returns; more sets `truncated`.
pub(crate) const MAX_TIMING_ACTIONS: i64 = 500;

/// The most timing records one read returns. A run has one per turn or invocation, so this is never reached
/// by a real run; it bounds the read all the same.
const MAX_TIMING_RECORDS: i64 = 200;

fn record_metrics(method: &'static str, started: Instant, result: &Result<impl Sized, Status>) {
    let status = match result {
        Ok(_) => "ok",
        Err(error) if error.code() == tonic::Code::FailedPrecondition => "refused",
        Err(_) => "error",
    };
    metrics::counter!(
        "mp_session_run_ledger_grpc_requests_total",
        "method" => method,
        "status" => status,
    )
    .increment(1);
    metrics::histogram!(
        "mp_session_run_ledger_grpc_request_duration_seconds",
        "method" => method,
    )
    .record(started.elapsed().as_secs_f64());
}

pub struct RunLedgerGrpc {
    pool: PgPool,
    /// `SESSION_CORE_REQUEST_SNAPSHOTS_ENABLED`, read once at boot.
    snapshots_enabled: bool,
    /// `SESSION_CORE_REQUEST_SNAPSHOT_TTL_DAYS`, read once at boot and clamped.
    ttl_days: i32,
    /// `SESSION_CORE_RUN_LEDGER_READ`, read once at boot.
    timing_read_enabled: bool,
    /// `SESSION_CORE_REQUEST_INSPECTOR_READ`, read once at boot.
    inspector_read_enabled: bool,
}

impl RunLedgerGrpc {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            snapshots_enabled: crate::request_snapshots::enabled(),
            ttl_days: crate::request_snapshots::ttl_days(),
            timing_read_enabled: mp_contracts::loop_fold::switch_from(
                std::env::var(RUN_LEDGER_READ_ENV).ok().as_deref(),
            ),
            inspector_read_enabled: mp_contracts::loop_fold::switch_from(
                std::env::var(REQUEST_INSPECTOR_READ_ENV).ok().as_deref(),
            ),
        }
    }

    /// Convenience for `grpc.rs`, which registers it with the auth interceptor.
    #[must_use]
    #[allow(dead_code)] // direct constructor retained for isolated service tests
    pub fn into_server(self) -> RunLedgerServiceServer<Self> {
        RunLedgerServiceServer::new(self)
    }
}

/// With the switch off the write refuses, so a deployment that never turns it on stores nothing.
fn require_snapshots(enabled: bool) -> Result<(), Status> {
    if enabled {
        return Ok(());
    }
    Err(Status::failed_precondition(
        "request snapshots are not enabled (SESSION_CORE_REQUEST_SNAPSHOTS_ENABLED)",
    ))
}

/// With the switch off the read is not served at all; model-gateway answers its route with a 404.
fn require_timing_read(enabled: bool) -> Result<(), Status> {
    if enabled {
        return Ok(());
    }
    Err(Status::unimplemented(
        "run timing is not enabled (SESSION_CORE_RUN_LEDGER_READ)",
    ))
}

/// With the switch off the inspector read is not served; model-gateway answers its route with a 404.
fn require_inspector_read(enabled: bool) -> Result<(), Status> {
    if enabled {
        return Ok(());
    }
    Err(Status::unimplemented(
        "request inspection is not enabled (SESSION_CORE_REQUEST_INSPECTOR_READ)",
    ))
}

fn db_error(error: sqlx::Error) -> Status {
    Status::internal(error.to_string())
}

/// The run's org, owner and retention class, after the owner check. A run of another org or user reads as
/// absent, never as a refusal that confirms it exists.
async fn owned_run(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    run_id: &str,
    intent: OwnerIntent,
) -> Result<(String, String, Option<RetentionClass>), Status> {
    let run: Option<(String, String, String)> =
        sqlx::query_as("SELECT org_id, user_id, retention_class FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_optional(pool)
            .await
            .map_err(db_error)?;
    let Some((org_id, user_id, retention_class)) = run else {
        return Err(Status::not_found("run not found"));
    };
    crate::auth::authorize_owner_row(caller, &org_id, &user_id, intent).map_err(|error| {
        if error.code() == tonic::Code::PermissionDenied {
            Status::not_found("run not found")
        } else {
            error
        }
    })?;
    Ok((org_id, user_id, RetentionClass::from_storage(&retention_class)))
}

/// Store one timing record as a `RUN_TIMING_RECORDED` event of its run.
///
/// # Errors
///
/// `invalid_argument` for a malformed record, `not_found` for a run that is not the caller's,
/// `zdr_durable_mutation_forbidden` for a ZDR caller or a run that is not persistent, `internal` for a
/// database error.
pub(crate) async fn record_run_timing(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: &pb::RecordRunTimingRequest,
) -> Result<pb::RecordRunTimingResponse, Status> {
    // A ZDR credential never creates durable state; the gateway does not call for one, and this is the floor.
    crate::auth::authorize_operation(caller, "session:write")?;
    let timing = req
        .timing
        .as_ref()
        .ok_or_else(|| Status::invalid_argument("timing is required"))?;
    let report = mp_contracts::run_timing::validate(timing)
        .map_err(|rejected| Status::invalid_argument(rejected.0))?;
    let (org_id, user_id, retention) =
        owned_run(pool, caller, &timing.run_id, OwnerIntent::Mutate).await?;
    // A ZDR run and a waived run keep nothing in a new store (DEC-32, D13).
    if retention != Some(RetentionClass::Persistent) {
        return Err(Refusal::DURABLE_MUTATION_FORBIDDEN.grpc_status(tonic::Code::FailedPrecondition));
    }

    let resource = format!("run:{}", timing.run_id);
    let idempotency_key = derive_idempotency_hash(
        "session-core",
        RUN_TIMING_EVENT_TYPE,
        &resource,
        &format!("{}:{}:{}", timing.run_id, timing.request_id, timing.schema),
    );
    let inserted = sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         VALUES ($1, $2, $3, $4, now(), $5, $6, $3, '', $7, $8, $9, 'session-core', 1)
         ON CONFLICT (org_id, idempotency_key) WHERE idempotency_key <> '' DO NOTHING",
    )
    .bind(mp_ids::new_ulid())
    .bind(RUN_TIMING_EVENT_TYPE)
    .bind(&timing.run_id)
    .bind(serde_json::json!({
        "schema": &timing.schema,
        "request_id": &timing.request_id,
        "phases_dropped": timing.phases_dropped,
        "report": report,
    }))
    .bind(&org_id)
    .bind(&user_id)
    .bind(&idempotency_key)
    .bind(&resource)
    .bind(RUN_TIMING_TYPE_URL)
    .execute(pool)
    .await
    .map_err(db_error)?;
    Ok(pb::RecordRunTimingResponse {
        stored: inserted.rows_affected() == 1,
    })
}

/// One stored record back in its wire shape. A payload that is not one this module wrote is skipped.
fn timing_from_row(run_id: &str, payload: &serde_json::Value, ts: DateTime<Utc>) -> Option<pb::RunTimingRecorded> {
    let report = payload.get("report").filter(|report| report.is_object())?;
    Some(pb::RunTimingRecorded {
        run_id: run_id.to_owned(),
        request_id: payload.get("request_id")?.as_str()?.to_owned(),
        schema: payload.get("schema")?.as_str()?.to_owned(),
        report_json: report.to_string(),
        phases_dropped: payload
            .get("phases_dropped")
            .and_then(serde_json::Value::as_u64)
            .and_then(|dropped| u32::try_from(dropped).ok())
            .unwrap_or(0),
        recorded_at: Some(crate::grpc::to_proto_timestamp(ts)),
    })
}

type ActionRow = (String, String, String, DateTime<Utc>, Option<DateTime<Utc>>, Option<String>);

/// Read a run's timing records and its tool calls' times, for the run's owner.
///
/// # Errors
///
/// `zdr_feature_not_supported` (`run_inspection`) for a ZDR caller, `invalid_argument` without a run id,
/// `not_found` for a run that is not the caller's, `internal` for a database error.
pub(crate) async fn get_run_timing(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    run_id: &str,
) -> Result<pb::GetRunTimingResponse, Status> {
    crate::auth::authorize_operation(caller, "session:read")?;
    if caller.zdr() {
        return Err(Refusal::feature_not_supported("run_inspection")
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    let run_id = run_id.trim();
    if run_id.is_empty() {
        return Err(Status::invalid_argument("run_id is required"));
    }
    let (_, _, retention) = owned_run(pool, caller, run_id, OwnerIntent::Read).await?;
    // A ZDR or waived run has no timing record, and its tool calls are not shown either (D13).
    if retention != Some(RetentionClass::Persistent) {
        return Ok(pb::GetRunTimingResponse::default());
    }

    let rows: Vec<(serde_json::Value, DateTime<Utc>)> = sqlx::query_as(
        "SELECT payload, ts FROM events
         WHERE run_id = $1 AND event_type = $2
         ORDER BY ts, id
         LIMIT $3",
    )
    .bind(run_id)
    .bind(RUN_TIMING_EVENT_TYPE)
    .bind(MAX_TIMING_RECORDS)
    .fetch_all(pool)
    .await
    .map_err(db_error)?;
    let timings = rows
        .iter()
        .filter_map(|(payload, ts)| timing_from_row(run_id, payload, *ts))
        .collect();

    // One row past the cap says whether there are more.
    let mut actions: Vec<ActionRow> = sqlx::query_as(
        "SELECT action_id, tool, status, reserved_at, finalized_at, effect_class
         FROM session_tool_audit_intents
         WHERE run_id = $1
         ORDER BY reserved_at, action_id
         LIMIT $2",
    )
    .bind(run_id)
    .bind(MAX_TIMING_ACTIONS + 1)
    .fetch_all(pool)
    .await
    .map_err(db_error)?;
    let truncated = actions.len() > usize::try_from(MAX_TIMING_ACTIONS).unwrap_or(usize::MAX);
    actions.truncate(usize::try_from(MAX_TIMING_ACTIONS).unwrap_or(usize::MAX));
    Ok(pb::GetRunTimingResponse {
        timings,
        actions: actions
            .into_iter()
            .map(|(action_id, tool, status, reserved_at, finalized_at, effect_class)| pb::RunActionTiming {
                action_id,
                tool,
                status,
                reserved_at: Some(crate::grpc::to_proto_timestamp(reserved_at)),
                finalized_at: finalized_at.map(crate::grpc::to_proto_timestamp),
                effect_class: effect_class.unwrap_or_default(),
            })
            .collect(),
        truncated,
    })
}

/// Who authored a prompt section, for decision D3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SectionSource {
    Platform,
    Org,
    User,
    Memory,
}

impl SectionSource {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Platform => "platform",
            Self::Org => "org",
            Self::User => "user",
            Self::Memory => "memory",
        }
    }

    /// Decision D3 (b): platform text is a product asset and is shown as label, digest and length only.
    const fn shows_text(self) -> bool {
        !matches!(self, Self::Platform)
    }
}

/// The label table: a system message's source tag (its `name`, as the snapshot manifest keeps it), the label
/// the inspector shows and who authored it. Round A C6 adds EP-8's locale, tone and profile sections and EP-6's
/// agent role instructions. A tag that is not here, and an untagged message, is platform text: a new prompt
/// block stays hidden until someone classifies it here.
const SECTION_LABELS: &[(&str, &str, SectionSource)] = &[
    ("platform_instructions", "Platform instructions", SectionSource::Platform),
    ("locale", "Locale", SectionSource::Platform),
    ("tone", "Tone", SectionSource::Platform),
    ("org_instructions", "Organisation instructions", SectionSource::Org),
    ("space_instructions", "Space instructions", SectionSource::Org),
    ("agent_role_instructions", "Agent role instructions", SectionSource::Org),
    ("persona", "Persona", SectionSource::User),
    ("user_profile", "Your profile", SectionSource::User),
    ("compaction_summary", "Conversation summary", SectionSource::User),
    ("memory", "Memory", SectionSource::Memory),
];

/// The label and source of a system message: from the label table by its tag, else platform, labelled by the
/// prompt tier the caller placed it in.
fn section_label(source_tag: &str, tier: &str) -> (String, SectionSource) {
    SECTION_LABELS
        .iter()
        .find(|(tag, _, _)| *tag == source_tag)
        .map_or_else(
            || {
                let label = if source_tag.is_empty() { tier } else { source_tag };
                (label.to_owned(), SectionSource::Platform)
            },
            |(_, label, source)| ((*label).to_owned(), *source),
        )
}

/// `text` split after its first `chars` characters, or `None` when it is shorter.
fn split_chars(text: &str, chars: usize) -> Option<(&str, &str)> {
    match text.char_indices().nth(chars) {
        Some((at, _)) => Some(text.split_at(at)),
        None if text.chars().count() == chars => Some((text, "")),
        None => None,
    }
}

/// The separator inference-core joins system messages with (`request_capture::system_text`).
const SYSTEM_SEPARATOR: &str = "\n\n";

/// The system prompt of a call as sections, with decision D3 applied.
///
/// The stored blob is the call's system messages joined by [`SYSTEM_SEPARATOR`]; the manifest names each one's
/// length and sha256, in order. The blob is walked by those lengths and a piece is kept only when it hashes to
/// its manifest digest, so a cut blob, a reordered one or a manifest that does not describe it yields no text
/// from the first mismatch on. Text is then dropped for every platform section.
fn prompt_sections(
    snapshot: &mp_contracts::request_snapshot::RequestSnapshotV1,
    system_text: Option<&str>,
) -> Vec<pb::PromptSection> {
    let stable = usize::try_from(snapshot.prompt_tiers.stable).unwrap_or(usize::MAX);
    let context_end = stable.saturating_add(usize::try_from(snapshot.prompt_tiers.context).unwrap_or(0));
    let mut rest = system_text;
    let mut sections = Vec::new();
    for (index, entry) in snapshot.messages.iter().enumerate() {
        if entry.role != "system" {
            continue;
        }
        let tier = if index < stable {
            "stable"
        } else if index < context_end {
            "context"
        } else {
            "system"
        };
        let (label, source) = section_label(&entry.source_tag, tier);
        let piece = rest.and_then(|text| {
            let text = if sections.is_empty() {
                text
            } else {
                text.strip_prefix(SYSTEM_SEPARATOR)?
            };
            split_chars(text, usize::try_from(entry.chars).ok()?)
        });
        let verified = piece.filter(|(body, _)| {
            mp_contracts::request_snapshot::sha256_hex(body.as_bytes()) == entry.sha256
        });
        rest = verified.map(|(_, after)| after);
        sections.push(pb::PromptSection {
            label,
            source: source.as_str().to_owned(),
            digest: entry.sha256.clone(),
            chars: entry.chars,
            text: verified
                .filter(|_| source.shows_text())
                .map(|(body, _)| body.to_owned())
                .unwrap_or_default(),
        });
    }
    sections
}

type SnapshotRow = (serde_json::Value, String, String, String);

/// A stored blob of the run by digest: its body and whether it was cut.
async fn snapshot_blob(
    pool: &PgPool,
    run_id: &str,
    digest: &str,
    kind: &str,
) -> Result<Option<(String, bool)>, Status> {
    if digest.is_empty() {
        return Ok(None);
    }
    sqlx::query_as(
        "SELECT body, truncated FROM run_request_snapshot_blobs
         WHERE run_id = $1 AND digest = $2 AND kind = $3 AND expires_at > now()",
    )
    .bind(run_id)
    .bind(digest)
    .bind(kind)
    .fetch_optional(pool)
    .await
    .map_err(db_error)
}

/// Read one model call's snapshot for the user who owns the run, with decision D3 applied.
///
/// # Errors
///
/// `zdr_feature_not_supported` (`run_inspection`) for a ZDR caller, `invalid_argument` without a run id or
/// call number, `not_found` for a run that is not the caller's own (another user, an org admin, a service, a
/// system-owned run), a run that is not persistent or a call that has no snapshot, `internal` for a database
/// error.
pub(crate) async fn get_request_snapshot(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: &pb::GetRequestSnapshotRequest,
) -> Result<pb::GetRequestSnapshotResponse, Status> {
    crate::auth::authorize_operation(caller, "session:read")?;
    if caller.zdr() {
        return Err(Refusal::feature_not_supported("run_inspection")
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    let run_id = req.run_id.trim();
    if run_id.is_empty() {
        return Err(Status::invalid_argument("run_id is required"));
    }
    let call_seq = i32::try_from(req.call_seq)
        .ok()
        .filter(|seq| *seq > 0)
        .ok_or_else(|| Status::invalid_argument("call_seq must be 1 or more"))?;
    // Owner only, stricter than the owner check: the org-wide read a service or a system-owned run gets there
    // would widen who sees prompt text (plan 3.3.2 risk 2), so only the run's own user passes.
    if caller.is_service() {
        return Err(Status::not_found("run not found"));
    }
    let (_, run_user, retention) = owned_run(pool, caller, run_id, OwnerIntent::Read).await?;
    if crate::auth::is_system_run_owner(&run_user) || caller.user_id() != Some(run_user.as_str()) {
        return Err(Status::not_found("run not found"));
    }
    // A ZDR or waived run has no snapshot (D13); it reads like a call that was never recorded.
    if retention != Some(RetentionClass::Persistent) {
        return Err(Status::not_found("request snapshot not found"));
    }

    let row: Option<SnapshotRow> = sqlx::query_as(
        "SELECT snapshot, system_digest, tools_digest, call_purpose FROM run_request_snapshots
         WHERE run_id = $1 AND call_seq = $2 AND expires_at > now()",
    )
    .bind(run_id)
    .bind(call_seq)
    .fetch_optional(pool)
    .await
    .map_err(db_error)?;
    let Some((snapshot_value, system_digest, tools_digest, call_purpose)) = row else {
        return Err(Status::not_found("request snapshot not found"));
    };
    let snapshot: mp_contracts::request_snapshot::RequestSnapshotV1 =
        serde_json::from_value(snapshot_value.clone())
            .map_err(|_| Status::internal("stored snapshot is not a request-snapshot-v1"))?;
    let system = snapshot_blob(pool, run_id, &system_digest, "system").await?;
    let tools = snapshot_blob(pool, run_id, &tools_digest, "tools").await?;
    let dropped: i64 = sqlx::query_scalar(
        "SELECT COALESCE((metadata->>'snapshots_dropped')::bigint, 0) FROM runs WHERE id = $1",
    )
    .bind(run_id)
    .fetch_one(pool)
    .await
    .map_err(db_error)?;

    let (tools_json, tools_truncated) = tools.unwrap_or_default();
    Ok(pb::GetRequestSnapshotResponse {
        snapshot_json: snapshot_value.to_string(),
        system_sections: prompt_sections(&snapshot, system.as_ref().map(|(body, _)| body.as_str())),
        tools_json,
        dropped: u64::try_from(dropped).unwrap_or(0),
        tools_truncated,
        system_truncated: system.as_ref().is_some_and(|(_, truncated)| *truncated),
        call_purpose,
    })
}

#[tonic::async_trait]
impl RunLedgerService for RunLedgerGrpc {
    async fn record_request_snapshot(
        &self,
        request: Request<pb::RecordRequestSnapshotRequest>,
    ) -> Result<Response<pb::RecordRequestSnapshotResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = crate::auth::identity(&request)?;
            crate::request_snapshots::authorize_writer(&caller)?;
            require_snapshots(self.snapshots_enabled)?;
            let req = request.into_inner();
            crate::request_snapshots::record(&self.pool, caller.org_id(), self.ttl_days, &req)
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("record_request_snapshot", started, &result);
        result
    }

    async fn record_run_timing(
        &self,
        request: Request<pb::RecordRunTimingRequest>,
    ) -> Result<Response<pb::RecordRunTimingResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = crate::auth::identity(&request)?;
            record_run_timing(&self.pool, &caller, request.get_ref())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("record_run_timing", started, &result);
        result
    }

    async fn get_run_timing(
        &self,
        request: Request<pb::GetRunTimingRequest>,
    ) -> Result<Response<pb::GetRunTimingResponse>, Status> {
        let started = Instant::now();
        let result = async {
            require_timing_read(self.timing_read_enabled)?;
            let caller = crate::auth::identity(&request)?;
            get_run_timing(&self.pool, &caller, &request.get_ref().run_id)
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("get_run_timing", started, &result);
        result
    }

    async fn get_request_snapshot(
        &self,
        request: Request<pb::GetRequestSnapshotRequest>,
    ) -> Result<Response<pb::GetRequestSnapshotResponse>, Status> {
        let started = Instant::now();
        let result = async {
            require_inspector_read(self.inspector_read_enabled)?;
            let caller = crate::auth::identity(&request)?;
            get_request_snapshot(&self.pool, &caller, request.get_ref())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("get_request_snapshot", started, &result);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loop_checkpoint::test_support::{scratch_pool, seed_run};

    #[test]
    fn snapshots_off_refuses_the_write() {
        let error = require_snapshots(false).unwrap_err();
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        assert!(require_snapshots(true).is_ok());
    }

    #[test]
    fn timing_read_off_is_not_served() {
        assert_eq!(require_timing_read(false).unwrap_err().code(), tonic::Code::Unimplemented);
        assert!(require_timing_read(true).is_ok());
    }

    #[test]
    fn a_stored_record_reads_back_in_its_wire_shape() {
        let payload = serde_json::json!({
            "schema": "turn-timing-v1",
            "request_id": "req-1",
            "phases_dropped": 2,
            "report": {"server_ms": 40},
        });
        let at = DateTime::from_timestamp(1_800_000_000, 0).expect("time");
        let timing = timing_from_row("run-1", &payload, at).expect("record");
        assert_eq!(timing.schema, "turn-timing-v1");
        assert_eq!(timing.report_json, r#"{"server_ms":40}"#);
        assert_eq!(timing.phases_dropped, 2);
        assert!(timing_from_row("run-1", &serde_json::json!({"report": "text"}), at).is_none());
    }

    fn timing(run_id: &str, request_id: &str) -> pb::RecordRunTimingRequest {
        pb::RecordRunTimingRequest {
            timing: Some(pb::RunTimingRecorded {
                run_id: run_id.to_owned(),
                request_id: request_id.to_owned(),
                schema: mp_contracts::run_timing::SCHEMA_TURN.to_owned(),
                report_json: r#"{"server_ms":40,"phases":[]}"#.to_owned(),
                phases_dropped: 0,
                recorded_at: None,
            }),
        }
    }

    async fn seed_intents(pool: &PgPool, run_id: &str, count: usize) {
        for index in 0..count {
            sqlx::query(
                "INSERT INTO session_tool_audit_intents
                    (run_id, action_id, request_id, tool, data_category, zdr, status, reserved_at, finalized_at, effect_class)
                 VALUES ($1, $2, 'req-1', 'web_search', 'public', false, 'completed',
                         now() - make_interval(secs => $3), now(), 'read')",
            )
            .bind(run_id)
            .bind(format!("act-{index:04}"))
            .bind(f64::from(u32::try_from(count - index).unwrap_or(0)))
            .execute(pool)
            .await
            .expect("seed intent");
        }
    }

    async fn timing_rows(pool: &PgPool, run_id: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM events WHERE run_id = $1 AND event_type = $2")
            .bind(run_id)
            .bind(RUN_TIMING_EVENT_TYPE)
            .fetch_one(pool)
            .await
            .expect("count")
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn get_run_timing_reads_events_and_intents() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        let owner = VerifiedIdentity::user_for_test(&org, &user);
        let stored = record_run_timing(&pool, &owner, &timing(&run, "req-1")).await.expect("stored");
        assert!(stored.stored);
        let repeat = record_run_timing(&pool, &owner, &timing(&run, "req-1")).await.expect("repeat");
        assert!(!repeat.stored, "the same record is stored once");
        seed_intents(&pool, &run, 2).await;

        let read = get_run_timing(&pool, &owner, &run).await.expect("read");
        assert_eq!(read.timings.len(), 1);
        assert_eq!(read.timings[0].request_id, "req-1");
        assert!(read.timings[0].recorded_at.is_some());
        assert_eq!(read.actions.len(), 2);
        assert_eq!(read.actions[0].action_id, "act-0000", "oldest reservation first");
        assert_eq!(read.actions[0].effect_class, "read");
        assert!(!read.truncated);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn get_run_timing_foreign_owner_not_found() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, _) = seed_run(&pool, "persistent").await;
        let other_user = VerifiedIdentity::user_for_test(&org, "someone-else");
        let other_org = VerifiedIdentity::user_for_test("another-org", "u1");
        for caller in [other_user, other_org] {
            assert_eq!(
                get_run_timing(&pool, &caller, &run).await.unwrap_err().code(),
                tonic::Code::NotFound
            );
            assert_eq!(
                record_run_timing(&pool, &caller, &timing(&run, "req-1")).await.unwrap_err().code(),
                tonic::Code::NotFound
            );
        }
        assert_eq!(timing_rows(&pool, &run).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn get_run_timing_truncates_at_500() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        seed_intents(&pool, &run, 501).await;
        let read = get_run_timing(&pool, &VerifiedIdentity::user_for_test(&org, &user), &run)
            .await
            .expect("read");
        assert_eq!(read.actions.len(), 500);
        assert!(read.truncated);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn record_run_timing_refuses_zdr_and_waived_runs() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        for class in ["zdr", "zdr_waived"] {
            let (run, org, user) = seed_run(&pool, class).await;
            let owner = VerifiedIdentity::user_for_test(&org, &user);
            let error = record_run_timing(&pool, &owner, &timing(&run, "req-1")).await.unwrap_err();
            assert_eq!(error.code(), tonic::Code::FailedPrecondition, "{class}");
            assert_eq!(timing_rows(&pool, &run).await, 0, "{class}: nothing stored");
            let read = get_run_timing(&pool, &owner, &run).await.expect("read");
            assert!(read.timings.is_empty() && read.actions.is_empty(), "{class}");
        }
        // A ZDR credential is refused on a persistent run too, before any row is read.
        let (run, org, user) = seed_run(&pool, "persistent").await;
        let zdr = VerifiedIdentity::user_for_test_with_zdr(&org, &user, true);
        assert_eq!(
            record_run_timing(&pool, &zdr, &timing(&run, "req-1")).await.unwrap_err().code(),
            tonic::Code::FailedPrecondition
        );
        assert_eq!(
            get_run_timing(&pool, &zdr, &run).await.unwrap_err().code(),
            tonic::Code::FailedPrecondition
        );
        assert_eq!(timing_rows(&pool, &run).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn run_timing_rows_erased_with_thread() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        let owner = VerifiedIdentity::user_for_test(&org, &user);
        record_run_timing(&pool, &owner, &timing(&run, "req-1")).await.expect("stored");
        seed_intents(&pool, &run, 1).await;
        let thread_id: String = sqlx::query_scalar("SELECT thread_id FROM runs WHERE id = $1")
            .bind(&run)
            .fetch_one(&pool)
            .await
            .expect("thread");
        let mut tx = pool.begin().await.expect("tx");
        crate::grpc::delete_thread_rows(&mut tx, &thread_id).await.expect("delete");
        tx.commit().await.expect("commit");
        assert_eq!(timing_rows(&pool, &run).await, 0);
        let intents: i64 =
            sqlx::query_scalar("SELECT count(*) FROM session_tool_audit_intents WHERE run_id = $1")
                .bind(&run)
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(intents, 0);
    }

    const PLATFORM_TEXT: &str = "You are Verevon. Follow the house rules.";
    const MEMORY_TEXT: &str = "The user prefers metric units.";

    /// A call whose prompt is one untagged platform message, one memory message and one user turn.
    fn inspected_snapshot() -> mp_contracts::request_snapshot::RequestSnapshotV1 {
        use mp_contracts::request_snapshot::{MessageManifestEntry, RequestSnapshotV1, SnapshotPromptTiers};
        let system = format!("{PLATFORM_TEXT}{SYSTEM_SEPARATOR}{MEMORY_TEXT}");
        RequestSnapshotV1 {
            schema: mp_contracts::request_snapshot::SCHEMA.to_owned(),
            model: "gpt-5.1".to_owned(),
            provider: "azure-openai".to_owned(),
            system_digest: mp_contracts::request_snapshot::sha256_hex(system.as_bytes()),
            prompt_tiers: SnapshotPromptTiers { stable: 1, context: 0 },
            messages: vec![
                MessageManifestEntry::of("system", "", false, PLATFORM_TEXT),
                MessageManifestEntry::of("system", "memory", false, MEMORY_TEXT),
                MessageManifestEntry::of("user", "", false, "What is 5 miles in km?"),
            ],
            ..RequestSnapshotV1::default()
        }
    }

    fn inspected_system_text() -> String {
        format!("{PLATFORM_TEXT}{SYSTEM_SEPARATOR}{MEMORY_TEXT}")
    }

    #[test]
    fn inspector_read_off_is_not_served() {
        assert_eq!(require_inspector_read(false).unwrap_err().code(), tonic::Code::Unimplemented);
        assert!(require_inspector_read(true).is_ok());
    }

    #[test]
    fn inspector_sections_hide_platform_text_and_show_memory() {
        let sections = prompt_sections(&inspected_snapshot(), Some(&inspected_system_text()));
        assert_eq!(sections.len(), 2, "one section per system message");
        assert_eq!(sections[0].label, "stable");
        assert_eq!(sections[0].source, "platform");
        assert!(sections[0].text.is_empty(), "platform text never leaves session-core");
        assert_eq!(
            sections[0].digest,
            mp_contracts::request_snapshot::sha256_hex(PLATFORM_TEXT.as_bytes())
        );
        assert_eq!(sections[0].chars, u32::try_from(PLATFORM_TEXT.chars().count()).unwrap());
        assert_eq!(sections[1].label, "Memory");
        assert_eq!(sections[1].source, "memory");
        assert_eq!(sections[1].text, MEMORY_TEXT);
    }

    #[test]
    fn inspector_label_table_classifies_round_a_sections() {
        assert_eq!(section_label("user_profile", "context").1, SectionSource::User);
        assert_eq!(section_label("tone", "context").1, SectionSource::Platform);
        assert_eq!(section_label("locale", "context").1, SectionSource::Platform);
        assert_eq!(section_label("agent_role_instructions", "context").1, SectionSource::Org);
        let (label, source) = section_label("some_new_block", "context");
        assert_eq!((label.as_str(), source), ("some_new_block", SectionSource::Platform));
    }

    #[test]
    fn inspector_sections_show_no_text_after_a_mismatch() {
        // A cut blob: the memory section cannot be matched to its digest, so it carries none.
        let cut = format!("{PLATFORM_TEXT}{SYSTEM_SEPARATOR}The user prefers");
        let sections = prompt_sections(&inspected_snapshot(), Some(&cut));
        assert!(sections.iter().all(|section| section.text.is_empty()));
        // No blob at all: labels, digests and lengths only.
        let sections = prompt_sections(&inspected_snapshot(), None);
        assert_eq!(sections.len(), 2);
        assert!(sections.iter().all(|section| section.text.is_empty()));
        // A first section that does not match stops the walk before the memory section.
        let swapped = format!("{MEMORY_TEXT}{SYSTEM_SEPARATOR}{PLATFORM_TEXT}");
        let sections = prompt_sections(&inspected_snapshot(), Some(&swapped));
        assert!(sections.iter().all(|section| section.text.is_empty()));
    }

    /// Store the inspected snapshot as call 1 of `run` through the real writer.
    async fn seed_snapshot(pool: &PgPool, run: &str, org: &str, dropped_before: u32) {
        let snapshot = inspected_snapshot();
        crate::request_snapshots::record(
            pool,
            org,
            7,
            &pb::RecordRequestSnapshotRequest {
                run_id: run.to_owned(),
                call_seq: 1,
                snapshot_json: snapshot.to_canonical_json(),
                system_text: inspected_system_text(),
                org_id: org.to_owned(),
                dropped_before,
                ..pb::RecordRequestSnapshotRequest::default()
            },
        )
        .await
        .expect("seed snapshot");
    }

    fn inspect(run: &str) -> pb::GetRequestSnapshotRequest {
        pb::GetRequestSnapshotRequest {
            run_id: run.to_owned(),
            call_seq: 1,
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn inspector_hides_platform_text() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        seed_snapshot(&pool, &run, &org, 0).await;
        let read = get_request_snapshot(&pool, &VerifiedIdentity::user_for_test(&org, &user), &inspect(&run))
            .await
            .expect("read");
        assert_eq!(read.system_sections.len(), 2);
        assert_eq!(read.system_sections[0].source, "platform");
        assert!(read.system_sections[0].text.is_empty());
        assert_eq!(read.system_sections[1].text, MEMORY_TEXT);
        assert!(!read.snapshot_json.contains(PLATFORM_TEXT), "the manifest holds no body");
        assert_eq!(read.dropped, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn inspector_owner_only() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        seed_snapshot(&pool, &run, &org, 0).await;
        // Another member, a member of another org and an org-wide service all read it as absent.
        for caller in [
            VerifiedIdentity::user_for_test(&org, "someone-else"),
            VerifiedIdentity::user_for_test("another-org", "u1"),
            VerifiedIdentity::service_for_test(&org, &["session:read"], false),
        ] {
            assert_eq!(
                get_request_snapshot(&pool, &caller, &inspect(&run)).await.unwrap_err().code(),
                tonic::Code::NotFound
            );
        }
        // The owner asking for a call that was never recorded.
        let missing = pb::GetRequestSnapshotRequest {
            run_id: run.clone(),
            call_seq: 2,
        };
        assert_eq!(
            get_request_snapshot(&pool, &VerifiedIdentity::user_for_test(&org, &user), &missing)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::NotFound
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn inspector_reports_dropped() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        seed_snapshot(&pool, &run, &org, 3).await;
        let read = get_request_snapshot(&pool, &VerifiedIdentity::user_for_test(&org, &user), &inspect(&run))
            .await
            .expect("read");
        assert_eq!(read.dropped, 3);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn inspector_refuses_zdr_bearer() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        seed_snapshot(&pool, &run, &org, 0).await;
        let zdr = VerifiedIdentity::user_for_test_with_zdr(&org, &user, true);
        assert_eq!(
            get_request_snapshot(&pool, &zdr, &inspect(&run)).await.unwrap_err().code(),
            tonic::Code::FailedPrecondition
        );
        // A ZDR run holds no snapshot and reads as one never recorded.
        let (zdr_run, zdr_org, zdr_user) = seed_run(&pool, "zdr").await;
        assert_eq!(
            get_request_snapshot(&pool, &VerifiedIdentity::user_for_test(&zdr_org, &zdr_user), &inspect(&zdr_run))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::NotFound
        );
    }
}
