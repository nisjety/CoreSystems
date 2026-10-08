//! Model `tool_action` audit publisher.
//!
//! Session Core transactionally records governed `STEP_COMPLETED` events and
//! the Gateway's reserve-before-execute/finalize-after-execute inline tool
//! lifecycle. Producer-scoped v2 events leave only through this durable outbox
//! and are acknowledged by JetStream before delivery state advances.
//!
//! The execution-core driver prefixes each per-tool step's output with
//! `[data_category=<class> zdr=<bool>]` (see `runtime_loop::agent::record_tool_step`);
//! this module parses that prefix plus the step's tool name to fill the audit
//! `details`.
//!
//! Wire shape: audit-core decodes the NATS message body *directly* into its
//! `AuditEvent` struct (`json.Unmarshal(data, &ev)`), so the message body is the
//! FLAT audit shape — NOT wrapped in an event `Envelope`. Field names mirror
//! audit-core's json tags: `occurred_at, org_id, user_id, plane, event, subject,
//! resource_id, outcome, details`. Required by audit-core's `Validate()`:
//! `org_id`, `plane`, `event`.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use sqlx::PgPool;
use tokio::sync::Mutex;

/// NATS subject of a model tool call's final outcome. audit-core consumes it
/// through its `verevon.audit.v2.model.>` durable on the Model Plane broker,
/// which exists only because `nats-provisioner` provisions the
/// `VEREVON_CONTROL_OBSERVABILITY` stream there (`go/services/nats-provisioner`;
/// its contract test publishes this subject through the real manifest).
pub const SUBJECT_MODEL_TOOL_ACTION: &str = "verevon.audit.v2.model.session-core.tool_action";

/// Audit event name of a tool action's final outcome.
pub const EVENT_TOOL_ACTION: &str = "tool_action";

/// Audit event name of the reservation that precedes an inline tool call.
pub const EVENT_TOOL_ACTION_RESERVED: &str = "tool_action_reserved";

/// Subject family carrying Session Core's flat audit-core bodies.
const AUDIT_SUBJECT_PREFIX: &str = "verevon.audit.v2.model.session-core.";

/// Subject family carrying canonical run-lifecycle `Envelope`s:
/// `mp.v1.run.<run_id>.event`.
const RUN_EVENT_SUBJECT_PREFIX: &str = "mp.v1.run.";
const RUN_EVENT_SUBJECT_SUFFIX: &str = ".event";

/// Delivery route for one outbox row, derived from the row's own `subject`.
///
/// Session Core may publish inside exactly these two subject families; a row
/// naming anything else is refused before it reaches NATS. The two families
/// carry DIFFERENT wire shapes (a flat audit body vs. a canonical `Envelope`),
/// so the authority check is per-route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxRoute<'a> {
    /// `verevon.audit.v2.model.session-core.*` — flat audit-core `AuditEvent`.
    Audit,
    /// `mp.v1.run.<run_id>.event` — run-lifecycle `Envelope`. Consumed by
    /// capability-core's session review (the skill-learning trigger) over core
    /// NATS, and captured by the `MODEL_PLANE_RUN_EVENTS` stream for insight-core.
    RunEvent { run_id: &'a str },
}

/// Classify an outbox subject, or `None` when it is outside Session Core's
/// publish authority.
#[must_use]
pub fn classify_outbox_subject(subject: &str) -> Option<OutboxRoute<'_>> {
    if subject.starts_with(AUDIT_SUBJECT_PREFIX) {
        return Some(OutboxRoute::Audit);
    }
    let run_id = subject
        .strip_prefix(RUN_EVENT_SUBJECT_PREFIX)?
        .strip_suffix(RUN_EVENT_SUBJECT_SUFFIX)?;
    // `mp.v1.run.*.event` is a SINGLE-token wildcard. A run id containing '.'
    // would resolve to a subject outside the granted scope, so refuse it rather
    // than let a row widen its own authority.
    if run_id.is_empty() || run_id.contains('.') {
        return None;
    }
    Some(OutboxRoute::RunEvent { run_id })
}

/// Verify a claimed row's payload really belongs to that row and to Session Core
/// before it is published.
fn check_outbox_authority(
    route: OutboxRoute<'_>,
    event_id: &str,
    payload: &serde_json::Value,
) -> Result<(), String> {
    if payload["event_id"].as_str() != Some(event_id)
        || payload["producer"].as_str() != Some("session-core")
    {
        return Err("outbox payload authority does not match its row".to_owned());
    }
    match route {
        OutboxRoute::Audit => {
            if payload["plane"].as_str() != Some("model") {
                return Err("audit payload authority does not match its outbox row".to_owned());
            }
        }
        OutboxRoute::RunEvent { run_id } => {
            // The subject names the run, so the envelope must name the same run.
            // This is what stops one run's row being published under another
            // run's subject.
            if payload["resource_ref"].as_str() != Some(format!("run:{run_id}").as_str()) {
                return Err("run event payload does not match its subject run".to_owned());
            }
            // Both are required by the consumer's decode; an empty value would
            // be dropped downstream, so fail loudly here instead.
            if payload["event_type"]
                .as_str()
                .unwrap_or_default()
                .is_empty()
                || payload["org_id"].as_str().unwrap_or_default().is_empty()
            {
                return Err("run event envelope is missing event_type or org_id".to_owned());
            }
        }
    }
    Ok(())
}

fn tool_action_event_id(run_id: &str, step_id: &str, phase: &str) -> String {
    let digest = blake3::hash(format!("{run_id}\0{step_id}\0{phase}").as_bytes());
    format!("tool:session-core:{digest}")
}

/// Audit event name for one phase of a tool action. This is the single place
/// that decides it: the body's `event` and the wire subject's last token must be
/// the same string, because audit-core refuses a message whose subject names a
/// different event than its payload (`eventAuthorityMatches`).
#[must_use]
pub fn tool_action_event_name(phase: &str) -> &'static str {
    if phase == "reserved" {
        EVENT_TOOL_ACTION_RESERVED
    } else {
        EVENT_TOOL_ACTION
    }
}

/// Wire subject for an audit event name: `<audit family prefix><event>`.
///
/// `None` when the name is not a single lowercase subject token, so a payload
/// cannot widen its own subject (a `.` would add a token, `*`/`>` a wildcard).
#[must_use]
pub fn audit_subject_for_event(event: &str) -> Option<String> {
    let single_token = !event.is_empty()
        && event.len() <= 64
        && event
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    single_token.then(|| format!("{AUDIT_SUBJECT_PREFIX}{event}"))
}

/// The subject a claimed outbox row is published on.
///
/// An audit row goes out on the subject its payload's `event` names, whatever
/// subject the row stored: rows written before the reserved phase had its own
/// subject carry `...tool_action` with an `event` of `tool_action_reserved`,
/// and publishing them as stored would be refused by audit-core and lost to its
/// dead-letter stream. A run-event row keeps its own subject (the route check
/// already tied it to the payload's run).
fn resolve_wire_subject(
    route: OutboxRoute<'_>,
    row_subject: &str,
    payload: &serde_json::Value,
) -> Result<String, String> {
    match route {
        OutboxRoute::Audit => payload["event"]
            .as_str()
            .and_then(audit_subject_for_event)
            .ok_or_else(|| {
                "audit payload event is not a single lowercase subject token".to_owned()
            }),
        OutboxRoute::RunEvent { .. } => Ok(row_subject.to_owned()),
    }
}

/// Parsed GDPR detail extracted from a per-tool step's output prefix
/// `[data_category=<class> zdr=<bool> tool=<name>]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolActionDetail {
    pub data_category: String,
    pub zdr: bool,
    /// Human-readable tool name carried in the prefix (`tool=<name>`, E5). `None`
    /// for steps written before E5 — callers then fall back to
    /// [`tool_name_from_step_id`], which yields the opaque provider call id.
    pub tool: Option<String>,
}

