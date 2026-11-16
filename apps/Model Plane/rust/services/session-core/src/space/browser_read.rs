//! `SpaceBrowserRead`: the room-scoped browser run reads Session Core answers for model-gateway (build
//! plan WS20-S3 and WS20-S4). The count and the run's Space (S3) serve the start path and execution-core.
//! The list (R2), timeline (R3) and evidence check (R4) serve the room, and are the only reads a room
//! member reaches.
//!
//! The S3 reads carry the caller's verified user credential only. The S4 room reads carry the caller's
//! Control `model.thread.read` decision for the Space as well (WS20-S4, SD-23): Session Core verifies it
//! through `crate::grpc::verify_space_read_claims`, refuses a removed member, and bounds every row to the
//! decision's audience ceiling. Every read refuses a zero-retention credential first (typed `browser_run`).
//!
//! The room reads are a content-free projection (SD-23: receipts only). The projector keeps the run's ids,
//! the starter's subject id, the phase, counts, and hosts; a timeline row keeps the closed fields named in
//! [`project_event`] and nothing else. A goal, a URL, a page title, a rationale, a selector, a decided-by
//! subject, a frame and an evidence byte never leave this file.
//!
//! The start path's own cap check, [`ensure_room_browser_capacity`] (W3-FIX-6), re-runs the same count
//! inside the run's start transaction under a per-Space advisory lock, so two starts cannot both pass.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1 as pb;
use mp_contracts::model_plane::v1::space_browser_read_server::SpaceBrowserRead;
use mp_contracts::space_browser::capacity_admits;
use mp_contracts::zdr_refusal::Refusal;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use tonic::metadata::MetadataValue;
use tonic::{Request, Response, Status};

use crate::auth::{authorize_operation, identity, VerifiedIdentity};
use crate::run_phase::ERROR_CODE_TRAILER;

/// The scope a service caller holds for these reads. A user credential needs no scope
/// (`authorize_operation`); its org and, for [`resolve_run_space`], its ownership are checked instead.
const SPACE_BROWSER_READ_SCOPE: &str = "session:read";

/// Browser runs of one Space in flight. Only runs the gateway's browser source started count
/// (`configured_source`), and a run in a terminal state (`completed`, `failed`, `cancelled`) does not.
/// `$3` is the start key to leave out (empty leaves none out).
pub(crate) const COUNT_ACTIVE_ROOM_BROWSER_RUNS_SQL: &str = "SELECT COUNT(*)::BIGINT \
    FROM runs r \
    JOIN managed_run_terminalization_outbox o ON o.run_id = r.id \
    WHERE r.org_id = $1 \
      AND r.space_id = $2 \
      AND o.configured_source = 'gateway_browser' \
      AND r.status NOT IN ('completed', 'failed', 'cancelled') \
      AND ($3 = '' OR o.start_key <> $3)";

/// The Space a run was started in, or the empty string for a run outside any Space.
const RUN_SPACE_SQL: &str = "SELECT COALESCE(space_id, '') FROM runs WHERE id = $1 AND org_id = $2";

/// The handler state: a pool handle. Reads only; nothing is written here.
#[derive(Clone)]
pub(crate) struct SpaceBrowserReadGrpc {
    pool: PgPool,
}

