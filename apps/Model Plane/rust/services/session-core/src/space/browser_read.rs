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
use crate::space::audience_window::floor_admits_at;

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
///
/// D18: `$5` is the member's WS04-S11 revision floor and `$6` the timestamp floor (both NULL without a window). A
/// run is admitted only when it is stamped at or after the revision floor and started at or after the timestamp
/// floor, the same rule the Space run list applies (`run_service_grpc`).
const ROOM_RUN_HEADER_SQL: &str = concat!(
    "SELECT r.id AS run_id, r.thread_id AS thread_id, r.user_id AS started_by, ",
    crate::run_phase::run_phase_sql!("r"),
    " AS phase, extract(epoch from r.created_at)::bigint AS started_at_unix, \
     r.created_at AS created_at, \
     COALESCE(extract(epoch from r.ended_at)::bigint, 0) AS finished_at_unix \
     FROM runs r \
     JOIN threads t ON t.id = r.thread_id \
     JOIN managed_run_terminalization_outbox o ON o.run_id = r.id \
     WHERE r.id = $1 AND r.org_id = $2 AND t.org_id = $2 AND t.space_id = $3 \
       AND r.parent_run_id IS NULL AND o.configured_source = 'gateway_browser' \
       AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $4) \
       AND ($5::BIGINT IS NULL OR r.recipient_audience_revision IS NULL \
            OR r.recipient_audience_revision >= $5::BIGINT) \
       AND ($6::TIMESTAMPTZ IS NULL OR r.created_at >= $6::TIMESTAMPTZ)"
);