/// Sink for audit event bodies. Abstracted so the publish path is unit-testable
/// against an in-memory recorder; production uses [`NatsAuditPublisher`].
pub trait AuditSink: Send + Sync {
    /// Publish a JSON audit body to `subject`.
    fn publish_audit(
        &self,
        subject: &str,
        body: &serde_json::Value,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;
}

/// Parse the `[data_category=<class> zdr=<bool>]` prefix execution-core writes
/// onto a per-tool step's `output` (or `error`). Returns `None` when the prefix
/// is absent, so non-tool steps (e.g. the terminal `final` step) are skipped.
#[must_use]
pub fn parse_tool_action_detail(output: &str, error: &str) -> Option<ToolActionDetail> {
    let source = if output.trim_start().starts_with("[data_category=") {
        output
    } else {
        error
    };
    let start = source.find("[data_category=")?;
    let end = source[start..].find(']')? + start;
    let inner = &source[start + 1..end]; // strip the surrounding [ ]

    let mut data_category = None;
    let mut zdr = None;
    let mut tool = None;
    for token in inner.split_whitespace() {
        if let Some(value) = token.strip_prefix("data_category=") {
            data_category = Some(value.to_owned());
        } else if let Some(value) = token.strip_prefix("zdr=") {
            zdr = Some(value == "true");
        } else if let Some(value) = token.strip_prefix("tool=") {
            if !value.is_empty() {
                tool = Some(value.to_owned());
            }
        }
    }

    Some(ToolActionDetail {
        data_category: data_category?,
        zdr: zdr.unwrap_or(false),
        tool,
    })
}

/// Resolve the human-readable tool name for an audit row: prefer the explicit
/// `tool=<name>` from the execution-core prefix (E5); fall back to extracting it
/// from the `step_id` (which yields the opaque provider call id) only for steps
/// written before E5.
#[must_use]
pub fn resolve_tool_name(detail: &ToolActionDetail, step_id: &str) -> String {
    detail
        .tool
        .clone()
        .unwrap_or_else(|| tool_name_from_step_id(step_id))
}

/// Extract the tool name from a per-tool `step_id`. execution-core mints these
/// as `tool_{seq}_{call_id_or_name}` (see `runtime_loop::agent::tool_step_id`);
/// the trailing segment is the tool call id or name. When it can't be parsed,
/// the raw `step_id` is returned so the audit row still carries a value.
#[must_use]
pub fn tool_name_from_step_id(step_id: &str) -> String {
    step_id
        .strip_prefix("tool_")
        .and_then(|rest| rest.split_once('_'))
        .map_or_else(|| step_id.to_owned(), |(_, name)| name.to_owned())
}

/// Build the FLAT audit body matching audit-core's `AuditEvent` json tags:
/// `{occurred_at, org_id, user_id, plane, event, subject, resource_id, outcome, details}`.
// One cohesive audit body; every arg is a distinct audit field, so a parameter
// struct would only add indirection without clarifying intent.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn build_tool_action_body(
    org_id: &str,
    user_id: &str,
    run_id: &str,
    step_id: &str,
    status: &str,
    tool: &str,
    detail: &ToolActionDetail,
    occurred_at: chrono::DateTime<chrono::Utc>,
) -> serde_json::Value {
    build_tool_action_body_for_phase(
        org_id,
        user_id,
        run_id,
        step_id,
        status,
        tool,
        detail,
        "final",
        occurred_at,
    )
}

/// Build a phase-specific tool action event. Reservation and final outcome use
/// different stable event identities, allowing both to be delivered exactly
/// once while preserving one logical action identifier.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn build_tool_action_body_for_phase(
    org_id: &str,
    user_id: &str,
    run_id: &str,
    step_id: &str,
    status: &str,
    tool: &str,
    detail: &ToolActionDetail,
    phase: &str,
    occurred_at: chrono::DateTime<chrono::Utc>,
) -> serde_json::Value {
    serde_json::json!({
        "event_id": tool_action_event_id(run_id, step_id, phase),
        "occurred_at": occurred_at.to_rfc3339(),
        "org_id": org_id,
        // audit ingest treats user as optional; agentic tool steps run on the
        // user's behalf — fall back to "agent" when the user is unknown.
        "user_id": if user_id.is_empty() { "agent" } else { user_id },
        "plane": "model",
        "producer": "session-core",
        "event": tool_action_event_name(phase),
        "subject": run_id,
        "resource_id": step_id,
        "outcome": status,
        "details": {
            "tool": tool,
            "data_category": detail.data_category,
            "zdr": detail.zdr,
            "phase": phase,
        },
    })
}

/// Publish one model tool-action audit event and require the transport's
/// acknowledgement. The outbox owns retry/error handling. The subject is the one
/// the body's own `event` names (see [`resolve_wire_subject`]).
pub async fn publish_tool_action<S: AuditSink>(
    sink: &S,
    body: &serde_json::Value,
) -> Result<(), String> {
    let subject = resolve_wire_subject(OutboxRoute::Audit, SUBJECT_MODEL_TOOL_ACTION, body)?;
    sink.publish_audit(&subject, body).await
}

/// Thin NATS transport for audit events. Publishes the literal audit subject
/// directly — the `verevon.audit.*` tree is outside `mp.v1.*`, so no subject
/// translation applies (unlike the run-event publisher).
pub struct NatsAuditPublisher {
    jetstream: async_nats::jetstream::Context,
}

impl NatsAuditPublisher {
    /// Connect to the NATS server at `url`.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    pub async fn connect(url: &str) -> Result<Self, async_nats::ConnectError> {
        let client = crate::nats_connection::connect(url).await?;
        tracing::info!(url = %url, "session-core audit publisher connected to NATS");
        Ok(Self {
            jetstream: async_nats::jetstream::new(client),
        })
    }
}

impl AuditSink for NatsAuditPublisher {
    async fn publish_audit(&self, subject: &str, body: &serde_json::Value) -> Result<(), String> {
        let bytes = serde_json::to_vec(body).map_err(|e| e.to_string())?;
        let event_id = body["event_id"]
            .as_str()
            .ok_or_else(|| "audit event_id is required".to_owned())?;
        let mut headers = async_nats::HeaderMap::new();
        headers.insert("Nats-Msg-Id", event_id);
        let acknowledgment = self
            .jetstream
            .publish_with_headers(subject.to_owned(), headers, bytes.into())
            .await
            .map_err(|e| e.to_string())?;
        acknowledgment.await.map_err(|e| e.to_string())?;
        Ok(())
    }
}

const MAX_AUDIT_DISPATCH_ATTEMPTS: i32 = 20;

/// Transactional `PostgreSQL` outbox for Session Core audit events. A fresh NATS
/// connection is established on demand after startup/connectivity failures;
/// once connected, async-nats handles reconnects for subsequent dispatches.
pub struct AuditOutbox {
    pool: PgPool,
    nats_url: String,
    publisher: Mutex<Option<NatsAuditPublisher>>,
}

impl AuditOutbox {
    #[must_use]
    pub fn new(pool: PgPool, nats_url: String) -> Arc<Self> {
        Arc::new(Self {
            pool,
            nats_url,
            publisher: Mutex::new(None),
        })
    }

