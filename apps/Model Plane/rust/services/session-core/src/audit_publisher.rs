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

use std::{sync::Arc, time::Duration};

use sqlx::PgPool;
use tokio::sync::Mutex;

/// NATS subject audit-core subscribes (`velion.audit.v1.>`) for model tool calls.
pub const SUBJECT_MODEL_TOOL_ACTION: &str = "velion.audit.v2.model.session-core.tool_action";

fn tool_action_event_id(run_id: &str, step_id: &str, phase: &str) -> String {
    let digest = blake3::hash(format!("{run_id}\0{step_id}\0{phase}").as_bytes());
    format!("tool:session-core:{digest}")
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
        "event": "tool_action",
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
/// acknowledgement. The outbox owns retry/error handling.
pub async fn publish_tool_action<S: AuditSink>(
    sink: &S,
    body: &serde_json::Value,
) -> Result<(), String> {
    sink.publish_audit(SUBJECT_MODEL_TOOL_ACTION, body).await
}

/// Thin NATS transport for audit events. Publishes the literal audit subject
/// directly — the `velion.audit.*` tree is outside `mp.v1.*`, so no subject
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
        let terminal = attempts >= MAX_AUDIT_DISPATCH_ATTEMPTS;
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
        Err(format!("publish audit event {event_id}: {error}"))
    }

    async fn publish_claimed(
        &self,
        event_id: &str,
        subject: &str,
        payload: &serde_json::Value,
    ) -> Result<(), String> {
        if !subject.starts_with("velion.audit.v2.model.session-core.") {
            return Err(format!(
                "audit subject {subject:?} is outside session-core authority"
            ));
        }
        if payload["event_id"].as_str() != Some(event_id)
            || payload["producer"].as_str() != Some("session-core")
            || payload["plane"].as_str() != Some("model")
        {
            return Err("audit payload authority does not match its outbox row".to_owned());
        }

        let mut publisher = self.publisher.lock().await;
        if publisher.is_none() {
            *publisher = Some(
                NatsAuditPublisher::connect(&self.nats_url)
                    .await
                    .map_err(|error| error.to_string())?,
            );
        }
        let publisher = publisher.as_ref().expect("publisher initialized");
        if subject == SUBJECT_MODEL_TOOL_ACTION {
            publish_tool_action(publisher, payload).await
        } else {
            publisher.publish_audit(subject, payload).await
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
        // Subject matches audit-core's `velion.audit.v1.<plane>.<event>` filter.
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
