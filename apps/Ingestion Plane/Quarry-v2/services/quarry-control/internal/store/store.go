// Package store holds the resource registry backend.
// Scaffold uses in-memory; Phase 1 swaps to pgx/Postgres with identical interface.
package store

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

type DB interface {
	Jobs() JobsStore
	Stores() ResourceStore[NamedStore]
	Snapshots() ResourceStore[Snapshot]
	Artifacts() ResourceStore[Artifact]
	Profiles() ResourceStore[BrowserProfile]
	Schedules() SchedulesStore
	Sources() SourcesStore
	Webhooks() ResourceStore[Webhook]
	WebhookDeliveries() WebhookDeliveryStore
	Blocklists() ResourceStore[BlocklistEntry]
	Events() EventLog
	// TeamUsage serves the four /v1/team/* aggregate endpoints plus the
	// activity feed. Both backends implement it: in-memory derives from the
	// event log + live job set, Postgres from SUM/GROUP BY over the same
	// tables.
	TeamUsage() TeamUsageStore
	// SnapshotsV2 is the enriched snapshot read model behind GET /v1/snapshots
	// (quarry_core::resources::Snapshot wire shape). Distinct from the legacy
	// ResourceStore[Snapshot] used by MountRestore — different table, different
	// producer. See SnapshotV2 for the coexistence rationale.
	SnapshotsV2() SnapshotsV2Store
	// ZdrTombstones is the metadata-only record of runs that happened under Zero
	// Data Retention (WS4.J, migration 015). OFF BY DEFAULT: nothing writes it
	// unless the edge's QUARRY_EDGE__ZDR_TOMBSTONE is on. See zdr_tombstones.go.
	ZdrTombstones() ZdrTombstoneStore
	// PurgeOrg hard-deletes every row this service holds for orgID across
	// every org-scoped table (jobs, schedules, quarry_sources,
	// quarry_benchmarks, quarry_idempotency_keys) — the GDPR cross-plane
	// erasure fan-out entry point. See gdpr_purge.go for the full scope
	// rationale (which tables qualify and which deliberately don't) and
	// SoftDeleteByOrg above for the org-scoping convention this mirrors,
	// at the opposite (hard/permanent, not soft/reversible) end. Idempotent:
	// a repeat call for an orgID with nothing left matches zero rows and
	// returns a zero PurgeResult, never an error.
	PurgeOrg(orgID string) (PurgeResult, error)
}

// RequestQueueSummary is the control-plane read model for the Rust-owned
// durable frontier. Control never mutates these rows; Quarry runtime remains
// the writer and authority for queue lifecycle.
type RequestQueueSummary struct {
	QueueID   string `json:"queue_id"`
	Name      string `json:"name"`
	Queued    uint64 `json:"queued"`
	InFlight  uint64 `json:"in_flight"`
	CreatedAt int64  `json:"created_at"`
}

// RequestQueueReader is optional so the in-memory control backend can keep
// working without pretending that an ephemeral process has durable queues.
// The Postgres backend implements it against the Rust-owned tables.
type RequestQueueReader interface {
	ListRequestQueues(orgID string, limit int, cursor string) ([]RequestQueueSummary, string, error)
}

type ResourceStore[T any] interface {
	Create(t T) error
	Get(id quarrycontracts.ID) (T, bool)
	List(limit int, cursor string) ([]T, string)
	Delete(id quarrycontracts.ID) error
}

type WebhookDeliveryStore interface {
	ResourceStore[WebhookDelivery]
	Update(d WebhookDelivery) error
	ClaimDue(now int64, limit int) ([]WebhookDelivery, error)
}

type SchedulesStore interface {
	ResourceStore[Schedule]
	UpdateEnabled(id quarrycontracts.ID, enabled bool) error
}

// SourcesStore is the durable registry of recurring ingestion targets a
// tenant has registered (Cycle 23 `quarry_sources`, migration 005). Every
// method is ORG-SCOPED: the org id is a verified value the edge stamps from
// the JWT, so a caller can never read or mutate another tenant's sources.
// Soft-delete only (set deleted_at) — list never returns soft-deleted rows.
type SourcesStore interface {
	// ListByOrg returns the org's non-deleted sources, newest-first, with an
	// opaque keyset cursor.
	ListByOrg(orgID string, limit int, cursor string) ([]Source, string)
	// Create inserts a new source. The caller mints the org-prefixed id and
	// stamps the org id; Create does not derive either.
	Create(s Source) error
	// UpsertByOrgAndURL is the idempotent counterpart to Create: when a live
	// (non-deleted) row already exists for the exact (org_id, url) pair, its
	// updated_at is refreshed and the EXISTING row is returned instead of
	// erroring; `created` reports which branch ran. This is what lets
	// POST /v1/sources be called repeatedly for the same website without
	// duplicating rows — added 2026-07-20 so quarry-runtime's crawl pipeline
	// (PageRunner's SourceRegistrar) can call this once per successfully
	// ingested page and have a multi-page crawl of one host collapse into a
	// single durable "tracked website" row.
	UpsertByOrgAndURL(s Source) (result Source, created bool, err error)
	// GetByOrg returns a single non-deleted source scoped to the org. The org
	// guard means a cross-tenant id returns (Source{}, false), never another
	// org's row.
	GetByOrg(orgID string, id quarrycontracts.ID) (Source, bool)
	// SoftDeleteByOrg sets deleted_at on a source the org owns. Returns
	// ErrNotFound when no live row matches BOTH the id AND the org — a
	// cross-tenant delete attempt is indistinguishable from "missing".
	SoftDeleteByOrg(orgID string, id quarrycontracts.ID) error
}