    pub fn start(self: &Arc<Self>) {
        let outbox = Arc::clone(self);
        tokio::spawn(async move {
            let mut maintenance_tick = 0_u32;
            loop {
                for _ in 0..50 {
                    match outbox.dispatch_one().await {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(error) => {
                            tracing::warn!(%error, "session-core audit outbox dispatch deferred");
                            break;
                        }
                    }
                }
                if let Err(error) = outbox.record_status_metrics().await {
                    tracing::warn!(%error, "session-core audit outbox status unavailable");
                }
                maintenance_tick = maintenance_tick.wrapping_add(1);
                if maintenance_tick.is_multiple_of(60) {
                    if let Err(error) = outbox.purge_published_payloads().await {
                        tracing::warn!(%error, "session-core published audit cleanup deferred");
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }

    async fn record_status_metrics(&self) -> Result<(), String> {
        let (pending, terminal, oldest_pending_seconds) = sqlx::query_as::<_, (i64, i64, f64)>(
            "SELECT
                    count(*) FILTER (WHERE published_at IS NULL AND terminal_at IS NULL),
                    count(*) FILTER (WHERE terminal_at IS NOT NULL),
                    COALESCE(EXTRACT(EPOCH FROM now() - min(created_at)
                        FILTER (WHERE published_at IS NULL AND terminal_at IS NULL)), 0)::float8
                 FROM session_audit_outbox",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        metrics::gauge!("mp_session_audit_outbox_pending").set(pending as f64);
        metrics::gauge!("mp_session_audit_outbox_terminal").set(terminal as f64);
        metrics::gauge!("mp_session_audit_outbox_oldest_pending_seconds")
            .set(oldest_pending_seconds.max(0.0));
        // Terminal rows per subject class. The aggregate gauge above says THAT
        // rows are stuck; this one says which kind, so an alert can name the
        // audit trail that has stopped instead of just a number.
        let mut terminal_totals: HashMap<&'static str, i64> = SUBJECT_CLASSES
            .iter()
            .map(|class| (*class, 0_i64))
            .collect();
        if terminal > 0 {
            let by_subject = sqlx::query_as::<_, (String, i64)>(
                "SELECT CASE WHEN subject LIKE 'mp.v1.run.%.event'
                             THEN 'mp.v1.run.<run>.event' ELSE subject END,
                        count(*)
                 FROM session_audit_outbox
                 WHERE terminal_at IS NOT NULL AND published_at IS NULL
                 GROUP BY 1",
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|error| error.to_string())?;
            for (subject, count) in by_subject {
                *terminal_totals
                    .entry(outbox_subject_class(&subject))
                    .or_default() += count;
            }
        }
        for (class, count) in terminal_totals {
            metrics::gauge!("mp_session_audit_outbox_terminal_by_subject", "subject_class" => class)
                .set(count as f64);
        }
        let (reserved, oldest_reserved_seconds) = sqlx::query_as::<_, (i64, f64)>(
            "SELECT count(*),
                    COALESCE(EXTRACT(EPOCH FROM now() - min(reserved_at)), 0)::float8
             FROM session_tool_audit_intents
             WHERE status = 'reserved'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        metrics::gauge!("mp_session_tool_audit_reservations_pending").set(reserved as f64);
        metrics::gauge!("mp_session_tool_audit_oldest_reservation_seconds")
            .set(oldest_reserved_seconds.max(0.0));
        Ok(())
    }

    async fn purge_published_payloads(&self) -> Result<(), String> {
        sqlx::query(
            "DELETE FROM session_audit_outbox
             WHERE event_id IN (
                 SELECT event_id FROM session_audit_outbox
                 WHERE published_at < now() - interval '7 days'
                 ORDER BY published_at
                 LIMIT 500
             )",
        )
        .execute(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Claim and deliver at most one event. A successful return only marks the
    /// row complete after `JetStream`'s `PubAck` is received.
    pub async fn dispatch_one(&self) -> Result<bool, String> {
        let row = sqlx::query_as::<_, (String, String, serde_json::Value, i32)>(
            "SELECT event_id, subject, payload, attempts FROM claim_session_audit_event()",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        let Some((event_id, subject, payload, attempts)) = row else {
            return Ok(false);
        };

        let publish_result = self.publish_claimed(&event_id, &subject, &payload).await;
        if publish_result.is_ok() {
            let completed = sqlx::query(
                "UPDATE session_audit_outbox
                 SET published_at = now(), processing_at = NULL, last_error = NULL
                 WHERE event_id = $1 AND attempts = $2 AND published_at IS NULL",
            )
            .bind(&event_id)
            .bind(attempts)
            .execute(&self.pool)
            .await
            .map_err(|error| error.to_string())?;
            if completed.rows_affected() != 1 {
                return Err(format!("audit completion lease lost for {event_id}"));
            }
            return Ok(true);
        }

        let error = publish_result.expect_err("checked above");
        let plan = plan_failure(&subject, &error, attempts);
        let terminal = plan.terminal;
        let next_attempt = chrono::Utc::now()
            + chrono::Duration::from_std(audit_retry_delay(attempts))
                .map_err(|duration_error| duration_error.to_string())?;
        let failed = sqlx::query(
            "UPDATE session_audit_outbox
             SET processing_at = NULL,
                 next_attempt_at = $3,
                 last_error = left($4, 2000),
                 terminal_at = CASE WHEN $5 THEN now() ELSE terminal_at END
             WHERE event_id = $1 AND attempts = $2 AND published_at IS NULL",
        )
        .bind(&event_id)
        .bind(attempts)
        .bind(next_attempt)
        .bind(&error)
        .bind(terminal)
        .execute(&self.pool)
        .await
        .map_err(|update_error| {
            format!("publish {event_id}: {error}; record retry: {update_error}")
        })?;
        if failed.rows_affected() != 1 {
            return Err(format!("audit failure lease lost for {event_id}"));
        }
        plan.emit();
        Err(format!("publish audit event {event_id}: {error}"))
    }

    async fn publish_claimed(
        &self,
        event_id: &str,
        subject: &str,
        payload: &serde_json::Value,
    ) -> Result<(), String> {
        let route = classify_outbox_subject(subject).ok_or_else(|| {
            format!("outbox subject {subject:?} is outside session-core authority")
        })?;
        check_outbox_authority(route, event_id, payload)?;

        let mut publisher = self.publisher.lock().await;
        if publisher.is_none() {
            *publisher = Some(
                NatsAuditPublisher::connect(&self.nats_url)
                    .await
                    .map_err(|error| error.to_string())?,
            );
        }
        let publisher = publisher.as_ref().expect("publisher initialized");
        match route {
            OutboxRoute::Audit => publish_tool_action(publisher, payload).await,
            OutboxRoute::RunEvent { .. } => publisher.publish_audit(subject, payload).await,
        }
    }
}

fn audit_retry_delay(attempt: i32) -> Duration {
    let attempt = attempt.max(1);
    let seconds = i64::from(attempt)
        .saturating_mul(i64::from(attempt))
        .min(300);
    Duration::from_secs(u64::try_from(seconds).unwrap_or(300))
}

/// Every value [`outbox_subject_class`] can return. The per-class gauge is set
/// for all of them on every tick, so a class with nothing stuck reads 0 instead
/// of being absent (an absent series cannot be told from a stopped reporter).
const SUBJECT_CLASSES: [&str; 5] = [
    "audit_tool_action",
    "audit_tool_action_reserved",
    "audit_other",
    "run_event",
    "unclassified",
];

/// Reasons that say the broker topology or the publisher's grant is wrong, not
/// that one row is bad: the same row would fail again until an operator acts.
const TOPOLOGY_REASONS: [&str; 2] = ["stream_not_found", "permission_denied"];

/// Bounded, content-free label for an outbox row's subject. It is never a run
/// id, an org id or any other per-tenant token, so it is safe as a metric label
/// and in a log line.
#[must_use]
pub fn outbox_subject_class(subject: &str) -> &'static str {
    match classify_outbox_subject(subject) {
        Some(OutboxRoute::RunEvent { .. }) => "run_event",
        Some(OutboxRoute::Audit) => match subject.strip_prefix(AUDIT_SUBJECT_PREFIX) {
            Some(EVENT_TOOL_ACTION) => "audit_tool_action",
            Some(EVENT_TOOL_ACTION_RESERVED) => "audit_tool_action_reserved",
            _ => "audit_other",
        },
        None => "unclassified",
    }
}

/// Why a delivery attempt failed, as a bounded label. The transport's error text
/// is read here and never forwarded as a label: it can carry subjects or ids.
#[must_use]
pub fn classify_publish_failure(error: &str) -> &'static str {
    let error = error.to_ascii_lowercase();
    if error.contains("no stream found") || error.contains("stream not found") {
        "stream_not_found"
    } else if error.contains("permission")
        || error.contains("authorization")
        || error.contains("authentication")
    {
        "permission_denied"
    } else if error.contains("subject token")
        || error.contains("authority")
        || error.contains("payload")
        || error.contains("envelope")
    {
        "refused_before_publish"
    } else if error.contains("timed out") || error.contains("timeout") {
        "timeout"
    } else if error.contains("connect")
        || error.contains("connection")
        || error.contains("broken pipe")
    {
        "transport"
    } else {
        "other"
    }
}

/// What one failed delivery attempt means for its row, and how loudly to say it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailurePlan {
    pub subject_class: &'static str,
    pub reason: &'static str,
    pub attempts: i32,
    /// The row has used its whole attempt budget and will not be claimed again
    /// until an operator requeues it (`requeue_session_audit_terminal`).
    pub terminal: bool,
}

#[must_use]
pub fn plan_failure(subject: &str, error: &str, attempts: i32) -> FailurePlan {
    FailurePlan {
        subject_class: outbox_subject_class(subject),
        reason: classify_publish_failure(error),
        attempts,
        terminal: attempts >= MAX_AUDIT_DISPATCH_ATTEMPTS,
    }
}

/// How often one kind of failure line may be logged. A backlog of thousands of
/// rows hits the same cause at once; one line per interval says it, with how
/// many rows it stands for.
const SIGNAL_LOG_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Default)]
struct LogThrottle {
    last: Option<Instant>,
    suppressed: u64,
}

impl LogThrottle {
    /// `Some(rows suppressed since the last line)` when a line may be logged at
    /// `now`; `None` when it falls inside the interval of the previous one.
    fn admit(&mut self, now: Instant) -> Option<u64> {
        match self.last {
            Some(last) if now.saturating_duration_since(last) < SIGNAL_LOG_INTERVAL => {
                self.suppressed += 1;
                None
            }
            _ => {
                self.last = Some(now);
                Some(std::mem::take(&mut self.suppressed))
            }
        }
    }
}

/// One throttle per (kind of line, subject class, reason).
#[derive(Debug, Default)]
pub struct SignalThrottles {
    inner: std::sync::Mutex<HashMap<(&'static str, &'static str, &'static str), LogThrottle>>,
}

impl SignalThrottles {
    fn admit(
        &self,
        kind: &'static str,
        subject_class: &'static str,
        reason: &'static str,
        now: Instant,
    ) -> Option<u64> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry((kind, subject_class, reason))
            .or_default()
            .admit(now)
    }
}

static SIGNAL_THROTTLES: std::sync::LazyLock<SignalThrottles> =
    std::sync::LazyLock::new(SignalThrottles::default);

impl FailurePlan {
    /// Say what happened: a counter for every failed attempt, a second one when
    /// the row turns terminal, and an ERROR line (throttled, content-free: the
    /// subject class, the reason and the attempt count only) for a terminal row
    /// and for the first failure of a topology problem.
    pub fn emit(&self) {
        self.emit_at(&SIGNAL_THROTTLES, Instant::now());
    }

    fn emit_at(&self, throttles: &SignalThrottles, now: Instant) {
        metrics::counter!(
            "mp_session_audit_outbox_publish_failures_total",
            "subject_class" => self.subject_class,
            "reason" => self.reason
        )
        .increment(1);
        if self.terminal {
            metrics::counter!(
                "mp_session_audit_outbox_terminal_total",
                "subject_class" => self.subject_class,
                "reason" => self.reason
            )
            .increment(1);
            if let Some(suppressed) =
                throttles.admit("terminal", self.subject_class, self.reason, now)
            {
                tracing::error!(
                    subject_class = self.subject_class,
                    reason = self.reason,
                    attempts = self.attempts,
                    rows_since_last_line = suppressed + 1,
                    "session-core outbox rows used every delivery attempt and are terminal; they are not delivered until requeued with requeue_session_audit_terminal()"
                );
            }
        } else if self.attempts <= 1 && TOPOLOGY_REASONS.contains(&self.reason) {
            if let Some(suppressed) =
                throttles.admit("first_failure", self.subject_class, self.reason, now)
            {
                tracing::error!(
                    subject_class = self.subject_class,
                    reason = self.reason,
                    rows_since_last_line = suppressed + 1,
                    "session-core outbox cannot deliver: the broker has no stream for this subject or refuses the publish; rows keep retrying and will turn terminal"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn durable_audit_outbox_migration_and_transaction_contract() {
        let migration = include_str!("../migrations/0012_audit_outbox.sql");
        for required in [
            "CREATE TABLE IF NOT EXISTS session_audit_outbox",
            "event_id TEXT PRIMARY KEY",
            "FOR UPDATE SKIP LOCKED",
            "published_at",
            "terminal_at",
        ] {
            assert!(migration.contains(required), "migration missing {required}");
        }

        let grpc_source = include_str!("grpc.rs");
        let enqueue = grpc_source
            .find("enqueue_tool_action_audit(&mut tx")
            .expect("tool audit intent must be enqueued in the step transaction");
        let commit = grpc_source[enqueue..]
            .find("tx.commit()")
            .map(|offset| enqueue + offset)
            .expect("step transaction must commit");
        assert!(
            enqueue < commit,
            "audit intent must be stored before step commit"
        );
    }

    #[test]
    fn inline_tool_reservation_is_durable_and_non_terminal() {
        let migration = include_str!("../migrations/0013_tool_audit_intents.sql");
        for required in [
            "CREATE TABLE IF NOT EXISTS session_tool_audit_intents",
            "UNIQUE (run_id, action_id)",
            "CHECK (status IN ('reserved', 'completed', 'failed'))",
        ] {
            assert!(migration.contains(required), "migration missing {required}");
        }

        let grpc_source = include_str!("grpc.rs");
        assert!(grpc_source.contains("reserve_tool_action_inner"));
        assert!(grpc_source.contains("finalize_tool_action_inner"));
        assert!(!grpc_source.contains("record_run_terminal(&mut tx, &tool_action"));
        assert!(grpc_source.contains("if req.terminal"));
    }

    /// The drainer publishes each row's OWN subject, so the route classifier is
    /// the whole publish-authority boundary. Both families must be admitted —
    /// before this, the run-event family was refused outright and every
    /// `RUN_COMPLETED` row would have burned to `terminal_at` after 20 attempts.
    #[test]
    fn outbox_routes_admit_audit_and_run_event_families() {
        assert_eq!(
            classify_outbox_subject(SUBJECT_MODEL_TOOL_ACTION),
            Some(OutboxRoute::Audit)
        );
        assert_eq!(
            classify_outbox_subject("mp.v1.run.run-abc.event"),
            Some(OutboxRoute::RunEvent { run_id: "run-abc" })
        );
    }

    #[test]
    fn outbox_routes_refuse_subjects_outside_session_core_authority() {
        for subject in [
            "verevon.audit.v2.model.model-gateway.tool_action",
            "mp.v1.usage.org-1",
            "mp.v1.run.run-abc.command",
            "mp.v1.run..event",
            // A dotted run id would widen a single-token wildcard's scope.
            "mp.v1.run.run.abc.event",
            "",
        ] {
            assert_eq!(
                classify_outbox_subject(subject),
                None,
                "{subject:?} must be refused"
            );
        }
    }

    #[test]
    fn run_event_authority_requires_matching_row_producer_and_run() {
        let envelope = serde_json::json!({
            "event_id": "evt-1",
            "event_type": "RUN_COMPLETED",
            "producer": "session-core",
            "org_id": "org-1",
            "resource_ref": "run:run-abc",
        });
        let route = OutboxRoute::RunEvent { run_id: "run-abc" };
        assert!(check_outbox_authority(route, "evt-1", &envelope).is_ok());
        // Row identity mismatch.
        assert!(check_outbox_authority(route, "evt-other", &envelope).is_err());
        // Subject names a different run than the envelope.
        assert!(check_outbox_authority(
            OutboxRoute::RunEvent { run_id: "run-xyz" },
            "evt-1",
            &envelope
        )
        .is_err());
    }

    #[test]
    fn run_event_authority_rejects_foreign_producer_and_empty_org() {
        let route = OutboxRoute::RunEvent { run_id: "run-abc" };
        let foreign = serde_json::json!({
            "event_id": "evt-1",
            "event_type": "RUN_COMPLETED",
            "producer": "model-gateway",
            "org_id": "org-1",
            "resource_ref": "run:run-abc",
        });
        assert!(check_outbox_authority(route, "evt-1", &foreign).is_err());
        let orgless = serde_json::json!({
            "event_id": "evt-1",
            "event_type": "RUN_COMPLETED",
            "producer": "session-core",
            "org_id": "",
            "resource_ref": "run:run-abc",
        });
        assert!(check_outbox_authority(route, "evt-1", &orgless).is_err());
    }

    /// An audit row still needs the flat body's `plane`, which a run envelope
    /// does not carry — the two shapes must not be interchangeable.
    #[test]
    fn audit_authority_still_requires_the_flat_model_plane_body() {
        let envelope = serde_json::json!({
            "event_id": "evt-1",
            "producer": "session-core",
            "resource_ref": "run:run-abc",
        });
        assert!(check_outbox_authority(OutboxRoute::Audit, "evt-1", &envelope).is_err());
        let audit = serde_json::json!({
            "event_id": "evt-1",
            "producer": "session-core",
            "plane": "model",
        });
        assert!(check_outbox_authority(OutboxRoute::Audit, "evt-1", &audit).is_ok());
    }

    #[test]
    fn audit_outbox_retry_delay_is_bounded() {
        assert_eq!(
            super::audit_retry_delay(0),
            std::time::Duration::from_secs(1)
        );
        assert_eq!(
            super::audit_retry_delay(100),
            std::time::Duration::from_secs(300)
        );
    }

    #[test]
    fn audit_outbox_exposes_lag_terminal_state_and_bounded_retention() {
        let source = include_str!("audit_publisher.rs");
        for required in [
            "mp_session_audit_outbox_pending",
            "mp_session_audit_outbox_terminal",
            "mp_session_audit_outbox_oldest_pending_seconds",
            "mp_session_tool_audit_reservations_pending",
            "mp_session_tool_audit_oldest_reservation_seconds",
            "published_at < now() - interval '7 days'",
        ] {
            assert!(
                source.contains(required),
                "outbox source missing {required}"
            );
        }
    }

    /// A row stored before the reserved phase had its own subject carries
    /// `...tool_action` with a payload `event` of `tool_action_reserved`. It must
    /// be published on the subject its event names: audit-core refuses a message
    /// whose subject's last token differs from the payload's event and
    /// dead-letters it. Live, 2,477 of the 4,992 stuck rows were of this kind.
    #[test]
    fn audit_wire_subject_follows_the_payload_event_not_the_stored_subject() {
        let legacy_reserved = serde_json::json!({ "event": "tool_action_reserved" });
        assert_eq!(
            resolve_wire_subject(
                OutboxRoute::Audit,
                SUBJECT_MODEL_TOOL_ACTION,
                &legacy_reserved
            )
            .expect("reserved event has a subject"),
            "verevon.audit.v2.model.session-core.tool_action_reserved"
        );
        let finished = serde_json::json!({ "event": "tool_action" });
        assert_eq!(
            resolve_wire_subject(OutboxRoute::Audit, SUBJECT_MODEL_TOOL_ACTION, &finished)
                .expect("final event has a subject"),
            SUBJECT_MODEL_TOOL_ACTION
        );
    }

    #[test]
    fn audit_wire_subject_refuses_an_event_that_would_widen_the_subject() {
        let too_long = "x".repeat(65);
        for event in [
            "",
            "tool.action",
            "tool_action.*",
            "tool_action>",
            "Tool_Action",
            "tool action",
            too_long.as_str(),
        ] {
            let payload = serde_json::json!({ "event": event });
            assert!(
                resolve_wire_subject(OutboxRoute::Audit, SUBJECT_MODEL_TOOL_ACTION, &payload)
                    .is_err(),
                "event {event:?} must not become part of a subject"
            );
        }
        assert!(resolve_wire_subject(
            OutboxRoute::Audit,
            SUBJECT_MODEL_TOOL_ACTION,
            &serde_json::json!({})
        )
        .is_err());
    }

    #[test]
    fn run_event_rows_keep_their_stored_subject() {
        let envelope = serde_json::json!({ "event_type": "RUN_COMPLETED" });
        assert_eq!(
            resolve_wire_subject(
                OutboxRoute::RunEvent { run_id: "run-abc" },
                "mp.v1.run.run-abc.event",
                &envelope
            )
            .expect("run event subject"),
            "mp.v1.run.run-abc.event"
        );
    }

    /// Both sides of the Session Core -> audit-core wire contract. The fixtures
    /// are read by this test and, from the other side, by audit-core's
    /// `subscriber` tests (`session_core_wire_fixtures_test.go`), which decode
    /// them with audit-core's own `DecodeAudit` and authority rule. If either
    /// side changes the body or the subject, one of the two fails.
    #[test]
    fn audit_bodies_and_subjects_match_the_committed_audit_core_wire_fixtures() {
        let at = chrono::DateTime::parse_from_rfc3339("2026-10-03T12:00:00Z")
            .expect("fixed instant")
            .with_timezone(&chrono::Utc);
        let detail = ToolActionDetail {
            data_category: "customer_private".to_owned(),
            zdr: false,
            tool: Some("knowledge_search".to_owned()),
        };
        for (name, raw, phase, outcome) in [
            (
                "final",
                include_str!("../tests/fixtures/audit-core-wire/tool_action_final.json"),
                "final",
                "completed",
            ),
            (
                "reserved",
                include_str!("../tests/fixtures/audit-core-wire/tool_action_reserved.json"),
                "reserved",
                "reserved",
            ),
        ] {
            let body = build_tool_action_body_for_phase(
                "org_fixture",
                "user_fixture",
                "run_fixture",
                "act_fixture",
                outcome,
                "knowledge_search",
                &detail,
                phase,
                at,
            );
            let subject =
                resolve_wire_subject(OutboxRoute::Audit, SUBJECT_MODEL_TOOL_ACTION, &body)
                    .expect("wire subject");
            // `UPDATE_AUDIT_CORE_FIXTURES=1 cargo test ... audit_core_wire_fixtures`
            // rewrites the fixtures from the real builder; review the diff, then
            // run audit-core's `subscriber` tests against the new files.
            if std::env::var_os("UPDATE_AUDIT_CORE_FIXTURES").is_some() {
                let path = format!(
                    "{}/tests/fixtures/audit-core-wire/tool_action_{name}.json",
                    env!("CARGO_MANIFEST_DIR")
                );
                let rendered = serde_json::to_string_pretty(
                    &serde_json::json!({ "subject": subject, "body": body }),
                )
                .expect("render fixture");
                std::fs::write(path, rendered + "\n").expect("write fixture");
                continue;
            }
            let fixture: serde_json::Value =
                serde_json::from_str(raw).expect("fixture is valid JSON");
            assert_eq!(fixture["subject"], subject, "{name}: wire subject");
            assert_eq!(fixture["body"], body, "{name}: wire body");
        }
    }

    #[test]
    fn outbox_subject_classes_are_bounded_and_carry_no_tenant_token() {
        assert_eq!(
            outbox_subject_class("verevon.audit.v2.model.session-core.tool_action"),
            "audit_tool_action"
        );
        assert_eq!(
            outbox_subject_class("verevon.audit.v2.model.session-core.tool_action_reserved"),
            "audit_tool_action_reserved"
        );
        assert_eq!(
            outbox_subject_class("verevon.audit.v2.model.session-core.something_new"),
            "audit_other"
        );
        assert_eq!(
            outbox_subject_class("mp.v1.run.run-secret-42.event"),
            "run_event"
        );
        assert_eq!(outbox_subject_class("mp.v1.usage.org-1"), "unclassified");
        assert_eq!(outbox_subject_class(""), "unclassified");
        // The gauge loop sets every class in SUBJECT_CLASSES, so a class the
        // classifier can return but the list lacks would never be reported.
        for subject in [
            "verevon.audit.v2.model.session-core.tool_action",
            "verevon.audit.v2.model.session-core.tool_action_reserved",
            "verevon.audit.v2.model.session-core.x",
            "mp.v1.run.r.event",
            "nonsense",
        ] {
            assert!(SUBJECT_CLASSES.contains(&outbox_subject_class(subject)));
        }
    }

    #[test]
    fn publish_failures_are_classified_into_bounded_reasons() {
        for (error, reason) in [
            ("no stream found for given subject", "stream_not_found"),
            (
                "Permissions Violation for Publish to \"verevon.audit.v2.model.x\"",
                "permission_denied",
            ),
            (
                "outbox subject \"x\" is outside session-core authority",
                "refused_before_publish",
            ),
            (
                "audit payload event is not a single lowercase subject token",
                "refused_before_publish",
            ),
            ("timed out", "timeout"),
            ("connection refused", "transport"),
            ("something nobody foresaw", "other"),
        ] {
            assert_eq!(classify_publish_failure(error), reason, "{error}");
        }
    }

    #[test]
    fn a_failure_is_terminal_exactly_at_the_attempt_budget() {
        let error = "no stream found for given subject";
        assert!(!plan_failure(SUBJECT_MODEL_TOOL_ACTION, error, 1).terminal);
        assert!(
            !plan_failure(
                SUBJECT_MODEL_TOOL_ACTION,
                error,
                MAX_AUDIT_DISPATCH_ATTEMPTS - 1
            )
            .terminal
        );
        let last = plan_failure(
            SUBJECT_MODEL_TOOL_ACTION,
            error,
            MAX_AUDIT_DISPATCH_ATTEMPTS,
        );
        assert!(last.terminal);
        assert_eq!(
            (last.subject_class, last.reason),
            ("audit_tool_action", "stream_not_found")
        );
    }

    #[test]
    fn a_log_line_repeats_at_most_once_per_interval_and_counts_what_it_hid() {
        let mut throttle = LogThrottle::default();
        let start = Instant::now();
        assert_eq!(throttle.admit(start), Some(0));
        assert_eq!(throttle.admit(start + Duration::from_secs(1)), None);
        assert_eq!(throttle.admit(start + Duration::from_secs(59)), None);
        assert_eq!(throttle.admit(start + SIGNAL_LOG_INTERVAL), Some(2));
    }

    /// Counter recorder for `metrics::with_local_recorder`: key rendered as
    /// `name{label=value,...}` (labels sorted) -> accumulated value.
    #[derive(Default, Clone)]
    struct CounterRecorder {
        counters: Arc<Mutex<HashMap<String, u64>>>,
    }

    struct RecordedCounter {
        key: String,
        counters: Arc<Mutex<HashMap<String, u64>>>,
    }

    impl metrics::CounterFn for RecordedCounter {
        fn increment(&self, value: u64) {
            *self
                .counters
                .lock()
                .unwrap()
                .entry(self.key.clone())
                .or_default() += value;
        }
        fn absolute(&self, value: u64) {
            self.counters
                .lock()
                .unwrap()
                .insert(self.key.clone(), value);
        }
    }

    impl metrics::Recorder for CounterRecorder {
        fn describe_counter(
            &self,
            _: metrics::KeyName,
            _: Option<metrics::Unit>,
            _: metrics::SharedString,
        ) {
        }
        fn describe_gauge(
            &self,
            _: metrics::KeyName,
            _: Option<metrics::Unit>,
            _: metrics::SharedString,
        ) {
        }
        fn describe_histogram(
            &self,
            _: metrics::KeyName,
            _: Option<metrics::Unit>,
            _: metrics::SharedString,
        ) {
        }
        fn register_counter(
            &self,
            key: &metrics::Key,
            _: &metrics::Metadata<'_>,
        ) -> metrics::Counter {
            let mut labels: Vec<String> = key
                .labels()
                .map(|label| format!("{}={}", label.key(), label.value()))
                .collect();
            labels.sort();
            metrics::Counter::from_arc(Arc::new(RecordedCounter {
                key: format!("{}{{{}}}", key.name(), labels.join(",")),
                counters: Arc::clone(&self.counters),
            }))
        }
        fn register_gauge(&self, _: &metrics::Key, _: &metrics::Metadata<'_>) -> metrics::Gauge {
            metrics::Gauge::noop()
        }
        fn register_histogram(
            &self,
            _: &metrics::Key,
            _: &metrics::Metadata<'_>,
        ) -> metrics::Histogram {
            metrics::Histogram::noop()
        }
    }

    /// Minimal `tracing` subscriber keeping every event as `LEVEL field=value ...`.
    #[derive(Default, Clone)]
    struct CapturedLines(Arc<Mutex<Vec<String>>>);

    struct LineVisitor<'a>(&'a mut String);

    impl tracing::field::Visit for LineVisitor<'_> {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            use std::fmt::Write;
            let _ = write!(self.0, "{}={value:?} ", field.name());
        }
    }

    impl tracing::Subscriber for CapturedLines {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            let mut line = format!("{} ", event.metadata().level());
            event.record(&mut LineVisitor(&mut line));
            self.0.lock().unwrap().push(line);
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    /// The failure this change exists for: rows that ran out of attempts must
    /// be counted per subject class and logged, without any tenant content, and
    /// a burst of identical rows must produce one line (carrying their count),
    /// not thousands.
    #[test]
    fn terminal_failures_are_counted_per_subject_and_logged_without_content() {
        let recorder = CounterRecorder::default();
        let lines = CapturedLines::default();
        let throttles = SignalThrottles::default();
        let start = Instant::now();
        let plan = plan_failure(
            "mp.v1.run.run-secret-42.event",
            "no stream found for given subject",
            MAX_AUDIT_DISPATCH_ATTEMPTS,
        );
        assert!(plan.terminal);
        tracing::subscriber::with_default(lines.clone(), || {
            metrics::with_local_recorder(&recorder, || {
                for millis in 0..3 {
                    plan.emit_at(&throttles, start + Duration::from_millis(millis));
                }
            });
        });
        let counters = recorder.counters.lock().unwrap().clone();
        assert_eq!(
            counters
                .get("mp_session_audit_outbox_terminal_total{reason=stream_not_found,subject_class=run_event}")
                .copied(),
            Some(3),
            "{counters:?}"
        );
        assert_eq!(
            counters
                .get("mp_session_audit_outbox_publish_failures_total{reason=stream_not_found,subject_class=run_event}")
                .copied(),
            Some(3),
            "{counters:?}"
        );
        let lines = lines.0.lock().unwrap().clone();
        assert_eq!(lines.len(), 1, "one line for the burst: {lines:?}");
        assert!(lines[0].starts_with("ERROR "), "{}", lines[0]);
        assert!(lines[0].contains("run_event") && lines[0].contains("stream_not_found"));
        for content in ["run-secret-42", "mp.v1.run", "org_", "tool:session-core"] {
            assert!(
                !lines[0].contains(content),
                "the line must not carry {content:?}: {}",
                lines[0]
            );
        }
    }

    #[test]
    fn a_topology_failure_is_loud_on_its_first_attempt_but_an_ordinary_one_is_not() {
        let lines = CapturedLines::default();
        let recorder = CounterRecorder::default();
        let throttles = SignalThrottles::default();
        let now = Instant::now();
        tracing::subscriber::with_default(lines.clone(), || {
            metrics::with_local_recorder(&recorder, || {
                plan_failure(
                    SUBJECT_MODEL_TOOL_ACTION,
                    "no stream found for given subject",
                    1,
                )
                .emit_at(&throttles, now);
                plan_failure(SUBJECT_MODEL_TOOL_ACTION, "something nobody foresaw", 1)
                    .emit_at(&throttles, now);
                // Not the first attempt and not terminal: counted, not logged.
                plan_failure(
                    SUBJECT_MODEL_TOOL_ACTION,
                    "no stream found for given subject",
                    7,
                )
                .emit_at(&throttles, now);
            });
        });
        let lines = lines.0.lock().unwrap().clone();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].starts_with("ERROR "));
        assert!(lines[0].contains("audit_tool_action") && lines[0].contains("stream_not_found"));
        let counters = recorder.counters.lock().unwrap().clone();
        assert_eq!(
            counters
                .get("mp_session_audit_outbox_publish_failures_total{reason=stream_not_found,subject_class=audit_tool_action}")
                .copied(),
            Some(2)
        );
        assert!(!counters
            .keys()
            .any(|key| key.starts_with("mp_session_audit_outbox_terminal_total")));
    }

    /// The recovery path for a terminal row is part of the schema, not a
    /// one-off statement: counted, guarded and not executable by PUBLIC.
    #[test]
    fn requeue_migration_gives_terminal_rows_a_documented_way_back() {
        let migration = include_str!("../migrations/0043_audit_outbox_requeue.sql");
        for required in [
            "ADD COLUMN IF NOT EXISTS requeue_count",
            "ADD COLUMN IF NOT EXISTS requeued_at",
            "CREATE OR REPLACE FUNCTION requeue_session_audit_terminal",
            "terminal_at IS NOT NULL",
            "published_at IS NULL",
            "FOR UPDATE SKIP LOCKED",
            "attempts = 0",
            "terminal_at = NULL",
            "REVOKE ALL ON FUNCTION requeue_session_audit_terminal",
        ] {
            assert!(
                migration.contains(required),
                "requeue migration missing {required}"
            );
        }
    }

    /// The DB-backed tests below share one outbox table, and the drainer claims
    /// ANY claimable row, so they must not overlap. (Run them against a
    /// disposable Postgres: they assume nothing else is writing to the table.)
    static OUTBOX_DB_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// Real Postgres: the requeue function must touch only terminal, unpublished
    /// rows of the named subject, honour the limit, keep the failure evidence
    /// until a publish succeeds, and be safe to run twice.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn requeue_function_returns_only_terminal_unpublished_rows_of_the_subject() {
        let _serial = OUTBOX_DB_LOCK.lock().await;
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        let pool = sqlx::PgPool::connect(&url).await.expect("connect postgres");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let suffix = mp_ids::new_ulid().to_lowercase();
        let target = format!("{AUDIT_SUBJECT_PREFIX}requeue_probe_{suffix}");
        let other = format!("{AUDIT_SUBJECT_PREFIX}requeue_other_{suffix}");
        let insert = |id: String, subject: String, attempts: i32, state: &'static str| {
            let pool = pool.clone();
            async move {
                sqlx::query(
                    "INSERT INTO session_audit_outbox
                        (event_id, subject, payload, attempts, terminal_at, published_at, last_error)
                     VALUES ($1, $2, '{}'::jsonb, $3,
                             CASE WHEN $4 = 'terminal' THEN now() END,
                             CASE WHEN $4 = 'published' THEN now() END,
                             CASE WHEN $4 = 'terminal' THEN 'no stream found for given subject' END)",
                )
                .bind(id)
                .bind(subject)
                .bind(attempts)
                .bind(state)
                .execute(&pool)
                .await
                .expect("seed outbox row");
            }
        };
        for index in 0..3 {
            insert(
                format!("probe-{suffix}-t{index}"),
                target.clone(),
                20,
                "terminal",
            )
            .await;
        }
        insert(
            format!("probe-{suffix}-pub"),
            target.clone(),
            1,
            "published",
        )
        .await;
        insert(format!("probe-{suffix}-pend"), target.clone(), 2, "pending").await;
        insert(format!("probe-{suffix}-oth"), other.clone(), 20, "terminal").await;

        let requeued: i32 = sqlx::query_scalar("SELECT requeue_session_audit_terminal($1, 2)")
            .bind(&target)
            .fetch_one(&pool)
            .await
            .expect("requeue with a limit");
        assert_eq!(requeued, 2, "the limit bounds one call");
        let requeued: i32 = sqlx::query_scalar("SELECT requeue_session_audit_terminal($1, 10)")
            .bind(&target)
            .fetch_one(&pool)
            .await
            .expect("requeue the rest");
        assert_eq!(requeued, 1, "only the one terminal row still left");
        let requeued: i32 = sqlx::query_scalar("SELECT requeue_session_audit_terminal($1, 10)")
            .bind(&target)
            .fetch_one(&pool)
            .await
            .expect("requeue again");
        assert_eq!(requeued, 0, "nothing terminal remains: safe to run twice");

        let claimable: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM session_audit_outbox
             WHERE subject = $1 AND attempts = 0 AND terminal_at IS NULL
               AND published_at IS NULL AND processing_at IS NULL
               AND next_attempt_at <= now() AND requeue_count = 1
               AND requeued_at IS NOT NULL
               AND last_error = 'no stream found for given subject'",
        )
        .bind(&target)
        .fetch_one(&pool)
        .await
        .expect("count claimable");
        assert_eq!(claimable, 3, "all three terminal rows are claimable again");
        let untouched: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM session_audit_outbox
             WHERE event_id IN ($1, $2, $3)
               AND requeue_count = 0 AND requeued_at IS NULL",
        )
        .bind(format!("probe-{suffix}-pub"))
        .bind(format!("probe-{suffix}-pend"))
        .bind(format!("probe-{suffix}-oth"))
        .fetch_one(&pool)
        .await
        .expect("count untouched");
        assert_eq!(untouched, 3, "published, pending and other-subject rows");
        let other_still_terminal: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM session_audit_outbox
             WHERE subject = $1 AND terminal_at IS NOT NULL AND attempts = 20",
        )
        .bind(&other)
        .fetch_one(&pool)
        .await
        .expect("count other subject");
        assert_eq!(other_still_terminal, 1);

        sqlx::query("DELETE FROM session_audit_outbox WHERE subject IN ($1, $2)")
            .bind(&target)
            .bind(&other)
            .execute(&pool)
            .await
            .expect("clean up probe rows");
    }

