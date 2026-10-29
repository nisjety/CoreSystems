//! Canonical job-history event envelope.
//!
//! Cycle 24 / cluster #7.
//!
//! ## Problem this solves
//!
//! Before today, every transport (SSE, NATS, webhooks, future
//! GraphQL subscriptions) re-modelled "progress for run X" with
//! slightly different field names. Frontends had to simulate phases
//! locally because the wire didn't carry `completed/total/discovered`
//! consistently. After a reconnect, the user saw a flat re-render
//! instead of the actual run state.
//!
//! `JobHistoryEvent` is the single shape every transport now emits.
//! The schema mirrors gap-quarry §10.1 #7's spec exactly so SDK
//! generators have one canonical type to consume.
//!
//! ## Subject taxonomy
//!
//! Per-transport subjects all derive from `(kind, stage)` via
//! `canonical_subject`. The convention is:
//!
//! | Transport | Subject template                          |
//! | --------- | ----------------------------------------- |
//! | NATS      | `quarry.jobs.<kind>.<stage>`              |
//! | SSE       | `<stage>` (event-stream `event:` field)    |
//! | Webhook   | `quarry.webhook.<kind>.<stage>`           |
//! | GraphQL   | `<kind>JobProgress` (subscription name)   |
//!
//! Helpers below produce each form so producers never invent the
//! string ad-hoc.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::kinds;
use crate::resources::JobResourceKind;

// =============================================================================
// JobHistoryEvent — canonical wire shape
// =============================================================================

/// One progress / state event for a run. Field set mirrors
/// gap-quarry §10.1 cluster #7 exactly so frontends can stop
/// simulating phases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobHistoryEvent {
    pub run_id: kinds::RunKind,
    pub org_id: String,
    pub kind: JobResourceKind,
    pub stage: JobStage,
    pub status: JobStatus,
    /// Monotonically increasing seq per `run_id`. Consumers detect
    /// gaps from a reconnect using this — backends MUST guarantee
    /// per-run total order.
    pub seq: u64,
    /// Pages / units completed so far.
    #[serde(default)]
    pub completed: u32,
    /// Best estimate of total work. `None` when the producer can't
    /// determine it (e.g. open-ended crawls).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
    /// New URLs discovered (or queries fanned out, depending on kind).
    #[serde(default)]
    pub discovered: u32,
    /// Currently queued items.
    #[serde(default)]
    pub queued: u32,
    /// Cumulative retry events for this run.
    #[serde(default)]
    pub retries: u32,
    /// Cumulative block events for this run.
    #[serde(default)]
    pub blocks: u32,
    /// Estimated time of completion. Producers MAY omit if they
    /// can't compute one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta: Option<DateTime<Utc>>,
    pub timestamp: DateTime<Utc>,
    /// Free-form per-stage payload (URL just fetched, error message,
    /// fingerprint, etc.). Schema lives on the producer side.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub payload: serde_json::Value,
}

impl JobHistoryEvent {
    /// Convenience constructor. `payload` defaults to JSON null;
    /// callers add stage-specific fields after construction.
    pub fn new(
        run_id: kinds::RunKind,
        org_id: impl Into<String>,
        kind: JobResourceKind,
        stage: JobStage,
        status: JobStatus,
        seq: u64,
    ) -> Self {
        Self {
            run_id,
            org_id: org_id.into(),
            kind,
            stage,
            status,
            seq,
            completed: 0,
            total: None,
            discovered: 0,
            queued: 0,
            retries: 0,
            blocks: 0,
            eta: None,
            timestamp: Utc::now(),
            payload: serde_json::Value::Null,
        }
    }
}

// =============================================================================
// Stage + Status enums
// =============================================================================

/// Coarse-grained phase of a run. Most transports filter on this to
/// render a step progress bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStage {
    /// Run created but not yet picked up by a worker.
    Queued,
    /// Run is being prepared (lease acquired, sources resolved).
    Starting,
    /// Active discovery + fetching.
    Running,
    /// Fetches done; transform / extract / ingest still running.
    Finalizing,
    /// All work durable; success terminal.
    Completed,
    /// Failed terminally (no more retries).
    Failed,
    /// Operator cancelled before terminal.
    Cancelled,
}

impl JobStage {
    pub fn as_str(self) -> &'static str {
        match self {
            JobStage::Queued => "queued",
            JobStage::Starting => "starting",
            JobStage::Running => "running",
            JobStage::Finalizing => "finalizing",
            JobStage::Completed => "completed",
            JobStage::Failed => "failed",
            JobStage::Cancelled => "cancelled",
        }
    }

    /// Whether this stage is a terminal one (no more events expected).
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            JobStage::Completed | JobStage::Failed | JobStage::Cancelled
        )
    }
}

/// Fine-grained outcome of the most recent event. Distinct from
/// `JobStage` because a `Running` stage can carry an `Ok` status
/// (progress tick) or an `Error` status (one page failed, but the
/// run continues).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Ok,
    Warn,
    Error,
}

// =============================================================================
// Subject helpers
// =============================================================================

