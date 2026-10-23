// ZDR run tombstones (WS4.J): a metadata-only record that a run happened under
// Zero Data Retention. See migrations/015_zdr_run_tombstones.sql for the table and
// crates/quarry-runtime/contracts/zdr-tombstone.schema.json for the allowlist.
//
// A tombstone holds ids, closed-vocabulary classes, counts and timestamps and
// nothing else. Validate is the one gate every writer goes through (the HTTP
// handler and both stores), and the Postgres CHECK constraints repeat it, so a
// value outside the vocabulary cannot be stored by any path.
//
// OFF BY DEFAULT: nothing writes a tombstone unless the edge's
// QUARRY_EDGE__ZDR_TOMBSTONE is on, and counsel review gates turning it on.
package store

import (
	"errors"
	"fmt"
	"math"
	"regexp"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

// Closed vocabularies. The same lists are in the schema file, in the migration's
// CHECK constraints and in the edge's serializer; tests compare all four.
var (
	ZdrTombstoneKinds    = []string{"scrape", "search", "answer", "extract", "agent_run"}
	ZdrTombstoneOutcomes = []string{"ok", "error", "refused", "cancelled", "timeout"}
	// ZdrTombstoneDrivers are quarry_core::output::DriverKind in snake_case.
	ZdrTombstoneDrivers = []string{"static", "browser", "tls"}
	// ZdrTombstoneErrorClasses are the labels the edge's zdr_log::class (an
	// ErrorCode wire name) and zdr_log::class_reqwest produce: never an error text.
	ZdrTombstoneErrorClasses = []string{
		"BAD_REQUEST", "UNAUTHORIZED", "FORBIDDEN", "NOT_FOUND", "CONFLICT",
		"RATE_LIMITED", "TIMEOUT", "SECURITY_BLOCKED", "ACTION_UNKNOWN",
		"CHALLENGE_DETECTED", "RUNTIME_NOT_READY", "CHECKPOINT_LOST",
		"TARGET_REPAIR_REQUIRED", "DRIVER_FAILED", "UPSTREAM_BLOCKED",
		"UNSUPPORTED", "UNSUPPORTED_MEDIA", "BUDGET_EXHAUSTED", "USER_TOOK_OVER",
		"INTERNAL", "timeout", "connect", "status", "decode", "request",
	}
)

// ZdrTombstoneRetentionClass is the only retention class a tombstone can carry.
const ZdrTombstoneRetentionClass = "zdr"

// maxZdrTombstoneCost bounds cost_units to what NUMERIC(18,6) holds.
const maxZdrTombstoneCost = 1e11

var zdrTombstoneID = regexp.MustCompile(`^[A-Za-z0-9_-]{1,64}$`)

// ErrZdrTombstoneInvalid is returned (wrapped) by Validate.
var ErrZdrTombstoneInvalid = errors.New("zdr tombstone: value outside the allowlist")

// ZdrTombstone is one row of zdr_run_tombstones. Optional fields are pointers so
// "absent" is stored as NULL, never as a made-up zero.
type ZdrTombstone struct {
	RunID          string
	OrgID          string
	Kind           string
	RetentionClass string
	Outcome        string
	Driver         *string
	StartedAt      *time.Time
	EndedAt        time.Time
	StepCount      *int
	CostUnits      *float64
	ErrorClass     *string
	CreatedAt      time.Time
}

// Validate refuses any value outside the allowlist. The error names the field,
// never the value (a value could be what the allowlist exists to keep out).
func (t ZdrTombstone) Validate() error {
	bad := func(field string) error { return fmt.Errorf("%w: %s", ErrZdrTombstoneInvalid, field) }
	if !zdrTombstoneID.MatchString(t.RunID) {
		return bad("run_id")
	}
	if !zdrTombstoneID.MatchString(t.OrgID) {
		return bad("org_id")
	}
	if !inList(ZdrTombstoneKinds, t.Kind) {
		return bad("kind")
	}
	if t.RetentionClass != ZdrTombstoneRetentionClass {
		return bad("retention_class")
	}
	if !inList(ZdrTombstoneOutcomes, t.Outcome) {
		return bad("outcome")
	}
	if t.Driver != nil && !inList(ZdrTombstoneDrivers, *t.Driver) {
		return bad("driver")
	}
	if t.EndedAt.IsZero() {
		return bad("ended_at")
	}
	if t.StartedAt != nil && t.StartedAt.After(t.EndedAt) {
		return bad("started_at")
	}
	if t.StepCount != nil && *t.StepCount < 0 {
		return bad("step_count")
	}
	if t.CostUnits != nil && (math.IsNaN(*t.CostUnits) || math.IsInf(*t.CostUnits, 0) || *t.CostUnits < 0 || *t.CostUnits >= maxZdrTombstoneCost) {
		return bad("cost_units")
	}
	if t.ErrorClass != nil && !inList(ZdrTombstoneErrorClasses, *t.ErrorClass) {
		return bad("error_class")
	}
	return nil
}

func inList(list []string, v string) bool {
	for _, item := range list {
		if item == v {
			return true
		}
	}
	return false
}

// ZdrTombstoneStore is the tombstone table. There is no update method and no
// read-by-run-id: a tombstone is written once, listed per org by operators'
// tooling (store-only, no HTTP route reads it), and aged out.
type ZdrTombstoneStore interface {
	// Put inserts the tombstone. A second Put for the same run_id is a no-op that
	// reports inserted=false (first write wins), so a retry never errors and a
	// caller learns nothing about whether a run id already existed. The same
	// no-op answer covers an org that was erased (PurgeOrg leaves a marker): a run
	// that ends after its org's erasure must not re-create a row for that org.
	Put(t ZdrTombstone) (inserted bool, err error)
	// SetErasureMarkers turns the write of an erasure marker by PurgeOrg on or off.
	// It is OFF until something turns it on: a marker keeps an erased org's id for at
	// least MinErasureMarkerDays, which is a new retention of an identifier after a
	// GDPR erasure and is therefore part of what counsel review gates. cmd/control
	// turns it on only together with the tombstone receiver
	// (QUARRY_ZDR_TOMBSTONE_RECEIVER).
	SetErasureMarkers(on bool)
	// DeleteErasureMarkersBefore removes the erasure markers older than cutoff
	// (the sweeper passes a cutoff at least MinErasureMarkerDays old) and returns
	// how many it removed.
	DeleteErasureMarkersBefore(cutoff time.Time) (int64, error)
	// DeleteBefore removes up to limit tombstones whose ended_at is before cutoff,
	// oldest first, and returns how many it removed. limit <= 0 removes nothing.
	DeleteBefore(cutoff time.Time, limit int) (int64, error)
	// CountBefore reports how many tombstones DeleteBefore(cutoff, ...) would remove
	// in total (the sweeper's dry run).
	CountBefore(cutoff time.Time) (int64, error)
	// ListByOrg returns the org's tombstones, newest ended_at first, at most limit.
	ListByOrg(orgID string, limit int) ([]ZdrTombstone, error)
}

// memZdrTombstones is the in-memory backend (dev and tests).
type memZdrTombstones struct {
	mu     sync.Mutex
	items  map[string]ZdrTombstone
	erased map[string]time.Time // org id -> when PurgeOrg ran
	// markers is the SetErasureMarkers switch; false means PurgeOrg writes none.
	markers atomic.Bool
}

func (m *memZdrTombstones) SetErasureMarkers(on bool) { m.markers.Store(on) }

func newMemZdrTombstones() *memZdrTombstones {
	return &memZdrTombstones{items: map[string]ZdrTombstone{}, erased: map[string]time.Time{}}
}

// MinErasureMarkerDays is the least time an erasure marker is kept, however short
// the tombstone retention is: long past any run that was in flight at erasure.
const MinErasureMarkerDays = 7

func (m *memZdrTombstones) Put(t ZdrTombstone) (bool, error) {
	if err := t.Validate(); err != nil {
		return false, err
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	if _, exists := m.items[t.RunID]; exists {
		return false, nil
	}
	if _, erased := m.erased[t.OrgID]; erased {
		return false, nil
	}
	if t.CreatedAt.IsZero() {
		t.CreatedAt = time.Now().UTC()
	}
	m.items[t.RunID] = t
	return true, nil
}

func (m *memZdrTombstones) DeleteBefore(cutoff time.Time, limit int) (int64, error) {
	if limit <= 0 {
		return 0, nil
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	var due []ZdrTombstone
	for _, t := range m.items {
		if t.EndedAt.Before(cutoff) {
			due = append(due, t)
		}
	}
	sort.Slice(due, func(i, j int) bool { return due[i].EndedAt.Before(due[j].EndedAt) })
	if len(due) > limit {
		due = due[:limit]
	}
	for _, t := range due {
		delete(m.items, t.RunID)
	}
	return int64(len(due)), nil
}

func (m *memZdrTombstones) CountBefore(cutoff time.Time) (int64, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	var n int64
	for _, t := range m.items {
		if t.EndedAt.Before(cutoff) {
			n++
		}
	}
	return n, nil
}

func (m *memZdrTombstones) ListByOrg(orgID string, limit int) ([]ZdrTombstone, error) {
	if strings.TrimSpace(orgID) == "" || limit <= 0 {
		return nil, nil
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []ZdrTombstone
	for _, t := range m.items {
		if t.OrgID == orgID {
			out = append(out, t)
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].EndedAt.After(out[j].EndedAt) })
	if len(out) > limit {
		out = out[:limit]
	}
	return out, nil
}

func (m *memZdrTombstones) DeleteErasureMarkersBefore(cutoff time.Time) (int64, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	var n int64
	for org, at := range m.erased {
		if at.Before(cutoff) {
			delete(m.erased, org)
			n++
		}
	}
	return n, nil
}

// purgeByOrg is the org-erasure path: it removes every tombstone of the org and
// leaves a marker so a later Put for the org is refused.
func (m *memZdrTombstones) purgeByOrg(orgID string) int64 {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.markers.Load() && zdrTombstoneID.MatchString(orgID) {
		m.erased[orgID] = time.Now().UTC()
	}
	var removed int64
	for id, t := range m.items {
		if t.OrgID == orgID {
			delete(m.items, id)
			removed++
		}
	}
	return removed
}