    /// The real drainer, a real broker and a real database in one run: every kind
    /// of row the outbox holds must leave through `AuditOutbox::dispatch_one` and
    /// be marked published, including a reservation stored under the old shared
    /// subject. Needs a Model Plane broker that `nats-provisioner` has
    /// provisioned (without its stream every publish fails with "no stream
    /// found for given subject", which is the defect this guards).
    ///
    /// ```text
    /// DATABASE_URL=postgres://...  NATS_URL=nats://127.0.0.1:4222 \
    /// NATS_USER=session-core-runtime NATS_PASSWORD=<32+ chars> \
    ///   cargo test -p session-core --bin session-core outbox_drain -- --ignored
    /// ```
    #[tokio::test]
    #[ignore = "requires DATABASE_URL (disposable Postgres) and NATS_URL/NATS_USER/NATS_PASSWORD of a provisioned Model Plane broker"]
    async fn outbox_drain_publishes_every_row_kind_through_a_provisioned_broker() {
        let _serial = OUTBOX_DB_LOCK.lock().await;
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        let nats_url = std::env::var("NATS_URL").expect("NATS_URL");
        let pool = sqlx::PgPool::connect(&url).await.expect("connect postgres");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let suffix = mp_ids::new_ulid().to_lowercase();
        let detail = ToolActionDetail {
            data_category: "customer_private".to_owned(),
            zdr: false,
            tool: Some("knowledge_search".to_owned()),
        };
        let at = chrono::Utc::now();
        let final_body = build_tool_action_body_for_phase(
            "org_probe",
            "user_probe",
            &format!("run_{suffix}"),
            "act_final",
            "completed",
            "knowledge_search",
            &detail,
            "final",
            at,
        );
        let reserved_body = build_tool_action_body_for_phase(
            "org_probe",
            "user_probe",
            &format!("run_{suffix}"),
            "act_reserved",
            "reserved",
            "knowledge_search",
            &detail,
            "reserved",
            at,
        );
        let legacy_body = build_tool_action_body_for_phase(
            "org_probe",
            "user_probe",
            &format!("run_{suffix}"),
            "act_legacy",
            "reserved",
            "knowledge_search",
            &detail,
            "reserved",
            at,
        );
        let run_id = format!("probe-run-{suffix}");
        let run_event_id = format!("probe-run-event-{suffix}");
        let run_body = serde_json::json!({
            "event_id": run_event_id,
            "event_type": "RUN_COMPLETED",
            "producer": "session-core",
            "org_id": "org_probe",
            "resource_ref": format!("run:{run_id}"),
        });
        let rows: Vec<(String, String, serde_json::Value)> = vec![
            (
                final_body["event_id"].as_str().unwrap().to_owned(),
                SUBJECT_MODEL_TOOL_ACTION.to_owned(),
                final_body,
            ),
            (
                reserved_body["event_id"].as_str().unwrap().to_owned(),
                format!("{AUDIT_SUBJECT_PREFIX}{EVENT_TOOL_ACTION_RESERVED}"),
                reserved_body,
            ),
            // Stored the way rows were before the reservation had its own
            // subject: `tool_action` subject, `tool_action_reserved` event.
            (
                legacy_body["event_id"].as_str().unwrap().to_owned(),
                SUBJECT_MODEL_TOOL_ACTION.to_owned(),
                legacy_body,
            ),
            (
                run_event_id.clone(),
                format!("mp.v1.run.{run_id}.event"),
                run_body,
            ),
        ];
        for (event_id, subject, body) in &rows {
            sqlx::query(
                "INSERT INTO session_audit_outbox (event_id, subject, payload) VALUES ($1, $2, $3)",
            )
            .bind(event_id)
            .bind(subject)
            .bind(body)
            .execute(&pool)
            .await
            .expect("seed outbox row");
        }

        let outbox = AuditOutbox::new(pool.clone(), nats_url);
        let mut dispatched = 0;
        while dispatched < 100 {
            match outbox.dispatch_one().await {
                Ok(true) => dispatched += 1,
                Ok(false) => break,
                Err(error) => panic!("dispatch failed: {error}"),
            }
        }
        assert!(dispatched >= rows.len(), "dispatched {dispatched} rows");

        for (event_id, subject, _) in &rows {
            let published: (bool, i32, Option<String>) = sqlx::query_as(
                "SELECT published_at IS NOT NULL, attempts, last_error
                 FROM session_audit_outbox WHERE event_id = $1",
            )
            .bind(event_id)
            .fetch_one(&pool)
            .await
            .expect("load row state");
            assert_eq!(
                published,
                (true, 1, None),
                "row on {subject} must be published on its first attempt"
            );
        }
        sqlx::query("DELETE FROM session_audit_outbox WHERE event_id = ANY($1)")
            .bind(rows.iter().map(|row| row.0.clone()).collect::<Vec<_>>())
            .execute(&pool)
            .await
            .expect("clean up probe rows");
    }