impl SpaceBrowserReadGrpc {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// A zero-retention credential is refused before any read: the typed `browser_run` refusal.
#[allow(clippy::result_large_err)]
fn refuse_zero_retention(caller: &VerifiedIdentity) -> Result<(), Status> {
    if caller.zdr() {
        return Err(Refusal::feature_not_supported("browser_run")
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    Ok(())
}

/// The typed refusal code of a room browser start over its Space's cap (W3-FIX-6). It is the gRPC
/// message and the `x-mp-error-code` trailer; model-gateway answers 429 with it.
pub(crate) const BROWSER_CAPACITY_CODE: &str = "browser_capacity";

/// The refusal of a room browser start while its Space already has its runs in flight:
/// `resource_exhausted`, with the `browser_capacity` message and trailer.
pub(crate) fn browser_capacity_status() -> Status {
    let mut status = Status::resource_exhausted(BROWSER_CAPACITY_CODE);
    status.metadata_mut().insert(
        ERROR_CODE_TRAILER,
        MetadataValue::from_static(BROWSER_CAPACITY_CODE),
    );
    status
}

/// Admits a `gateway_browser` run on a Space thread into its Space, or refuses it at the cap
/// (SD-23: `ROOM_RUNS_PER_SPACE` runs in flight per Space). Call inside the start transaction, before
/// the run row is written. A thread outside any Space passes untouched, and so does a thread that is
/// not `user_id`'s: the start's own insert refuses that one.
///
/// The per-Space advisory lock is taken before the count and held until the start commits, so a
/// second start in the same Space waits and then counts the first run. The count is the same SQL the
/// model-gateway pre-check reads. A count that cannot be read admits nothing.
#[allow(clippy::result_large_err)]
pub(crate) async fn ensure_room_browser_capacity(
    transaction: &mut Transaction<'_, Postgres>,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
) -> Result<(), Status> {
    let space_id = sqlx::query_scalar::<_, Option<String>>(
        "SELECT space_id FROM threads WHERE id = $1 AND org_id = $2 AND user_id = $3",
    )
    .bind(thread_id)
    .bind(org_id)
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| {
        tracing::warn!(error = %error, "room browser capacity thread read failed");
        Status::unavailable("browser run capacity is unavailable")
    })?
    .flatten();
    let Some(space_id) = space_id.filter(|id| !id.trim().is_empty()) else {
        return Ok(());
    };
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("space-browser-run-cap\u{1f}{org_id}\u{1f}{space_id}"))
        .execute(&mut **transaction)
        .await
        .map_err(|error| {
            tracing::warn!(error = %error, "room browser capacity lock failed");
            Status::unavailable("browser run capacity is unavailable")
        })?;
    let active = sqlx::query_scalar::<_, i64>(COUNT_ACTIVE_ROOM_BROWSER_RUNS_SQL)
        .bind(org_id)
        .bind(&space_id)
        .bind("")
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| {
            tracing::warn!(error = %error, "room browser capacity count failed");
            Status::unavailable("browser run capacity is unavailable")
        })?;
    if capacity_admits(u32::try_from(active).unwrap_or(u32::MAX)) {
        Ok(())
    } else {
        Err(browser_capacity_status())
    }
}

/// The room read page sizes (WS20-S4): the list's default and most, and the timeline's.
const DEFAULT_LIST_LIMIT: u32 = 20;
const MAX_LIST_LIMIT: u32 = 50;
const DEFAULT_TIMELINE_LIMIT: u32 = 50;
const MAX_TIMELINE_LIMIT: u32 = 200;
/// The hosts one list row carries at most.
const MAX_ROW_HOSTS: usize = 8;
/// The longest a room read verdict holds, whatever the decision's own expiry (the caller asks again).
const MAX_VERDICT_SECONDS: i64 = 600;
/// The longest id a room read accepts.
const MAX_ID_BYTES: usize = 128;

/// The event types the timeline projects, in the order the projector reads them. The SQL filter and
/// [`project_event`] share this list, so a type outside it never reaches a caller.
const PROJECTED_EVENT_TYPES: &[&str] = &[
    "BROWSER_ACTION_DISPATCHED",
    "BROWSER_OBSERVATION_RECEIVED",
    "BROWSER_RUN_PAUSED",
    "BROWSER_RUN_RESUMED",
    "BROWSER_ACTION_APPROVAL_REQUIRED",
    "BROWSER_ACTION_DECIDED",
    // WS20-S7 names: not written until that slice lands (UNVERIFIED names, see the depth file).
    "BROWSER_HANDOFF_REQUESTED",
    "BROWSER_HANDOFF_RESOLVED",
    // The run's terminal rows, read for the `result` kind (the outcome only).
    "RUN_COMPLETED",
    "RUN_FAILED",
];

