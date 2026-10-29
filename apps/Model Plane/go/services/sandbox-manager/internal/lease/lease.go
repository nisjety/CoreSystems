// Package lease holds the in-memory sandbox lease store and domain types.
// A lease represents a bounded right to use a sandbox for a thread- or
// agent-scoped workload. The store is safe for concurrent use.
package lease

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"sync"
	"time"
)

// Sentinel errors for lease lookups.
var (
	ErrLeaseNotFound = errors.New("lease not found")
	ErrLeaseExpired  = errors.New("lease expired")
)

// Lease is an issued sandbox reservation.
type Lease struct {
	ID        string
	ScopeID   string
	ScopeType string
	OrgID     string
	OwnerID   string
	Endpoint  string
	ExpiresAt time.Time
	CreatedAt time.Time
}

// IsExpired reports whether the lease has passed its TTL relative to now.
func (l *Lease) IsExpired(now time.Time) bool {
	return now.After(l.ExpiresAt)
}

// Store is an in-memory lease registry keyed by lease ID.
type Store struct {
	mu     sync.RWMutex
	byID   map[string]*Lease
	nowFn  func() time.Time
	randFn func([]byte) (int, error)
}

// NewStore constructs an empty Store.
func NewStore() *Store {
	return &Store{
		byID:   make(map[string]*Lease),
		nowFn:  time.Now,
		randFn: rand.Read,
	}
}

// Create mints a new lease with the given scope and TTL.
func (s *Store) Create(scopeID, scopeType, orgID, ownerID string, ttl time.Duration) (*Lease, error) {
	id, err := s.newID()
	if err != nil {
		return nil, err
	}
	now := s.nowFn()
	l := &Lease{
		ID:        id,
		ScopeID:   scopeID,
		ScopeType: scopeType,
		OrgID:     orgID,
		OwnerID:   ownerID,
		Endpoint:  "sandbox://" + id,
		ExpiresAt: now.Add(ttl),
		CreatedAt: now,
	}
	s.mu.Lock()
	s.byID[id] = clone(l)
	s.mu.Unlock()
	return clone(l), nil
}

// GetScoped returns a lease by ID only within the verified organization and
// optional user owner. It reports expiration without exposing other tenants.
func (s *Store) GetScoped(id, orgID, ownerID string) (*Lease, error) {
	s.mu.RLock()
	l, ok := s.byID[id]
	s.mu.RUnlock()
	if !ok || l.OrgID != orgID || (ownerID != "" && l.OwnerID != ownerID) {
		return nil, ErrLeaseNotFound
	}
	if l.IsExpired(s.nowFn()) {
		return nil, ErrLeaseExpired
	}
	return clone(l), nil
}

// ReleaseScoped removes a lease only inside the verified identity scope.
func (s *Store) ReleaseScoped(id, orgID, ownerID string) (bool, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	l, ok := s.byID[id]
	if !ok || l.OrgID != orgID || (ownerID != "" && l.OwnerID != ownerID) {
		return false, ErrLeaseNotFound
	}
	delete(s.byID, id)
	return true, nil
}

func clone(value *Lease) *Lease {
	copy := *value
	return &copy
}

func (s *Store) newID() (string, error) {
	var buf [16]byte
	if _, err := s.randFn(buf[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(buf[:]), nil
}
