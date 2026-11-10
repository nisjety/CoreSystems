// Package store holds the resource registry backend.
// Scaffold uses in-memory; Phase 1 swaps to pgx/Postgres with identical interface.
package store

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"regexp"
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
	Webhooks() WebhooksStore
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
	// GetByOrg and ListByWebhook are the tenant reads (INNH-5): a delivery of another
	// org answers exactly like a missing one. status "" lists every status.
	GetByOrg(id quarrycontracts.ID, orgID string) (WebhookDelivery, bool)
	ListByWebhook(orgID string, webhookID quarrycontracts.ID, status string, limit int, cursor string) ([]WebhookDelivery, string)
	// DeleteTerminalBefore hard-deletes deliveries that reached a terminal status
	// (success, failed, dlq) and were created before cutoff (unix seconds): the delivery
	// rows hold the whole event payload, so they are bounded by a retention window.
	DeleteTerminalBefore(cutoff int64) (int64, error)
}

// WebhooksStore is the webhook subscription table. The tenant methods (GetByOrg,
// ListByOrg, CountByOrg, DeleteByOrg, UpdateByOrg, CreateCapped) are the only ones the
// org-scoped /v1/webhook-subscriptions routes reach: a row of another org answers exactly
// like a missing one. ListActiveByOrg is the fan-out read and takes an org by design:
// there is no all-orgs read of subscriptions, so delivery cannot leak across tenants.
// The unscoped ResourceStore methods stay for the dispatcher (Get by delivery.webhook_id)
// and for tests.
type WebhooksStore interface {
	ResourceStore[Webhook]
	// CreateCapped inserts an org-scoped subscription unless the org already holds
	// maxPerOrg of them (ErrQuotaExceeded) or has one with the same url and events
	// (ErrConflict). The count and the insert are one atomic step.
	CreateCapped(w Webhook, maxPerOrg int) error
	GetByOrg(id quarrycontracts.ID, orgID string) (Webhook, bool)
	ListByOrg(orgID string, limit int, cursor string) ([]Webhook, string)
	CountByOrg(orgID string) int
	DeleteByOrg(id quarrycontracts.ID, orgID string) error
	// UpdateByOrg writes the mutable fields (Events, Active, Secret, SecretHint,
	// Description, NotifyOnChange) of the row with w.ID in w.OrgID.
	UpdateByOrg(w Webhook) error
	// ListActiveByOrg returns up to limit active subscriptions of orgID. orgID "" returns none.
	ListActiveByOrg(orgID string, limit int) []Webhook
}

// SchedulesStore is the schedule table. The tenant methods (GetByOrg,
// ListByOrg, ListByOrgTag, DeleteByOrg, UpdateEnabledByOrg, UpdateTagByOrg)
// are the only ones the edge reaches; a row that belongs to another org
// answers exactly like a missing one. The unscoped methods (Get, List, Delete, UpdateEnabled, ListDesired)
// serve the orchestrator reconciler and the internal source-monitor path.
type SchedulesStore interface {
	ResourceStore[Schedule]
	UpdateEnabled(id quarrycontracts.ID, enabled bool) error
	GetByOrg(id quarrycontracts.ID, orgID string) (Schedule, bool)
	ListByOrg(orgID string, limit int, cursor string) ([]Schedule, string)
	DeleteByOrg(id quarrycontracts.ID, orgID string) error
	UpdateEnabledByOrg(id quarrycontracts.ID, orgID string, enabled bool) error
	// ListByOrgTag is ListByOrg restricted to the schedules of orgID whose tag is
	// tag. The tag goes through NormalizeTag first, so "Pricing" finds "pricing".
	// "" lists the untagged schedules. An empty orgID, or a tag that does not
	// normalise, matches nothing.
	ListByOrgTag(orgID string, tag string, limit int, cursor string) ([]Schedule, string)
	// UpdateTagByOrg sets the tag of the schedule id in orgID ("" clears it). A row
	// of another org answers ErrNotFound, as a missing one does. It does not move
	// updated_at, which stays the create or enable/disable stamp.
	UpdateTagByOrg(id quarrycontracts.ID, orgID string, tag string) error
	// ListDesired is the orchestrator's all-orgs read (internal path only).
	ListDesired(limit int, cursor string) ([]Schedule, string)
	// UpdateRuntime writes the executor's bookkeeping (WS15.S2 writes it; the
	// method exists now so the column contract is one place).
	UpdateRuntime(id quarrycontracts.ID, lastRunAt, nextRunAt *int64, skippedOverlap int64) error
}

// ValidationError is a schedule definition that cannot be stored. Reason is
// a stable machine token (invalid_timezone, cadence_too_frequent, ...) that
// the handler returns in error.details.reason; Message is for people.
type ValidationError struct {
	Reason  string
	Message string
}