/// One room browser run, joined to its thread, when it is a root browser run of this Space and org inside the
/// decision's audience ceiling. Anything else is absent, so the caller answers NotFound: there is no oracle
/// for a run id the viewer may not see.
const ROOM_RUN_HEADER_SQL: &str = concat!(
    "SELECT r.id AS run_id, r.thread_id AS thread_id, r.user_id AS started_by, ",
    crate::run_phase::run_phase_sql!("r"),
    " AS phase, extract(epoch from r.created_at)::bigint AS started_at_unix, \
     COALESCE(extract(epoch from r.ended_at)::bigint, 0) AS finished_at_unix \
     FROM runs r \
     JOIN threads t ON t.id = r.thread_id \
     JOIN managed_run_terminalization_outbox o ON o.run_id = r.id \
     WHERE r.id = $1 AND r.org_id = $2 AND t.org_id = $2 AND t.space_id = $3 \
       AND r.parent_run_id IS NULL AND o.configured_source = 'gateway_browser' \
       AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $4)"
);

/// The room's browser runs of one Space, newest first, keyset-paged by `(created_at, id)`. `$3` names one
/// thread (empty for every thread), `$5` is the last run of the previous page (empty for the first).
const ROOM_RUN_LIST_SQL: &str = concat!(
    "SELECT r.id AS run_id, r.thread_id AS thread_id, r.user_id AS started_by, ",
    crate::run_phase::run_phase_sql!("r"),
    " AS phase, extract(epoch from r.created_at)::bigint AS started_at_unix, \
     COALESCE(extract(epoch from r.ended_at)::bigint, 0) AS finished_at_unix \
     FROM runs r \
     JOIN threads t ON t.id = r.thread_id \
     JOIN managed_run_terminalization_outbox o ON o.run_id = r.id \
     WHERE r.org_id = $1 AND t.org_id = $1 AND t.space_id = $2 \
       AND r.parent_run_id IS NULL AND o.configured_source = 'gateway_browser' \
       AND ($3 = '' OR r.thread_id = $3) \
       AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $4) \
       AND ($5 = '' OR (r.created_at, r.id) < \
            (SELECT c.created_at, c.id FROM runs c WHERE c.id = $5 AND c.org_id = $1)) \
     ORDER BY r.created_at DESC, r.id DESC \
     LIMIT $6"
);

/// The acting rows of the listed runs, in time order: the step count and the hosts come from these. Only the
/// two URL-bearing fields are selected, and only their hosts leave [`fold_run_facts`].
const ROOM_RUN_ACTING_SQL: &str = "SELECT e.run_id AS run_id, e.event_type AS event_type, \
    e.payload->>'url' AS url, e.payload->>'page_url' AS page_url \
    FROM events e \
    WHERE e.org_id = $1 AND e.run_id = ANY($2) \
      AND e.event_type IN ('BROWSER_ACTION_DISPATCHED', 'BROWSER_OBSERVATION_RECEIVED') \
    ORDER BY e.ts, e.id";

/// One page of a run's projected events after the cursor `(ts, id)`, in time order. Only the projected types
/// (`$3`) are read, so the database never returns an event type the projector would drop.
const ROOM_TIMELINE_SQL: &str = "SELECT e.id AS id, e.ts AS ts, e.event_type AS event_type, e.payload AS payload \
    FROM events e \
    WHERE e.org_id = $1 AND e.run_id = $2 AND e.event_type = ANY($3) \
      AND ($4::timestamptz IS NULL OR (e.ts, e.id) > ($4::timestamptz, $5::text)) \
    ORDER BY e.ts, e.id \
    LIMIT $6";

/// The timestamp of the cursor event, which must belong to the same run.
const EVENT_CURSOR_SQL: &str = "SELECT ts FROM events WHERE id = $1 AND org_id = $2 AND run_id = $3";

/// Whether the run recorded an observation whose screenshot reference is this evidence id.
const ROOM_EVIDENCE_SQL: &str = "SELECT 1 FROM events \
    WHERE org_id = $1 AND run_id = $2 AND event_type = 'BROWSER_OBSERVATION_RECEIVED' \
      AND payload->>'screenshot_ref' = $3 \
    LIMIT 1";

/// A room browser run's header row (the list's rows and the timeline's header share the shape).
#[derive(sqlx::FromRow)]
struct RunHeaderRow {
    run_id: String,
    thread_id: String,
    started_by: String,
    phase: String,
    started_at_unix: i64,
    finished_at_unix: i64,
}

