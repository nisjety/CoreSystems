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
    /// A live agent run was released by the sweeper (idle, over its lifetime, or a
    /// takeover nobody came back to). Payload: `reason`, `idle_s`, `age_s`; no URL.
    /// A non-ZDR run's checkpoint stays `active`, so it can be resumed.
    AgentExpired,
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
    /// the verevon onboarding wizard — render real brand colors and the
    /// detected logo immediately, instead of waiting for downstream
    /// model-plane interpretation.
    BrandingExtracted,
    /// Emitted by the page pipeline once per successfully transformed HTML
    /// page, AFTER readability/markdown/metadata have run. Carries the
    /// page-level extraction the earlier `page_fetched` (pre-transform, only
    /// `{url,status,duration_ms,content_type}`) and `artifact_written`
    /// (artifact ids + byte counts) never could: `title`, `title_source`
    /// (`html` | `model` | `host`), a whitespace-collapsed ~300-char
    /// `excerpt` of the markdown, `word_count`, `lang`, the `driver` that
    /// served the fetch, and an optional one-sentence model `summary`.
    /// Additive and backward compatible — consumers that only know
    /// `page_fetched` keep working; the verevon onboarding wizard maps this
    /// into its snippet cards so a crawl shows real titles and text instead
    /// of `N utdrag samlet` over empty excerpts.
    PageExtracted,
    /// WS6.B: an agent run reached a budget bound while enforcement is `on`
    /// (once per segment of the run). Payload: `dimension`, `limit`, `spent`,
    /// `unit`, `estimated`, `limit_source`. User-facing (X-12): the run's SSE
    /// stream sends it as `agent.budget_exhausted`; the run trace still needs a
    /// projection row (WS10).
    AgentBudgetExhausted,
    /// WS15.L2: a caller asked for a pause, resume or cancel of a job. Written by
    /// control when the request is recorded; the workflow acts on it later.
    /// Payload: `action`, `actor`, `actor_type`.
    ControlRequested,
    /// WS15.L2: a job's workflow ran past its execution timeout. Payload:
    /// `timeout_type`, `limit_s`.
    RunTimedOut,
    /// WS15.S4 mirror: a scheduled fire that did not start a job. Payload: `reason`.
    ScheduleSkipped,
    /// WS8.F: an agent attached to a live run and was given a tab. Payload: `attach_id`,
    /// `tab_id`. No URL and no title.
    AgentAttached,
    /// WS8.F: an agent attachment ended (released, aborted, revoked, idle or session ended).
    /// Payload: `attach_id`, `disposition`. No URL and no title.
    AgentDetached,
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