func (e *ValidationError) Error() string { return e.Message }

func invalid(reason, format string, args ...any) error {
	return &ValidationError{Reason: reason, Message: fmt.Sprintf(format, args...)}
}

// maxTagLength bounds one tag (E3.5-6, migration 027). A tag is a short operator
// label that groups monitors and sources; it is not free text.
const maxTagLength = 32

// tagPattern is the tag grammar once lowercased: a letter or a digit first, then
// letters, digits, '.', '_' or '-'.
var tagPattern = regexp.MustCompile(`^[a-z0-9][a-z0-9._-]*$`)

// NormalizeTag returns the stored form of a tag: trimmed and lowercased. The
// empty string is the untagged value and is valid. Anything else must be 1 to
// 32 characters of the tag grammar, or it is refused with reason invalid_tag.
// The message never echoes the input, so a long value cannot inflate a response.
func NormalizeTag(raw string) (string, error) {
	t := strings.ToLower(strings.TrimSpace(raw))
	if t == "" {
		return "", nil
	}
	if len(t) > maxTagLength || !tagPattern.MatchString(t) {
		return "", invalid("invalid_tag", "tag must be 1 to %d characters: a-z, 0-9, '.', '_' or '-', starting with a letter or digit", maxTagLength)
	}
	return t, nil
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
	// ListByOrgTag is ListByOrg restricted to the org's live sources whose tag is
	// tag, with the same tag rules as SchedulesStore.ListByOrgTag.
	ListByOrgTag(orgID, tag string, limit int, cursor string) ([]Source, string)
	// UpdateTagByOrg sets the tag of a live source the org owns ("" clears it) and
	// bumps updated_at. A cross-tenant or soft-deleted id answers ErrNotFound.
	UpdateTagByOrg(orgID string, id quarrycontracts.ID, tag string) error
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
	// SetControl records one control request against a job (WS15.L2), as one
	// write: a cancel of an accepted job ends it at once, any other allowed verb
	// sets desired_state and raises desired_seq. A refused request is a
	// *ControlError; a repeat of a request already pending returns the job.
	SetControl(orgID string, id quarrycontracts.ID, action string) (Job, error)
	// AckControl records that the orchestrator applied the desired state up to
	// appliedSeq. It only raises applied_seq. Internal.
	AckControl(id quarrycontracts.ID, appliedSeq int64) error
	// PendingControl lists up to limit jobs whose desired state is ahead of what
	// was applied, across organizations. Internal, for the orchestrator loop.
	PendingControl(limit int) []Job
	// ListRunning lists up to limit running or paused jobs that have a workflow,
	// across organizations. Internal, for the orchestrator's sweep.
	ListRunning(limit int) []Job
	// Finalize ends a job the orchestrator found ended (a timeout, a forced
	// cancel). Terminal only: a job that already ended is returned with
	// changed=false. Internal.
	Finalize(id quarrycontracts.ID, status, reason string, finishedAt int64) (Job, bool, error)
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
	// AppendNext assigns the event's seq as one above the highest seq of its
	// run and appends it, as one atomic step (D2-016). NextSeq followed by
	// Append is two steps: two writers of one run read the same max, one insert
	// loses on (run_id, seq), and the batch handler's replay tolerance then
	// drops that event without a trace. AppendNext holds a per-run lock from the
	// read of the max to the commit of the insert, so the seqs of a run are
	// unique, gap-free and committed in order, and a follower that has read seq N
	// never misses an event below N. An event with no run gets seq 1 (the seq of
	// a job's one event before it has a run). It returns the seq it stored, and
	// the evt.Seq the caller set is ignored.
	AppendNext(evt quarrycontracts.Event) (uint64, error)
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
	// Name is the operator-facing label (WS15.S1, migration 017). Empty for
	// legacy rows; the tenant view falls back to the target.
	Name string `json:"name,omitempty"`
	// Tag is the operator label that groups this monitor (E3.5-6, migration 027).
	// "" is untagged; a stored value has been through NormalizeTag.
	Tag string `json:"tag,omitempty"`
	// Config is the per-kind payload: {url} for scrape and crawl, {urls} for
	// batch, plus max_pages, max_depth and ingest. JSONB in the table.
	Config map[string]any `json:"config,omitempty"`
	// Timezone is an IANA zone the cron is read in. Default UTC.
	Timezone string `json:"timezone,omitempty"`
	// OverlapPolicy is skip | allow | cancel. Default skip.
	OverlapPolicy string `json:"overlap_policy,omitempty"`
	// CatchupWindowS is how far back a missed fire may be replayed (0..86400).
	CatchupWindowS int64 `json:"catchup_window_s,omitempty"`
	// PauseOnFailure pauses the schedule after a failed fire.
	PauseOnFailure bool `json:"pause_on_failure,omitempty"`
	// JitterS spreads each fire by up to this many seconds.
	JitterS int64 `json:"jitter_s,omitempty"`
	// UpdatedAt is unix seconds of the last create or enable/disable stamp.
	// 0 means a pre-017 row (the migration keys on it).
	UpdatedAt int64 `json:"updated_at,omitempty"`
	// LastRunAt and NextRunAt are written by the executor (WS15.S2).
	LastRunAt *int64 `json:"last_run_at,omitempty"`
	NextRunAt *int64 `json:"next_run_at,omitempty"`
	// SkippedOverlapCount counts fires skipped because a run was in flight.
	SkippedOverlapCount int64 `json:"skipped_overlap_count,omitempty"`
	// Activate is INPUT-ONLY. A create starts the schedule enabled only when
	// the body carries activate:true; the edge sets it for user tokens only.
	// It is never persisted: the stored state is Enabled.
	Activate bool `json:"activate,omitempty"`
}