/// One acting event's URL fields, read for the host and the step count.
#[derive(sqlx::FromRow)]
struct ActingRow {
    run_id: String,
    event_type: String,
    url: Option<String>,
    page_url: Option<String>,
}

/// One projectable timeline row: the event's cursor id, its time, its type and its payload.
#[derive(sqlx::FromRow)]
struct TimelineRow {
    id: String,
    ts: DateTime<Utc>,
    event_type: String,
    payload: Value,
}

/// The verdict of a room read: the caller it was verified for, the audience ceiling the decision names, and
/// how long the verdict holds.
struct RoomRead {
    org_id: String,
    audience_revision: i64,
    valid_until_unix: i64,
}

/// The shared prelude of the three room reads, each refusal failing closed in this order: the typed
/// zero-retention refusal, the read scope, the id shape, the Control `model.thread.read` decision for this
/// Space (signature, org, subject, window, and its own zero-retention claim), and the removed-member table.
/// Only a caller with no revocation row for the Space gets a verdict.
#[allow(clippy::result_large_err)]
async fn authorize_room_read(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    space_id: &str,
    decision_ref: &str,
    decision_token: &str,
) -> Result<RoomRead, Status> {
    refuse_zero_retention(caller)?;
    authorize_operation(caller, SPACE_BROWSER_READ_SCOPE)?;
    let Some(user_id) = caller.user_id().filter(|id| !id.trim().is_empty()) else {
        return Err(Status::permission_denied(
            "a room browser read requires an authenticated user subject",
        ));
    };
    let now = Utc::now();
    let decision = crate::grpc::verify_space_read_claims(
        decision_token,
        decision_ref,
        caller.org_id(),
        user_id,
        space_id,
        now,
    )?;
    if decision.zero_data_retention {
        return Err(Refusal::feature_not_supported("browser_run")
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    let (revoked,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM space_membership_revocations \
         WHERE space_ref = $1 AND subject_id = $2)",
    )
    .bind(space_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(|error| {
        tracing::warn!(error = %error, "room browser read revocation check failed");
        Status::unavailable("browser run read is unavailable")
    })?;
    if revoked {
        return Err(Status::permission_denied(
            "Space membership required for this run",
        ));
    }
    Ok(RoomRead {
        org_id: caller.org_id().to_owned(),
        audience_revision: decision.recipient_audience_revision,
        valid_until_unix: decision
            .expires_at
            .timestamp()
            .min(now.timestamp().saturating_add(MAX_VERDICT_SECONDS)),
    })
}

/// A required id, trimmed, of at most [`MAX_ID_BYTES`].
#[allow(clippy::result_large_err)]
fn bounded_id(value: &str, message: &'static str) -> Result<String, Status> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_ID_BYTES {
        return Err(Status::invalid_argument(message));
    }
    Ok(trimmed.to_owned())
}

/// An optional id, trimmed: empty stays empty, and anything over [`MAX_ID_BYTES`] is refused.
#[allow(clippy::result_large_err)]
fn optional_id(value: &str) -> Result<String, Status> {
    let trimmed = value.trim();
    if trimmed.len() > MAX_ID_BYTES {
        return Err(Status::invalid_argument("an id is too long"));
    }
    Ok(trimmed.to_owned())
}

/// The page size a room read asked for: zero is the default, and anything above the most is refused.
#[allow(clippy::result_large_err)]
fn page_limit(requested: u32, default: u32, most: u32) -> Result<u32, Status> {
    if requested == 0 {
        Ok(default)
    } else if requested <= most {
        Ok(requested)
    } else {
        Err(Status::invalid_argument("limit is out of range"))
    }
}

/// The host of a URL, or of a bare host name, lowercase, or empty. Only the host leaves this file: the
/// scheme, userinfo, port, path, query and fragment are dropped. An IP literal, or any byte that is not a
/// hostname byte, yields empty, so nothing else can ride out in a host field.
#[must_use]
pub(crate) fn host_of(raw: &str) -> String {
    let rest = raw.split_once("://").map_or(raw, |(_, after)| after);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    if host_port.starts_with('[') {
        return String::new();
    }
    let host = host_port
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let hostname = !host.is_empty()
        && host.len() <= 253
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.');
    if hostname {
        host
    } else {
        String::new()
    }
}

/// One projected timeline row, before it becomes the wire message. Only the closed fields exist here, so the
/// projector cannot copy a rationale, a title, a selector or a full URL by accident.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ProjectedEvent {
    pub(crate) kind: &'static str,
    pub(crate) step: String,
    pub(crate) action_type: String,
    pub(crate) host: String,
    pub(crate) status: String,
    pub(crate) risk_category: String,
    pub(crate) evidence_id: String,
    pub(crate) approval_id: String,
    pub(crate) decision: String,
    pub(crate) handoff_kind: String,
    pub(crate) outcome: String,
}