    /// The recovery path end to end: rows that ran out of attempts (as the live
    /// 4,992 did, `last_error` "no stream found for given subject") are not
    /// claimed again; once the cause is fixed, `requeue_session_audit_terminal`
    /// makes the drainer deliver them, on the subject their own event names, and
    /// the failure evidence is cleared only by that successful publish.
    /// Same environment as `outbox_drain_publishes_every_row_kind_...`.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL (disposable Postgres) and NATS_URL/NATS_USER/NATS_PASSWORD of a provisioned Model Plane broker"]
    async fn requeued_terminal_rows_are_delivered_once_the_cause_is_fixed() {
        let _serial = OUTBOX_DB_LOCK.lock().await;
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        let nats_url = std::env::var("NATS_URL").expect("NATS_URL");
        let pool = sqlx::PgPool::connect(&url).await.expect("connect postgres");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let suffix = mp_ids::new_ulid().to_lowercase();
        let detail = ToolActionDetail {
            data_category: "customer_private".to_owned(),
            zdr: false,
            tool: Some("knowledge_search".to_owned()),
        };
        let at = chrono::Utc::now();
        let mut ids = Vec::new();
        // Two final outcomes and one reservation stored under the old shared
        // subject: the three shapes the live stuck rows have.
        for (step, phase, outcome) in [
            ("stuck_final_a", "final", "completed"),
            ("stuck_final_b", "final", "failed"),
            ("stuck_reserved", "reserved", "reserved"),
        ] {
            let body = build_tool_action_body_for_phase(
                "org_probe_requeue",
                "user_probe",
                &format!("run_{suffix}"),
                step,
                outcome,
                "knowledge_search",
                &detail,
                phase,
                at,
            );
            let event_id = body["event_id"].as_str().unwrap().to_owned();
            sqlx::query(
                "INSERT INTO session_audit_outbox
                    (event_id, subject, payload, attempts, terminal_at, last_error)
                 VALUES ($1, $2, $3, 20, now(), 'no stream found for given subject')",
            )
            .bind(&event_id)
            .bind(SUBJECT_MODEL_TOOL_ACTION)
            .bind(&body)
            .execute(&pool)
            .await
            .expect("seed terminal row");
            ids.push(event_id);
        }

        let outbox = AuditOutbox::new(pool.clone(), nats_url);
        assert!(
            !outbox.dispatch_one().await.expect("drain terminal rows"),
            "a terminal row must not be claimed again: that is the dead end the requeue path removes"
        );

        let requeued: i32 = sqlx::query_scalar("SELECT requeue_session_audit_terminal($1, 10)")
            .bind(SUBJECT_MODEL_TOOL_ACTION)
            .fetch_one(&pool)
            .await
            .expect("requeue");
        assert_eq!(requeued, 3);

        let mut delivered = 0;
        while delivered < 10 {
            match outbox.dispatch_one().await {
                Ok(true) => delivered += 1,
                Ok(false) => break,
                Err(error) => panic!("replay failed: {error}"),
            }
        }
        assert_eq!(delivered, 3);
        for event_id in &ids {
            let state: (bool, i32, Option<String>, i32) = sqlx::query_as(
                "SELECT published_at IS NOT NULL, attempts, last_error, requeue_count
                 FROM session_audit_outbox WHERE event_id = $1",
            )
            .bind(event_id)
            .fetch_one(&pool)
            .await
            .expect("load replayed row");
            assert_eq!(
                state,
                (true, 1, None, 1),
                "published on the first attempt after the requeue, evidence cleared by the success, requeue counted"
            );
        }
        sqlx::query("DELETE FROM session_audit_outbox WHERE event_id = ANY($1)")
            .bind(&ids)
            .execute(&pool)
            .await
            .expect("clean up probe rows");
    }