// ApplyDefaults fills the fields a create leaves empty, so every stored row
// has the same shape whichever path created it (the source-monitor path does
// not go through Validate). now is unix seconds.
func (s *Schedule) ApplyDefaults(now int64) {
	if s.Config == nil {
		s.Config = map[string]any{}
	}
	if s.Timezone == "" {
		s.Timezone = "UTC"
	}
	if s.OverlapPolicy == "" {
		s.OverlapPolicy = "skip"
	}
	if s.CreatedAt == 0 {
		s.CreatedAt = now
	}
	if s.UpdatedAt == 0 {
		s.UpdatedAt = s.CreatedAt
	}
}

// Source is a user-registered recurring ingestion target (Cycle 23
// `quarry_sources`). It mirrors the Rust edge's `quarry_core::resources::Source`
// field-for-field so the edge's `forward_list::<Source>` deserializer round-trips
// without a Go-flavor envelope. OrgID is stamped server-side from the verified
// JWT and is NOT NULL in the DB; the JSON tag carries it on the wire so the edge
// surfaces it. CreatedAt/UpdatedAt are unix-millis to match the rest of the store.
// Tag (E3.5-6, migration 027) is the operator label: "" is untagged, and a stored
// value has been through NormalizeTag.
type Source struct {
	ID        quarrycontracts.ID `json:"source_id"`
	OrgID     string             `json:"org_id"`
	Name      string             `json:"name"`
	URL       string             `json:"url"`
	Kind      string             `json:"kind"`   // crawl | scrape | search
	Status    string             `json:"status"` // active | paused | deleted
	Config    map[string]any     `json:"config,omitempty"`
	Tag       string             `json:"tag,omitempty"`
	CreatedAt int64              `json:"created_at"`
	UpdatedAt int64              `json:"updated_at"`
}

// Webhook is a delivery subscription. OrgID is the tenant that owns it; a row with an empty
// OrgID (created before INNH-5) belongs to nobody and receives no delivery. Secret is the
// HMAC key and is never serialized: the subscription routes show it once, on create and on
// rotate, from the value they minted.
type Webhook struct {
	ID          quarrycontracts.ID `json:"id"`
	OrgID       string             `json:"org_id,omitempty"`
	URL         string             `json:"url"`
	Secret      string             `json:"-"`
	SecretHint  string             `json:"secret_hint,omitempty"`
	Events      []string           `json:"events"`
	Active      bool               `json:"active"`
	Description string             `json:"description,omitempty"`
	CreatedBy   string             `json:"created_by,omitempty"`
	CreatedAt   int64              `json:"created_at"`

	// NotifyOnChange is the subscription's notifyOnChange filter (E3.5-7) as the JSON document the
	// control plane normalised. Empty means the subscription takes every change. The store keeps the
	// bytes and does not read them: the filter type lives in the resources package.
	NotifyOnChange json.RawMessage `json:"notify_on_change,omitempty"`
}