type JobsStore interface {
	ResourceStore[Job]
	// ListByOrg returns the org's jobs, newest-first, with an opaque
	// keyset cursor — backs GET /v1/jobs. Mirrors SourcesStore.ListByOrg.
	ListByOrg(orgID string, limit int, cursor string) ([]Job, string)
	// GetByOrg returns a job scoped to the org. A cross-tenant id returns
	// (Job{}, false) — indistinguishable from a genuinely missing id,
	// mirroring SourcesStore.GetByOrg's guard.
	GetByOrg(orgID string, id quarrycontracts.ID) (Job, bool)
	// GetByRunID returns the job that owns a dispatched Temporal run id
	// (Job.RunID, stamped by Update once the orchestrator dispatches the
	// job — see 006_jobs_run_id.sql). Unscoped by org: callers resolving
	// `GET /v1/runs/:id/events` don't have a caller-asserted org_id yet —
	// the job's own OrgID IS the org_id for every event in that run.
	GetByRunID(runID quarrycontracts.ID) (Job, bool)
	ListBySchedule(scheduleID quarrycontracts.ID, limit int, cursor string) ([]Job, string)
	// ListByKind returns jobs whose Kind matches exactly AND whose OrgID
	// matches orgID, newest-first, with the same opaque keyset cursor
	// convention as List. Backs GET /v1/{kind}/jobs — mirrors
	// ListBySchedule's filter pattern. The Postgres impl uses the
	// (org_id, kind, created_at) index (migration 010_jobs_org_id.sql).
	ListByKind(orgID, kind string, limit int, cursor string) ([]Job, string)
	// Update replaces a job record by ID. Used by the orchestrator's
	// jobs dispatcher to transition accepted → running and stamp the
	// run_id once a Temporal workflow has been started.
	Update(j Job) error
	// FindByIdempotencyKey returns an existing job that was created by
	// orgID with the same Idempotency-Key header, or (Job{}, false) when
	// no such record exists FOR THAT ORG — a key collision with another
	// tenant's job is treated as a miss, never returning a cross-tenant
	// record. Used by createJob to de-dupe client retries.
	FindByIdempotencyKey(orgID, key string) (Job, bool)

	// ClaimAccepted leases up to limit accepted jobs to a dispatcher for
	// `lease` and returns them, oldest first within each organization and
	// round-robin across organizations: the first job of every organization with
	// room, then the second, and so on. An organization has room while the
	// jobs it has running plus those leased and not yet started are below its
	// cap, which is the job's own params.admission.cap or defaultCap. A job
	// whose lease has expired is claimable again. Only the kinds the dispatcher
	// can execute are claimed (see AdmissionKind).
	ClaimAccepted(limit int, now time.Time, lease time.Duration, defaultCap int) ([]Job, error)
	// CountAccepted is how many of the organization's jobs are waiting: accepted
	// and not currently leased.
	CountAccepted(orgID string, now time.Time) int
	// FailExpired fails every waiting (accepted, unleased) job created before
	// the cutoff and returns them, with params.failure_reason set to
	// "backlog_expired".
	FailExpired(createdBefore, now time.Time) ([]Job, error)
	// ListByKindStatus is ListByKind restricted to the given statuses (an empty
	// list means any status). Backs GET /v1/{kind}/jobs?status=a,b.
	ListByKindStatus(orgID, kind string, statuses []string, limit int, cursor string) ([]Job, string)
	// ListActiveByOrg returns the organization's non-terminal jobs (accepted,
	// running, paused), oldest first, for the ongoing list. Org-scoped by
	// design: the ongoing list is a tenant-facing read.
	ListActiveByOrg(orgID string, limit int) []Job
	// UpdateIfStatus replaces a job only while its stored status is expected.
	// A job in another status is ErrConflict; a missing job is ErrNotFound.
	UpdateIfStatus(j Job, expected string) error
}

// AdmissionKind reports whether the dispatcher executes jobs of this kind, and
// so whether admission applies to them.
func AdmissionKind(kind string) bool {
	switch kind {
	case "scrape", "crawl", "batch":
		return true
	}
	return false
}

// JobAdmissionCap is the job's concurrency cap for its organization: the
// params.admission.cap the edge stamped at submit time when it is a positive
// number, otherwise def.
func JobAdmissionCap(j Job, def int) int {
	admission, ok := j.Params["admission"].(map[string]any)
	if !ok {
		return def
	}
	switch v := admission["cap"].(type) {
	case float64:
		if v >= 1 && v <= 1_000_000 {
			return int(v)
		}
	case int:
		if v >= 1 && v <= 1_000_000 {
			return v
		}
	case int64:
		if v >= 1 && v <= 1_000_000 {
			return int(v)
		}
	}
	return def
}

type EventLog interface {
	Append(evt quarrycontracts.Event) error
	ForRun(runID quarrycontracts.ID, afterSeq uint64, limit int) []quarrycontracts.Event
	ForJob(jobID quarrycontracts.ID, afterSeq uint64, limit int) []quarrycontracts.Event
	// NextSeq returns one above the current max seq for runID, or 1
	// when no events exist yet. The handler uses this to assign durable
	// per-run seq server-side regardless of caller numbering.
	NextSeq(runID quarrycontracts.ID) uint64
	// FindByIdempotencyKey resolves a change-webhook retry to the event
	// already recorded under the caller's Idempotency-Key header. Scoped
	// to type='change_detected' — exactly the rows the partial unique
	// index (migration 013) constrains — so keys from other producers can
	// never shadow an edge retry. The org is deliberately NOT part of the
	// lookup: the edge mints UUID keys, so a cross-org collision is not a
	// realistic threat, and the lookup only ever surfaces an event_id,
	// never payload content, to the retrying caller.
	FindByIdempotencyKey(key string) (quarrycontracts.Event, bool)
	// SummaryForJob counts a job's events by type: the count, the highest seq
	// and the first and last timestamp of each type.
	SummaryForJob(jobID quarrycontracts.ID) map[quarrycontracts.EventType]EventTypeSummary
	// CountsForJobs counts events by type for each of the jobs, in one query per
	// page (the job list's stats). Jobs with no events are absent from the map.
	CountsForJobs(jobIDs []quarrycontracts.ID) map[quarrycontracts.ID]map[quarrycontracts.EventType]int64
	// ForJobTypes is ForJob restricted to the given event types, in seq order.
	ForJobTypes(jobID quarrycontracts.ID, types []quarrycontracts.EventType, afterSeq uint64, limit int) []quarrycontracts.Event
}

// EventTypeSummary is what SummaryForJob reports for one event type.
type EventTypeSummary struct {
	Count  int64
	MaxSeq uint64
	First  time.Time
	Last   time.Time
}

// IsTerminalJobStatus reports whether a job status is final: the job takes no
// further transition, and a terminal event never moves it back.
func IsTerminalJobStatus(status string) bool {
	switch status {
	case "completed", "failed", "cancelled", "timed_out", "skipped":
		return true
	}
	return false
}

// IsActiveJobStatus reports whether a job is still in flight: waiting, running
// or paused. These are the statuses the ongoing list shows.
func IsActiveJobStatus(status string) bool {
	switch status {
	case "accepted", "running", "paused":
		return true
	}
	return false
}

// ---- resource types -------------------------------------------------------