/// The room's browser runs of one Space, newest first, keyset-paged by `(created_at, id)`. `$3` names one
/// thread (empty for every thread), `$5` is the last run of the previous page (empty for the first). `$7` and
/// `$8` are the member's revision and timestamp floors (D18; NULL without a window), applied as in
/// [`ROOM_RUN_HEADER_SQL`].
const ROOM_RUN_LIST_SQL: &str = concat!(
    "SELECT r.id AS run_id, r.thread_id AS thread_id, r.user_id AS started_by, ",
    crate::run_phase::run_phase_sql!("r"),
    " AS phase, extract(epoch from r.created_at)::bigint AS started_at_unix, \
     r.created_at AS created_at, \
     COALESCE(extract(epoch from r.ended_at)::bigint, 0) AS finished_at_unix \
     FROM runs r \
     JOIN threads t ON t.id = r.thread_id \
     JOIN managed_run_terminalization_outbox o ON o.run_id = r.id \
     WHERE r.org_id = $1 AND t.org_id = $1 AND t.space_id = $2 \
       AND r.parent_run_id IS NULL AND o.configured_source = 'gateway_browser' \
       AND ($3 = '' OR r.thread_id = $3) \
       AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $4) \
       AND ($7::BIGINT IS NULL OR r.recipient_audience_revision IS NULL \
            OR r.recipient_audience_revision >= $7::BIGINT) \
       AND ($8::TIMESTAMPTZ IS NULL OR r.created_at >= $8::TIMESTAMPTZ) \
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
/// (`$3`) are read, so the database never returns an event type the projector would drop. `$7` is the member's
/// timestamp floor (D18; NULL without a window): an event before it is not read, whatever its run.
const ROOM_TIMELINE_SQL: &str = "SELECT e.id AS id, e.ts AS ts, e.event_type AS event_type, e.payload AS payload \
    FROM events e \
    WHERE e.org_id = $1 AND e.run_id = $2 AND e.event_type = ANY($3) \
      AND ($4::timestamptz IS NULL OR (e.ts, e.id) > ($4::timestamptz, $5::text)) \
      AND ($7::TIMESTAMPTZ IS NULL OR e.ts >= $7::TIMESTAMPTZ) \
    ORDER BY e.ts, e.id \
    LIMIT $6";

/// The timestamp of the cursor event, which must belong to the same run.
const EVENT_CURSOR_SQL: &str = "SELECT ts FROM events WHERE id = $1 AND org_id = $2 AND run_id = $3";

/// Whether the run recorded an observation whose screenshot reference is this evidence id. `$4` is the member's
/// timestamp floor (D18; NULL without a window): an observation before it does not authorize its screenshot.
const ROOM_EVIDENCE_SQL: &str = "SELECT 1 FROM events \
    WHERE org_id = $1 AND run_id = $2 AND event_type = 'BROWSER_OBSERVATION_RECEIVED' \
      AND payload->>'screenshot_ref' = $3 \
      AND ($4::TIMESTAMPTZ IS NULL OR ts >= $4::TIMESTAMPTZ) \
    LIMIT 1";

/// A room browser run's header row (the list's rows and the timeline's header share the shape).
#[derive(sqlx::FromRow)]
struct RunHeaderRow {
    run_id: String,
    thread_id: String,
    started_by: String,
    phase: String,
    started_at_unix: i64,
    /// The run's own start instant, checked against the member's timestamp floor (D18).
    created_at: DateTime<Utc>,
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

/// The verdict of a room read: the caller it was verified for, the audience ceiling the decision names, the
/// member's own history floors (D18 and WS04-S11; `None` for full history), and how long the verdict holds.
struct RoomRead {
    org_id: String,
    audience_revision: i64,
    /// WS04-S11: the revision floor. A run stamped with an older revision is not shown.
    floor: Option<i64>,
    /// D18: the timestamp floor. A run started before it, and any of its events before it, is not shown.
    floor_at: Option<DateTime<Utc>>,
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
        floor: decision.history_floor,
        floor_at: decision.history_floor_at,
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
    // A dotted-quad IPv4 literal is not a hostname: its last label is all digits, and no hostname ends in a numeric label.
    let last_label_numeric = host
        .rsplit('.')
        .next()
        .is_some_and(|label| label.bytes().all(|byte| byte.is_ascii_digit()));
    let hostname = !host.is_empty()
        && host.len() <= 253
        && !last_label_numeric
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

/// The run's header when it is a room browser run of this Space inside the audience ceiling and the member's
/// floors. Otherwise NotFound: a run the viewer may not see is indistinguishable from a run that does not exist.
/// The evidence and timeline reads take their run through here, so a run before the floor is refused for each.
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
        .bind(read.floor)
        .bind(read.floor_at)
        .fetch_optional(pool)
        .await
        .map_err(|error| {
            tracing::warn!(error = %error, "room browser run header read failed");
            Status::unavailable("browser run is unavailable")
        })?
        .filter(|row| floor_admits_at(row.created_at, read.floor_at))
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
            .bind(read.floor)
            .bind(read.floor_at)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| {
                tracing::warn!(error = %error, "list_room_browser_runs failed");
                Status::unavailable("browser runs are unavailable")
            })?;
        // D18: a run that started before the member's floor is not listed, whatever the SQL returned.
        rows.retain(|row| floor_admits_at(row.created_at, read.floor_at));
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
            .bind(read.floor_at)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| {
                tracing::warn!(error = %error, "read_room_browser_timeline failed");
                Status::unavailable("browser run timeline is unavailable")
            })?;
        // D18: an event before the member's floor is not shown, whatever the SQL returned.
        rows.retain(|row| floor_admits_at(row.ts, read.floor_at));
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
            .bind(read.floor_at)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- host_of ----------------------------------------------------------------------------

    #[test]
    fn host_of_keeps_only_the_lowercase_host() {
        for (raw, expected) in [
            ("https://example.com/a/b?q=1#frag", "example.com"),
            ("http://EXAMPLE.com", "example.com"),
            ("https://Example.COM:8443/x", "example.com"),
            ("ftp://files.example.org/x", "files.example.org"),
            ("https://sub.example.co.uk/path", "sub.example.co.uk"),
            ("https://xn--bcher-kva.example/", "xn--bcher-kva.example"),
            ("https://a-b.example/", "a-b.example"),
            // A bare host name, with or without a port or a path.
            ("example.com", "example.com"),
            ("EXAMPLE.com:80", "example.com"),
            ("example.com/path?q=1", "example.com"),
            ("Handoff.Example.net:9000", "handoff.example.net"),
            // Nothing past the authority survives, whatever it looks like.
            ("https://example.com?q=a@b", "example.com"),
            ("https://example.com#a@b", "example.com"),
            ("https://example.com/path@evil.example", "example.com"),
            ("https://example.com:80:90/", "example.com"),
        ] {
            assert_eq!(host_of(raw), expected, "{raw:?}");
        }
    }

    #[test]
    fn host_of_drops_the_userinfo_so_a_credential_never_rides_in_the_host() {
        for (raw, expected) in [
            ("https://user:pass@example.com/x", "example.com"),
            ("https://user@example.com", "example.com"),
            ("https://user:pass@example.com:8080/x", "example.com"),
            ("https://a@b@example.com/", "example.com"),
            // The host a browser would reach is the one after the last `@`.
            ("https://evil.example@example.com/", "example.com"),
            ("http://@example.com", "example.com"),
        ] {
            let host = host_of(raw);
            assert_eq!(host, expected, "{raw:?}");
            assert!(!host.contains('@') && !host.contains(':'), "{raw:?} -> {host:?}");
        }
    }

    #[test]
    fn host_of_yields_empty_for_a_bracketed_or_bare_ipv6_literal() {
        for raw in [
            "https://[::1]/",
            "https://[::1]:8080/x",
            "https://[2001:db8::1]/a",
            "http://user@[::1]/",
            "[::1]",
            "[2001:db8::1]:443",
            // Bare, an IPv6 literal splits at its first colon and leaves nothing.
            "::1",
            "https://::1/",
        ] {
            assert_eq!(host_of(raw), "", "{raw:?}");
        }
    }

    /// The doc comment of `host_of` says an IP literal yields empty (and `site_policy::canonical_host` refuses a
    /// host whose last label is all digits). A dotted IPv4 literal is made only of hostname bytes, so it passes.
    #[test]
    fn host_of_yields_empty_for_an_ipv4_literal() {
        for raw in [
            "https://10.0.0.1/",
            "http://192.168.1.5:8080/admin",
            "https://127.0.0.1",
            "93.184.216.34",
            "https://user:pw@169.254.169.254/latest/meta-data",
        ] {
            assert_eq!(host_of(raw), "", "an IP literal must not ride out as a host: {raw:?}");
        }
    }

    #[test]
    fn host_of_yields_empty_for_spaces_and_bytes_that_are_not_hostname_bytes() {
        for raw in [
            "exa mple.com",
            "https://exa mple.com/x",
            " example.com",
            "example.com ",
            "https://example.com\t/x",
            "example.com\n",
            "exa_mple.com",
            "ex%61mple.com",
            "ex*mple.com",
            "exam,ple.com",
            "example.com;rm",
            "example.com\\evil",
            "ex\u{e4}mple.com",
            "exa\u{0}mple.com",
            "\u{202e}example.com",
        ] {
            assert_eq!(host_of(raw), "", "{raw:?}");
        }
    }

    #[test]
    fn host_of_yields_empty_for_nothing_and_for_an_oversize_host() {
        for raw in ["", "https://", "http:///path", "https://:8080/", "https://?q=1", "https://#frag", "://"] {
            assert_eq!(host_of(raw), "", "{raw:?}");
        }
        let longest = format!("{}.com", "a".repeat(249));
        assert_eq!(longest.len(), 253);
        assert_eq!(host_of(&format!("https://{longest}/")), longest);
        let too_long = format!("{}.com", "a".repeat(250));
        assert_eq!(too_long.len(), 254);
        assert_eq!(host_of(&format!("https://{too_long}/")), "");
    }

    // ---- project_event ----------------------------------------------------------------------

    /// A payload that carries every field any event kind reads, and every field the room must never see.
    fn payload() -> Value {
        json!({
            "action_id": "act-7",
            "action_type": "click",
            "url": "https://Example.com:8443/secret/path?token=CANARY-URL#frag",
            "page_url": "https://pages.example.org/inbox?msg=CANARY-PAGE",
            "page_title": "CANARY-TITLE",
            "title": "CANARY-TITLE",
            "reason": "CANARY-REASON",
            "rationale": "CANARY-RATIONALE",
            "selector": "#CANARY-SELECTOR",
            "dom_ref": "CANARY-DOM",
            "decided_by": "CANARY-SUBJECT",
            "decision": "approved",
            "approval_id": "appr-1",
            "risk_category": "financial",
            "status": "loaded",
            "screenshot_ref": "shot-1",
            "kind": "login",
            "host": "Handoff.Example.net:9000",
            "outcome": "resolved",
            "goal": "CANARY-GOAL",
            "frame": "CANARY-FRAME",
        })
    }

    fn projected(kind: &'static str) -> ProjectedEvent {
        ProjectedEvent { kind, ..ProjectedEvent::default() }
    }

    /// The projector goldens: each event type of [`PROJECTED_EVENT_TYPES`] keeps its closed fields and
    /// nothing else, whatever else the payload carries.
    #[test]
    fn project_event_keeps_only_the_closed_fields_of_each_kind() {
        let payload = payload();
        let goldens: Vec<(&str, ProjectedEvent)> = vec![
            (
                "BROWSER_ACTION_DISPATCHED",
                ProjectedEvent {
                    step: "act-7".to_owned(),
                    action_type: "click".to_owned(),
                    host: "example.com".to_owned(),
                    ..projected("action")
                },
            ),
            (
                "BROWSER_OBSERVATION_RECEIVED",
                ProjectedEvent {
                    step: "act-7".to_owned(),
                    status: "loaded".to_owned(),
                    host: "pages.example.org".to_owned(),
                    evidence_id: "shot-1".to_owned(),
                    ..projected("observation")
                },
            ),
            ("BROWSER_RUN_PAUSED", projected("paused")),
            ("BROWSER_RUN_RESUMED", projected("resumed")),
            (
                "BROWSER_ACTION_APPROVAL_REQUIRED",
                ProjectedEvent {
                    step: "act-7".to_owned(),
                    action_type: "click".to_owned(),
                    host: "example.com".to_owned(),
                    risk_category: "financial".to_owned(),
                    approval_id: "appr-1".to_owned(),
                    ..projected("approval_required")
                },
            ),
            (
                "BROWSER_ACTION_DECIDED",
                ProjectedEvent {
                    step: "act-7".to_owned(),
                    approval_id: "appr-1".to_owned(),
                    decision: "approved".to_owned(),
                    ..projected("approval_decided")
                },
            ),
            (
                "BROWSER_HANDOFF_REQUESTED",
                ProjectedEvent {
                    approval_id: "appr-1".to_owned(),
                    handoff_kind: "login".to_owned(),
                    host: "handoff.example.net".to_owned(),
                    ..projected("handoff_requested")
                },
            ),
            (
                "BROWSER_HANDOFF_RESOLVED",
                ProjectedEvent {
                    approval_id: "appr-1".to_owned(),
                    outcome: "resolved".to_owned(),
                    ..projected("handoff_resolved")
                },
            ),
            (
                "RUN_COMPLETED",
                ProjectedEvent { outcome: "completed".to_owned(), ..projected("result") },
            ),
            (
                "RUN_FAILED",
                ProjectedEvent { outcome: "failed".to_owned(), ..projected("result") },
            ),
        ];
        assert_eq!(goldens.len(), PROJECTED_EVENT_TYPES.len(), "every projected type has a golden");
        for (event_type, golden) in goldens {
            assert!(PROJECTED_EVENT_TYPES.contains(&event_type), "{event_type} is not in the shared list");
            let row = project_event(event_type, &payload).unwrap_or_else(|| panic!("{event_type} must project"));
            assert_eq!(row, golden, "{event_type}");
            // The projection is built from closed fields, so nothing of the payload's private content survives.
            let shown = format!("{row:?}");
            for private in ["CANARY", "secret", "token=", "://", "8443", "#frag"] {
                assert!(!shown.contains(private), "{event_type} leaked {private:?}: {shown}");
            }
        }
    }

    #[test]
    fn an_action_never_yields_the_reason_or_a_full_url_and_a_decision_never_yields_who_decided() {
        let payload = payload();
        let action = project_event("BROWSER_ACTION_DISPATCHED", &payload).expect("projects");
        assert_eq!(action.host, "example.com", "the host only");
        let shown = format!("{action:?}");
        for private in ["CANARY-REASON", "CANARY-RATIONALE", "CANARY-URL", "secret/path", "https://"] {
            assert!(!shown.contains(private), "{private}: {shown}");
        }
        let decided = project_event("BROWSER_ACTION_DECIDED", &payload).expect("projects");
        assert_eq!(decided.decision, "approved");
        assert!(!format!("{decided:?}").contains("CANARY-SUBJECT"), "decided_by never leaves the plane");
        // An observation never carries the page title, the DOM reference or the page URL.
        let observed = project_event("BROWSER_OBSERVATION_RECEIVED", &payload).expect("projects");
        let shown = format!("{observed:?}");
        for private in ["CANARY-TITLE", "CANARY-DOM", "CANARY-PAGE", "inbox"] {
            assert!(!shown.contains(private), "{private}: {shown}");
        }
    }

    #[test]
    fn project_event_returns_none_for_a_type_the_room_never_sees() {
        for event_type in [
            "BROWSER_FRAME_CAPTURED",
            "BROWSER_DOM_SNAPSHOT",
            "RUN_STARTED",
            "RUN_CANCELLED",
            "TOOL_CALLED",
            "MESSAGE_REVISED",
            "",
            "browser_action_dispatched",
            "BROWSER_ACTION_DISPATCHED ",
        ] {
            assert_eq!(project_event(event_type, &payload()), None, "{event_type:?}");
        }
    }

    #[test]
    fn project_event_tolerates_an_empty_or_odd_payload() {
        for payload in [json!({}), Value::Null, json!([]), json!("text"), json!({"url": 5, "action_id": null})] {
            let row = project_event("BROWSER_ACTION_DISPATCHED", &payload).expect("projects");
            assert_eq!(row, projected("action"), "{payload}");
        }
        assert_eq!(
            project_event("RUN_FAILED", &Value::Null),
            Some(ProjectedEvent { outcome: "failed".to_owned(), ..projected("result") }),
            "the outcome of a result row comes from its event type, not its payload"
        );
        assert_eq!(
            project_event("RUN_COMPLETED", &json!({"outcome": "CANARY-OUTCOME"})),
            Some(ProjectedEvent { outcome: "completed".to_owned(), ..projected("result") })
        );
    }

    // ---- fold_run_facts ---------------------------------------------------------------------

    const DISPATCHED: &str = "BROWSER_ACTION_DISPATCHED";
    const OBSERVED: &str = "BROWSER_OBSERVATION_RECEIVED";

    fn acting(run: &str, event_type: &str, url: Option<&str>, page_url: Option<&str>) -> ActingRow {
        ActingRow {
            run_id: run.to_owned(),
            event_type: event_type.to_owned(),
            url: url.map(str::to_owned),
            page_url: page_url.map(str::to_owned),
        }
    }

    fn hosts(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn fold_run_facts_counts_only_dispatched_actions_as_steps() {
        let rows = vec![
            acting("run-a", DISPATCHED, Some("https://a.example/x"), None),
            acting("run-a", OBSERVED, None, Some("https://a.example/y")),
            acting("run-a", DISPATCHED, Some("https://a.example/z"), None),
            acting("run-a", OBSERVED, None, Some("https://a.example/w")),
            acting("run-a", OBSERVED, None, None),
        ];
        let facts = fold_run_facts(&rows);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts["run-a"], RunFacts { steps: 2, hosts: hosts(&["a.example"]) });

        // A run with observations only has no steps.
        let observed_only = fold_run_facts(&[acting("run-b", OBSERVED, None, Some("https://b.example/"))]);
        assert_eq!(observed_only["run-b"], RunFacts { steps: 0, hosts: hosts(&["b.example"]) });
    }

    #[test]
    fn fold_run_facts_keeps_distinct_hosts_first_seen_first() {
        let rows = vec![
            acting("run-a", DISPATCHED, Some("https://b.example/1"), None),
            acting("run-a", OBSERVED, None, Some("https://a.example/1")),
            acting("run-a", DISPATCHED, Some("https://B.example/2"), None),
            acting("run-a", DISPATCHED, Some("https://c.example/3"), None),
            acting("run-a", OBSERVED, None, Some("https://a.example/4")),
        ];
        assert_eq!(
            fold_run_facts(&rows)["run-a"],
            RunFacts { steps: 3, hosts: hosts(&["b.example", "a.example", "c.example"]) }
        );
    }

    #[test]
    fn fold_run_facts_keeps_at_most_eight_hosts_but_counts_every_step() {
        assert_eq!(MAX_ROW_HOSTS, 8);
        let rows: Vec<ActingRow> = (1..=12)
            .map(|n| acting("run-a", DISPATCHED, Some(&format!("https://h{n}.example/")), None))
            .collect();
        let facts = &fold_run_facts(&rows)["run-a"];
        assert_eq!(facts.steps, 12, "the step count is not capped");
        assert_eq!(
            facts.hosts,
            hosts(&["h1.example", "h2.example", "h3.example", "h4.example", "h5.example", "h6.example", "h7.example", "h8.example"]),
            "the first eight hosts seen, in order"
        );
    }

    #[test]
    fn fold_run_facts_reads_the_page_url_before_the_url_and_skips_what_is_no_host() {
        let rows = vec![
            // page_url wins over url when it is present and not empty.
            acting("run-a", OBSERVED, Some("https://url.example/"), Some("https://page.example/")),
            // An empty page_url falls back to url.
            acting("run-a", OBSERVED, Some("https://fallback.example/"), Some("")),
            // Neither names a host: nothing is added, and a dispatched row still counts as a step.
            acting("run-a", DISPATCHED, Some("not a url with spaces"), None),
            acting("run-a", DISPATCHED, None, None),
            acting("run-a", DISPATCHED, Some("https://[::1]/"), None),
        ];
        assert_eq!(
            fold_run_facts(&rows)["run-a"],
            RunFacts { steps: 3, hosts: hosts(&["page.example", "fallback.example"]) }
        );
    }

    #[test]
    fn fold_run_facts_keeps_each_run_apart_and_answers_nothing_for_no_rows() {
        assert!(fold_run_facts(&[]).is_empty());
        let rows = vec![
            acting("run-a", DISPATCHED, Some("https://a.example/"), None),
            acting("run-b", DISPATCHED, Some("https://b.example/"), None),
            acting("run-a", DISPATCHED, Some("https://a2.example/"), None),
        ];
        let facts = fold_run_facts(&rows);
        assert_eq!(facts["run-a"], RunFacts { steps: 2, hosts: hosts(&["a.example", "a2.example"]) });
        assert_eq!(facts["run-b"], RunFacts { steps: 1, hosts: hosts(&["b.example"]) });
    }

    // ---- the capacity refusal, the count and the page rules ---------------------------------

    #[test]
    fn the_capacity_refusal_is_resource_exhausted_with_the_message_and_the_trailer() {
        assert_eq!(BROWSER_CAPACITY_CODE, "browser_capacity");
        let status = browser_capacity_status();
        assert_eq!(status.code(), tonic::Code::ResourceExhausted);
        assert_eq!(status.message(), "browser_capacity");
        assert_eq!(
            status.metadata().get(ERROR_CODE_TRAILER).and_then(|value| value.to_str().ok()),
            Some("browser_capacity")
        );
    }

    #[test]
    fn the_cap_counts_only_in_flight_gateway_browser_runs_of_one_space() {
        let sql = COUNT_ACTIVE_ROOM_BROWSER_RUNS_SQL.split_whitespace().collect::<Vec<_>>().join(" ");
        for fragment in [
            "r.org_id = $1",
            "r.space_id = $2",
            "o.configured_source = 'gateway_browser'",
            "r.status NOT IN ('completed', 'failed', 'cancelled')",
            "($3 = '' OR o.start_key <> $3)",
        ] {
            assert!(sql.contains(fragment), "the count lacks {fragment:?}: {sql}");
        }
        assert_eq!(mp_contracts::space_browser::ROOM_RUNS_PER_SPACE, 2);
        assert!(capacity_admits(0) && capacity_admits(1) && !capacity_admits(2) && !capacity_admits(u32::MAX));
    }

    #[test]
    fn the_page_limit_defaults_and_refuses_what_is_over_the_most() {
        assert_eq!(page_limit(0, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT).expect("default"), 20);
        assert_eq!(page_limit(1, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT).expect("one"), 1);
        assert_eq!(page_limit(50, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT).expect("the most"), 50);
        assert_eq!(page_limit(0, DEFAULT_TIMELINE_LIMIT, MAX_TIMELINE_LIMIT).expect("default"), 50);
        assert_eq!(page_limit(200, DEFAULT_TIMELINE_LIMIT, MAX_TIMELINE_LIMIT).expect("the most"), 200);
        for over in [(51, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT), (201, DEFAULT_TIMELINE_LIMIT, MAX_TIMELINE_LIMIT)] {
            let status = page_limit(over.0, over.1, over.2).expect_err("over the most");
            assert_eq!(status.code(), tonic::Code::InvalidArgument);
        }
    }

    #[test]
    fn ids_are_trimmed_and_bounded() {
        assert_eq!(bounded_id("  run-1  ", "run_id is required").expect("trimmed"), "run-1");
        for bad in ["", "   ", &"r".repeat(MAX_ID_BYTES + 1)] {
            let status = bounded_id(bad, "run_id is required").expect_err("refused");
            assert_eq!(status.code(), tonic::Code::InvalidArgument);
        }
        assert_eq!(bounded_id(&"r".repeat(MAX_ID_BYTES), "x").expect("at the cap").len(), MAX_ID_BYTES);
        assert_eq!(optional_id("   ").expect("blank stays empty"), "");
        assert_eq!(optional_id(" evt-1 ").expect("trimmed"), "evt-1");
        assert_eq!(
            optional_id(&"e".repeat(MAX_ID_BYTES + 1)).expect_err("too long").code(),
            tonic::Code::InvalidArgument
        );
    }

    // ---- real Postgres (every test is `#[ignore]`d) -----------------------------------------
    //
    // Run with `DATABASE_URL` pointing at a disposable Postgres whose database name contains `test`:
    // `cargo test -p session-core --bin session-core space::browser_read -- --ignored --test-threads=1`

    mod db_tests {
        use super::*;
        use crate::grpc::space_w1_tests::{mint, StreamKeysEnv, DECISION_REF};
        use crate::space::test_support::{clean_space, connect, seed_space_run, seed_space_thread};
        use mp_contracts::space_browser::ROOM_RUNS_PER_SPACE;

        const NEEDS_DB: &str = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)";
        /// The person who owns every thread and starts every run (`seed_space_run` names this owner).
        const OWNER: &str = "owner-1";
        /// A member who reads the room.
        const VIEWER: &str = "viewer-1";

        fn ids(prefix: &str) -> (String, String, String) {
            let suffix = mp_ids::new_ulid();
            (format!("org-{prefix}-{suffix}"), format!("space-{prefix}-{suffix}"), suffix)
        }

        /// The Space's decision ceiling: every seeded thread and run is stamped at revision 1.
        const CEILING: i64 = 4;

        // ---- seeding ----

        /// A run in `status` on `thread`, started through `source` (`gateway_browser` is a room browser run).
        async fn seed_run(pool: &PgPool, org: &str, space: &str, thread: &str, run: &str, status: &str, source: &str) {
            seed_space_run(pool, org, space, thread, run, status, "persistent").await;
            let step = match source {
                "gateway_browser" => "model-gateway-browser-agent-final",
                "execution_agent" => "execution-core-agent-final",
                "execution_browser" => "execution-core-browser-final",
                _ => "model-gateway-direct-inference-final",
            };
            sqlx::query(
                "INSERT INTO managed_run_terminalization_outbox \
                 (run_id, org_id, user_id, start_key, configured_source, configured_terminal_step_id, zdr, deadline_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, false, now() + interval '15 minutes')",
            )
            .bind(run)
            .bind(org)
            .bind(OWNER)
            .bind(format!("sk-{run}"))
            .bind(source)
            .bind(step)
            .execute(pool)
            .await
            .expect("seed the start obligation");
        }

        /// One event of `run`; a higher `order` is later. Returns the event id.
        async fn seed_event(pool: &PgPool, org: &str, run: &str, event_type: &str, order: i32, payload: Value) -> String {
            let id = format!("evt-{order:03}-{}", mp_ids::new_ulid());
            sqlx::query(
                "INSERT INTO events (id, event_type, run_id, payload, ts, org_id) \
                 VALUES ($1, $2, $3, $4, now() - make_interval(secs => $5), $6)",
            )
            .bind(&id)
            .bind(event_type)
            .bind(run)
            .bind(payload)
            .bind(f64::from(1000 - order))
            .bind(org)
            .execute(pool)
            .await
            .expect("seed event");
            id
        }

        async fn cleanup(pool: &PgPool, org: &str, space: &str) {
            for sql in [
                "DELETE FROM plans WHERE run_id IN (SELECT id FROM runs WHERE org_id = $1)",
                "DELETE FROM events WHERE org_id = $1",
                "DELETE FROM managed_run_terminalization_outbox WHERE org_id = $1",
                "DELETE FROM space_membership_revocations WHERE org_id = $1",
            ] {
                sqlx::query(sql).bind(org).execute(pool).await.ok();
            }
            clean_space(pool, org, space).await;
            sqlx::query("DELETE FROM threads WHERE org_id = $1").bind(org).execute(pool).await.ok();
        }

        // ---- the start path ----

        fn browser_start(org: &str, user: &str, thread: &str, key: &str) -> pb::StartManagedRunRequest {
            pb::StartManagedRunRequest {
                wake: None,
                thread_id: thread.to_owned(),
                parent_run_id: String::new(),
                agent_id: "room-browser-agent".to_owned(),
                goal: "open the page".to_owned(),
                mode: "execute".to_owned(),
                org_id: org.to_owned(),
                user_id: user.to_owned(),
                start_key: key.to_owned(),
                terminal_source: pb::ManagedRunSource::GatewayBrowser as i32,
                agent_version_pin: None,
            }
        }

        async fn start(
            pool: &PgPool,
            org: &str,
            user: &str,
            thread: &str,
            key: &str,
        ) -> Result<crate::terminalization::ManagedRunStart, Status> {
            crate::terminalization::start_managed_run_inner(pool, browser_start(org, user, thread, key), false).await
        }

        fn cap() -> usize {
            usize::try_from(ROOM_RUNS_PER_SPACE).expect("a small cap")
        }

        /// Starts `count` room browser runs, each on a thread of its own, and returns the start keys.
        async fn fill_space(pool: &PgPool, org: &str, space: &str, suffix: &str, count: usize) -> Vec<(String, String)> {
            let mut started = Vec::new();
            for n in 0..count {
                let thread = format!("thread-{n}-{suffix}");
                let key = format!("start-{n}-{suffix}");
                seed_space_thread(pool, org, space, &thread, OWNER).await;
                let run = start(pool, org, OWNER, &thread, &key).await.expect("admitted under the cap");
                assert!(!run.already_started);
                started.push((thread, key));
            }
            started
        }

        fn assert_browser_capacity(status: &Status) {
            assert_eq!(status.code(), tonic::Code::ResourceExhausted, "{status:?}");
            assert_eq!(status.message(), "browser_capacity");
            assert_eq!(
                status.metadata().get(ERROR_CODE_TRAILER).and_then(|value| value.to_str().ok()),
                Some("browser_capacity"),
                "the typed trailer names the refusal"
            );
        }

        async fn count(pool: &PgPool, sql: &str, first: &str, second: &str) -> i64 {
            sqlx::query_scalar(sql).bind(first).bind(second).fetch_one(pool).await.expect("count")
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn a_browser_start_at_the_cap_is_resource_exhausted_and_writes_neither_a_run_nor_an_outbox_row() {
            let Some(pool) = connect().await else { return };
            let (org, space, suffix) = ids("cap");
            fill_space(&pool, &org, &space, &suffix, cap()).await;

            let thread = format!("thread-over-{suffix}");
            let key = format!("start-over-{suffix}");
            seed_space_thread(&pool, &org, &space, &thread, OWNER).await;
            let refusal = start(&pool, &org, OWNER, &thread, &key).await.expect_err("the Space is at its cap");
            assert_browser_capacity(&refusal);

            let runs = count(&pool, "SELECT count(*) FROM runs WHERE org_id = $1 AND thread_id = $2", &org, &thread).await;
            assert_eq!(runs, 0, "the refused start wrote no run, {NEEDS_DB}");
            let outbox = count(
                &pool,
                "SELECT count(*) FROM managed_run_terminalization_outbox WHERE org_id = $1 AND start_key = $2",
                &org,
                &key,
            )
            .await;
            assert_eq!(outbox, 0, "the refused start wrote no outbox row");
            let in_space = count(&pool, "SELECT count(*) FROM runs WHERE org_id = $1 AND space_id = $2", &org, &space).await;
            assert_eq!(in_space, i64::from(ROOM_RUNS_PER_SPACE), "only the admitted runs exist");
            cleanup(&pool, &org, &space).await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn two_concurrent_browser_starts_with_one_free_slot_admit_exactly_one() {
            let Some(pool) = connect().await else { return };
            let (org, space, suffix) = ids("race");
            fill_space(&pool, &org, &space, &suffix, cap() - 1).await;
            let (thread_a, thread_b) = (format!("thread-a-{suffix}"), format!("thread-b-{suffix}"));
            seed_space_thread(&pool, &org, &space, &thread_a, OWNER).await;
            seed_space_thread(&pool, &org, &space, &thread_b, OWNER).await;
            let (key_a, key_b) = (format!("start-a-{suffix}"), format!("start-b-{suffix}"));

            let (first, second) = tokio::join!(
                start(&pool, &org, OWNER, &thread_a, &key_a),
                start(&pool, &org, OWNER, &thread_b, &key_b),
            );
            let results = [first, second];
            assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1, "exactly one start is admitted, {NEEDS_DB}");
            let loser = results.into_iter().find_map(Result::err).expect("the other start is refused");
            assert_browser_capacity(&loser);
            let in_space = count(
                &pool,
                "SELECT count(*) FROM managed_run_terminalization_outbox o JOIN runs r ON r.id = o.run_id \
                 WHERE r.org_id = $1 AND r.space_id = $2 AND o.configured_source = 'gateway_browser'",
                &org,
                &space,
            )
            .await;
            assert_eq!(in_space, i64::from(ROOM_RUNS_PER_SPACE), "the Space holds exactly its cap of runs");
            cleanup(&pool, &org, &space).await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn a_replay_of_an_admitted_start_key_is_already_started_at_the_cap_and_never_refused() {
            let Some(pool) = connect().await else { return };
            let (org, space, suffix) = ids("replay");
            let started = fill_space(&pool, &org, &space, &suffix, cap()).await;

            let (thread, key) = &started[0];
            let first_run: String = sqlx::query_scalar(
                "SELECT o.run_id FROM managed_run_terminalization_outbox o WHERE o.org_id = $1 AND o.start_key = $2",
            )
            .bind(&org)
            .bind(key)
            .fetch_one(&pool)
            .await
            .expect("the admitted run");
            let replay = start(&pool, &org, OWNER, thread, key).await.expect("a replay is never refused");
            assert!(replay.already_started, "{NEEDS_DB}");
            assert_eq!(replay.run_id, first_run);
            let runs = count(&pool, "SELECT count(*) FROM runs WHERE org_id = $1 AND space_id = $2", &org, &space).await;
            assert_eq!(runs, i64::from(ROOM_RUNS_PER_SPACE), "a replay writes nothing new");
            cleanup(&pool, &org, &space).await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn a_start_on_a_thread_outside_any_space_or_on_another_users_thread_is_not_capped_here() {
            let Some(pool) = connect().await else { return };
            let (org, space, suffix) = ids("scope");
            fill_space(&pool, &org, &space, &suffix, cap()).await;

            // A thread in no Space is admitted by the cap check and then starts normally.
            let plain = format!("thread-plain-{suffix}");
            sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)")
                .bind(&plain)
                .bind(&org)
                .bind(OWNER)
                .execute(&pool)
                .await
                .expect("seed an ordinary chat thread");
            let mut tx = pool.begin().await.expect("begin");
            ensure_room_browser_capacity(&mut tx, &org, OWNER, &plain).await.expect("no Space, no cap");
            tx.rollback().await.ok();
            let started = start(&pool, &org, OWNER, &plain, &format!("start-plain-{suffix}"))
                .await
                .expect("a start outside any Space is not capped");
            assert!(!started.already_started, "{NEEDS_DB}");

            // Another user's Space thread is not capped here either: the start's own insert refuses it.
            let theirs = format!("thread-theirs-{suffix}");
            seed_space_thread(&pool, &org, &space, &theirs, "owner-2").await;
            let mut tx = pool.begin().await.expect("begin");
            ensure_room_browser_capacity(&mut tx, &org, OWNER, &theirs)
                .await
                .expect("the cap check passes another user's thread through");
            tx.rollback().await.ok();
            let refusal = start(&pool, &org, OWNER, &theirs, &format!("start-theirs-{suffix}"))
                .await
                .expect_err("the insert refuses a thread that is not the caller's");
            assert_eq!(refusal.code(), tonic::Code::NotFound, "{refusal:?}");
            assert_ne!(refusal.message(), "browser_capacity");

            // A thread that does not exist passes the cap check too.
            let mut tx = pool.begin().await.expect("begin");
            ensure_room_browser_capacity(&mut tx, &org, OWNER, "thread-missing").await.expect("nothing to cap");
            tx.rollback().await.ok();

            // The caller's own Space thread is still refused at the cap.
            let mine = format!("thread-mine-{suffix}");
            seed_space_thread(&pool, &org, &space, &mine, OWNER).await;
            let mut tx = pool.begin().await.expect("begin");
            let refusal = ensure_room_browser_capacity(&mut tx, &org, OWNER, &mine).await.expect_err("at the cap");
            tx.rollback().await.ok();
            assert_browser_capacity(&refusal);
            cleanup(&pool, &org, &space).await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn terminal_runs_and_runs_of_other_sources_are_not_counted() {
            let Some(pool) = connect().await else { return };
            let (org, space, suffix) = ids("count");
            let thread = format!("thread-{suffix}");
            seed_space_thread(&pool, &org, &space, &thread, OWNER).await;
            let in_flight = |pool: PgPool, org: String, space: String, exclude: String| async move {
                sqlx::query_scalar::<_, i64>(COUNT_ACTIVE_ROOM_BROWSER_RUNS_SQL)
                    .bind(org)
                    .bind(space)
                    .bind(exclude)
                    .fetch_one(&pool)
                    .await
                    .expect("count")
            };

            // Browser runs that ended, and in-flight runs of every other source, are not counted.
            for (n, status) in ["completed", "failed", "cancelled"].into_iter().enumerate() {
                seed_run(&pool, &org, &space, &thread, &format!("run-done-{n}-{suffix}"), status, "gateway_browser").await;
            }
            for (n, (status, source)) in [("running", "execution_agent"), ("queued", "gateway_direct"), ("running", "execution_browser")]
                .into_iter()
                .enumerate()
            {
                seed_run(&pool, &org, &space, &thread, &format!("run-other-{n}-{suffix}"), status, source).await;
            }
            assert_eq!(in_flight(pool.clone(), org.clone(), space.clone(), String::new()).await, 0, "{NEEDS_DB}");
            let mut tx = pool.begin().await.expect("begin");
            ensure_room_browser_capacity(&mut tx, &org, OWNER, &thread).await.expect("nothing in flight");
            tx.rollback().await.ok();

            // Live browser runs are counted, queued or running, and the cap bites at ROOM_RUNS_PER_SPACE.
            seed_run(&pool, &org, &space, &thread, &format!("run-live-0-{suffix}"), "running", "gateway_browser").await;
            assert_eq!(in_flight(pool.clone(), org.clone(), space.clone(), String::new()).await, 1);
            let mut tx = pool.begin().await.expect("begin");
            ensure_room_browser_capacity(&mut tx, &org, OWNER, &thread).await.expect("one free slot");
            tx.rollback().await.ok();
            seed_run(&pool, &org, &space, &thread, &format!("run-live-1-{suffix}"), "queued", "gateway_browser").await;
            assert_eq!(in_flight(pool.clone(), org.clone(), space.clone(), String::new()).await, 2);
            let mut tx = pool.begin().await.expect("begin");
            let refusal = ensure_room_browser_capacity(&mut tx, &org, OWNER, &thread).await.expect_err("at the cap");
            tx.rollback().await.ok();
            assert_browser_capacity(&refusal);

            // The exclusion leaves one start key out of the count, and another Space is a count of its own.
            let excluded = format!("sk-run-live-0-{suffix}");
            assert_eq!(in_flight(pool.clone(), org.clone(), space.clone(), excluded).await, 1);
            assert_eq!(in_flight(pool.clone(), org.clone(), format!("{space}-other"), String::new()).await, 0);
            assert_eq!(in_flight(pool.clone(), format!("{org}-other"), space.clone(), String::new()).await, 0);
            cleanup(&pool, &org, &space).await;
        }

        // ---- the three room reads (WS20-S4) ----

        fn read_token(org: &str, user: &str, space: &str, revision: i64) -> String {
            mint(
                crate::grpc::CONTROL_SPACE_DECISION_AUDIENCE,
                &["thread:read"],
                false,
                |claims| {
                    claims["org_id"] = json!(org);
                    claims["subject_id"] = json!(user);
                    claims["space_ref"] = json!(space);
                    claims["recipient_audience_revision"] = json!(revision);
                    claims["issued_at"] = json!(Utc::now().to_rfc3339());
                    claims["expires_at"] = json!((Utc::now() + chrono::Duration::minutes(2)).to_rfc3339());
                },
                false,
            )
        }

        fn request_for<T>(message: T, org: &str, user: &str) -> Request<T> {
            let mut request = Request::new(message);
            request.extensions_mut().insert(VerifiedIdentity::user_for_test(org, user));
            request
        }

        async fn list_runs(pool: &PgPool, org: &str, user: &str, space: &str, thread: &str) -> Result<pb::ListRoomBrowserRunsResponse, Status> {
            let request = pb::ListRoomBrowserRunsRequest {
                space_id: space.to_owned(),
                thread_id: thread.to_owned(),
                limit: 0,
                before_run_id: String::new(),
                space_read_decision_ref: DECISION_REF.to_owned(),
                space_read_decision_token: read_token(org, user, space, CEILING),
            };
            SpaceBrowserReadGrpc::new(pool.clone())
                .list_room_browser_runs(request_for(request, org, user))
                .await
                .map(Response::into_inner)
        }

        async fn read_timeline(pool: &PgPool, org: &str, user: &str, space: &str, run: &str) -> Result<pb::ReadRoomBrowserTimelineResponse, Status> {
            let request = pb::ReadRoomBrowserTimelineRequest {
                space_id: space.to_owned(),
                run_id: run.to_owned(),
                after_event_id: String::new(),
                limit: 0,
                space_read_decision_ref: DECISION_REF.to_owned(),
                space_read_decision_token: read_token(org, user, space, CEILING),
            };
            SpaceBrowserReadGrpc::new(pool.clone())
                .read_room_browser_timeline(request_for(request, org, user))
                .await
                .map(Response::into_inner)
        }

        async fn authorize_evidence(
            pool: &PgPool,
            org: &str,
            user: &str,
            space: &str,
            run: &str,
            evidence: &str,
        ) -> Result<pb::AuthorizeRoomBrowserEvidenceResponse, Status> {
            let request = pb::AuthorizeRoomBrowserEvidenceRequest {
                space_id: space.to_owned(),
                run_id: run.to_owned(),
                evidence_id: evidence.to_owned(),
                space_read_decision_ref: DECISION_REF.to_owned(),
                space_read_decision_token: read_token(org, user, space, CEILING),
            };
            SpaceBrowserReadGrpc::new(pool.clone())
                .authorize_room_browser_evidence(request_for(request, org, user))
                .await
                .map(Response::into_inner)
        }

        /// A browser run with one action, its observation and a screenshot reference, on a new thread.
        async fn seed_observed_run(pool: &PgPool, org: &str, space: &str, name: &str, shot: &str) -> (String, String) {
            let thread = format!("thread-{name}");
            let run = format!("run-{name}");
            seed_space_thread(pool, org, space, &thread, OWNER).await;
            seed_run(pool, org, space, &thread, &run, "running", "gateway_browser").await;
            seed_event(pool, org, &run, "BROWSER_ACTION_DISPATCHED", 1, json!({"action_id": "a1", "action_type": "navigate", "url": "https://example.com/"})).await;
            seed_event(
                pool,
                org,
                &run,
                "BROWSER_OBSERVATION_RECEIVED",
                2,
                json!({"action_id": "a1", "status": "loaded", "page_url": "https://example.com/", "screenshot_ref": shot}),
            )
            .await;
            (thread, run)
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn projector_drops_reason_title_selector_and_full_url_through_the_three_rpcs() {
            let Some(pool) = connect().await else { return };
            let _keys = StreamKeysEnv::publish();
            let (org, space, suffix) = ids("proj");
            let (thread, run) = (format!("thread-{suffix}"), format!("run-{suffix}"));
            seed_space_thread(&pool, &org, &space, &thread, OWNER).await;
            seed_run(&pool, &org, &space, &thread, &run, "running", "gateway_browser").await;
            let events = [
                (
                    "BROWSER_ACTION_DISPATCHED",
                    json!({"action_id": "a1", "action_type": "navigate", "url": "https://Example.com/secret?token=CANARY-URL",
                           "reason": "CANARY-REASON", "selector": "#CANARY-SELECTOR", "title": "CANARY-TITLE"}),
                ),
                (
                    "BROWSER_OBSERVATION_RECEIVED",
                    json!({"action_id": "a1", "status": "loaded", "page_url": "https://pages.example.org/inbox?m=CANARY-URL",
                           "page_title": "CANARY-TITLE", "screenshot_ref": "shot-1", "dom_ref": "CANARY-DOM"}),
                ),
                (
                    "BROWSER_ACTION_APPROVAL_REQUIRED",
                    json!({"action_id": "a2", "action_type": "click", "url": "https://pay.example.com/checkout?CANARY-URL",
                           "selector": "#CANARY-SELECTOR", "reason": "CANARY-REASON", "risk_category": "financial", "approval_id": "appr-1"}),
                ),
                (
                    "BROWSER_ACTION_DECIDED",
                    json!({"action_id": "a2", "approval_id": "appr-1", "decision": "approved", "decided_by": "CANARY-SUBJECT"}),
                ),
                ("BROWSER_FRAME_CAPTURED", json!({"frame": "CANARY-FRAME"})),
                ("RUN_COMPLETED", json!({})),
            ];
            for (order, (event_type, payload)) in (1..).zip(events) {
                seed_event(&pool, &org, &run, event_type, order, payload).await;
            }

            let listed = list_runs(&pool, &org, VIEWER, &space, "").await.expect("the list answers");
            assert_eq!(listed.runs.len(), 1, "{NEEDS_DB}");
            assert_eq!(listed.runs[0].run_id, run);
            assert_eq!(listed.runs[0].started_by_subject_id, OWNER);
            assert_eq!(listed.runs[0].hosts, ["example.com", "pages.example.org"]);
            assert_eq!(listed.runs[0].step_count, 1);

            let timeline = read_timeline(&pool, &org, VIEWER, &space, &run).await.expect("the timeline answers");
            let shown: Vec<(&str, &str, &str, &str, &str)> = timeline
                .events
                .iter()
                .map(|event| (event.kind.as_str(), event.step.as_str(), event.host.as_str(), event.evidence_id.as_str(), event.decision.as_str()))
                .collect();
            assert_eq!(
                shown,
                [
                    ("action", "a1", "example.com", "", ""),
                    ("observation", "a1", "pages.example.org", "shot-1", ""),
                    ("approval_required", "a2", "pay.example.com", "", ""),
                    ("approval_decided", "a2", "", "", "approved"),
                    ("result", "", "", "", ""),
                ],
                "the unknown frame event is not projected"
            );
            assert_eq!(timeline.events[2].risk_category, "financial");
            assert_eq!(timeline.events[2].approval_id, "appr-1");
            assert_eq!(timeline.events[4].outcome, "completed");
            assert_eq!(timeline.run.as_ref().map(|header| header.run_id.as_str()), Some(run.as_str()));

            let authorized = authorize_evidence(&pool, &org, VIEWER, &space, &run, "shot-1").await.expect("the evidence is authorized");
            assert_eq!(authorized.artifact_id, "shot-1");

            // Nothing private reaches any of the three answers.
            let everything = format!("{listed:?}{timeline:?}{authorized:?}");
            for private in ["CANARY", "token=", "://", "secret", "checkout", "inbox"] {
                assert!(!everything.contains(private), "{private:?} leaked: {everything}");
            }
            cleanup(&pool, &org, &space).await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn foreign_run_is_not_found_on_all_three_reads() {
            let Some(pool) = connect().await else { return };
            let _keys = StreamKeysEnv::publish();
            let (org, space, suffix) = ids("foreign");
            let other_org = format!("{org}-other");
            let other_space = format!("{space}-other");

            // The run the viewer may see.
            let (_, visible) = seed_observed_run(&pool, &org, &space, &format!("visible-{suffix}"), "shot-visible").await;
            // A run of another Space of the same org.
            let (_, other_space_run) = seed_observed_run(&pool, &org, &other_space, &format!("space-{suffix}"), "shot-space").await;
            // A run of another org that wears the same Space id.
            let (_, other_org_run) = seed_observed_run(&pool, &other_org, &space, &format!("org-{suffix}"), "shot-org").await;
            // A run on a thread stamped beyond the decision's audience ceiling.
            let (beyond_thread, beyond_run) = seed_observed_run(&pool, &org, &space, &format!("beyond-{suffix}"), "shot-beyond").await;
            sqlx::query("UPDATE threads SET recipient_audience_revision = $2 WHERE id = $1")
                .bind(&beyond_thread)
                .bind(CEILING + 1)
                .execute(&pool)
                .await
                .expect("restamp the thread above the ceiling");
            // A run that did not come from the room's browser source.
            let agent_thread = format!("thread-agent-{suffix}");
            seed_space_thread(&pool, &org, &space, &agent_thread, OWNER).await;
            let agent_run = format!("run-agent-{suffix}");
            seed_run(&pool, &org, &space, &agent_thread, &agent_run, "running", "execution_agent").await;
            seed_event(&pool, &org, &agent_run, "BROWSER_OBSERVATION_RECEIVED", 1, json!({"screenshot_ref": "shot-agent"})).await;

            // The one the viewer may see reads normally.
            read_timeline(&pool, &org, VIEWER, &space, &visible).await.expect("the visible run reads");
            authorize_evidence(&pool, &org, VIEWER, &space, &visible, "shot-visible").await.expect("its evidence reads");

            // Every other run, and one that does not exist, is NotFound on the timeline and the evidence check.
            for (why, run, shot) in [
                ("another Space", other_space_run.as_str(), "shot-space"),
                ("another org", other_org_run.as_str(), "shot-org"),
                ("a thread beyond the audience ceiling", beyond_run.as_str(), "shot-beyond"),
                ("a run of another source", agent_run.as_str(), "shot-agent"),
                ("a run that does not exist", "run-missing", "shot-x"),
            ] {
                let status = read_timeline(&pool, &org, VIEWER, &space, run).await.expect_err(why);
                assert_eq!(status.code(), tonic::Code::NotFound, "timeline, {why}: {status:?}");
                let status = authorize_evidence(&pool, &org, VIEWER, &space, run, shot).await.expect_err(why);
                assert_eq!(status.code(), tonic::Code::NotFound, "evidence, {why}: {status:?}");
            }

            // The list has no run id to ask for: the same runs are simply absent, by Space, org, ceiling and source.
            let listed = list_runs(&pool, &org, VIEWER, &space, "").await.expect("the list answers");
            let listed_ids: Vec<&str> = listed.runs.iter().map(|run| run.run_id.as_str()).collect();
            assert_eq!(listed_ids, [visible.as_str()], "{NEEDS_DB}");
            // A thread filter that names a foreign thread lists nothing; it never widens the Space.
            for foreign_thread in [format!("thread-space-{suffix}"), format!("thread-org-{suffix}"), beyond_thread, agent_thread] {
                let listed = list_runs(&pool, &org, VIEWER, &space, &foreign_thread).await.expect("the list answers");
                assert!(listed.runs.is_empty(), "{foreign_thread}: {listed:?}");
            }
            cleanup(&pool, &org, &space).await;
            cleanup(&pool, &org, &other_space).await;
            cleanup(&pool, &other_org, &space).await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn revoked_member_denied_on_all_three_reads() {
            let Some(pool) = connect().await else { return };
            let _keys = StreamKeysEnv::publish();
            let (org, space, suffix) = ids("revoked");
            let (_, run) = seed_observed_run(&pool, &org, &space, &format!("run-{suffix}"), "shot-1").await;

            let all_three = |viewer: &'static str| {
                let (pool, org, space, run) = (pool.clone(), org.clone(), space.clone(), run.clone());
                async move {
                    (
                        list_runs(&pool, &org, viewer, &space, "").await.map(|_| ()),
                        read_timeline(&pool, &org, viewer, &space, &run).await.map(|_| ()),
                        authorize_evidence(&pool, &org, viewer, &space, &run, "shot-1").await.map(|_| ()),
                    )
                }
            };
            let (list, timeline, evidence) = all_three(VIEWER).await;
            list.expect("a member lists");
            timeline.expect("a member reads the timeline");
            evidence.expect("a member reads the evidence");

            sqlx::query("INSERT INTO space_membership_revocations (space_ref, org_id, subject_id) VALUES ($1, $2, $3)")
                .bind(&space)
                .bind(&org)
                .bind(VIEWER)
                .execute(&pool)
                .await
                .expect("revoke the member");
            let (list, timeline, evidence) = all_three(VIEWER).await;
            for (read, result) in [("list", list), ("timeline", timeline), ("evidence", evidence)] {
                let status = result.expect_err(read);
                assert_eq!(status.code(), tonic::Code::PermissionDenied, "{read}: {status:?}, {NEEDS_DB}");
            }

            // Another member of the Space is unaffected, and so is the revoked member in another Space.
            let (list, timeline, evidence) = all_three("viewer-2").await;
            list.expect("another member lists");
            timeline.expect("another member reads the timeline");
            evidence.expect("another member reads the evidence");
            cleanup(&pool, &org, &space).await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn evidence_id_must_belong_to_run() {
            let Some(pool) = connect().await else { return };
            let _keys = StreamKeysEnv::publish();
            let (org, space, suffix) = ids("evidence");
            let (_, first) = seed_observed_run(&pool, &org, &space, &format!("first-{suffix}"), "shot-first").await;
            let (_, second) = seed_observed_run(&pool, &org, &space, &format!("second-{suffix}"), "shot-second").await;

            let own = authorize_evidence(&pool, &org, VIEWER, &space, &first, "shot-first").await.expect("its own screenshot");
            assert_eq!(own.artifact_id, "shot-first");
            // A screenshot reference of another run of the same Space is not this run's evidence.
            let status = authorize_evidence(&pool, &org, VIEWER, &space, &first, "shot-second").await.expect_err("another run's");
            assert_eq!(status.code(), tonic::Code::NotFound, "{status:?}, {NEEDS_DB}");
            let status = authorize_evidence(&pool, &org, VIEWER, &space, &second, "shot-first").await.expect_err("the other way round");
            assert_eq!(status.code(), tonic::Code::NotFound, "{status:?}");
            // An unknown reference is the same answer.
            let status = authorize_evidence(&pool, &org, VIEWER, &space, &first, "shot-unknown").await.expect_err("unknown");
            assert_eq!(status.code(), tonic::Code::NotFound, "{status:?}");
            // So is a reference that only an action row (not an observation) carries.
            seed_event(&pool, &org, &first, "BROWSER_ACTION_DISPATCHED", 3, json!({"action_id": "a2", "screenshot_ref": "shot-from-action"})).await;
            let status = authorize_evidence(&pool, &org, VIEWER, &space, &first, "shot-from-action").await.expect_err("not an observation");
            assert_eq!(status.code(), tonic::Code::NotFound, "{status:?}");
            cleanup(&pool, &org, &space).await;
        }

        /// A read token for a member whose D18 timestamp floor is `floor_at`, signed by Control as a window's own row.
        fn read_token_floored(org: &str, user: &str, space: &str, revision: i64, floor_at: DateTime<Utc>) -> String {
            mint(
                crate::grpc::CONTROL_SPACE_DECISION_AUDIENCE,
                &["thread:read"],
                false,
                |claims| {
                    claims["org_id"] = json!(org);
                    claims["subject_id"] = json!(user);
                    claims["space_ref"] = json!(space);
                    claims["recipient_audience_revision"] = json!(revision);
                    claims["history_floor_at"] = json!(floor_at.to_rfc3339());
                    claims["issued_at"] = json!(Utc::now().to_rfc3339());
                    claims["expires_at"] = json!((Utc::now() + chrono::Duration::minutes(2)).to_rfc3339());
                },
                false,
            )
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn a_member_with_a_timestamp_floor_reads_no_run_or_event_from_before_it_on_any_room_read() {
            let Some(pool) = connect().await else { return };
            let _keys = StreamKeysEnv::publish();
            let (org, space, suffix) = ids("floor");
            let (_, earlier) = seed_observed_run(&pool, &org, &space, &format!("earlier-{suffix}"), "shot-earlier").await;
            let (_, later) = seed_observed_run(&pool, &org, &space, &format!("later-{suffix}"), "shot-later").await;
            // The floor is an hour ago. The earlier run started two days ago; the later run started now.
            sqlx::query("UPDATE runs SET created_at = now() - interval '2 days' WHERE id = $1")
                .bind(&earlier)
                .execute(&pool)
                .await
                .expect("date the earlier run before the floor");
            // The later run's observation is dated before the floor as well: an event is floored on its own instant.
            sqlx::query(
                "UPDATE events SET ts = now() - interval '2 days' \
                 WHERE run_id = $1 AND event_type = 'BROWSER_OBSERVATION_RECEIVED'",
            )
            .bind(&later)
            .execute(&pool)
            .await
            .expect("date the observation before the floor");
            let floor = Utc::now() - chrono::Duration::hours(1);
            let token = read_token_floored(&org, VIEWER, &space, CEILING, floor);

            // Without the window the member reads the earlier run, so the window is what hides it.
            list_runs(&pool, &org, VIEWER, &space, "").await.expect("the unwindowed list reads");
            read_timeline(&pool, &org, VIEWER, &space, &earlier).await.expect("the unwindowed timeline reads");

            let store = SpaceBrowserReadGrpc::new(pool.clone());
            let listed = store
                .list_room_browser_runs(request_for(
                    pb::ListRoomBrowserRunsRequest {
                        space_id: space.clone(),
                        thread_id: String::new(),
                        limit: 0,
                        before_run_id: String::new(),
                        space_read_decision_ref: DECISION_REF.to_owned(),
                        space_read_decision_token: token.clone(),
                    },
                    &org,
                    VIEWER,
                ))
                .await
                .expect("the windowed list reads")
                .into_inner();
            let ids_listed: Vec<&str> = listed.runs.iter().map(|run| run.run_id.as_str()).collect();
            assert_eq!(ids_listed, vec![later.as_str()], "only the run started after the floor is listed");

            // The earlier run is NotFound on its header, its timeline and its evidence, the same as a run that is absent.
            let timeline = store
                .read_room_browser_timeline(request_for(
                    pb::ReadRoomBrowserTimelineRequest {
                        space_id: space.clone(),
                        run_id: earlier.clone(),
                        after_event_id: String::new(),
                        limit: 0,
                        space_read_decision_ref: DECISION_REF.to_owned(),
                        space_read_decision_token: token.clone(),
                    },
                    &org,
                    VIEWER,
                ))
                .await
                .expect_err("a run before the floor has no timeline");
            assert_eq!(timeline.code(), tonic::Code::NotFound, "{timeline:?}, {NEEDS_DB}");
            let evidence = store
                .authorize_room_browser_evidence(request_for(
                    pb::AuthorizeRoomBrowserEvidenceRequest {
                        space_id: space.clone(),
                        run_id: earlier.clone(),
                        evidence_id: "shot-earlier".to_owned(),
                        space_read_decision_ref: DECISION_REF.to_owned(),
                        space_read_decision_token: token.clone(),
                    },
                    &org,
                    VIEWER,
                ))
                .await
                .expect_err("a screenshot of a run before the floor is not authorized");
            assert_eq!(evidence.code(), tonic::Code::NotFound, "{evidence:?}");

            // The later run's timeline omits the observation dated before the floor, and its screenshot is not authorized.
            let kept = store
                .read_room_browser_timeline(request_for(
                    pb::ReadRoomBrowserTimelineRequest {
                        space_id: space.clone(),
                        run_id: later.clone(),
                        after_event_id: String::new(),
                        limit: 0,
                        space_read_decision_ref: DECISION_REF.to_owned(),
                        space_read_decision_token: token.clone(),
                    },
                    &org,
                    VIEWER,
                ))
                .await
                .expect("the later run reads")
                .into_inner();
            assert!(
                kept.events.iter().all(|event| event.evidence_id != "shot-later"),
                "the observation dated before the floor is not shown: {kept:?}"
            );
            let dated_before = store
                .authorize_room_browser_evidence(request_for(
                    pb::AuthorizeRoomBrowserEvidenceRequest {
                        space_id: space.clone(),
                        run_id: later.clone(),
                        evidence_id: "shot-later".to_owned(),
                        space_read_decision_ref: DECISION_REF.to_owned(),
                        space_read_decision_token: token.clone(),
                    },
                    &org,
                    VIEWER,
                ))
                .await
                .expect_err("a screenshot whose observation is before the floor is not authorized");
            assert_eq!(dated_before.code(), tonic::Code::NotFound, "{dated_before:?}");
            cleanup(&pool, &org, &space).await;
        }
    }
}