type WebhookDelivery struct {
	ID        quarrycontracts.ID `json:"id"`
	WebhookID quarrycontracts.ID `json:"webhook_id"`
	OrgID     string             `json:"org_id,omitempty"`
	EventID   quarrycontracts.ID `json:"event_id"`
	EventType string             `json:"event_type,omitempty"`
	// DedupeKey, when set, is unique per webhook: a second delivery of the same logical
	// event (one change reaching control by two paths) is dropped with ErrConflict.
	DedupeKey     string `json:"dedupe_key,omitempty"`
	Payload       string `json:"payload"`
	Attempt       int    `json:"attempt"`
	Status        string `json:"status"` // pending | in_flight | success | failed | dlq
	LastError     string `json:"last_error,omitempty"`
	NextAttemptAt int64  `json:"next_attempt_at"`
	CreatedAt     int64  `json:"created_at"`
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
	webhooks     *memWebhooks
	whDeliveries *genericStore[WebhookDelivery]
	blocklists   *genericStore[BlocklistEntry]
	events       *memEventLog
	snapshotsV2  *memSnapshotsV2
	jobClaims    *jobClaimTable
	zdrTombs     *memZdrTombstones
	schedReqs    *memScheduleRequests
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
		webhooks:     &memWebhooks{newGeneric[Webhook](func(w Webhook) quarrycontracts.ID { return w.ID })},
		whDeliveries: newGeneric[WebhookDelivery](func(d WebhookDelivery) quarrycontracts.ID { return d.ID }),
		blocklists:   newGeneric[BlocklistEntry](func(b BlocklistEntry) quarrycontracts.ID { return b.ID }),
		events:       &memEventLog{},
		snapshotsV2:  &memSnapshotsV2{},
		jobClaims:    &jobClaimTable{until: map[quarrycontracts.ID]time.Time{}},
		zdrTombs:     newMemZdrTombstones(),
		schedReqs:    newMemScheduleRequests(),
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
func (d *memDB) Webhooks() WebhooksStore                 { return d.webhooks }
func (d *memDB) WebhookDeliveries() WebhookDeliveryStore {
	return &memWebhookDeliveries{d.whDeliveries}
}
func (d *memDB) Blocklists() ResourceStore[BlocklistEntry] { return d.blocklists }
func (d *memDB) Events() EventLog                          { return d.events }

type memSchedules struct{ *genericStore[Schedule] }

// Create applies the same defaults the pg store applies (ApplyDefaults).
func (m *memSchedules) Create(s Schedule) error {
	tag, err := NormalizeTag(s.Tag)
	if err != nil {
		return err
	}
	s.Tag = tag
	s.ApplyDefaults(time.Now().Unix())
	return m.genericStore.Create(s)
}

func (m *memSchedules) UpdateEnabled(id quarrycontracts.ID, enabled bool) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	it, ok := m.items[id]
	if !ok {
		return ErrNotFound
	}
	it.Enabled = enabled
	it.UpdatedAt = time.Now().Unix()
	m.items[id] = it
	return nil
}

func (m *memSchedules) GetByOrg(id quarrycontracts.ID, orgID string) (Schedule, bool) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	it, ok := m.items[id]
	if !ok || it.OrgID != orgID {
		return Schedule{}, false
	}
	return it, true
}

func (m *memSchedules) ListByOrg(orgID string, limit int, cursor string) ([]Schedule, string) {
	m.mu.RLock()
	all := make([]Schedule, 0, len(m.items))
	for _, it := range m.items {
		if it.OrgID == orgID {
			all = append(all, it)
		}
	}
	m.mu.RUnlock()
	return pageSchedulesMem(all, limit, cursor)
}

// ListByOrgTag is ListByOrg restricted to the schedules of orgID whose tag is
// tag, after NormalizeTag. An empty orgID, or a tag that does not normalise,
// matches nothing.
func (m *memSchedules) ListByOrgTag(orgID, tag string, limit int, cursor string) ([]Schedule, string) {
	norm, err := NormalizeTag(tag)
	if err != nil || orgID == "" {
		return nil, ""
	}
	m.mu.RLock()
	all := make([]Schedule, 0, len(m.items))
	for _, it := range m.items {
		if it.OrgID == orgID && it.Tag == norm {
			all = append(all, it)
		}
	}
	m.mu.RUnlock()
	return pageSchedulesMem(all, limit, cursor)
}

// UpdateTagByOrg sets the tag of a schedule the org owns. Like the pg store, it
// leaves UpdatedAt alone.
func (m *memSchedules) UpdateTagByOrg(id quarrycontracts.ID, orgID string, tag string) error {
	norm, err := NormalizeTag(tag)
	if err != nil {
		return err
	}
	if orgID == "" {
		return ErrNotFound
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	it, ok := m.items[id]
	if !ok || it.OrgID != orgID {
		return ErrNotFound
	}
	it.Tag = norm
	m.items[id] = it
	return nil
}

func (m *memSchedules) ListDesired(limit int, cursor string) ([]Schedule, string) {
	m.mu.RLock()
	all := make([]Schedule, 0, len(m.items))
	for _, it := range m.items {
		all = append(all, it)
	}
	m.mu.RUnlock()
	return pageSchedulesMem(all, limit, cursor)
}

// DeleteByOrg checks the org and then deletes. The two steps are not atomic
// in this dev store; the pg store does it in one WHERE clause.
func (m *memSchedules) DeleteByOrg(id quarrycontracts.ID, orgID string) error {
	if _, ok := m.GetByOrg(id, orgID); !ok {
		return ErrNotFound
	}
	return m.genericStore.Delete(id)
}

func (m *memSchedules) UpdateEnabledByOrg(id quarrycontracts.ID, orgID string, enabled bool) error {
	if _, ok := m.GetByOrg(id, orgID); !ok {
		return ErrNotFound
	}
	return m.UpdateEnabled(id, enabled)
}

func (m *memSchedules) UpdateRuntime(id quarrycontracts.ID, lastRunAt, nextRunAt *int64, skippedOverlap int64) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	it, ok := m.items[id]
	if !ok {
		return ErrNotFound
	}
	it.LastRunAt, it.NextRunAt, it.SkippedOverlapCount = lastRunAt, nextRunAt, skippedOverlap
	m.items[id] = it
	return nil
}