type Job struct {
	ID quarrycontracts.ID `json:"id"`
	// OrgID is the tenant that created this job. Stamped server-side from
	// the edge-verified `?org_id` query param — createJob rejects an empty
	// value — never trusted from the request body, mirroring Source/
	// Schedule's org_id. NOT NULL in the DB (migration 010). GET /v1/jobs
	// and GET /v1/{kind}/jobs filter on this so one org can never see
	// another's crawl/scrape job history, params, or status.
	OrgID string `json:"org_id"`
	// Kind mirrors quarry-core::resources::JobResourceKind's valid
	// values 1:1 (crawl | search | extract | research | agent | batch |
	// scrape) — every value GET /v1/{kind}/jobs can forward through
	// JobSummary. NOT validated against this set at create time
	// (createJob passes the client-supplied string straight through);
	// job_wire.go's toJobWire defensively skips any other value rather
	// than let one bad row break the whole list.
	Kind       string                    `json:"kind"`
	Status     string                    `json:"status"`
	Policy     quarrycontracts.RunPolicy `json:"policy"`
	Params     map[string]any            `json:"params,omitempty"`
	ScheduleID *quarrycontracts.ID       `json:"schedule_id,omitempty"`
	CreatedAt  int64                     `json:"created_at"`
	// RunID is the Temporal workflow run id once the orchestrator has
	// picked the job up. Nil while status is "accepted"; populated when
	// the dispatcher transitions the job to "running". Events emitted
	// by the workflow set both job_id and run_id, so consumers can poll
	// either /v1/jobs/{id}/events or /v1/runs/{run_id}/events.
	RunID *quarrycontracts.ID `json:"run_id,omitempty"`
	// IdempotencyKey is supplied by clients via the `Idempotency-Key`
	// header on POST /v1/jobs/. If a job with the same key already
	// exists the handler returns the existing record (200) rather than
	// creating a duplicate (201). Nil for legacy / schedule-driven jobs.
	IdempotencyKey *string `json:"idempotency_key,omitempty"`
	// Statistics is what the finished run reported about itself (pages
	// visited and failed, status classes, drivers, error categories,
	// duration). Set from the terminal event; absent while the job runs and
	// for job kinds that do not report it. Closed vocabularies only.
	Statistics map[string]any `json:"statistics,omitempty"`
	// Origin says who started the job and from which run. Stamped at create time
	// from the verified caller; never read from the request body.
	Origin JobOrigin `json:"origin"`
	// DesiredState is the state the caller asked the job to reach next (for
	// example "paused" or "cancelled"). DesiredSeq rises with each request and
	// AppliedSeq is the last request the orchestrator carried out (WS15.L2).
	DesiredState string `json:"desired_state,omitempty"`
	DesiredSeq   int64  `json:"desired_seq"`
	AppliedSeq   int64  `json:"applied_seq"`
	// WorkflowID is the Temporal workflow id the orchestrator started. Empty
	// until the job is dispatched.
	WorkflowID string `json:"workflow_id,omitempty"`
	// DeadlineAt is when the job times out, in unix millis. Nil when unbounded.
	DeadlineAt *int64 `json:"deadline_at,omitempty"`
	// TerminalReason is the event type that ended the job, for example
	// "run_completed". Empty while the job is not terminal.
	TerminalReason string `json:"terminal_reason,omitempty"`
	// FinishedAt is when the job reached a terminal status, in unix millis.
	FinishedAt *int64 `json:"finished_at,omitempty"`
}

// JobOrigin is who started a job and from which run. Type is one of the closed
// set in OriginTypes; control refuses any other value at create time. Rows
// written before migration 016 read back as "unknown".
type JobOrigin struct {
	Type      string `json:"type"`
	Actor     string `json:"actor,omitempty"`
	Ref       string `json:"ref,omitempty"`
	CreatedBy string `json:"created_by,omitempty"`
}

// OriginTypes is the closed set of job origins a caller may declare.
var OriginTypes = map[string]bool{"user": true, "agent": true, "schedule": true, "webhook": true, "api": true}

// MarshalJSON renders Job.CreatedAt as an RFC3339 string on the wire
// instead of the raw Unix-millis int64 it's stored as internally.
//
// CreatedAt stays `int64` (unix millis) on the Go struct — every
// internal consumer (Postgres INSERT/SELECT bind params, the
// (created_at, id) keyset-pagination cursor in store/pg/resources.go,
// dispatcher/janitor comparisons) depends on that representation and
// is untouched by this method.
//
// The wire format matters because quarry-core::resources::JobSummary
// (crates/quarry-core/src/resources.rs, the Rust struct every
// `GET /v1/{kind}/jobs` and `GET /v1/jobs` response is decoded into via
// quarry-edge's `forward_list`) declares `created_at: DateTime<Utc>`,
// which serde only accepts as an RFC3339 string — the same convention
// already used for every sibling resource in that file (Source,
// Snapshot, RequestQueueSummary, BenchmarkSummary) and for event
// timestamps on this very API (`quarrycontracts.Event.Timestamp` is a
// Go `time.Time`, which `encoding/json` already renders as RFC3339).
// Before this method, Job was the one holdout emitting a bare integer,
// which quarry-edge failed to parse with "invalid type: integer
// `<millis>`, expected an RFC 3339 formatted date and time string".
func (j Job) MarshalJSON() ([]byte, error) {
	type wire struct {
		ID             quarrycontracts.ID        `json:"id"`
		OrgID          string                    `json:"org_id"`
		Kind           string                    `json:"kind"`
		Status         string                    `json:"status"`
		Policy         quarrycontracts.RunPolicy `json:"policy"`
		Params         map[string]any            `json:"params,omitempty"`
		ScheduleID     *quarrycontracts.ID       `json:"schedule_id,omitempty"`
		CreatedAt      string                    `json:"created_at"`
		RunID          *quarrycontracts.ID       `json:"run_id,omitempty"`
		IdempotencyKey *string                   `json:"idempotency_key,omitempty"`
		Statistics     map[string]any            `json:"statistics,omitempty"`
		Origin         JobOrigin                 `json:"origin"`
		DesiredState   string                    `json:"desired_state,omitempty"`
		DesiredSeq     int64                     `json:"desired_seq"`
		AppliedSeq     int64                     `json:"applied_seq"`
		WorkflowID     string                    `json:"workflow_id,omitempty"`
		DeadlineAt     *string                   `json:"deadline_at,omitempty"`
		TerminalReason string                    `json:"terminal_reason,omitempty"`
		FinishedAt     *string                   `json:"finished_at,omitempty"`
	}
	return json.Marshal(wire{
		ID:             j.ID,
		OrgID:          j.OrgID,
		Kind:           j.Kind,
		Status:         j.Status,
		Policy:         j.Policy,
		Params:         j.Params,
		ScheduleID:     j.ScheduleID,
		CreatedAt:      time.UnixMilli(j.CreatedAt).UTC().Format(time.RFC3339Nano),
		RunID:          j.RunID,
		IdempotencyKey: j.IdempotencyKey,
		Statistics:     j.Statistics,
		Origin:         j.Origin,
		DesiredState:   j.DesiredState,
		DesiredSeq:     j.DesiredSeq,
		AppliedSeq:     j.AppliedSeq,
		WorkflowID:     j.WorkflowID,
		DeadlineAt:     unixMilliPtrRFC3339(j.DeadlineAt),
		TerminalReason: j.TerminalReason,
		FinishedAt:     unixMilliPtrRFC3339(j.FinishedAt),
	})
}