    /// In-memory sink recording published (subject, body) pairs for assertions.
    #[derive(Default)]
    struct RecordingSink {
        published: Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl AuditSink for RecordingSink {
        async fn publish_audit(
            &self,
            subject: &str,
            body: &serde_json::Value,
        ) -> Result<(), String> {
            self.published
                .lock()
                .unwrap()
                .push((subject.to_owned(), body.clone()));
            Ok(())
        }
    }

    #[test]
    fn parses_prefix_from_output() {
        let detail = parse_tool_action_detail(
            "[data_category=customer_private zdr=true tool=knowledge_search] some tool output",
            "",
        )
        .expect("prefix present");
        assert_eq!(detail.data_category, "customer_private");
        assert!(detail.zdr);
        assert_eq!(detail.tool.as_deref(), Some("knowledge_search"));
    }

    #[test]
    fn parses_prefix_from_error_when_output_empty() {
        let detail =
            parse_tool_action_detail("", "[data_category=public_non_personal zdr=false] boom")
                .expect("prefix present");
        assert_eq!(detail.data_category, "public_non_personal");
        assert!(!detail.zdr);
        // No tool= token (pre-E5 prefix) → tool is None and the caller falls back.
        assert!(detail.tool.is_none());
    }

    #[test]
    fn resolve_tool_name_prefers_prefix_over_step_id() {
        let with_tool = ToolActionDetail {
            data_category: "public_non_personal".to_owned(),
            zdr: false,
            tool: Some("company_lookup".to_owned()),
        };
        // The step_id suffix is the opaque provider call id; the prefix wins.
        assert_eq!(
            resolve_tool_name(&with_tool, "tool_1_call-abc123"),
            "company_lookup"
        );

        let legacy = ToolActionDetail {
            data_category: "public_non_personal".to_owned(),
            zdr: false,
            tool: None,
        };
        // Pre-E5 step with no tool= → fall back to the step_id-derived name.
        assert_eq!(
            resolve_tool_name(&legacy, "tool_2_yr_weather"),
            "yr_weather"
        );
    }