// pageSchedulesMem sorts newest first (created_at desc, id desc, as the pg
// store does) and pages by the last id of the previous page.
func pageSchedulesMem(all []Schedule, limit int, cursor string) ([]Schedule, string) {
	sort.Slice(all, func(i, j int) bool {
		if all[i].CreatedAt != all[j].CreatedAt {
			return all[i].CreatedAt > all[j].CreatedAt
		}
		return all[i].ID > all[j].ID
	})
	if limit <= 0 {
		limit = 50
	}
	start := 0
	if cursor != "" {
		start = len(all)
		for i, s := range all {
			if string(s.ID) == cursor {
				start = i + 1
				break
			}
		}
	}
	end := start + limit
	if end > len(all) {
		end = len(all)
	}
	out := append([]Schedule(nil), all[start:end]...)
	next := ""
	if end < len(all) && len(out) > 0 {
		next = string(out[len(out)-1].ID)
	}
	return out, next
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
	tag, err := NormalizeTag(s.Tag)
	if err != nil {
		return err
	}
	s.Tag = tag
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
	tag, err := NormalizeTag(s.Tag)
	if err != nil {
		return Source{}, false, err
	}
	s.Tag = tag
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

// ListByOrgTag is ListByOrg restricted to the org's live sources whose tag is
// tag, after NormalizeTag. Like ListByOrg it returns no cursor: the in-memory
// store is the dev store and does not page sources.
func (m *memSources) ListByOrgTag(orgID, tag string, limit int, _ string) ([]Source, string) {
	norm, err := NormalizeTag(tag)
	if err != nil || orgID == "" {
		return nil, ""
	}
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
		if s.OrgID != orgID || s.Tag != norm {
			continue
		}
		if len(out) == limit {
			break
		}
		out = append(out, s)
	}
	return out, ""
}

// UpdateTagByOrg sets the tag of a live source the org owns and bumps updated_at.
// A cross-tenant or soft-deleted id answers ErrNotFound, as SoftDeleteByOrg does.
func (m *memSources) UpdateTagByOrg(orgID string, id quarrycontracts.ID, tag string) error {
	norm, err := NormalizeTag(tag)
	if err != nil {
		return err
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	s, ok := m.items[id]
	if !ok || m.deleted[id] || s.OrgID != orgID || orgID == "" {
		return ErrNotFound
	}
	s.Tag = norm
	s.UpdatedAt = time.Now().UnixMilli()
	m.items[id] = s
	return nil
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

func (m *memWebhookDeliveries) Create(d WebhookDelivery) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	if _, exists := m.items[d.ID]; exists {
		return ErrConflict
	}
	if d.DedupeKey != "" {
		for _, it := range m.items {
			if it.WebhookID == d.WebhookID && it.DedupeKey == d.DedupeKey {
				return ErrConflict
			}
		}
	}
	m.items[d.ID] = d
	m.order = append(m.order, d.ID)
	return nil
}

func (m *memWebhookDeliveries) GetByOrg(id quarrycontracts.ID, orgID string) (WebhookDelivery, bool) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	it, ok := m.items[id]
	if !ok || orgID == "" || it.OrgID != orgID {
		return WebhookDelivery{}, false
	}
	return it, true
}

func (m *memWebhookDeliveries) ListByWebhook(orgID string, webhookID quarrycontracts.ID, status string, limit int, cursor string) ([]WebhookDelivery, string) {
	m.mu.RLock()
	var all []WebhookDelivery
	if orgID != "" {
		for _, it := range m.items {
			if it.OrgID == orgID && it.WebhookID == webhookID && (status == "" || it.Status == status) {
				all = append(all, it)
			}
		}
	}
	m.mu.RUnlock()
	sort.Slice(all, func(i, j int) bool {
		if all[i].CreatedAt != all[j].CreatedAt {
			return all[i].CreatedAt > all[j].CreatedAt
		}
		return all[i].ID > all[j].ID
	})
	return pageByLastID(all, limit, cursor, func(d WebhookDelivery) quarrycontracts.ID { return d.ID })
}