// unixMilliPtrRFC3339 renders an optional unix-millis timestamp as RFC3339,
// or nil when it is unset.
func unixMilliPtrRFC3339(ms *int64) *string {
	if ms == nil {
		return nil
	}
	s := time.UnixMilli(*ms).UTC().Format(time.RFC3339Nano)
	return &s
}

// UnmarshalJSON is MarshalJSON's symmetric counterpart: it accepts
// CreatedAt as the RFC3339 string the wire format now uses and converts
// it back to the internal Unix-millis representation. Nothing in this
// package decodes a Job from client-supplied JSON today (create/update
// handlers decode into their own request-shaped structs), but this
// keeps `Job` round-trippable through encoding/json for tests and any
// future caller, rather than silently accepting a MarshalJSON without
// its inverse.
func (j *Job) UnmarshalJSON(data []byte) error {
	type wire struct {
		ID             quarrycontracts.ID        `json:"id"`
		OrgID          string                    `json:"org_id"`
		Kind           string                    `json:"kind"`
		Status         string                    `json:"status"`
		Policy         quarrycontracts.RunPolicy `json:"policy"`
		Params         map[string]any            `json:"params,omitempty"`
		ScheduleID     *quarrycontracts.ID       `json:"schedule_id,omitempty"`
		CreatedAt      string                    `json:"created_at"`
		RunID          *quarrycontracts.ID       `json:"run_id,omitempty"`
		IdempotencyKey *string                   `json:"idempotency_key,omitempty"`
		Statistics     map[string]any            `json:"statistics,omitempty"`
		Origin         JobOrigin                 `json:"origin"`
		DesiredState   string                    `json:"desired_state,omitempty"`
		DesiredSeq     int64                     `json:"desired_seq"`
		AppliedSeq     int64                     `json:"applied_seq"`
		WorkflowID     string                    `json:"workflow_id,omitempty"`
		DeadlineAt     *string                   `json:"deadline_at,omitempty"`
		TerminalReason string                    `json:"terminal_reason,omitempty"`
		FinishedAt     *string                   `json:"finished_at,omitempty"`
	}
	var w wire
	if err := json.Unmarshal(data, &w); err != nil {
		return err
	}
	*j = Job{
		ID:             w.ID,
		OrgID:          w.OrgID,
		Kind:           w.Kind,
		Status:         w.Status,
		Policy:         w.Policy,
		Params:         w.Params,
		ScheduleID:     w.ScheduleID,
		RunID:          w.RunID,
		IdempotencyKey: w.IdempotencyKey,
		Statistics:     w.Statistics,
		Origin:         w.Origin,
		DesiredState:   w.DesiredState,
		DesiredSeq:     w.DesiredSeq,
		AppliedSeq:     w.AppliedSeq,
		WorkflowID:     w.WorkflowID,
		TerminalReason: w.TerminalReason,
	}
	if w.CreatedAt != "" {
		t, err := time.Parse(time.RFC3339Nano, w.CreatedAt)
		if err != nil {
			return fmt.Errorf("job.created_at: %w", err)
		}
		j.CreatedAt = t.UnixMilli()
	}
	if w.DeadlineAt != nil {
		t, err := time.Parse(time.RFC3339Nano, *w.DeadlineAt)
		if err != nil {
			return fmt.Errorf("job.deadline_at: %w", err)
		}
		ms := t.UnixMilli()
		j.DeadlineAt = &ms
	}
	if w.FinishedAt != nil {
		t, err := time.Parse(time.RFC3339Nano, *w.FinishedAt)
		if err != nil {
			return fmt.Errorf("job.finished_at: %w", err)
		}
		ms := t.UnixMilli()
		j.FinishedAt = &ms
	}
	return nil
}

type NamedStore struct {
	ID        quarrycontracts.ID `json:"id"`
	Name      string             `json:"name"`
	Kind      string             `json:"kind"` // key_value | dataset | request_queue
	CreatedAt int64              `json:"created_at"`
}

type Snapshot struct {
	ID        quarrycontracts.ID `json:"id"`
	RunID     quarrycontracts.ID `json:"run_id"`
	Bucket    string             `json:"bucket"`
	CreatedAt int64              `json:"created_at"`
}

type Artifact struct {
	ID        quarrycontracts.ID `json:"id"`
	RunID     quarrycontracts.ID `json:"run_id"`
	Kind      string             `json:"kind"`
	Key       string             `json:"key"`
	Bytes     uint64             `json:"bytes"`
	CreatedAt int64              `json:"created_at"`
}

type BrowserProfile struct {
	ID          quarrycontracts.ID `json:"id"`
	Name        string             `json:"name"`
	SnapshotURI string             `json:"snapshot_uri"`
	CreatedAt   int64              `json:"created_at"`
}

type Schedule struct {
	ID quarrycontracts.ID `json:"id"`
	// OrgID is the tenant that owns this schedule. Stamped server-side
	// from the edge's verified JWT (never trusted from a client body),
	// it rides into the Temporal workflow Args so every change-monitor
	// run, baseline, and diff stays org-scoped. NOT NULL in the DB.
	OrgID      string `json:"org_id"`
	Cron       string `json:"cron"`
	TargetKind string `json:"target_kind"` // scrape | crawl | batch | change_monitor
	TargetRef  string `json:"target_ref"`  // url | job template id
	Enabled    bool   `json:"enabled"`
	CreatedAt  int64  `json:"created_at"`
	// CreatedBy is the user_id of whoever created the schedule (stamped
	// server-side from the edge's verified JWT). It rides into the
	// change-monitor workflow so the in-product notification on a detected
	// change reaches the person who set the monitor up. Empty for legacy
	// rows / non-user-initiated schedules.
	CreatedBy string `json:"created_by,omitempty"`
	// Zdr is the Zero Data Retention posture of the caller, STAMPED AT
	// CREATE (migration 023, WS4.H). The edge sets it from the verified
	// claims and a client body can never lower it (the edge's
	// CreateScheduleRequest has no such field). nil means "created before
	// the stamp existed" and is distinct from false ("stamped standard");
	// both serialise to workflow args exactly as before. It rides into the
	// change-monitor workflow so the orchestrator can declare the posture
	// on /v1/internal/change/record. An org that turns ZDR on LATER keeps
	// the stamp it was created with (documented gap, RUNBOOKS).
	Zdr *bool `json:"zdr,omitempty"`
	// Preset is INPUT-ONLY for change_monitor schedules: the caller sends
	// a fixed cadence ("hourly"|"daily"|"weekly") which Validate maps to a
	// literal 5-field cron. It is never persisted (no DB column) — only
	// the resolved Cron is stored.
	Preset string `json:"preset,omitempty"`
}