/// The string at `key` of an event payload, or empty.
fn field(payload: &Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Projects one event row by its type, or `None` for a type the room never sees. This is the allowlist of
/// WS20-S4 (design D5), field by field:
/// - `action`: the step, the action type and the target's host (never the URL or the model's reason);
/// - `observation`: the step, the page status and the page's host, and the screenshot reference as the
///   evidence id (never the page title, the DOM reference or the URL);
/// - `paused` and `resumed`: the kind only;
/// - `approval_required`: the step, action type, host, risk category and approval id (never the selector,
///   the reason or the URL);
/// - `approval_decided`: the approval id and the decision (never who decided);
/// - `handoff_requested` and `handoff_resolved`: the closed kind, host and approval id, or the closed outcome;
/// - `result`: the terminal outcome of the run, read from its terminal event type.
#[must_use]
pub(crate) fn project_event(event_type: &str, payload: &Value) -> Option<ProjectedEvent> {
    let mut row = ProjectedEvent::default();
    match event_type {
        "BROWSER_ACTION_DISPATCHED" => {
            row.kind = "action";
            row.step = field(payload, "action_id");
            row.action_type = field(payload, "action_type");
            row.host = host_of(&field(payload, "url"));
        }
        "BROWSER_OBSERVATION_RECEIVED" => {
            row.kind = "observation";
            row.step = field(payload, "action_id");
            row.status = field(payload, "status");
            row.host = host_of(&field(payload, "page_url"));
            row.evidence_id = field(payload, "screenshot_ref");
        }
        "BROWSER_RUN_PAUSED" => row.kind = "paused",
        "BROWSER_RUN_RESUMED" => row.kind = "resumed",
        "BROWSER_ACTION_APPROVAL_REQUIRED" => {
            row.kind = "approval_required";
            row.step = field(payload, "action_id");
            row.action_type = field(payload, "action_type");
            row.host = host_of(&field(payload, "url"));
            row.risk_category = field(payload, "risk_category");
            row.approval_id = field(payload, "approval_id");
        }
        "BROWSER_ACTION_DECIDED" => {
            row.kind = "approval_decided";
            row.step = field(payload, "action_id");
            row.approval_id = field(payload, "approval_id");
            row.decision = field(payload, "decision");
        }
        "BROWSER_HANDOFF_REQUESTED" => {
            row.kind = "handoff_requested";
            row.approval_id = field(payload, "approval_id");
            row.handoff_kind = field(payload, "kind");
            row.host = host_of(&field(payload, "host"));
        }
        "BROWSER_HANDOFF_RESOLVED" => {
            row.kind = "handoff_resolved";
            row.approval_id = field(payload, "approval_id");
            row.outcome = field(payload, "outcome");
        }
        "RUN_COMPLETED" => {
            row.kind = "result";
            row.outcome = "completed".to_owned();
        }
        "RUN_FAILED" => {
            row.kind = "result";
            row.outcome = "failed".to_owned();
        }
        _ => return None,
    }
    Some(row)
}

/// What a list row shows of a run's acting events: its step count and its hosts, first seen first.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct RunFacts {
    pub(crate) steps: u32,
    pub(crate) hosts: Vec<String>,
}