func (m *memWebhookDeliveries) DeleteTerminalBefore(cutoff int64) (int64, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	var removed int64
	kept := make([]quarrycontracts.ID, 0, len(m.order))
	for _, id := range m.order {
		it := m.items[id]
		if it.CreatedAt < cutoff && (it.Status == "success" || it.Status == "failed" || it.Status == "dlq") {
			delete(m.items, id)
			removed++
			continue
		}
		kept = append(kept, id)
	}
	m.order = kept
	return removed, nil
}

// pageByLastID pages a newest-first slice by the id of the last row of the previous page.
func pageByLastID[T any](all []T, limit int, cursor string, idOf func(T) quarrycontracts.ID) ([]T, string) {
	if limit <= 0 {
		limit = 50
	}
	start := 0
	if cursor != "" {
		start = len(all)
		for i, it := range all {
			if string(idOf(it)) == cursor {
				start = i + 1
				break
			}
		}
	}
	end := start + limit
	if end > len(all) {
		end = len(all)
	}
	out := append([]T(nil), all[start:end]...)
	next := ""
	if end < len(all) && len(out) > 0 {
		next = string(idOf(out[len(out)-1]))
	}
	return out, next
}

// memWebhooks is the in-memory WebhooksStore. Every tenant method enforces the org guard
// itself, because the pg store relies on its WHERE clause for the same invariant.
type memWebhooks struct{ *genericStore[Webhook] }

func sameEvents(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

func (m *memWebhooks) CreateCapped(w Webhook, maxPerOrg int) error {
	if w.OrgID == "" {
		return fmt.Errorf("store: CreateCapped requires an org")
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	if _, exists := m.items[w.ID]; exists {
		return ErrConflict
	}
	count := 0
	for _, it := range m.items {
		if it.OrgID != w.OrgID {
			continue
		}
		count++
		if it.URL == w.URL && sameEvents(it.Events, w.Events) {
			return ErrConflict
		}
	}
	if maxPerOrg > 0 && count >= maxPerOrg {
		return ErrQuotaExceeded
	}
	m.items[w.ID] = w
	m.order = append(m.order, w.ID)
	return nil
}

func (m *memWebhooks) GetByOrg(id quarrycontracts.ID, orgID string) (Webhook, bool) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	it, ok := m.items[id]
	if !ok || orgID == "" || it.OrgID != orgID {
		return Webhook{}, false
	}
	return it, true
}

func (m *memWebhooks) ListByOrg(orgID string, limit int, cursor string) ([]Webhook, string) {
	m.mu.RLock()
	var all []Webhook
	if orgID != "" {
		for _, it := range m.items {
			if it.OrgID == orgID {
				all = append(all, it)
			}
		}
	}
	m.mu.RUnlock()
	sort.Slice(all, func(i, j int) bool {
		if all[i].CreatedAt != all[j].CreatedAt {
			return all[i].CreatedAt > all[j].CreatedAt
		}
		return all[i].ID > all[j].ID
	})
	return pageByLastID(all, limit, cursor, func(w Webhook) quarrycontracts.ID { return w.ID })
}

func (m *memWebhooks) CountByOrg(orgID string) int {
	if orgID == "" {
		return 0
	}
	m.mu.RLock()
	defer m.mu.RUnlock()
	n := 0
	for _, it := range m.items {
		if it.OrgID == orgID {
			n++
		}
	}
	return n
}

func (m *memWebhooks) DeleteByOrg(id quarrycontracts.ID, orgID string) error {
	if _, ok := m.GetByOrg(id, orgID); !ok {
		return ErrNotFound
	}
	return m.genericStore.Delete(id)
}

func (m *memWebhooks) UpdateByOrg(w Webhook) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	it, ok := m.items[w.ID]
	if !ok || w.OrgID == "" || it.OrgID != w.OrgID {
		return ErrNotFound
	}
	it.Events, it.Active, it.Secret, it.SecretHint, it.Description = w.Events, w.Active, w.Secret, w.SecretHint, w.Description
	it.NotifyOnChange = w.NotifyOnChange
	m.items[w.ID] = it
	return nil
}

func (m *memWebhooks) ListActiveByOrg(orgID string, limit int) []Webhook {
	if orgID == "" {
		return nil
	}
	m.mu.RLock()
	defer m.mu.RUnlock()
	var out []Webhook
	for _, id := range m.order {
		it := m.items[id]
		if it.OrgID == orgID && it.Active {
			out = append(out, it)
			if limit > 0 && len(out) >= limit {
				break
			}
		}
	}
	return out
}

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

