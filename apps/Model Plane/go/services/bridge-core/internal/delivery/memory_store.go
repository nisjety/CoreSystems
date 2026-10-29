package delivery

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"sort"
	"sync"
	"time"
)

// MemoryStore is an in-process, goroutine-safe Store. It is the default outbox
// backing for bridge-core, which has no database wiring. Records live for the
// lifetime of the process; a Postgres implementation of Store can replace it
// without touching the worker or adapters.
type MemoryStore struct {
	mu      sync.Mutex
	records map[string]*Record
	// claimed tracks records currently leased by a ClaimDue caller so a second
	// concurrent claim does not double-process the same record.
	claimed map[string]bool
}

// NewMemoryStore constructs an empty in-memory outbox.
func NewMemoryStore() *MemoryStore {
	return &MemoryStore{
		records: make(map[string]*Record),
		claimed: make(map[string]bool),
	}
}

// Enqueue inserts a new pending record.
func (m *MemoryStore) Enqueue(_ context.Context, rec Record) (Record, error) {
	id, err := generateID()
	if err != nil {
		return Record{}, fmt.Errorf("generate delivery id: %w", err)
	}
	now := time.Now().UTC()
	rec.ID = id
	rec.Status = StatusPending
	rec.Attempts = 0
	rec.CreatedAt = now
	rec.UpdatedAt = now
	if rec.NextAttemptAt.IsZero() {
		rec.NextAttemptAt = now
	}

	stored := rec
	m.mu.Lock()
	m.records[id] = &stored
	m.mu.Unlock()
	return stored, nil
}

// ClaimDue leases up to limit due pending records. Leased records are hidden
// from subsequent ClaimDue calls until finalised or re-armed via MarkRetry.
func (m *MemoryStore) ClaimDue(_ context.Context, now time.Time, limit int) ([]Record, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	var due []*Record
	for _, r := range m.records {
		if r.Status != StatusPending {
			continue
		}
		if m.claimed[r.ID] {
			continue
		}
		if r.NextAttemptAt.After(now) {
			continue
		}
		due = append(due, r)
	}

	// Oldest-armed first for fair, deterministic draining.
	sort.Slice(due, func(i, j int) bool {
		return due[i].NextAttemptAt.Before(due[j].NextAttemptAt)
	})

	if limit > 0 && len(due) > limit {
		due = due[:limit]
	}

	out := make([]Record, 0, len(due))
	for _, r := range due {
		m.claimed[r.ID] = true
		out = append(out, *r) // copy
	}
	return out, nil
}

// MarkDelivered finalises a record as delivered and releases its lease.
func (m *MemoryStore) MarkDelivered(_ context.Context, id string) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	r, ok := m.records[id]
	if !ok {
		return ErrNotFound
	}
	r.Status = StatusDelivered
	r.UpdatedAt = time.Now().UTC()
	delete(m.claimed, id)
	return nil
}

// MarkRetry records a failed attempt and re-arms the record.
func (m *MemoryStore) MarkRetry(_ context.Context, id, attemptErr string, nextAttemptAt time.Time) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	r, ok := m.records[id]
	if !ok {
		return ErrNotFound
	}
	r.Attempts++
	r.LastError = attemptErr
	r.Status = StatusPending
	r.NextAttemptAt = nextAttemptAt
	r.UpdatedAt = time.Now().UTC()
	delete(m.claimed, id)
	return nil
}

// MarkDead moves a record to the dead-letter state and releases its lease.
func (m *MemoryStore) MarkDead(_ context.Context, id, attemptErr string) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	r, ok := m.records[id]
	if !ok {
		return ErrNotFound
	}
	r.Attempts++
	r.LastError = attemptErr
	r.Status = StatusDead
	r.UpdatedAt = time.Now().UTC()
	delete(m.claimed, id)
	return nil
}

// Get returns a copy of the record with the given ID.
func (m *MemoryStore) Get(_ context.Context, id string) (Record, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	r, ok := m.records[id]
	if !ok {
		return Record{}, ErrNotFound
	}
	return *r, nil
}

// PendingCount returns the number of records not yet delivered or dead.
func (m *MemoryStore) PendingCount(_ context.Context) (int, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	n := 0
	for _, r := range m.records {
		if r.Status == StatusPending {
			n++
		}
	}
	return n, nil
}

// generateID returns a 16-byte hex-encoded random identifier, matching the
// session registry's ID format.
func generateID() (string, error) {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		return "", fmt.Errorf("read random bytes: %w", err)
	}
	return hex.EncodeToString(b), nil
}