/// Folds the acting rows, in time order, into each run's step count and distinct hosts (at most
/// [`MAX_ROW_HOSTS`]). Only the counts and the hosts survive the fold.
fn fold_run_facts(rows: &[ActingRow]) -> HashMap<String, RunFacts> {
    let mut facts: HashMap<String, RunFacts> = HashMap::new();
    for row in rows {
        let entry = facts.entry(row.run_id.clone()).or_default();
        if row.event_type == "BROWSER_ACTION_DISPATCHED" {
            entry.steps = entry.steps.saturating_add(1);
        }
        let raw = row
            .page_url
            .as_deref()
            .filter(|url| !url.is_empty())
            .or(row.url.as_deref())
            .unwrap_or_default();
        let host = host_of(raw);
        if !host.is_empty() && entry.hosts.len() < MAX_ROW_HOSTS && !entry.hosts.contains(&host) {
            entry.hosts.push(host);
        }
    }
    facts
}

/// The run's header when it is a room browser run of this Space inside the audience ceiling. Otherwise
/// NotFound: a run the viewer may not see is indistinguishable from a run that does not exist.
#[allow(clippy::result_large_err)]
async fn room_run_header(
    pool: &PgPool,
    run_id: &str,
    read: &RoomRead,
    space_id: &str,
) -> Result<RunHeaderRow, Status> {
    sqlx::query_as::<_, RunHeaderRow>(ROOM_RUN_HEADER_SQL)
        .bind(run_id)
        .bind(&read.org_id)
        .bind(space_id)
        .bind(read.audience_revision)
        .fetch_optional(pool)
        .await
        .map_err(|error| {
            tracing::warn!(error = %error, "room browser run header read failed");
            Status::unavailable("browser run is unavailable")
        })?
        .ok_or_else(|| Status::not_found("run not found"))
}

fn header_to_wire(row: RunHeaderRow) -> pb::RoomBrowserRunHeader {
    pb::RoomBrowserRunHeader {
        run_id: row.run_id,
        thread_id: row.thread_id,
        started_by_subject_id: row.started_by,
        phase: row.phase,
        started_at_unix: row.started_at_unix,
        finished_at_unix: row.finished_at_unix,
    }
}

fn wire_event(row: &TimelineRow, projected: ProjectedEvent) -> pb::RoomBrowserEvent {
    pb::RoomBrowserEvent {
        event_id: row.id.clone(),
        at_unix: row.ts.timestamp(),
        kind: projected.kind.to_owned(),
        step: projected.step,
        action_type: projected.action_type,
        host: projected.host,
        status: projected.status,
        risk_category: projected.risk_category,
        evidence_id: projected.evidence_id,
        approval_id: projected.approval_id,
        decision: projected.decision,
        handoff_kind: projected.handoff_kind,
        outcome: projected.outcome,
    }
}

#[tonic::async_trait]
impl SpaceBrowserRead for SpaceBrowserReadGrpc {
    async fn count_active_room_browser_runs(
        &self,
        request: Request<pb::CountActiveRoomBrowserRunsRequest>,
    ) -> Result<Response<pb::CountActiveRoomBrowserRunsResponse>, Status> {
        let caller = identity(&request)?;
        refuse_zero_retention(&caller)?;
        authorize_operation(&caller, SPACE_BROWSER_READ_SCOPE)?;
        let req = request.into_inner();
        caller.authorize_org(&req.org_id)?;
        if req.space_id.trim().is_empty() {
            return Err(Status::invalid_argument("space_id is required"));
        }
        // A count that cannot be read is an error, never zero: the caller answers 503 and admits nothing.
        let count = sqlx::query_scalar::<_, i64>(COUNT_ACTIVE_ROOM_BROWSER_RUNS_SQL)
            .bind(&req.org_id)
            .bind(&req.space_id)
            .bind(&req.exclude_start_key)
            .fetch_one(&self.pool)
            .await
            .map_err(|error| {
                tracing::warn!(error = %error, "count_active_room_browser_runs failed");
                Status::unavailable("browser run count is unavailable")
            })?;
        let active_runs = u32::try_from(count)
            .map_err(|_| Status::internal("browser run count is out of range"))?;
        Ok(Response::new(pb::CountActiveRoomBrowserRunsResponse { active_runs }))
    }

