// GDPR org-erasure hard purge — the counterpart to SourcesStore's
// SoftDeleteByOrg (soft, reversible, single-row) at the opposite end of the
// org lifecycle: PurgeOrg permanently removes every row this service holds
// for one org, across every table that actually carries an org_id column.
//
// Scope (enumerated against the real schema in internal/store/pg/migrations,
// not assumed from the DB interface's shape):
//   - jobs                    org_id NOT NULL (010_jobs_org_id.sql) — the
//     crawl/scrape/search/etc. runs themselves.
//   - schedules                org_id NOT NULL (008_schedules_org_id.sql).
//   - schedule_requests        org_id NOT NULL (026_schedule_requests.sql,
//     WS15.S3b) — trigger/backfill request rows; no FK to schedules, so
//     purged by their own org_id before the schedules.
//   - quarry_sources           org_id NOT NULL (005_cycle23.sql) — Sources().
//   - quarry_benchmarks        org_id NOT NULL (005_cycle23.sql) — schema
//     exists as a cycle-28 placeholder; no Go code writes rows yet. Included
//     so a future write path never leaves erasure with a silent gap.
//   - quarry_idempotency_keys  org_id NOT NULL, part of the PK
//     (005_cycle23.sql) — response_body can carry org-specific cached
//     payload bytes. Same placeholder status as quarry_benchmarks; included
//     for the same reason.
//   - zdr_run_tombstones       org_id NOT NULL (015_zdr_run_tombstones.sql,
//     WS4.J) — the metadata-only ZDR run records. Included so erasure of an
//     org leaves no trace that it ran under ZDR either.
//
// Deliberately NOT touched — none of these carry an org_id column in the
// actual schema, so a `WHERE org_id = $1` predicate against them would
// either match nothing or require an indirect join via run_id/job_id that
// this package will not risk introducing (the safety rule for every purge
// statement is a direct, literal org_id column, never a derived subject
// set): stores (datasets), snapshots, artifacts, profiles, blocklist_entries,
// events. In the current schema these are shared/global resources, not
// per-tenant data.
//
// webhooks and webhook_deliveries ARE org data since migration 025 (INNH-5, org_id NOT
// NULL DEFAULT ”): a subscription holds a customer URL and a secret, a delivery holds the
// whole event payload (page URLs, fingerprints). Both are purged by org_id. A legacy row
// with org_id ” belongs to no org and is never matched here.
//
// The Postgres implementation (internal/store/pg/gdpr_purge.go) mirrors this
// exact table list.
package store

