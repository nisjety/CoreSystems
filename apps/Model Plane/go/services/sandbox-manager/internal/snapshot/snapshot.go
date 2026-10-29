// Package snapshot holds the in-memory sandbox snapshot store and domain
// types. A snapshot captures a labelled checkpoint of a sandbox associated
// with a lease. The store is safe for concurrent use.
package snapshot

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"sync"
	"time"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/lease"
)

// Sentinel errors for snapshot lookups.
var (
	ErrSnapshotNotFound = errors.New("snapshot not found")
	ErrInvalidLease     = errors.New("invalid lease for snapshot")
)

// Snapshot is a labelled checkpoint of a sandbox.
type Snapshot struct {
	ID        string
	LeaseID   string
	Label     string
	ObjectKey string
	CreatedAt time.Time
}

// Store is an in-memory snapshot registry keyed by snapshot ID.
type Store struct {
	mu     sync.RWMutex
	byID   map[string]*Snapshot
	nowFn  func() time.Time
	randFn func([]byte) (int, error)
}

// NewStore constructs an empty Store.
func NewStore() *Store {
	return &Store{
		byID:   make(map[string]*Snapshot),
		nowFn:  time.Now,
		randFn: rand.Read,
	}
}

// Create produces a new snapshot bound to the supplied lease.
func (s *Store) Create(l *lease.Lease, label string) (*Snapshot, error) {
	if l == nil || l.ID == "" {
		return nil, ErrInvalidLease
	}
	id, err := s.newID()
	if err != nil {
		return nil, err
	}
	snap := &Snapshot{
		ID:        id,
		LeaseID:   l.ID,
		Label:     label,
		ObjectKey: "snapshots/" + l.ID + "/" + id,
		CreatedAt: s.nowFn(),
	}
	s.mu.Lock()
	s.byID[id] = snap
	s.mu.Unlock()
	return snap, nil
}

// Get returns a snapshot by ID, or ErrSnapshotNotFound.
func (s *Store) Get(id string) (*Snapshot, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	snap, ok := s.byID[id]
	if !ok {
		return nil, ErrSnapshotNotFound
	}
	return snap, nil
}

func (s *Store) newID() (string, error) {
	var buf [16]byte
	if _, err := s.randFn(buf[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(buf[:]), nil
}
