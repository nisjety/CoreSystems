//! Wire shapes for Quarry's REST resources.
//!
//! Cycle 22 / cluster #4 part 1.
//!
//! These types are the **public contract** every list endpoint
//! returns. They live in `quarry-core` so the Rust edge, the Go
//! control plane, and (future) the GraphQL overlay all serialize
//! against one definition. Tests pin the JSON shape so a silent
//! struct-field rename is caught at CI time.
//!
//! Status field semantics are resource-specific — see the doc comment
//! on each type.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::kinds;

// =============================================================================
// Source — input URL feeds users register for recurring scrapes.
// =============================================================================

/// A `Source` represents one user-registered ingestion target (e.g.
/// "the AWS blog", "Acme Corp's pricing page"). Sources have a
/// recurring schedule and produce `Snapshot`s on every refresh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub source_id: kinds::SourceKind,
    pub org_id: String,
    pub name: String,
    pub url: String,
    /// `"crawl" | "scrape" | "search"` — drives which kind of job
    /// runs on refresh.
    pub kind: String,
    /// `"active" | "paused" | "deleted"`.
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Free-form per-source configuration (max_pages, include_patterns,
    /// presets, etc). JSONB on the Go control side.
    #[serde(default)]
    pub config: serde_json::Value,
}

// =============================================================================
// Snapshot — a captured + diffed page version.
// =============================================================================

/// A `Snapshot` is one historical version of a tracked source/URL.
/// Snapshots carry the fingerprint + diff metadata so callers can
/// detect change without re-fetching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub snapshot_id: kinds::SnapshotKind,
    pub org_id: String,
    /// Optional — set when the snapshot was produced as part of a
    /// recurring source refresh. Standalone scrapes leave this `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<kinds::SourceKind>,
    pub url: String,
    pub fingerprint: String,
    /// Previous snapshot's fingerprint, or `None` if this is the
    /// first version of the URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_fingerprint: Option<String>,
    /// `"unchanged" | "modified" | "new"`.
    pub change_status: String,
    pub captured_at: DateTime<Utc>,
    /// Artifact pointing at the captured markdown/HTML bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<kinds::ArtifactKind>,
}

// =============================================================================
// ArtifactSummary — list-view of stored bytes (markdown / HTML / PDF / etc.).
// =============================================================================

/// Compact list-view of an artifact. The full bytes are fetched via
/// `GET /v1/artifacts/:id/raw`; this shape only carries metadata so a
/// list of 1k artifacts doesn't drag 100MB of bodies into a JSON
/// response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactSummary {
    pub artifact_id: kinds::ArtifactKind,
    pub org_id: String,
    /// `"markdown" | "html" | "raw" | "pdf" | "screenshot" | "extract"`.
    pub kind: String,
    pub bytes: u64,
    /// `blake3:<hex>` content hash, useful for dedup detection.
    pub sha256: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Optional URL the bytes were captured from. Carried in metadata
    /// rather than as a typed field because not every artifact has a
    /// single source URL (e.g. AI-generated extract artifacts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
}

// =============================================================================
// JobSummary — per-kind run lists.
// =============================================================================

/// Discriminant for `/v1/{kind}/jobs`. **Distinct** from
/// [`crate::ids::kinds::JobKind`] — that's a phantom-typed ULID marker
/// (`job_<ulid>`). This enum is the resource discriminator used in
/// JSON responses, telling consumers which concrete kind of job a
/// `JobSummary` describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobResourceKind {
    Crawl,
    Search,
    Extract,
    Research,
    Agent,
    Batch,
    /// Single-page fetch. Quarry-control's `store.Job.Kind` produces this
    /// value constantly (it's the most common kind created via `POST
    /// /v1/jobs`) — it must have a variant here or every `scrape` job
    /// fails `JobResourceKind` deserialization the moment it's listed.
    Scrape,
}

impl JobResourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobResourceKind::Crawl => "crawl",
            JobResourceKind::Search => "search",
            JobResourceKind::Extract => "extract",
            JobResourceKind::Research => "research",
            JobResourceKind::Agent => "agent",
            JobResourceKind::Batch => "batch",
            JobResourceKind::Scrape => "scrape",
        }
    }

    /// Parse the path segment from `/v1/<segment>/jobs`. Returns `None`
    /// for unknown segments — handlers return 404 in that case.
    pub fn from_path_segment(s: &str) -> Option<Self> {
        match s {
            "crawl" => Some(JobResourceKind::Crawl),
            "search" => Some(JobResourceKind::Search),
            "extract" => Some(JobResourceKind::Extract),
            "research" => Some(JobResourceKind::Research),
            "agent" => Some(JobResourceKind::Agent),
            "batch" => Some(JobResourceKind::Batch),
            "scrape" => Some(JobResourceKind::Scrape),
            _ => None,
        }
    }
}