// Source is a user-registered recurring ingestion target (Cycle 23
// `quarry_sources`). It mirrors the Rust edge's `quarry_core::resources::Source`
// field-for-field so the edge's `forward_list::<Source>` deserializer round-trips
// without a Go-flavor envelope. OrgID is stamped server-side from the verified
// JWT and is NOT NULL in the DB; the JSON tag carries it on the wire so the edge
// surfaces it. CreatedAt/UpdatedAt are unix-millis to match the rest of the store.
type Source struct {
	ID        quarrycontracts.ID `json:"source_id"`
	OrgID     string             `json:"org_id"`
	Name      string             `json:"name"`
	URL       string             `json:"url"`
	Kind      string             `json:"kind"`   // crawl | scrape | search
	Status    string             `json:"status"` // active | paused | deleted
	Config    map[string]any     `json:"config,omitempty"`
	CreatedAt int64              `json:"created_at"`
	UpdatedAt int64              `json:"updated_at"`
}

type Webhook struct {
	ID        quarrycontracts.ID `json:"id"`
	URL       string             `json:"url"`
	Secret    string             `json:"secret"`
	Events    []string           `json:"events"`
	Active    bool               `json:"active"`
	CreatedAt int64              `json:"created_at"`
}

type WebhookDelivery struct {
	ID            quarrycontracts.ID `json:"id"`
	WebhookID     quarrycontracts.ID `json:"webhook_id"`
	EventID       quarrycontracts.ID `json:"event_id"`
	Payload       string             `json:"payload"`
	Attempt       int                `json:"attempt"`
	Status        string             `json:"status"` // pending | in_flight | success | failed | dlq
	LastError     string             `json:"last_error,omitempty"`
	NextAttemptAt int64              `json:"next_attempt_at"`
	CreatedAt     int64              `json:"created_at"`
}

type BlocklistEntry struct {
	ID        quarrycontracts.ID `json:"id"`
	Pattern   string             `json:"pattern"`
	IsRegex   bool               `json:"is_regex"`
	CreatedAt int64              `json:"created_at"`
}

// ---- in-memory impl -------------------------------------------------------

type memDB struct {
	mu           sync.RWMutex
	jobs         *genericStore[Job]
	stores       *genericStore[NamedStore]
	snaps        *genericStore[Snapshot]
	arts         *genericStore[Artifact]
	profiles     *genericStore[BrowserProfile]
	schedules    *genericStore[Schedule]
	sources      *memSources
	webhooks     *genericStore[Webhook]
	whDeliveries *genericStore[WebhookDelivery]
	blocklists   *genericStore[BlocklistEntry]
	events       *memEventLog
	snapshotsV2  *memSnapshotsV2
	jobClaims    *jobClaimTable
	zdrTombs     *memZdrTombstones
}

func NewMemory() DB {
	return &memDB{
		jobs:         newGeneric[Job](func(j Job) quarrycontracts.ID { return j.ID }),
		stores:       newGeneric[NamedStore](func(s NamedStore) quarrycontracts.ID { return s.ID }),
		snaps:        newGeneric[Snapshot](func(s Snapshot) quarrycontracts.ID { return s.ID }),
		arts:         newGeneric[Artifact](func(a Artifact) quarrycontracts.ID { return a.ID }),
		profiles:     newGeneric[BrowserProfile](func(p BrowserProfile) quarrycontracts.ID { return p.ID }),
		schedules:    newGeneric[Schedule](func(s Schedule) quarrycontracts.ID { return s.ID }),
		sources:      newMemSources(),
		webhooks:     newGeneric[Webhook](func(w Webhook) quarrycontracts.ID { return w.ID }),
		whDeliveries: newGeneric[WebhookDelivery](func(d WebhookDelivery) quarrycontracts.ID { return d.ID }),
		blocklists:   newGeneric[BlocklistEntry](func(b BlocklistEntry) quarrycontracts.ID { return b.ID }),
		events:       &memEventLog{},
		snapshotsV2:  &memSnapshotsV2{},
		jobClaims:    &jobClaimTable{until: map[quarrycontracts.ID]time.Time{}},
		zdrTombs:     newMemZdrTombstones(),
	}
}

func (d *memDB) ZdrTombstones() ZdrTombstoneStore { return d.zdrTombs }

func (d *memDB) Jobs() JobsStore                         { return &memJobs{d.jobs, d.jobClaims} }
func (d *memDB) Stores() ResourceStore[NamedStore]       { return d.stores }
func (d *memDB) Snapshots() ResourceStore[Snapshot]      { return d.snaps }
func (d *memDB) Artifacts() ResourceStore[Artifact]      { return d.arts }
func (d *memDB) Profiles() ResourceStore[BrowserProfile] { return d.profiles }
func (d *memDB) Schedules() SchedulesStore               { return &memSchedules{d.schedules} }
func (d *memDB) Sources() SourcesStore                   { return d.sources }
func (d *memDB) Webhooks() ResourceStore[Webhook]        { return d.webhooks }
func (d *memDB) WebhookDeliveries() WebhookDeliveryStore {
	return &memWebhookDeliveries{d.whDeliveries}
}
func (d *memDB) Blocklists() ResourceStore[BlocklistEntry] { return d.blocklists }
func (d *memDB) Events() EventLog                          { return d.events }

type memSchedules struct{ *genericStore[Schedule] }

func (m *memSchedules) UpdateEnabled(id quarrycontracts.ID, enabled bool) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	it, ok := m.items[id]
	if !ok {
		return ErrNotFound
	}
	it.Enabled = enabled
	m.items[id] = it
	return nil
}

// memSources is the dev-only in-memory SourcesStore. Unlike genericStore it
// MUST enforce the org guard + soft-delete in every method, because those are
// the security invariants the pg store relies on the SQL WHERE clause for.
type memSources struct {
	mu    sync.RWMutex
	items map[quarrycontracts.ID]Source
	order []quarrycontracts.ID // insertion order; we list newest-first
	// deleted records the soft-delete tombstone set (id present == deleted_at set).
	deleted map[quarrycontracts.ID]bool
}

func newMemSources() *memSources {
	return &memSources{
		items:   make(map[quarrycontracts.ID]Source),
		deleted: make(map[quarrycontracts.ID]bool),
	}
}

func (m *memSources) Create(s Source) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	if _, exists := m.items[s.ID]; exists {
		return ErrConflict
	}
	m.items[s.ID] = s
	m.order = append(m.order, s.ID)
	return nil
}

// UpsertByOrgAndURL scans the org's live rows for a matching URL (dev-only
// in-memory store, so a linear scan is fine — the pg impl uses a real unique
// index). A match refreshes updated_at and is returned as-is; no match
// inserts `s` the same way Create does.
func (m *memSources) UpsertByOrgAndURL(s Source) (Source, bool, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	for _, id := range m.order {
		if m.deleted[id] {
			continue
		}
		existing := m.items[id]
		if existing.OrgID == s.OrgID && existing.URL == s.URL {
			existing.UpdatedAt = s.UpdatedAt
			m.items[id] = existing
			return existing, false, nil
		}
	}
	m.items[s.ID] = s
	m.order = append(m.order, s.ID)
	return s, true, nil
}