/// NATS subject for a per-run event. `quarry.jobs.<kind>.<stage>`.
/// Subscribers can wildcard at every level — `quarry.jobs.crawl.>`
/// gets all crawl stages, `quarry.jobs.*.failed` gets every kind's
/// failures.
pub fn nats_subject(kind: JobResourceKind, stage: JobStage) -> String {
    format!("quarry.jobs.{}.{}", kind.as_str(), stage.as_str())
}

/// Webhook event-type field — matches the existing `quarry.webhook.*`
/// naming so consumers needn't relearn.
pub fn webhook_subject(kind: JobResourceKind, stage: JobStage) -> String {
    format!("quarry.webhook.{}.{}", kind.as_str(), stage.as_str())
}

/// SSE event name. Just the stage — the surrounding HTTP context
/// already pins kind + run_id via path params.
pub fn sse_event_name(stage: JobStage) -> &'static str {
    stage.as_str()
}

/// GraphQL subscription field name. Matches the `JobResourceKind`
/// snake_case form + `JobProgress` suffix (e.g. `crawlJobProgress`).
/// Pinned via test so frontends can rely on the shape.
pub fn graphql_subscription_field(kind: JobResourceKind) -> String {
    // Convert "crawl" → "crawlJobProgress". For multi-word kinds we
    // get title-case — none exist today but the function is forward-
    // compatible.
    let kind_str = kind.as_str();
    let mut chars = kind_str.chars();
    let lower = chars.next().map(|c| c.to_ascii_lowercase());
    let rest = chars.as_str();
    let head = lower.map(|c| c.to_string()).unwrap_or_default();
    format!("{head}{rest}JobProgress")
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_stage_serializes_snake_case() {
        let s = serde_json::to_string(&JobStage::Finalizing).unwrap();
        assert_eq!(s, "\"finalizing\"");
        let s = serde_json::to_string(&JobStage::Completed).unwrap();
        assert_eq!(s, "\"completed\"");
    }

    #[test]
    fn job_status_serializes_snake_case() {
        let s = serde_json::to_string(&JobStatus::Warn).unwrap();
        assert_eq!(s, "\"warn\"");
    }

    #[test]
    fn job_stage_is_terminal_helper() {
        assert!(JobStage::Completed.is_terminal());
        assert!(JobStage::Failed.is_terminal());
        assert!(JobStage::Cancelled.is_terminal());
        assert!(!JobStage::Running.is_terminal());
        assert!(!JobStage::Queued.is_terminal());
    }

    #[test]
    fn nats_subject_canonical_shape() {
        assert_eq!(
            nats_subject(JobResourceKind::Crawl, JobStage::Running),
            "quarry.jobs.crawl.running"
        );
        assert_eq!(
            nats_subject(JobResourceKind::Batch, JobStage::Failed),
            "quarry.jobs.batch.failed"
        );
    }

    #[test]
    fn webhook_subject_uses_webhook_prefix() {
        assert_eq!(
            webhook_subject(JobResourceKind::Search, JobStage::Completed),
            "quarry.webhook.search.completed"
        );
    }

    #[test]
    fn sse_event_name_is_just_stage() {
        assert_eq!(sse_event_name(JobStage::Running), "running");
        assert_eq!(sse_event_name(JobStage::Failed), "failed");
    }

    #[test]
    fn graphql_subscription_field_pattern() {
        assert_eq!(
            graphql_subscription_field(JobResourceKind::Crawl),
            "crawlJobProgress"
        );
        assert_eq!(
            graphql_subscription_field(JobResourceKind::Research),
            "researchJobProgress"
        );
    }

    #[test]
    fn job_history_event_json_pins_field_names() {
        // Frontends + SDK generators depend on this exact wire shape.
        // Any silent field rename trips this test.
        let evt = JobHistoryEvent::new(
            crate::ids::Id::new(),
            "org_a",
            JobResourceKind::Crawl,
            JobStage::Running,
            JobStatus::Ok,
            42,
        );
        let s = serde_json::to_string(&evt).unwrap();
        for field in &[
            "\"run_id\":",
            "\"org_id\":",
            "\"kind\":",
            "\"stage\":",
            "\"status\":",
            "\"seq\":",
            "\"completed\":",
            "\"discovered\":",
            "\"queued\":",
            "\"retries\":",
            "\"blocks\":",
            "\"timestamp\":",
        ] {
            assert!(s.contains(field), "missing field {field} in {s}");
        }
        // Optional fields with `skip_serializing_if` MUST be absent.
        assert!(!s.contains("\"total\""));
        assert!(!s.contains("\"eta\""));
        assert!(!s.contains("\"payload\""));
    }

    #[test]
    fn job_history_event_roundtrips_through_json() {
        let mut evt = JobHistoryEvent::new(
            crate::ids::Id::new(),
            "org_a",
            JobResourceKind::Crawl,
            JobStage::Finalizing,
            JobStatus::Warn,
            7,
        );
        evt.completed = 100;
        evt.total = Some(150);
        evt.payload = serde_json::json!({"url": "https://example.com"});
        let s = serde_json::to_string(&evt).unwrap();
        let back: JobHistoryEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(back.seq, 7);
        assert_eq!(back.total, Some(150));
        assert_eq!(back.payload["url"], "https://example.com");
        assert_eq!(back.stage, JobStage::Finalizing);
    }
}