/// Compact list-view of a job. `status` is the union of every
/// kind-specific status because dashboards filter across kinds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSummary {
    /// The job's own resource id (`job_<ulid>`), i.e. what `GET
    /// /v1/jobs/:id` addresses. NOT the Temporal run id — that's a
    /// separate, optional identity a job only gains once dispatched
    /// (see quarry-control's `store.Job.RunID`). Uses `kinds::JobKind`
    /// (prefix `job_`) to match `Event.job_id` and quarry-control's own
    /// `quarrycontracts.KindJob` prefix.
    pub job_id: kinds::JobKind,
    pub kind: JobResourceKind,
    pub org_id: String,
    /// `"queued" | "running" | "completed" | "failed" | "cancelled"`.
    pub status: String,
    pub created_at: DateTime<Utc>,
    /// The Temporal run id, populated once the orchestrator dispatches this
    /// job (`store.Job.RunID` on the quarry-control side). Consumers that
    /// read a run's durable event history (`GET /v1/runs/:id/events`) MUST
    /// use this, never `job_id` — the events endpoint only accepts run ids.
    /// `None` while the job is still queued/accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<kinds::RunKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    /// Page count, query, URL — whatever's natural for the kind.
    #[serde(default)]
    pub stats: serde_json::Value,
}

// =============================================================================
// RequestQueue — cycle 23 / cluster #4 part 2
// =============================================================================

/// A logical queue of URLs / scrape jobs. Mirrors the Postgres
/// `quarry_request_queues` row created by cycle 20.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestQueueSummary {
    pub queue_id: kinds::QueueKind,
    pub org_id: String,
    pub name: String,
    /// `"crawl" | "batch" | "scrape" | "search"` — informational.
    pub kind: String,
    /// `"active" | "draining" | "deleted"`.
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub stats: RequestQueueStats,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestQueueStats {
    pub queued: u64,
    pub in_flight: u64,
    pub acked: u64,
    pub failed: u64,
}

// =============================================================================
// Benchmark — cycle 23 / cluster #4 part 2
// =============================================================================

/// Live benchmark run summary. The full corpus / baselines / per-test
/// scores are fetched via a dedicated GET /v1/benchmarks/:id detail
/// route. This list-view is what the dashboard renders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSummary {
    pub benchmark_id: String,
    pub org_id: String,
    pub name: String,
    /// `"static-html" | "js-heavy" | "bot-sensitive" | "ecommerce" |
    /// "docs-blog" | "pdf" | "login-profile-restore" |
    /// "crawl-sitemap" | "change-tracking"` — see gap-quarry §10.1 #12.
    pub suite: String,
    /// `"firecrawl-self-host" | "firecrawl-cloud" | "trafilatura" |
    /// "mozilla-readability" | "v1-local"`.
    pub baseline: Option<String>,
    pub status: String,
    pub last_run_at: Option<DateTime<Utc>>,
    pub latest_score: Option<f64>,
}

// =============================================================================
// Team — cycle 23 / cluster #4 part 2
// =============================================================================

/// Period of time the usage window covers. Free-form so dashboards can
/// query "today", "7d", "30d", or a specific YYYY-MM-DD.
pub type UsagePeriod = String;

/// Team credit usage for a window. Returned by `GET /v1/team/credit-usage`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamCreditUsage {
    pub org_id: String,
    pub period: UsagePeriod,
    pub credits_used: f64,
    /// Hard ceiling for the period. `None` = uncapped (enterprise tier).
    pub credits_limit: Option<f64>,
    pub utilization_percent: f64,
}

/// Team token usage for a window. Returned by `GET /v1/team/token-usage`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamTokenUsage {
    pub org_id: String,
    pub period: UsagePeriod,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Convenience: `input + output`.
    pub total_tokens: u64,
    /// Cents (micro-USD * 100 / 1_000_000). `None` until pricing is
    /// wired into billing-core (cycle 24 follow-up).
    pub cost_micro_usd: Option<i64>,
}