func (m *memSources) GetByOrg(orgID string, id quarrycontracts.ID) (Source, bool) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	s, ok := m.items[id]
	// Org guard + soft-delete filter: a mismatched org or a tombstoned row is
	// reported as "not found" — identical to a genuinely missing id.
	if !ok || m.deleted[id] || s.OrgID != orgID {
		return Source{}, false
	}
	return s, true
}

func (m *memSources) ListByOrg(orgID string, limit int, _ string) ([]Source, string) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	if limit <= 0 {
		limit = 50
	}
	out := make([]Source, 0, limit)
	// Newest-first: walk insertion order in reverse.
	for i := len(m.order) - 1; i >= 0; i-- {
		id := m.order[i]
		if m.deleted[id] {
			continue
		}
		s := m.items[id]
		if s.OrgID != orgID {
			continue
		}
		if len(out) == limit {
			break
		}
		out = append(out, s)
	}
	return out, ""
}

func (m *memSources) SoftDeleteByOrg(orgID string, id quarrycontracts.ID) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	s, ok := m.items[id]
	if !ok || m.deleted[id] || s.OrgID != orgID {
		return ErrNotFound
	}
	m.deleted[id] = true
	s.Status = "deleted"
	m.items[id] = s
	return nil
}

// listWhere is the in-memory counterpart of the keyset-paged list queries: the
// jobs keep returns true for, newest first, after the cursor.
func (m *memJobs) listWhere(keep func(Job) bool, limit int, cur string) ([]Job, string) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	var curTS int64
	var curID string
	haveCur := false
	if cur != "" {
		if raw, err := base64.RawURLEncoding.DecodeString(cur); err == nil {
			parts := strings.SplitN(string(raw), "|", 2)
			if len(parts) == 2 {
				if _, err := fmt.Sscan(parts[0], &curTS); err == nil {
					curID = parts[1]
					haveCur = true
				}
			}
		}
	}
	if limit <= 0 {
		limit = 50
	}
	out := make([]Job, 0, limit)
	for i := len(m.order) - 1; i >= 0; i-- {
		it := m.items[m.order[i]]
		if !keep(it) {
			continue
		}
		if haveCur && !(it.CreatedAt < curTS || (it.CreatedAt == curTS && string(it.ID) < curID)) {
			continue
		}
		if len(out) == limit {
			last := out[len(out)-1]
			return out, base64.RawURLEncoding.EncodeToString([]byte(fmt.Sprintf("%d|%s", last.CreatedAt, string(last.ID))))
		}
		out = append(out, it)
	}
	return out, ""
}

// ListByKindStatus mirrors the Postgres query: kind and org, optionally
// restricted to a set of statuses.
func (m *memJobs) ListByKindStatus(orgID, kind string, statuses []string, limit int, cur string) ([]Job, string) {
	want := map[string]bool{}
	for _, s := range statuses {
		want[s] = true
	}
	return m.listWhere(func(it Job) bool {
		return it.Kind == kind && it.OrgID == orgID && (len(want) == 0 || want[it.Status])
	}, limit, cur)
}

// ListActiveByOrg returns the organization's jobs that are still in flight,
// oldest first. Mirrors the Postgres partial index idx_jobs_active.
func (m *memJobs) ListActiveByOrg(orgID string, limit int) []Job {
	m.mu.RLock()
	defer m.mu.RUnlock()
	if limit <= 0 {
		limit = 50
	}
	out := []Job{}
	for _, id := range m.order {
		it := m.items[id]
		if it.OrgID != orgID || !IsActiveJobStatus(it.Status) {
			continue
		}
		out = append(out, it)
		if len(out) == limit {
			break
		}
	}
	return out
}

// UpdateIfStatus is the compare-and-set form of Update: the write happens only
// while the stored status is expected.
func (m *memJobs) UpdateIfStatus(j Job, expected string) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	cur, ok := m.items[j.ID]
	if !ok {
		return ErrNotFound
	}
	if cur.Status != expected {
		return ErrConflict
	}
	m.items[j.ID] = j
	return nil
}

// jobClaimTable holds the in-memory lease of each claimed job (the Postgres
// store keeps it in jobs.claimed_until).
type jobClaimTable struct {
	mu    sync.Mutex
	until map[quarrycontracts.ID]time.Time
}

type memJobs struct {
	*genericStore[Job]
	claims *jobClaimTable
}

// ClaimAccepted mirrors the Postgres claim statement; see JobsStore.
func (m *memJobs) ClaimAccepted(limit int, now time.Time, lease time.Duration, defaultCap int) ([]Job, error) {
	if limit <= 0 {
		return nil, nil
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	m.claims.mu.Lock()
	defer m.claims.mu.Unlock()

	inUse := map[string]int{}
	var waiting []Job
	for _, j := range m.items {
		leased := m.claims.until[j.ID].After(now)
		switch {
		case j.Status == "running":
			inUse[j.OrgID]++
		case j.Status == "accepted" && leased:
			inUse[j.OrgID]++
		case j.Status == "accepted" && AdmissionKind(j.Kind):
			waiting = append(waiting, j)
		}
	}
	sort.Slice(waiting, func(a, b int) bool {
		if waiting[a].CreatedAt != waiting[b].CreatedAt {
			return waiting[a].CreatedAt < waiting[b].CreatedAt
		}
		return waiting[a].ID < waiting[b].ID
	})
	type ranked struct {
		job  Job
		rank int
	}
	seen := map[string]int{}
	var admitted []ranked
	for _, j := range waiting {
		seen[j.OrgID]++
		room := JobAdmissionCap(j, defaultCap) - inUse[j.OrgID]
		if room < 0 {
			room = 0
		}
		if seen[j.OrgID] <= room {
			admitted = append(admitted, ranked{job: j, rank: seen[j.OrgID]})
		}
	}
	sort.SliceStable(admitted, func(a, b int) bool {
		if admitted[a].rank != admitted[b].rank {
			return admitted[a].rank < admitted[b].rank
		}
		if admitted[a].job.CreatedAt != admitted[b].job.CreatedAt {
			return admitted[a].job.CreatedAt < admitted[b].job.CreatedAt
		}
		return admitted[a].job.ID < admitted[b].job.ID
	})
	if len(admitted) > limit {
		admitted = admitted[:limit]
	}
	out := make([]Job, 0, len(admitted))
	for _, a := range admitted {
		m.claims.until[a.job.ID] = now.Add(lease)
		out = append(out, a.job)
	}
	return out, nil
}

// CountAccepted counts the organization's waiting jobs.
func (m *memJobs) CountAccepted(orgID string, now time.Time) int {
	m.mu.RLock()
	defer m.mu.RUnlock()
	m.claims.mu.Lock()
	defer m.claims.mu.Unlock()
	n := 0
	for _, j := range m.items {
		if j.OrgID == orgID && j.Status == "accepted" && !m.claims.until[j.ID].After(now) {
			n++
		}
	}
	return n
}

// FailExpired fails waiting jobs created before the cutoff.
func (m *memJobs) FailExpired(createdBefore, now time.Time) ([]Job, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.claims.mu.Lock()
	defer m.claims.mu.Unlock()
	var failed []Job
	for id, j := range m.items {
		if j.Status != "accepted" || j.CreatedAt >= createdBefore.UnixMilli() || m.claims.until[id].After(now) {
			continue
		}
		params := make(map[string]any, len(j.Params)+1)
		for k, v := range j.Params {
			params[k] = v
		}
		params["failure_reason"] = "backlog_expired"
		j.Params = params
		j.Status = "failed"
		m.items[id] = j
		failed = append(failed, j)
	}
	return failed, nil
}

// FindByIdempotencyKey scans the in-memory job set for a record with the
// supplied key AND org. Linear scan is fine — the memory store is dev-only
// and job counts here are bounded. A key match under a different org is
// not returned — that would leak a cross-tenant job record to createJob's
// caller.
func (m *memJobs) FindByIdempotencyKey(orgID, key string) (Job, bool) {
	if key == "" {
		return Job{}, false
	}
	m.mu.RLock()
	defer m.mu.RUnlock()
	for _, j := range m.items {
		if j.IdempotencyKey != nil && *j.IdempotencyKey == key && j.OrgID == orgID {
			return j, true
		}
	}
	return Job{}, false
}

// Update replaces a Job record by id. Returns ErrNotFound when the id
// is unknown. The dispatcher uses this to transition accepted → running
// and stamp the run_id; we keep the existing CreatedAt + Params so a
// race between the dispatcher and a concurrent reader observes a
// consistent record.
func (m *memJobs) Update(j Job) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	if _, ok := m.items[j.ID]; !ok {
		return ErrNotFound
	}
	m.items[j.ID] = j
	return nil
}

