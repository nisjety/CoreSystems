//! Model `tool_action` audit publisher.
//!
//! When session-core records a tool `STEP_COMPLETED` (a non-terminal per-tool
//! step minted by execution-core's governed multi-tool loop), it best-effort
//! publishes a `velion.audit.v1.model.tool_action` event so audit-core's
//! retention subscriber (`velion.audit.v1.>`) durably records the agentic tool
//! call for GDPR.
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

/// NATS subject audit-core subscribes (`velion.audit.v1.>`) for model tool calls.
pub const SUBJECT_MODEL_TOOL_ACTION: &str = "velion.audit.v1.model.tool_action";

/// Parsed GDPR detail extracted from a per-tool step's output prefix
/// `[data_category=<class> zdr=<bool>]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolActionDetail {
    pub data_category: String,
    pub zdr: bool,
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
    for token in inner.split_whitespace() {
        if let Some(value) = token.strip_prefix("data_category=") {
            data_category = Some(value.to_owned());
        } else if let Some(value) = token.strip_prefix("zdr=") {
            zdr = Some(value == "true");
        }
    }

    Some(ToolActionDetail {
        data_category: data_category?,
        zdr: zdr.unwrap_or(false),
    })
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
    serde_json::json!({
        "occurred_at": occurred_at.to_rfc3339(),
        "org_id": org_id,
        // audit ingest treats user as optional; agentic tool steps run on the
        // user's behalf — fall back to "agent" when the user is unknown.
        "user_id": if user_id.is_empty() { "agent" } else { user_id },
        "plane": "model",
        "event": "tool_action",
        "subject": run_id,
        "resource_id": step_id,
        "outcome": status,
        "details": {
            "tool": tool,
            "data_category": detail.data_category,
            "zdr": detail.zdr,
        },
    })
}

/// Best-effort publish of a model tool-action audit event. A publish failure is
/// logged and swallowed — it must never fail the run that triggered the step.
pub async fn publish_tool_action<S: AuditSink>(sink: &S, body: &serde_json::Value) {
    if let Err(error) = sink.publish_audit(SUBJECT_MODEL_TOOL_ACTION, body).await {
        tracing::warn!(error = %error, subject = SUBJECT_MODEL_TOOL_ACTION, "tool_action audit publish failed (best-effort)");
    }
}

/// Thin NATS transport for audit events. Publishes the literal audit subject
/// directly — the `velion.audit.*` tree is outside `mp.v1.*`, so no subject
/// translation applies (unlike the run-event publisher).
pub struct NatsAuditPublisher {
    client: async_nats::Client,
}

impl NatsAuditPublisher {
    /// Connect to the NATS server at `url`.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    pub async fn connect(url: &str) -> Result<Self, async_nats::ConnectError> {
        let client = async_nats::connect(url).await?;
        tracing::info!(url = %url, "session-core audit publisher connected to NATS");
        Ok(Self { client })
    }
}

impl AuditSink for NatsAuditPublisher {
    async fn publish_audit(&self, subject: &str, body: &serde_json::Value) -> Result<(), String> {
        let bytes = serde_json::to_vec(body).map_err(|e| e.to_string())?;
        self.client
            .publish(subject.to_owned(), bytes.into())
            .await
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

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
            "[data_category=customer_private zdr=true] some tool output",
            "",
        )
        .expect("prefix present");
        assert_eq!(detail.data_category, "customer_private");
        assert!(detail.zdr);
    }

    #[test]
    fn parses_prefix_from_error_when_output_empty() {
        let detail =
            parse_tool_action_detail("", "[data_category=public_non_personal zdr=false] boom")
                .expect("prefix present");
        assert_eq!(detail.data_category, "public_non_personal");
        assert!(!detail.zdr);
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
        publish_tool_action(&sink, &body).await;

        let published = sink.published.lock().unwrap();
        assert_eq!(published.len(), 1, "exactly one audit event published");
        let (subject, body) = &published[0];
        // Subject matches audit-core's `velion.audit.v1.<plane>.<event>` filter.
        assert_eq!(subject, SUBJECT_MODEL_TOOL_ACTION);
        // Body matches audit-core's AuditEvent json tags (required: org_id/plane/event).
        assert_eq!(body["org_id"], "org_1");
        assert_eq!(body["user_id"], "user_1");
        assert_eq!(body["plane"], "model");
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