/// Per-org concurrency snapshot. `current` is the live in-flight count
/// across every host; `ceiling` is the org's configured limit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamConcurrency {
    pub org_id: String,
    pub current: u32,
    pub ceiling: u32,
    /// Optional per-host detail — only the top-N busiest hosts are
    /// returned to keep the response bounded.
    #[serde(default)]
    pub by_host: Vec<HostConcurrency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostConcurrency {
    pub host: String,
    pub current: u32,
    pub ceiling: u32,
    /// EWMA latency in ms. Sourced from the HostScheduler (cycle 21).
    pub ewma_latency_ms: Option<f64>,
}

/// Per-org queue snapshot. Aggregates over `quarry_queue_items` rows
/// (cycle 20). Detailed per-queue stats live in `RequestQueueSummary`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamQueueStatus {
    pub org_id: String,
    pub queued_total: u64,
    pub in_flight_total: u64,
    /// Top-N busiest queues. Empty when the org has no queues.
    #[serde(default)]
    pub by_queue: Vec<QueueStatusEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStatusEntry {
    pub queue_id: kinds::QueueKind,
    pub name: String,
    pub queued: u64,
    pub in_flight: u64,
}

/// Recent activity feed for the org. Each entry is a compact event
/// summary suitable for a dashboard timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamActivityEntry {
    pub event_id: kinds::EventKind,
    pub org_id: String,
    /// `"page_fetched" | "host_discovered" | "search_issued" | ...`
    /// (mirrors `EventType` token strings).
    pub event_type: String,
    pub run_id: Option<kinds::RunKind>,
    pub ts: DateTime<Utc>,
    /// Compact, human-readable summary the dashboard renders directly.
    pub summary: String,
}

// =============================================================================
// Schedule — cycle 23 / cluster #5
// =============================================================================

/// `OverlapPolicy` controls what happens when a scheduled run is due
/// while the previous run is still in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OverlapPolicy {
    /// Cancel any in-flight run before starting the new one.
    Cancel,
    /// Skip the new run if one is still in flight (default).
    #[default]
    Skip,
    /// Allow both to run concurrently. Use sparingly — most workloads
    /// don't tolerate parallel runs of the same schedule.
    Allow,
}