    async fn resolve_run_space(
        &self,
        request: Request<pb::ResolveRunSpaceRequest>,
    ) -> Result<Response<pb::ResolveRunSpaceResponse>, Status> {
        let caller = identity(&request)?;
        refuse_zero_retention(&caller)?;
        authorize_operation(&caller, SPACE_BROWSER_READ_SCOPE)?;
        let req = request.into_inner();
        if req.run_id.trim().is_empty()
            || req.org_id.trim().is_empty()
            || req.user_id.trim().is_empty()
        {
            return Err(Status::invalid_argument(
                "run_id, org_id, and user_id are required",
            ));
        }
        caller.authorize_org(&req.org_id)?;
        if !caller.is_service() {
            caller.authorize_user(&req.user_id)?;
        }
        // Only the run's owner learns its Space (the same owner check ResolveRunOwner makes).
        if !crate::retention_receipt::run_owner_matches(
            &self.pool,
            &caller,
            &req.run_id,
            &req.org_id,
            &req.user_id,
        )
        .await?
        {
            return Err(Status::permission_denied("run access denied"));
        }
        let space_id = sqlx::query_scalar::<_, String>(RUN_SPACE_SQL)
            .bind(&req.run_id)
            .bind(&req.org_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| {
                tracing::warn!(error = %error, "resolve_run_space failed");
                Status::unavailable("run Space is unavailable")
            })?
            .ok_or_else(|| Status::permission_denied("run access denied"))?;
        Ok(Response::new(pb::ResolveRunSpaceResponse { space_id }))
    }