    #[test]
    fn no_prefix_is_skipped() {
        assert!(parse_tool_action_detail("plain final answer", "").is_none());
    }

    #[test]
    fn extracts_tool_name_from_step_id() {
        assert_eq!(tool_name_from_step_id("tool_1_call-1"), "call-1");
        assert_eq!(tool_name_from_step_id("tool_2_yr_weather"), "yr_weather");
        // Non-tool step ids fall back to the raw id.
        assert_eq!(tool_name_from_step_id("final"), "final");
    }

    /// audit-core authorizes an audit message only when the subject's last token
    /// is the payload's `event` (`subscriber::eventAuthorityMatches`). The
    /// reserved phase of an inline tool call is a different audit event
    /// (`tool_action_reserved`) from its final outcome (`tool_action`), so the
    /// builder must name the event per phase. Before this, the builder always
    /// said `tool_action` and only the gRPC enqueue site patched the reserved
    /// body afterwards, while the row kept the `tool_action` subject: audit-core
    /// would have dead-lettered every reserved event as an authority mismatch.
    #[test]
    fn tool_action_event_name_is_phase_specific() {
        let detail = ToolActionDetail {
            data_category: "customer_private".to_owned(),
            zdr: false,
            tool: Some("knowledge_search".to_owned()),
        };
        let at = chrono::Utc::now();
        let reserved = build_tool_action_body_for_phase(
            "org_1",
            "user_1",
            "run_1",
            "act_1",
            "reserved",
            "knowledge_search",
            &detail,
            "reserved",
            at,
        );
        assert_eq!(reserved["event"], "tool_action_reserved");
        let finished = build_tool_action_body_for_phase(
            "org_1",
            "user_1",
            "run_1",
            "act_1",
            "completed",
            "knowledge_search",
            &detail,
            "final",
            at,
        );
        assert_eq!(finished["event"], "tool_action");
    }