/// Wire shape for `GET /v1/schedules` list + `GET /v1/schedules/:id`
/// detail. Maps 1:1 onto Temporal's `Schedule` proto but uses
/// Quarry-native field names so the wire shape doesn't leak the
/// orchestrator vendor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleSummary {
    pub schedule_id: kinds::ScheduleKind,
    pub org_id: String,
    pub name: String,
    /// Which kind of job this schedule launches.
    pub kind: JobResourceKind,
    /// Standard 5-field cron, or `None` for one-shot delayed-start
    /// schedules (see `schedule_at`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    /// One-shot trigger time. When `Some`, `cron` must be `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub overlap_policy: OverlapPolicy,
    /// Window for catching up missed runs after Temporal recovery.
    /// `0` disables catchup — the schedule simply waits for the next
    /// natural firing.
    #[serde(default)]
    pub catchup_window_s: u64,
    /// Whether the schedule pauses itself after a failure.
    #[serde(default)]
    pub pause_on_failure: bool,
    /// `"active" | "paused" | "deleted"`.
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    /// Per-job-kind configuration the schedule was created with —
    /// e.g. seed URL + max_pages for a crawl. Mirrors the request
    /// body the schedule was created from.
    #[serde(default)]
    pub config: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_resource_kind_path_segment_roundtrip() {
        for &k in &[
            JobResourceKind::Crawl,
            JobResourceKind::Search,
            JobResourceKind::Extract,
            JobResourceKind::Research,
            JobResourceKind::Agent,
            JobResourceKind::Batch,
            JobResourceKind::Scrape,
        ] {
            let s = k.as_str();
            assert_eq!(JobResourceKind::from_path_segment(s), Some(k));
        }
    }

    /// Pins the exact wire shape quarry-control's `toJobWire` (Go,
    /// `services/quarry-control/internal/resources/job_wire.go`) emits
    /// for `GET /v1/{kind}/jobs`. If either side drifts — a renamed
    /// field, a prefix mismatch on `job_id`, or a `kind` value missing
    /// its `JobResourceKind` variant — this test catches it here
    /// instead of at the SPA.
    #[test]
    fn job_summary_deserializes_go_control_wire_shape() {
        let wire = serde_json::json!({
            "job_id": "job_01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "kind": "scrape",
            "org_id": "org_a",
            "status": "accepted",
            "created_at": "2026-07-20T12:00:00Z"
        });
        let summary: JobSummary = serde_json::from_value(wire)
            .expect("JobSummary must deserialize quarry-control's job_wire.go shape");
        assert_eq!(summary.kind, JobResourceKind::Scrape);
        assert_eq!(summary.job_id.to_string(), "job_01ARZ3NDEKTSV4RRFFQ69G5FAV");
        assert_eq!(summary.org_id, "org_a");
        assert!(summary.started_at.is_none());
        assert!(summary.completed_at.is_none());
    }

    #[test]
    fn job_resource_kind_rejects_unknown_segment() {
        assert!(JobResourceKind::from_path_segment("unknown").is_none());
        assert!(JobResourceKind::from_path_segment("").is_none());
    }

    #[test]
    fn schedule_overlap_policy_defaults_to_skip() {
        assert_eq!(OverlapPolicy::default(), OverlapPolicy::Skip);
    }

    #[test]
    fn schedule_overlap_policy_json_is_snake_case() {
        let s = serde_json::to_string(&OverlapPolicy::Cancel).unwrap();
        assert_eq!(s, "\"cancel\"");
        let s = serde_json::to_string(&OverlapPolicy::Allow).unwrap();
        assert_eq!(s, "\"allow\"");
    }

    #[test]
    fn schedule_summary_json_wire_shape() {
        let sched = ScheduleSummary {
            schedule_id: crate::ids::Id::new(),
            org_id: "org_a".into(),
            name: "nightly-docs".into(),
            kind: JobResourceKind::Crawl,
            cron: Some("0 3 * * *".into()),
            schedule_at: None,
            overlap_policy: OverlapPolicy::Skip,
            catchup_window_s: 3600,
            pause_on_failure: true,
            status: "active".into(),
            created_at: Utc::now(),
            last_run_at: None,
            next_run_at: None,
            config: serde_json::json!({"url": "https://example.com"}),
        };
        let s = serde_json::to_string(&sched).unwrap();
        // Field name pins — silent renames break SDK generators.
        assert!(s.contains("\"schedule_id\":"));
        assert!(s.contains("\"overlap_policy\":\"skip\""));
        assert!(s.contains("\"catchup_window_s\":3600"));
        assert!(s.contains("\"pause_on_failure\":true"));
        // schedule_at absent (None + skip_serializing_if).
        assert!(!s.contains("\"schedule_at\""));
    }

    #[test]
    fn team_concurrency_omits_empty_by_host() {
        let tc = TeamConcurrency {
            org_id: "org_a".into(),
            current: 4,
            ceiling: 16,
            by_host: vec![],
        };
        let s = serde_json::to_string(&tc).unwrap();
        // Empty vec is serialised as `[]` (no skip_serializing_if on
        // Vec by default). Pin the field name so it stays stable.
        assert!(s.contains("\"by_host\":[]"));
        assert!(s.contains("\"current\":4"));
        assert!(s.contains("\"ceiling\":16"));
    }

    #[test]
    fn benchmark_summary_optional_fields_omit_when_none() {
        let b = BenchmarkSummary {
            benchmark_id: "b1".into(),
            org_id: "org_a".into(),
            name: "static-html-bench".into(),
            suite: "static-html".into(),
            baseline: None,
            status: "pending".into(),
            last_run_at: None,
            latest_score: None,
        };
        let s = serde_json::to_string(&b).unwrap();
        // Sanity: these are real fields.
        assert!(s.contains("\"benchmark_id\":\"b1\""));
        assert!(s.contains("\"suite\":\"static-html\""));
    }

    #[test]
    fn artifact_summary_json_uses_snake_case_field_names() {
        let a = ArtifactSummary {
            artifact_id: crate::ids::Id::new(),
            org_id: "org_a".into(),
            kind: "markdown".into(),
            bytes: 1024,
            sha256: Some("blake3:abc".into()),
            created_at: Utc::now(),
            source_url: Some("https://example.com".into()),
        };
        let s = serde_json::to_string(&a).unwrap();
        // Field names MUST be snake_case to match the wire shape every
        // SDK generator expects.
        assert!(s.contains("\"artifact_id\":"));
        assert!(s.contains("\"org_id\":"));
        assert!(s.contains("\"source_url\":"));
        assert!(s.contains("\"created_at\":"));
        // sha256 is camel-cased? — assert it's not, to catch mishaps.
        assert!(!s.contains("\"sha256_b3\":"));
    }
}