// GetByOrg returns a job scoped to the org — mirrors memSources.GetByOrg's
// guard: a cross-tenant id is indistinguishable from a missing one.
func (m *memJobs) GetByOrg(orgID string, id quarrycontracts.ID) (Job, bool) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	j, ok := m.items[id]
	if !ok || j.OrgID != orgID {
		return Job{}, false
	}
	return j, true
}

// GetByRunID scans for the job whose RunID matches — the in-memory store
// has no secondary index, but its dataset is test-only (bounded size).
func (m *memJobs) GetByRunID(runID quarrycontracts.ID) (Job, bool) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	for _, j := range m.items {
		if j.RunID != nil && *j.RunID == runID {
			return j, true
		}
	}
	return Job{}, false
}

// ListByOrg returns the org's jobs, newest-first. Mirrors ListByKind's
// cursor handling — see that method for the encoding.
func (m *memJobs) ListByOrg(orgID string, limit int, cur string) ([]Job, string) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	var curTS int64
	var curID string
	haveCur := false
	if cur != "" {
		if raw, err := base64.RawURLEncoding.DecodeString(cur); err == nil {
			parts := strings.SplitN(string(raw), "|", 2)
			if len(parts) == 2 {
				if _, err := fmt.Sscan(parts[0], &curTS); err == nil {
					curID = parts[1]
					haveCur = true
				}
			}
		}
	}
	if limit <= 0 {
		limit = 50
	}
	out := make([]Job, 0, limit)
	for i := len(m.order) - 1; i >= 0; i-- {
		it := m.items[m.order[i]]
		if it.OrgID != orgID {
			continue
		}
		if haveCur {
			if !(it.CreatedAt < curTS || (it.CreatedAt == curTS && string(it.ID) < curID)) {
				continue
			}
		}
		if len(out) == limit {
			last := out[len(out)-1]
			return out, base64.RawURLEncoding.EncodeToString([]byte(fmt.Sprintf("%d|%s", last.CreatedAt, string(last.ID))))
		}
		out = append(out, it)
	}
	return out, ""
}

// ListByKind returns jobs whose Kind matches exactly AND whose OrgID
// matches orgID, newest-first. Mirrors ListBySchedule's cursor handling —
// see that method for the encoding.
func (m *memJobs) ListByKind(orgID, kind string, limit int, cur string) ([]Job, string) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	var curTS int64
	var curID string
	haveCur := false
	if cur != "" {
		if raw, err := base64.RawURLEncoding.DecodeString(cur); err == nil {
			parts := strings.SplitN(string(raw), "|", 2)
			if len(parts) == 2 {
				if _, err := fmt.Sscan(parts[0], &curTS); err == nil {
					curID = parts[1]
					haveCur = true
				}
			}
		}
	}
	if limit <= 0 {
		limit = 50
	}
	out := make([]Job, 0, limit)
	for i := len(m.order) - 1; i >= 0; i-- {
		it := m.items[m.order[i]]
		if it.Kind != kind || it.OrgID != orgID {
			continue
		}
		if haveCur {
			if !(it.CreatedAt < curTS || (it.CreatedAt == curTS && string(it.ID) < curID)) {
				continue
			}
		}
		if len(out) == limit {
			last := out[len(out)-1]
			return out, base64.RawURLEncoding.EncodeToString([]byte(fmt.Sprintf("%d|%s", last.CreatedAt, string(last.ID))))
		}
		out = append(out, it)
	}
	return out, ""
}

func (m *memJobs) ListBySchedule(sid quarrycontracts.ID, limit int, cur string) ([]Job, string) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	var curTS int64
	var curID string
	haveCur := false
	if cur != "" {
		if raw, err := base64.RawURLEncoding.DecodeString(cur); err == nil {
			parts := strings.SplitN(string(raw), "|", 2)
			if len(parts) == 2 {
				if _, err := fmt.Sscan(parts[0], &curTS); err == nil {
					curID = parts[1]
					haveCur = true
				}
			}
		}
	}
	if limit <= 0 {
		limit = 50
	}
	out := make([]Job, 0, limit)
	for i := len(m.order) - 1; i >= 0; i-- {
		it := m.items[m.order[i]]
		if it.ScheduleID == nil || *it.ScheduleID != sid {
			continue
		}
		if haveCur {
			if !(it.CreatedAt < curTS || (it.CreatedAt == curTS && string(it.ID) < curID)) {
				continue
			}
		}
		if len(out) == limit {
			last := out[len(out)-1]
			return out, base64.RawURLEncoding.EncodeToString([]byte(fmt.Sprintf("%d|%s", last.CreatedAt, string(last.ID))))
		}
		out = append(out, it)
	}
	return out, ""
}