    async fn list_room_browser_runs(
        &self,
        request: Request<pb::ListRoomBrowserRunsRequest>,
    ) -> Result<Response<pb::ListRoomBrowserRunsResponse>, Status> {
        let caller = identity(&request)?;
        let req = request.into_inner();
        let space_id = bounded_id(&req.space_id, "space_id is required")?;
        let read = authorize_room_read(
            &self.pool,
            &caller,
            &space_id,
            &req.space_read_decision_ref,
            &req.space_read_decision_token,
        )
        .await?;
        let limit = page_limit(req.limit, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT)?;
        let thread_id = optional_id(&req.thread_id)?;
        let before_run_id = optional_id(&req.before_run_id)?;
        // One row more than the page, to learn whether a next page exists.
        let fetch = i64::from(limit) + 1;
        let mut rows = sqlx::query_as::<_, RunHeaderRow>(ROOM_RUN_LIST_SQL)
            .bind(&read.org_id)
            .bind(&space_id)
            .bind(&thread_id)
            .bind(read.audience_revision)
            .bind(&before_run_id)
            .bind(fetch)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| {
                tracing::warn!(error = %error, "list_room_browser_runs failed");
                Status::unavailable("browser runs are unavailable")
            })?;
        let more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let next_run_id = if more {
            rows.last().map(|row| row.run_id.clone()).unwrap_or_default()
        } else {
            String::new()
        };
        let run_ids: Vec<String> = rows.iter().map(|row| row.run_id.clone()).collect();
        let facts = room_run_facts(&self.pool, &read.org_id, &run_ids).await?;
        let runs = rows
            .into_iter()
            .map(|row| {
                let fact = facts.get(&row.run_id).cloned().unwrap_or_default();
                pb::RoomBrowserRun {
                    run_id: row.run_id,
                    thread_id: row.thread_id,
                    started_by_subject_id: row.started_by,
                    phase: row.phase,
                    started_at_unix: row.started_at_unix,
                    finished_at_unix: row.finished_at_unix,
                    hosts: fact.hosts,
                    step_count: fact.steps,
                }
            })
            .collect();
        Ok(Response::new(pb::ListRoomBrowserRunsResponse {
            runs,
            next_run_id,
            valid_until_unix: read.valid_until_unix,
        }))
    }

    async fn read_room_browser_timeline(
        &self,
        request: Request<pb::ReadRoomBrowserTimelineRequest>,
    ) -> Result<Response<pb::ReadRoomBrowserTimelineResponse>, Status> {
        let caller = identity(&request)?;
        let req = request.into_inner();
        let space_id = bounded_id(&req.space_id, "space_id is required")?;
        let run_id = bounded_id(&req.run_id, "run_id is required")?;
        let read = authorize_room_read(
            &self.pool,
            &caller,
            &space_id,
            &req.space_read_decision_ref,
            &req.space_read_decision_token,
        )
        .await?;
        let limit = page_limit(req.limit, DEFAULT_TIMELINE_LIMIT, MAX_TIMELINE_LIMIT)?;
        let header = room_run_header(&self.pool, &run_id, &read, &space_id).await?;
        let after_event_id = optional_id(&req.after_event_id)?;
        // The cursor is an event of this run; anything else is NotFound, so a stale or forged id learns nothing.
        let after_ts = if after_event_id.is_empty() {
            None
        } else {
            Some(
                sqlx::query_scalar::<_, DateTime<Utc>>(EVENT_CURSOR_SQL)
                    .bind(&after_event_id)
                    .bind(&read.org_id)
                    .bind(&run_id)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|error| {
                        tracing::warn!(error = %error, "room browser timeline cursor failed");
                        Status::unavailable("browser run timeline is unavailable")
                    })?
                    .ok_or_else(|| Status::not_found("event not found"))?,
            )
        };
        let types: Vec<String> = PROJECTED_EVENT_TYPES
            .iter()
            .map(|event_type| (*event_type).to_owned())
            .collect();
        let fetch = i64::from(limit) + 1;
        let mut rows = sqlx::query_as::<_, TimelineRow>(ROOM_TIMELINE_SQL)
            .bind(&read.org_id)
            .bind(&run_id)
            .bind(types)
            .bind(after_ts)
            .bind(&after_event_id)
            .bind(fetch)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| {
                tracing::warn!(error = %error, "read_room_browser_timeline failed");
                Status::unavailable("browser run timeline is unavailable")
            })?;
        let more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let next_event_id = if more {
            rows.last().map(|row| row.id.clone()).unwrap_or_default()
        } else {
            String::new()
        };
        let events = rows
            .iter()
            .filter_map(|row| {
                project_event(&row.event_type, &row.payload)
                    .map(|projected| wire_event(row, projected))
            })
            .collect();
        Ok(Response::new(pb::ReadRoomBrowserTimelineResponse {
            run: Some(header_to_wire(header)),
            events,
            next_event_id,
            valid_until_unix: read.valid_until_unix,
        }))
    }

    async fn authorize_room_browser_evidence(
        &self,
        request: Request<pb::AuthorizeRoomBrowserEvidenceRequest>,
    ) -> Result<Response<pb::AuthorizeRoomBrowserEvidenceResponse>, Status> {
        let caller = identity(&request)?;
        let req = request.into_inner();
        let space_id = bounded_id(&req.space_id, "space_id is required")?;
        let run_id = bounded_id(&req.run_id, "run_id is required")?;
        let evidence_id = bounded_id(&req.evidence_id, "evidence_id is required")?;
        let read = authorize_room_read(
            &self.pool,
            &caller,
            &space_id,
            &req.space_read_decision_ref,
            &req.space_read_decision_token,
        )
        .await?;
        room_run_header(&self.pool, &run_id, &read, &space_id).await?;
        let shown = sqlx::query_scalar::<_, i32>(ROOM_EVIDENCE_SQL)
            .bind(&read.org_id)
            .bind(&run_id)
            .bind(&evidence_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| {
                tracing::warn!(error = %error, "authorize_room_browser_evidence failed");
                Status::unavailable("browser run evidence is unavailable")
            })?;
        if shown.is_none() {
            return Err(Status::not_found("evidence not found"));
        }
        Ok(Response::new(pb::AuthorizeRoomBrowserEvidenceResponse {
            artifact_id: evidence_id,
        }))
    }
}

/// The step counts and hosts of the given runs, read from their acting events and folded by
/// [`fold_run_facts`]. No run ids, no rows, no query when the page is empty.
#[allow(clippy::result_large_err)]
async fn room_run_facts(
    pool: &PgPool,
    org_id: &str,
    run_ids: &[String],
) -> Result<HashMap<String, RunFacts>, Status> {
    if run_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query_as::<_, ActingRow>(ROOM_RUN_ACTING_SQL)
        .bind(org_id)
        .bind(run_ids.to_vec())
        .fetch_all(pool)
        .await
        .map_err(|error| {
            tracing::warn!(error = %error, "room browser run facts failed");
            Status::unavailable("browser runs are unavailable")
        })?;
    Ok(fold_run_facts(&rows))
}