    #[tokio::test]
    async fn publishes_tool_action_with_audit_core_shape() {
        let sink = RecordingSink::default();
        let detail = ToolActionDetail {
            data_category: "customer_private".to_owned(),
            zdr: true,
            tool: Some("knowledge_search".to_owned()),
        };
        let body = build_tool_action_body(
            "org_1",
            "user_1",
            "run_1",
            "tool_1_call-1",
            "running",
            "knowledge_search",
            &detail,
            chrono::Utc::now(),
        );
        publish_tool_action(&sink, &body).await.unwrap();

        let published = sink.published.lock().unwrap();
        assert_eq!(published.len(), 1, "exactly one audit event published");
        let (subject, body) = &published[0];
        // Subject matches audit-core's `verevon.audit.v1.<plane>.<event>` filter.
        assert_eq!(subject, SUBJECT_MODEL_TOOL_ACTION);
        // Body matches audit-core's AuditEvent json tags (required: org_id/plane/event).
        assert_eq!(body["org_id"], "org_1");
        assert_eq!(body["user_id"], "user_1");
        assert_eq!(body["plane"], "model");
        assert_eq!(body["producer"], "session-core");
        assert!(body["event_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("tool:session-core:")));
        assert_eq!(body["event"], "tool_action");
        assert_eq!(body["subject"], "run_1");
        assert_eq!(body["resource_id"], "tool_1_call-1");
        assert_eq!(body["outcome"], "running");
        assert_eq!(body["details"]["tool"], "knowledge_search");
        assert_eq!(body["details"]["data_category"], "customer_private");
        assert_eq!(body["details"]["zdr"], true);
        assert!(body["occurred_at"].is_string());
    }
}