type memWebhookDeliveries struct{ *genericStore[WebhookDelivery] }

func (m *memWebhookDeliveries) Update(d WebhookDelivery) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	if _, ok := m.items[d.ID]; !ok {
		return ErrNotFound
	}
	m.items[d.ID] = d
	return nil
}

func (m *memWebhookDeliveries) ClaimDue(now int64, limit int) ([]WebhookDelivery, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	var out []WebhookDelivery
	for _, id := range m.order {
		it := m.items[id]
		if it.Status == "pending" && it.NextAttemptAt <= now {
			out = append(out, it)
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].NextAttemptAt < out[j].NextAttemptAt })
	if limit > 0 && len(out) > limit {
		out = out[:limit]
	}
	return out, nil
}

type genericStore[T any] struct {
	mu    sync.RWMutex
	items map[quarrycontracts.ID]T
	order []quarrycontracts.ID
	idOf  func(T) quarrycontracts.ID
}

func newGeneric[T any](idOf func(T) quarrycontracts.ID) *genericStore[T] {
	return &genericStore[T]{items: make(map[quarrycontracts.ID]T), idOf: idOf}
}

func (s *genericStore[T]) Create(t T) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	id := s.idOf(t)
	if _, exists := s.items[id]; exists {
		return ErrConflict
	}
	s.items[id] = t
	s.order = append(s.order, id)
	return nil
}

func (s *genericStore[T]) Get(id quarrycontracts.ID) (T, bool) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	v, ok := s.items[id]
	return v, ok
}

func (s *genericStore[T]) List(limit int, _ string) ([]T, string) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	if limit <= 0 || limit > len(s.order) {
		limit = len(s.order)
	}
	out := make([]T, 0, limit)
	for _, id := range s.order[:limit] {
		out = append(out, s.items[id])
	}
	return out, ""
}

func (s *genericStore[T]) Delete(id quarrycontracts.ID) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if _, ok := s.items[id]; !ok {
		return ErrNotFound
	}
	delete(s.items, id)
	for i, x := range s.order {
		if x == id {
			s.order = append(s.order[:i], s.order[i+1:]...)
			break
		}
	}
	return nil
}

type memEventLog struct {
	mu     sync.RWMutex
	events []quarrycontracts.Event
}

func (l *memEventLog) Append(evt quarrycontracts.Event) error {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.events = append(l.events, evt)
	return nil
}

func (l *memEventLog) ForRun(runID quarrycontracts.ID, afterSeq uint64, limit int) []quarrycontracts.Event {
	l.mu.RLock()
	defer l.mu.RUnlock()
	out := make([]quarrycontracts.Event, 0, limit)
	for _, e := range l.events {
		if e.RunID == nil || *e.RunID != runID {
			continue
		}
		if e.Seq <= afterSeq {
			continue
		}
		out = append(out, e)
		if limit > 0 && len(out) >= limit {
			break
		}
	}
	return out
}

func (l *memEventLog) NextSeq(runID quarrycontracts.ID) uint64 {
	l.mu.RLock()
	defer l.mu.RUnlock()
	var maxSeq uint64
	for _, e := range l.events {
		if e.RunID == nil || *e.RunID != runID {
			continue
		}
		if e.Seq > maxSeq {
			maxSeq = e.Seq
		}
	}
	return maxSeq + 1
}

func (l *memEventLog) ForJob(jobID quarrycontracts.ID, afterSeq uint64, limit int) []quarrycontracts.Event {
	l.mu.RLock()
	defer l.mu.RUnlock()
	out := make([]quarrycontracts.Event, 0, limit)
	for _, e := range l.events {
		if e.JobID == nil || *e.JobID != jobID {
			continue
		}
		if e.Seq <= afterSeq {
			continue
		}
		out = append(out, e)
		if limit > 0 && len(out) >= limit {
			break
		}
	}
	return out
}

// SummaryForJob counts a job's events by type.
func (l *memEventLog) SummaryForJob(jobID quarrycontracts.ID) map[quarrycontracts.EventType]EventTypeSummary {
	l.mu.RLock()
	defer l.mu.RUnlock()
	out := map[quarrycontracts.EventType]EventTypeSummary{}
	for _, e := range l.events {
		if e.JobID == nil || *e.JobID != jobID {
			continue
		}
		s := out[e.Type]
		s.Count++
		if e.Seq > s.MaxSeq {
			s.MaxSeq = e.Seq
		}
		if s.First.IsZero() || e.Timestamp.Before(s.First) {
			s.First = e.Timestamp
		}
		if e.Timestamp.After(s.Last) {
			s.Last = e.Timestamp
		}
		out[e.Type] = s
	}
	return out
}

// CountsForJobs counts events by type for each listed job.
func (l *memEventLog) CountsForJobs(jobIDs []quarrycontracts.ID) map[quarrycontracts.ID]map[quarrycontracts.EventType]int64 {
	want := make(map[quarrycontracts.ID]bool, len(jobIDs))
	for _, id := range jobIDs {
		want[id] = true
	}
	l.mu.RLock()
	defer l.mu.RUnlock()
	out := map[quarrycontracts.ID]map[quarrycontracts.EventType]int64{}
	for _, e := range l.events {
		if e.JobID == nil || !want[*e.JobID] {
			continue
		}
		counts := out[*e.JobID]
		if counts == nil {
			counts = map[quarrycontracts.EventType]int64{}
			out[*e.JobID] = counts
		}
		counts[e.Type]++
	}
	return out
}

// ForJobTypes returns the job's events of the given types after afterSeq.
func (l *memEventLog) ForJobTypes(jobID quarrycontracts.ID, types []quarrycontracts.EventType, afterSeq uint64, limit int) []quarrycontracts.Event {
	want := make(map[quarrycontracts.EventType]bool, len(types))
	for _, t := range types {
		want[t] = true
	}
	l.mu.RLock()
	defer l.mu.RUnlock()
	out := []quarrycontracts.Event{}
	for _, e := range l.events {
		if e.JobID == nil || *e.JobID != jobID || e.Seq <= afterSeq || !want[e.Type] {
			continue
		}
		out = append(out, e)
		if limit > 0 && len(out) >= limit {
			break
		}
	}
	return out
}

// FindByIdempotencyKey mirrors the pg partial-unique-index scope: only
// change_detected rows participate. Linear scan is fine — the memory store
// is dev-only.
func (l *memEventLog) FindByIdempotencyKey(key string) (quarrycontracts.Event, bool) {
	if key == "" {
		return quarrycontracts.Event{}, false
	}
	l.mu.RLock()
	defer l.mu.RUnlock()
	for _, e := range l.events {
		if e.Type == quarrycontracts.EvtChangeDetected && e.IdempotencyKey == key {
			return e, true
		}
	}
	return quarrycontracts.Event{}, false
}
