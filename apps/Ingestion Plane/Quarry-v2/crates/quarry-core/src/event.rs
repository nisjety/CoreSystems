//! Event envelope (SSE + webhook + durable history). Mirrors CONTRACTS §3.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::kinds;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    RunStarted,
    RunPaused,
    RunResumed,
    RunCancelled,
    RunCompleted,
    RunFailed,
    PageQueued,
    PageFetched,
    PageFailed,
    PageBlocked,
    PageRetried,
    PageEscalated,
    ArtifactWritten,
    SnapshotCreated,
    StoreRecordWritten,
    /// Emitted when a Data Plane ingest attempt (`DataPlaneIngest::ingest`)
    /// returns an error — e.g. the service-token mint for the run's `org_id`
    /// was refused by Auth Core, or documents-api-go rejected the write.
    /// Previously this failure was ONLY `tracing::warn!`'d as "non-fatal" and
    /// never surfaced on the job's event stream, so a crawl could show
    /// "completed" while zero pages were ever durably persisted. Consumers
    /// (the Home dashboard's crawl-status widget) must treat a job as
    /// actually indexed only when ingest was requested AND this event was
    /// never seen (or `StoreRecordWritten` was seen instead).
    StoreRecordFailed,
    LeaseAcquired,
    LeaseReleased,
    ProfileRestored,
    ProfileCaptured,
    ChangeDetected,
    ChangeUnchanged,
    ScheduleFired,
    AgentStarted,
    ActionStarted,
    ActionCompleted,
    ActionFailed,
    ObservationReady,
    AgentCompleted,
    AgentFailed,
    /// Cycle 19 / cluster #19: emitted by `/v1/search` route on every
    /// query. Reserved for future `autocomplete-core` Sonic ingest into
    /// the `queries` bucket. Payload includes `query` + `provider` +
    /// `result_count`.
    SearchIssued,
    /// Cycle 19 / cluster #19: emitted by `CrawlFrontier::seed` when a
    /// new host is added to scope. Reserved for `autocomplete-core`
    /// Sonic ingest into the `hosts` bucket. Payload includes `host` +
    /// `seed_url`.
    HostDiscovered,
    /// Emitted by the page pipeline after each successful HTML fetch
    /// with the output of `quarry_transform::branding_rendered::extract`
    /// (favicon, theme color, palette, logo candidate, font family,
    /// site name, og:image). Lets downstream consumers — most notably
    /// the velion onboarding wizard — render real brand colors and the
    /// detected logo immediately, instead of waiting for downstream
    /// model-plane interpretation.
    BrandingExtracted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_id: kinds::EventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<kinds::RunKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<kinds::JobKind>,
    #[serde(rename = "type")]
    pub event_type: EventType,
    pub ts: DateTime<Utc>,
    pub seq: u64,
    pub payload: serde_json::Value,
    pub idempotency_key: String,
}
