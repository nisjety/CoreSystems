// Package store holds the resource registry backend.
// Scaffold uses in-memory; Phase 1 swaps to pgx/Postgres with identical interface.
package store

import (
	"encoding/base64"
	"fmt"
	"sort"
	"strings"
	"sync"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

type DB interface {
	Jobs() JobsStore
	Stores() ResourceStore[NamedStore]
	Snapshots() ResourceStore[Snapshot]
	Artifacts() ResourceStore[Artifact]
	Profiles() ResourceStore[BrowserProfile]
	Schedules() SchedulesStore
	Webhooks() ResourceStore[Webhook]
	WebhookDeliveries() WebhookDeliveryStore
	Blocklists() ResourceStore[BlocklistEntry]
	Events() EventLog
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

type JobsStore interface {
	ResourceStore[Job]
	ListBySchedule(scheduleID quarrycontracts.ID, limit int, cursor string) ([]Job, string)
	// Update replaces a job record by ID. Used by the orchestrator's
	// jobs dispatcher to transition accepted → running and stamp the
	// run_id once a Temporal workflow has been started.
	Update(j Job) error
	// FindByIdempotencyKey returns an existing job that was created
	// with the same Idempotency-Key header, or (Job{}, false) when no
	// such record exists. Used by createJob to de-dupe client retries.
	FindByIdempotencyKey(key string) (Job, bool)
}

type EventLog interface {
	Append(evt quarrycontracts.Event) error
	ForRun(runID quarrycontracts.ID, afterSeq uint64, limit int) []quarrycontracts.Event
	ForJob(jobID quarrycontracts.ID, afterSeq uint64, limit int) []quarrycontracts.Event
	// NextSeq returns one above the current max seq for runID, or 1
	// when no events exist yet. The handler uses this to assign durable
	// per-run seq server-side regardless of caller numbering.
	NextSeq(runID quarrycontracts.ID) uint64
}

// ---- resource types -------------------------------------------------------

type Job struct {
	ID         quarrycontracts.ID        `json:"id"`
	Kind       string                    `json:"kind"` // scrape | crawl | batch | schedule
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
	ID         quarrycontracts.ID `json:"id"`
	Cron       string             `json:"cron"`
	TargetKind string             `json:"target_kind"` // scrape | crawl | batch
	TargetRef  string             `json:"target_ref"`  // url | job template id
	Enabled    bool               `json:"enabled"`
	CreatedAt  int64              `json:"created_at"`
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
	webhooks     *genericStore[Webhook]
	whDeliveries *genericStore[WebhookDelivery]
	blocklists   *genericStore[BlocklistEntry]
	events       *memEventLog
}

func NewMemory() DB {
	return &memDB{
		jobs:         newGeneric[Job](func(j Job) quarrycontracts.ID { return j.ID }),
		stores:       newGeneric[NamedStore](func(s NamedStore) quarrycontracts.ID { return s.ID }),
		snaps:        newGeneric[Snapshot](func(s Snapshot) quarrycontracts.ID { return s.ID }),
		arts:         newGeneric[Artifact](func(a Artifact) quarrycontracts.ID { return a.ID }),
		profiles:     newGeneric[BrowserProfile](func(p BrowserProfile) quarrycontracts.ID { return p.ID }),
		schedules:    newGeneric[Schedule](func(s Schedule) quarrycontracts.ID { return s.ID }),
		webhooks:     newGeneric[Webhook](func(w Webhook) quarrycontracts.ID { return w.ID }),
		whDeliveries: newGeneric[WebhookDelivery](func(d WebhookDelivery) quarrycontracts.ID { return d.ID }),
		blocklists:   newGeneric[BlocklistEntry](func(b BlocklistEntry) quarrycontracts.ID { return b.ID }),
		events:       &memEventLog{},
	}
}

func (d *memDB) Jobs() JobsStore                         { return &memJobs{d.jobs} }
func (d *memDB) Stores() ResourceStore[NamedStore]       { return d.stores }
func (d *memDB) Snapshots() ResourceStore[Snapshot]      { return d.snaps }
func (d *memDB) Artifacts() ResourceStore[Artifact]      { return d.arts }
func (d *memDB) Profiles() ResourceStore[BrowserProfile] { return d.profiles }
func (d *memDB) Schedules() SchedulesStore               { return &memSchedules{d.schedules} }
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

type memJobs struct{ *genericStore[Job] }

// FindByIdempotencyKey scans the in-memory job set for a record with
// the supplied key. Linear scan is fine — the memory store is dev-only
// and job counts here are bounded.
func (m *memJobs) FindByIdempotencyKey(key string) (Job, bool) {
	if key == "" {
		return Job{}, false
	}
	m.mu.RLock()
	defer m.mu.RUnlock()
	for _, j := range m.items {
		if j.IdempotencyKey != nil && *j.IdempotencyKey == key {
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