// AppendNext holds the log's write lock across the read of the max and the
// append, so concurrent writers of one run get distinct, ordered seqs.
func (l *memEventLog) AppendNext(evt quarrycontracts.Event) (uint64, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	for _, e := range l.events {
		if e.EventID == evt.EventID {
			// The Postgres store's primary key on event_id.
			return 0, ErrConflict
		}
	}
	evt.Seq = 1
	if evt.RunID != nil {
		var maxSeq uint64
		for _, e := range l.events {
			if e.RunID != nil && *e.RunID == *evt.RunID && e.Seq > maxSeq {
				maxSeq = e.Seq
			}
		}
		evt.Seq = maxSeq + 1
	}
	l.events = append(l.events, evt)
	return evt.Seq, nil
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

// ---- schedule requests (WS15.S3, migration 026) ----------------------------

// Schedule request actions and states. A trigger or backfill is recorded as a
// row that the orchestrator claims (pull model, X-9 / D14 option 2): control
// holds no Temporal client, so a request is durable before anything runs and an
// orchestrator restart between request and claim loses nothing.
const (
	ScheduleRequestTrigger  = "trigger"
	ScheduleRequestBackfill = "backfill"

	ScheduleRequestPending  = "pending"
	ScheduleRequestClaimed  = "claimed"
	ScheduleRequestDone     = "done"
	ScheduleRequestFailed   = "failed"
	ScheduleRequestRejected = "rejected"
)

// ScheduleRequest is one trigger or backfill of a schedule. Times are unix
// milliseconds. Params is {start_at, end_at, overlap_policy} for a backfill and
// empty for a trigger; Result is what the orchestrator reported (the workflow
// or schedule id). The row holds no page content.
type ScheduleRequest struct {
	ID             quarrycontracts.ID `json:"id"`
	ScheduleID     quarrycontracts.ID `json:"schedule_id"`
	OrgID          string             `json:"org_id"`
	Action         string             `json:"action"`
	Params         map[string]any     `json:"params,omitempty"`
	Status         string             `json:"status"`
	ClaimedBy      string             `json:"claimed_by,omitempty"`
	LeaseUntil     int64              `json:"lease_until,omitempty"`
	Attempts       int                `json:"attempts"`
	Result         map[string]any     `json:"result,omitempty"`
	Error          string             `json:"error,omitempty"`
	RequestedBy    string             `json:"requested_by,omitempty"`
	IdempotencyKey string             `json:"-"`
	CreatedAt      int64              `json:"created_at"`
	FinishedAt     *int64             `json:"finished_at,omitempty"`
}

// ScheduleRequestTerminal reports whether status ends a request.
func ScheduleRequestTerminal(status string) bool {
	switch status {
	case ScheduleRequestDone, ScheduleRequestFailed, ScheduleRequestRejected:
		return true
	}
	return false
}

// ScheduleRequestsStore is the request table. Create refuses a second open
// (pending or claimed) request of the same schedule and a repeated
// (org, idempotency key) with ErrConflict. Claim hands each claimable row to one
// worker: a pending row, or a claimed row whose lease ran out (the worker died).
// Complete, Fail and Release act only on a row the named worker still holds;
// anything else is ErrConflict, an unknown id ErrNotFound.
type ScheduleRequestsStore interface {
	Create(req ScheduleRequest) error
	// GetByOrg is the tenant read: a request of another org is not found.
	GetByOrg(id quarrycontracts.ID, orgID string) (ScheduleRequest, bool)
	// HasPending reports an open request of the schedule. action "" matches any.
	HasPending(scheduleID quarrycontracts.ID, action string) bool
	Claim(worker string, limit int, lease time.Duration, now time.Time) ([]ScheduleRequest, error)
	Complete(id quarrycontracts.ID, worker string, result map[string]any, now time.Time) error
	// Fail ends a held request as failed or rejected with a short reason.
	Fail(id quarrycontracts.ID, worker, status, reason string, now time.Time) error
	// Release gives a held request back to the queue (a transient error); the
	// attempt stays counted and the reason is kept as the last error.
	Release(id quarrycontracts.ID, worker, reason string) error
}

// ScheduleRequestsProvider is implemented by a DB that has the request table.
// It is optional, like RequestQueueReader, so a DB without it answers 501
// instead of failing to compile; both backends here implement it.
type ScheduleRequestsProvider interface {
	ScheduleRequests() ScheduleRequestsStore
}

// ScheduleRequestReasonMax bounds the stored error text.
const ScheduleRequestReasonMax = 256

// ClipScheduleRequestReason trims a reason to ScheduleRequestReasonMax bytes.
func ClipScheduleRequestReason(s string) string {
	if len(s) > ScheduleRequestReasonMax {
		return s[:ScheduleRequestReasonMax]
	}
	return s
}

type memScheduleRequests struct {
	mu    sync.Mutex
	items map[quarrycontracts.ID]ScheduleRequest
	order []quarrycontracts.ID
}

func newMemScheduleRequests() *memScheduleRequests {
	return &memScheduleRequests{items: map[quarrycontracts.ID]ScheduleRequest{}}
}

func (d *memDB) ScheduleRequests() ScheduleRequestsStore { return d.schedReqs }

func copyAnyMap(in map[string]any) map[string]any {
	if in == nil {
		return nil
	}
	out := make(map[string]any, len(in))
	for k, v := range in {
		out[k] = v
	}
	return out
}

func cloneScheduleRequest(r ScheduleRequest) ScheduleRequest {
	r.Params = copyAnyMap(r.Params)
	r.Result = copyAnyMap(r.Result)
	if r.FinishedAt != nil {
		v := *r.FinishedAt
		r.FinishedAt = &v
	}
	return r
}

func (m *memScheduleRequests) Create(req ScheduleRequest) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	if _, dup := m.items[req.ID]; dup {
		return ErrConflict
	}
	for _, id := range m.order {
		it := m.items[id]
		if it.ScheduleID == req.ScheduleID && !ScheduleRequestTerminal(it.Status) {
			return ErrConflict
		}
		if req.IdempotencyKey != "" && it.OrgID == req.OrgID && it.IdempotencyKey == req.IdempotencyKey {
			return ErrConflict
		}
	}
	if req.Status == "" {
		req.Status = ScheduleRequestPending
	}
	m.items[req.ID] = cloneScheduleRequest(req)
	m.order = append(m.order, req.ID)
	return nil
}

