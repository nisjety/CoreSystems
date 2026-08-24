// In-memory (dev) implementations of the cycle-24 store surfaces. The
// aggregates derive honestly from what this ephemeral process has actually
// seen — event log for usage/activity, live job set for concurrency —
// rather than returning fabricated numbers behind a real-looking shape.
package store

import (
	"encoding/base64"
	"fmt"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

// ---- shared list cursors ---------------------------------------------------
//
// Byte-identical encoding to the pg package's encodeCursor/decodeCursor
// (base64 raw-URL of "<millis>|<id>") so a cursor minted by either backend
// is valid on both and the wire contract stays stable across dev/prod.
func encodeListCursor(createdAt int64, id string) string {
	return base64.RawURLEncoding.EncodeToString(
		[]byte(fmt.Sprintf("%d|%s", createdAt, id)),
	)
}

// decodeListCursor is the forgiving decoder: missing OR malformed cursors
// start from the top instead of erroring (ListFilter documents this as
// "bad cursor = fresh result window").
func decodeListCursor(s string) (createdAt int64, id string, ok bool) {
	if s == "" {
		return 0, "", false
	}
	raw, err := base64.RawURLEncoding.DecodeString(s)
	if err != nil {
		return 0, "", false
	}
	parts := strings.SplitN(string(raw), "|", 2)
	if len(parts) != 2 {
		return 0, "", false
	}
	var ts int64
	if _, err := fmt.Sscan(parts[0], &ts); err != nil {
		return 0, "", false
	}
	return ts, parts[1], true
}

// creditsPerPage is the dev-mode credit price of one fetched page,
// mirroring quarry_core::credits::CREDIT_COST_PER_PAGE (the same constant
// the Rust side prices /v1/crawl budget checks with).
const creditsPerPage = 1.0

// periodWindow resolves a free-form period token ("today", "7d", "30d",
// "YYYY-MM-DD") to an inclusive start bound. Unknown tokens degrade to 7d,
// mirroring the edge's documented contract ("backends interpret unknown
// values as 7d").
func periodWindow(period string, now time.Time) time.Time {
	switch strings.TrimSpace(period) {
	case "today":
		y, m, d := now.Date()
		return time.Date(y, m, d, 0, 0, 0, 0, now.Location())
	case "30d":
		return now.AddDate(0, 0, -30)
	case "7d":
		fallthrough
	default:
		if day, err := time.ParseInLocation("2006-01-02", strings.TrimSpace(period), now.Location()); err == nil {
			return day
		}
		return now.AddDate(0, 0, -7)
	}
}

// eventsForOrg returns the events whose owning job (resolved via job_id →
// Job.OrgID, or run_id → the run's owning job when no job id was stamped)
// belongs to orgID. Events with no resolvable owner are skipped: they carry
// no tenant attribution, so attributing them would be fabrication.
func (d *memDB) eventsForOrg(orgID string) []quarrycontracts.Event {
	d.mu.RLock()
	defer d.mu.RUnlock()
	orgByJob := make(map[quarrycontracts.ID]string, len(d.jobs.items))
	orgByRun := make(map[quarrycontracts.ID]string)
	for _, j := range d.jobs.items {
		orgByJob[j.ID] = j.OrgID
		if j.RunID != nil {
			orgByRun[*j.RunID] = j.OrgID
		}
	}
	out := make([]quarrycontracts.Event, 0, len(d.events.events))
	for _, e := range d.events.events {
		var owner string
		switch {
		case e.JobID != nil:
			owner = orgByJob[*e.JobID]
		case e.RunID != nil:
			owner = orgByRun[*e.RunID]
		}
		if owner != "" && owner == orgID {
			out = append(out, e)
		}
	}
	return out
}

func (d *memDB) TeamUsage() TeamUsageStore { return &memTeamUsage{db: d} }

type memTeamUsage struct{ db *memDB }

func (m *memTeamUsage) CreditUsage(orgID, period string) (TeamCreditUsageRow, error) {
	since := periodWindow(period, time.Now())
	var used float64
	for _, e := range m.db.eventsForOrg(orgID) {
		if e.Timestamp.Before(since) {
			continue
		}
		if pages, ok := e.Payload["pages"].(float64); ok {
			used += pages * creditsPerPage
		} else if e.Type == quarrycontracts.EvtPageFetched {
			used += creditsPerPage
		}
	}
	// No ceiling is modeled in-memory (Control Plane owns entitlements),
	// so CreditsLimit stays nil — "uncapped" — and utilization stays 0.
	return TeamCreditUsageRow{CreditsUsed: used}, nil
}

func (m *memTeamUsage) TokenUsage(orgID, period string) (TeamTokenUsageRow, error) {
	since := periodWindow(period, time.Now())
	var row TeamTokenUsageRow
	for _, e := range m.db.eventsForOrg(orgID) {
		if e.Timestamp.Before(since) {
			continue
		}
		u, ok := e.Payload["usage"].(map[string]any)
		if !ok {
			continue
		}
		in, _ := u["input_tokens"].(float64)
		out, _ := u["output_tokens"].(float64)
		row.InputTokens += uint64(in)
		row.OutputTokens += uint64(out)
	}
	row.TotalTokens = row.InputTokens + row.OutputTokens
	return row, nil
}

func (m *memTeamUsage) Concurrency(orgID string) (TeamConcurrencyRow, error) {
	// Live in-flight count: jobs this org has in "running" state right now,
	// bucketed per host derived from the job's params.url.
	var row TeamConcurrencyRow
	m.db.mu.RLock()
	jobs := make([]Job, 0, len(m.db.jobs.items))
	for _, j := range m.db.jobs.items {
		jobs = append(jobs, j)
	}
	m.db.mu.RUnlock()

	byHost := map[string]uint32{}
	for _, j := range jobs {
		if j.OrgID != orgID || j.Status != "running" {
			continue
		}
		row.Current++
		if host := hostOfJob(j); host != "" {
			byHost[host]++
		}
	}
	for h, c := range byHost {
		row.ByHost = append(row.ByHost, HostConcurrencyRow{Host: h, Current: c})
	}
	sort.Slice(row.ByHost, func(i, k int) bool { return row.ByHost[i].Current > row.ByHost[k].Current })
	return row, nil
}

func (m *memTeamUsage) QueueStatus(_ string) (TeamQueueStatusRow, error) {
	// The in-memory backend has no durable queue frontier (the pg impl
	// reads the Rust-owned quarry_request_queues tables). Honest zeros.
	return TeamQueueStatusRow{ByQueue: []QueueStatusEntryRow{}}, nil
}

func (m *memTeamUsage) Activity(orgID string, f ListFilter) ([]ActivityEntry, string) {
	events := m.db.eventsForOrg(orgID)

	sort.SliceStable(events, func(i, k int) bool {
		mi, mk := events[i].Timestamp.UnixMilli(), events[k].Timestamp.UnixMilli()
		if mi != mk {
			if f.SortDescending {
				return mi > mk
			}
			return mi < mk
		}
		if f.SortDescending {
			return events[i].EventID > events[k].EventID
		}
		return events[i].EventID < events[k].EventID
	})

	limit := f.Limit
	if limit <= 0 {
		limit = 50
	}

	type cursorKey struct {
		ts int64
		id string
	}
	var after cursorKey
	haveCur := false
	if curTS, curID, ok := decodeListCursor(f.Cursor); ok {
		after = cursorKey{curTS, curID}
		haveCur = true
	}

	inWindow := func(e quarrycontracts.Event) bool {
		if f.Status != "" && string(e.Type) != f.Status {
			return false
		}
		ms := e.Timestamp.UnixMilli()
		if f.CreatedBefore != nil && ms > f.CreatedBefore.UnixMilli() {
			return false
		}
		if f.CreatedAfter != nil && ms < f.CreatedAfter.UnixMilli() {
			return false
		}
		return true
	}

	out := make([]ActivityEntry, 0, limit)
	for _, e := range events {
		if !inWindow(e) {
			continue
		}
		key := cursorKey{e.Timestamp.UnixMilli(), string(e.EventID)}
		if haveCur {
			if f.SortDescending {
				if !(key.ts < after.ts || (key.ts == after.ts && key.id < after.id)) {
					continue
				}
			} else if !(key.ts > after.ts || (key.ts == after.ts && key.id > after.id)) {
				continue
			}
		}
		if len(out) == limit {
			last := out[len(out)-1]
			return out, encodeListCursor(last.Ts.UnixMilli(), string(last.EventID))
		}
		out = append(out, ActivityEntry{
			EventID:   e.EventID,
			EventType: string(e.Type),
			RunID:     e.RunID,
			Ts:        e.Timestamp,
			Summary:   activitySummary(e),
		})
	}
	return out, ""
}

// activitySummary renders the compact human-readable summary the dashboard
// shows directly (quarry_core::resources::TeamActivityEntry.summary). Only
// event types that actually exist in quarrycontracts carry bespoke text;
// everything else falls back to the raw type token rather than a lie.
func activitySummary(e quarrycontracts.Event) string {
	url, _ := e.Payload["url"].(string)
	switch e.Type {
	case quarrycontracts.EvtPageFetched:
		if url != "" {
			return "fetched " + url
		}
		return "page fetched"
	case quarrycontracts.EvtPageFailed:
		if url != "" {
			return "failed to fetch " + url
		}
		return "page failed"
	case quarrycontracts.EvtPageBlocked:
		if url != "" {
			return "blocked while fetching " + url
		}
		return "page blocked"
	case quarrycontracts.EvtPageQueued:
		if url != "" {
			return "queued " + url
		}
		return "page queued"
	case quarrycontracts.EvtSnapshotCreated:
		if url != "" {
			return "snapshot captured for " + url
		}
		return "snapshot captured"
	default:
		return string(e.Type)
	}
}

// hostOfJob extracts the hostname from a job's params.url when present.
// Jobs without a URL param contribute to Current but no per-host bucket.
func hostOfJob(j Job) string {
	raw, ok := j.Params["url"].(string)
	if !ok || raw == "" {
		return ""
	}
	u := strings.TrimPrefix(strings.TrimPrefix(raw, "https://"), "http://")
	if i := strings.IndexAny(u, "/?#"); i >= 0 {
		u = u[:i]
	}
	return strings.ToLower(u)
}

// ---- snapshots v2 ----------------------------------------------------------

// SnapshotsV2 returns the SHARED in-memory instance — a fresh store per
// call would make every Create invisible to the next ListByOrg.
func (d *memDB) SnapshotsV2() SnapshotsV2Store { return d.snapshotsV2 }

type memSnapshotsV2 struct {
	mu    sync.RWMutex
	items map[quarrycontracts.ID]SnapshotV2
	order []quarrycontracts.ID // insertion order; listed newest-first
}

func (m *memSnapshotsV2) Create(s SnapshotV2) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.items == nil {
		m.items = make(map[quarrycontracts.ID]SnapshotV2)
	}
	if _, exists := m.items[s.ID]; exists {
		return ErrConflict
	}
	m.items[s.ID] = s
	m.order = append(m.order, s.ID)
	return nil
}