import (
	"fmt"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

// PurgeResult reports how many rows PurgeOrg hard-deleted from each
// org-scoped table. Every field is a plain row count — safe to log, safe to
// sum via Total, and (because every underlying statement is a
// `DELETE ... WHERE org_id = $1`) safe to compare across a redelivered or
// duplicate call: a second PurgeOrg for the same org returns an all-zero
// PurgeResult, not an error.
type PurgeResult struct {
	JobsDeleted            int64 `json:"jobs_deleted"`
	SchedulesDeleted       int64 `json:"schedules_deleted"`
	SourcesDeleted         int64 `json:"sources_deleted"`
	BenchmarksDeleted      int64 `json:"benchmarks_deleted"`
	IdempotencyKeysDeleted int64 `json:"idempotency_keys_deleted"`
	ZdrTombstonesDeleted   int64 `json:"zdr_tombstones_deleted"`
	// WebhooksDeleted and WebhookDeliveriesDeleted: the org's webhook subscriptions and
	// their delivery rows, which hold full event payloads (INNH-5, migration 025).
	WebhooksDeleted          int64 `json:"webhooks_deleted"`
	WebhookDeliveriesDeleted int64 `json:"webhook_deliveries_deleted"`
	// ScheduleRequestsDeleted: the org's trigger/backfill request rows (WS15.S3b,
	// migration 026). They hold ids, window times and short error text only.
	ScheduleRequestsDeleted int64 `json:"schedule_requests_deleted"`
}

// Total sums every deleted-row count. Convenience for logging and tests.
func (r PurgeResult) Total() int64 {
	return r.JobsDeleted + r.SchedulesDeleted + r.SourcesDeleted + r.BenchmarksDeleted + r.IdempotencyKeysDeleted + r.ZdrTombstonesDeleted +
		r.WebhooksDeleted + r.WebhookDeliveriesDeleted + r.ScheduleRequestsDeleted
}

// PurgeOrg hard-deletes every row this in-memory store holds for orgID
// across jobs, schedules, and sources — the three org-scoped resources the
// dev/in-memory backend actually models. quarry_benchmarks and
// quarry_idempotency_keys are Postgres-only placeholder tables with no
// in-memory representation (see the package doc above), so they never
// contribute a nonzero count here; that is correct, not a gap. Safe to call
// twice: an orgID with nothing left to delete returns a zero PurgeResult,
// never an error.
func (d *memDB) PurgeOrg(orgID string) (PurgeResult, error) {
	if orgID == "" {
		return PurgeResult{}, fmt.Errorf("store: PurgeOrg requires a non-empty orgID")
	}
	jobsDeleted := d.jobs.purgeByOrg(orgID, func(j Job) string { return j.OrgID })
	requestsDeleted := d.schedReqs.purgeByOrg(orgID)
	schedulesDeleted := d.schedules.purgeByOrg(orgID, func(s Schedule) string { return s.OrgID })
	sourcesDeleted := d.sources.PurgeByOrg(orgID)
	tombstonesDeleted := d.zdrTombs.purgeByOrg(orgID)
	deliveriesDeleted := d.whDeliveries.purgeByOrg(orgID, func(w WebhookDelivery) string { return w.OrgID })
	webhooksDeleted := d.webhooks.purgeByOrg(orgID, func(w Webhook) string { return w.OrgID })
	return PurgeResult{
		JobsDeleted:              int64(jobsDeleted),
		SchedulesDeleted:         int64(schedulesDeleted),
		SourcesDeleted:           int64(sourcesDeleted),
		ZdrTombstonesDeleted:     tombstonesDeleted,
		WebhooksDeleted:          int64(webhooksDeleted),
		WebhookDeliveriesDeleted: int64(deliveriesDeleted),
		ScheduleRequestsDeleted:  int64(requestsDeleted),
	}, nil
}

// purgeByOrg permanently removes every schedule request row of orgID, open or
// finished (WS15.S3b). Matches the literal OrgID only; safe to call twice.
func (m *memScheduleRequests) purgeByOrg(orgID string) int {
	if m == nil || orgID == "" {
		return 0
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	var removed int
	kept := make([]quarrycontracts.ID, 0, len(m.order))
	for _, id := range m.order {
		if m.items[id].OrgID == orgID {
			delete(m.items, id)
			removed++
			continue
		}
		kept = append(kept, id)
	}
	m.order = kept
	return removed
}

// purgeByOrg removes every item for which orgOf(item) equals orgID,
// returning the count removed. Generic over genericStore[T] so Jobs and
// Schedules — the two genericStore-backed resources that carry an OrgID
// field — share one implementation instead of two copy-pasted loops.
func (s *genericStore[T]) purgeByOrg(orgID string, orgOf func(T) string) int {
	s.mu.Lock()
	defer s.mu.Unlock()
	var removed int
	kept := make([]quarrycontracts.ID, 0, len(s.order))
	for _, id := range s.order {
		if orgOf(s.items[id]) == orgID {
			delete(s.items, id)
			removed++
			continue
		}
		kept = append(kept, id)
	}
	s.order = kept
	return removed
}

// PurgeByOrg permanently removes every source belonging to orgID —
// including rows already soft-deleted by SoftDeleteByOrg, so a GDPR
// erasure closes out a soft-delete tombstone too, not just live rows. Safe
// to call twice: nothing left to delete returns 0.
func (m *memSources) PurgeByOrg(orgID string) int {
	m.mu.Lock()
	defer m.mu.Unlock()
	var removed int
	kept := make([]quarrycontracts.ID, 0, len(m.order))
	for _, id := range m.order {
		if m.items[id].OrgID == orgID {
			delete(m.items, id)
			delete(m.deleted, id)
			removed++
			continue
		}
		kept = append(kept, id)
	}
	m.order = kept
	return removed
}