func (m *memScheduleRequests) GetByOrg(id quarrycontracts.ID, orgID string) (ScheduleRequest, bool) {
	m.mu.Lock()
	defer m.mu.Unlock()
	it, ok := m.items[id]
	if !ok || orgID == "" || it.OrgID != orgID {
		return ScheduleRequest{}, false
	}
	return cloneScheduleRequest(it), true
}

func (m *memScheduleRequests) HasPending(scheduleID quarrycontracts.ID, action string) bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	for _, id := range m.order {
		it := m.items[id]
		if it.ScheduleID != scheduleID || ScheduleRequestTerminal(it.Status) {
			continue
		}
		if action == "" || it.Action == action {
			return true
		}
	}
	return false
}

func (m *memScheduleRequests) Claim(worker string, limit int, lease time.Duration, now time.Time) ([]ScheduleRequest, error) {
	if worker == "" {
		return nil, fmt.Errorf("claim: worker required")
	}
	if limit <= 0 {
		limit = 1
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	nowMs := now.UnixMilli()
	var out []ScheduleRequest
	// Oldest first: order is insertion order, which is created_at order.
	for _, id := range m.order {
		if len(out) >= limit {
			break
		}
		it := m.items[id]
		claimable := it.Status == ScheduleRequestPending ||
			(it.Status == ScheduleRequestClaimed && it.LeaseUntil < nowMs)
		if !claimable {
			continue
		}
		it.Status = ScheduleRequestClaimed
		it.ClaimedBy = worker
		it.LeaseUntil = now.Add(lease).UnixMilli()
		it.Attempts++
		m.items[id] = it
		out = append(out, cloneScheduleRequest(it))
	}
	return out, nil
}

// held returns the row when worker still holds it.
func (m *memScheduleRequests) held(id quarrycontracts.ID, worker string) (ScheduleRequest, error) {
	it, ok := m.items[id]
	if !ok {
		return ScheduleRequest{}, ErrNotFound
	}
	if it.Status != ScheduleRequestClaimed || it.ClaimedBy != worker {
		return ScheduleRequest{}, ErrConflict
	}
	return it, nil
}

func (m *memScheduleRequests) Complete(id quarrycontracts.ID, worker string, result map[string]any, now time.Time) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	it, err := m.held(id, worker)
	if err != nil {
		return err
	}
	fin := now.UnixMilli()
	it.Status = ScheduleRequestDone
	it.Result = copyAnyMap(result)
	it.Error = ""
	it.LeaseUntil = 0
	it.FinishedAt = &fin
	m.items[id] = it
	return nil
}

func (m *memScheduleRequests) Fail(id quarrycontracts.ID, worker, status, reason string, now time.Time) error {
	if status != ScheduleRequestFailed && status != ScheduleRequestRejected {
		return fmt.Errorf("fail: status must be failed or rejected")
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	it, err := m.held(id, worker)
	if err != nil {
		return err
	}
	fin := now.UnixMilli()
	it.Status = status
	it.Error = ClipScheduleRequestReason(reason)
	it.LeaseUntil = 0
	it.FinishedAt = &fin
	m.items[id] = it
	return nil
}

func (m *memScheduleRequests) Release(id quarrycontracts.ID, worker, reason string) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	it, err := m.held(id, worker)
	if err != nil {
		return err
	}
	it.Status = ScheduleRequestPending
	it.ClaimedBy = ""
	it.LeaseUntil = 0
	it.Error = ClipScheduleRequestReason(reason)
	m.items[id] = it
	return nil
}