func (m *memSnapshotsV2) GetByOrg(orgID string, id quarrycontracts.ID) (SnapshotV2, bool) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	s, ok := m.items[id]
	if !ok || s.OrgID != orgID {
		return SnapshotV2{}, false
	}
	return s, true
}

func (m *memSnapshotsV2) ListByOrg(orgID string, f ListFilter) ([]SnapshotV2, string) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	limit := f.Limit
	if limit <= 0 {
		limit = 50
	}

	matches := func(s SnapshotV2) bool {
		if s.OrgID != orgID {
			return false
		}
		if f.Status != "" && s.ChangeStatus != f.Status {
			return false
		}
		if f.CreatedBefore != nil && s.CreatedAt > f.CreatedBefore.UnixMilli() {
			return false
		}
		if f.CreatedAfter != nil && s.CreatedAt < f.CreatedAfter.UnixMilli() {
			return false
		}
		return true
	}

	// Walk insertion order forward or backward depending on sort direction,
	// applying the shared keyset cursor over (created_at, id).
	order := make([]int, 0, len(m.order))
	if f.SortDescending {
		for i := len(m.order) - 1; i >= 0; i-- {
			order = append(order, i)
		}
	} else {
		for i := range m.order {
			order = append(order, i)
		}
	}

	curTS, curID, haveCur := decodeListCursor(f.Cursor)

	out := make([]SnapshotV2, 0, limit)
	for _, i := range order {
		s := m.items[m.order[i]]
		if !matches(s) {
			continue
		}
		idStr := string(s.ID)
		if haveCur {
			if f.SortDescending {
				if !(s.CreatedAt < curTS || (s.CreatedAt == curTS && idStr < curID)) {
					continue
				}
			} else if !(s.CreatedAt > curTS || (s.CreatedAt == curTS && idStr > curID)) {
				continue
			}
		}
		if len(out) == limit {
			last := out[len(out)-1]
			return out, encodeListCursor(last.CreatedAt, string(last.ID))
		}
		out = append(out, s)
	}
	return out, ""
}

func (m *memSnapshotsV2) Delete(id quarrycontracts.ID) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	if _, ok := m.items[id]; !ok {
		return ErrNotFound
	}
	delete(m.items, id)
	for i, x := range m.order {
		if x == id {
			m.order = append(m.order[:i], m.order[i+1:]...)
			break
		}
	}
	return nil
}
