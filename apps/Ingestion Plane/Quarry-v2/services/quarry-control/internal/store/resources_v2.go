// Org-scoped read models backing the edge-forwarded list families that
// cycle 23 left stubbed. Every interface here follows the same convention
// as SourcesStore: the org id arrives from the edge-verified `?org_id`
// query param (stamped from the JWT), every method filters on it, and a
// cross-tenant id is indistinguishable from a missing one.
//
// Wire shapes these stores project into live in resources/cycle24.go and
// are pinned against Rust serde output by pkg/quarrycontracts tests.
package store

import (
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

// ListFilter is the common filter model shared by every list family in
// this file. It mirrors the query string quarry-edge's forward_list sends
// (status / created_before / created_after / limit / cursor / sort) so
// handlers translate 1:1 without per-family parsing drift.
//
//   - Status is free-form: each resource owns its taxonomy, exactly like
//     quarry_core::pagination::StatusFilter.
//   - CreatedBefore/CreatedAfter are inclusive RFC3339 bounds on the
//     resource's creation timestamp.
//   - Cursor is the opaque base64 "<millis>|<id>" keyset token shared with
//     the jobs implementation; malformed cursors start from the top (the
//     same forgiving decode the pg helpers use).
//   - Sort descending = newest-first (the default); ascending = oldest.
type ListFilter struct {
	Status         string
	CreatedBefore  *time.Time
	CreatedAfter   *time.Time
	Limit          int
	Cursor         string
	SortDescending bool // true when sort ∈ {newest, desc} or omitted
}

// TeamCreditUsageRow is one org's credit usage for a period window,
// aggregated over the event log. Mirrors
// quarry_core::resources::TeamCreditUsage field-for-field via the JSON
// tags in resources/cycle24.go.
type TeamCreditUsageRow struct {
	CreditsUsed        float64
	CreditsLimit       *float64
	UtilizationPercent float64
}

// TeamTokenUsageRow aggregates input/output tokens over the event log.
// TotalTokens is derived (input + output), never stored separately.
type TeamTokenUsageRow struct {
	InputTokens  uint64
	OutputTokens uint64
	TotalTokens  uint64
	CostMicroUSD *int64
}

// HostConcurrencyRow is the per-host detail row of a concurrency snapshot.
type HostConcurrencyRow struct {
	Host          string
	Current       uint32
	Ceiling       uint32
	EWMALatencyMs *float64
}

// TeamConcurrencyRow is the per-org in-flight snapshot across hosts.
type TeamConcurrencyRow struct {
	Current uint32
	Ceiling uint32
	ByHost  []HostConcurrencyRow
}

// QueueStatusEntryRow is the per-queue detail row of a queue-status snapshot.
type QueueStatusEntryRow struct {
	QueueID  string
	Name     string
	Queued   uint64
	InFlight uint64
}

// TeamQueueStatusRow aggregates queued/in-flight counts across an org's queues.
type TeamQueueStatusRow struct {
	QueuedTotal   uint64
	InFlightTotal uint64
	ByQueue       []QueueStatusEntryRow
}

// RequestQueueSummaryV2 is the control-plane read model for one Rust-owned
// durable queue, upgraded to the full quarry_core::resources::
// RequestQueueSummary wire shape. CreatedAt stays unix-millis internally
// (store convention); the handler renders RFC3339.
type RequestQueueSummaryV2 struct {
	QueueID   string
	Name      string
	Kind      string // crawl | batch | scrape | search (informational)
	Status    string // active | draining | deleted
	CreatedAt int64  // unix millis
	Stats     RequestQueueStatsRow
}

// RequestQueueStatsRow carries the four frontier counters the Rust
// RequestQueueStats struct requires on every decode. Acked/Failed live in
// dedicated columns; Queued/InFlight derive from frontier tables at read time.
type RequestQueueStatsRow struct {
	Queued   uint64
	InFlight uint64
	Acked    uint64
	Failed   uint64
}

// ActivityEntry is one compact event-feed row projected into
// quarry_core::resources::TeamActivityEntry by the handler.
type ActivityEntry struct {
	EventID   quarrycontracts.ID
	EventType string
	RunID     *quarrycontracts.ID
	Ts        time.Time
	Summary   string
}

// TeamUsageStore serves the four /v1/team/* aggregate endpoints plus the
// activity feed for one org. All windows are computed by the store from
// the period token; unknown periods degrade to "7d" at the handler layer.
//
// The in-memory implementation derives everything from its event log and
// job set; the Postgres implementation runs SUM/GROUP BY queries against
// the events/jobs tables. Neither fabricates numbers: an org with no rows
// gets honest zeros.
type TeamUsageStore interface {
	CreditUsage(orgID, period string) (TeamCreditUsageRow, error)
	TokenUsage(orgID, period string) (TeamTokenUsageRow, error)
	Concurrency(orgID string) (TeamConcurrencyRow, error)
	QueueStatus(orgID string) (TeamQueueStatusRow, error)
	Activity(orgID string, f ListFilter) ([]ActivityEntry, string)
}

// SnapshotV2 is the durable change-tracking record behind GET /v1/snapshots.
//
// It mirrors quarry_core::resources::Snapshot (crates/quarry-core/src/
// resources.rs) field-for-field — including the prefixed-ULID id kinds
// (snap_/src_/art_) whose Rust FromStr validates prefix AND ULID shape, so
// mint ids with KindSnapshot/KindSource/KindArtifact only. CreatedAt stays
// unix-millis internally (store convention); the handler renders RFC3339.
//
// This coexists with the legacy ResourceStore[Snapshot] ({id,run_id,bucket})
// used by MountRestore: different table (quarry_snapshots_v2), different
// producer (change tracking). No producer of the legacy wire shape exists
// today, but its consumer (restore.go) keeps working untouched.
type SnapshotV2 struct {
	ID              quarrycontracts.ID // snap_<ulid>
	OrgID           string
	SourceID        *quarrycontracts.ID // src_<ulid>, nil for standalone scrapes
	URL             string
	Fingerprint     string
	PrevFingerprint *string // nil for the first version of the URL
	ChangeStatus    string  // unchanged | modified | new
	CreatedAt       int64   // unix millis; rendered as captured_at RFC3339 on the wire
	ArtifactID      *quarrycontracts.ID // art_<ulid>
}

// SnapshotsV2Store is the org-scoped read/write surface for
// quarry_snapshots_v2. ListByOrg returns newest-first with the standard
// opaque keyset cursor, honoring ListFilter's status / created_before /
// created_after knobs (status matches ChangeStatus).
type SnapshotsV2Store interface {
	Create(s SnapshotV2) error
	GetByOrg(orgID string, id quarrycontracts.ID) (SnapshotV2, bool)
	ListByOrg(orgID string, f ListFilter) ([]SnapshotV2, string)
	Delete(id quarrycontracts.ID) error
}
