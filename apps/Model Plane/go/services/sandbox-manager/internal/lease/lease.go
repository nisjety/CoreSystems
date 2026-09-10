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

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

// Sentinel errors for lease lookups.
var (
	ErrLeaseNotFound = errors.New("lease not found")
	ErrLeaseExpired  = errors.New("lease expired")
	// ErrLeaseBackendMismatch signals that a caller's asserted backend id
	// does not match the backend this lease was pinned to at AcquireLease
	// time. A Space-scoped lease request that lands on a different
	// sandbox-manager instance than the one its capability decision named
	// must be refused, never silently served by whichever instance answered.
	ErrLeaseBackendMismatch = errors.New("lease backend mismatch")
)

// Lease is an issued sandbox reservation.
type Lease struct {
	ID        string
	ScopeID   string
	ScopeType string
	OrgID     string
	OwnerID   string
	Endpoint  string
	// SpaceID is set only for a Space-scoped lease (one acquired with a
	// verified Space capability decision); empty for the pre-existing
	// thread/agent-scoped acquisition path.
	SpaceID string
	// BackendID is this process's own configured backend id, stamped at
	// Create time — empty for a non-Space lease, where there is nothing to
	// pin.
	BackendID string
	State     mpv1.SandboxLifecycleState
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

// Create mints a new lease with the given scope and TTL. spaceID and
// backendID are empty for the pre-existing thread/agent-scoped acquisition
// path; a Space-scoped caller supplies both, and the lease starts in
// SCRATCH — credential-free by construction until ActivateLease promotes it.
func (s *Store) Create(scopeID, scopeType, orgID, ownerID, spaceID, backendID string, ttl time.Duration) (*Lease, error) {
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
		SpaceID:   spaceID,
		BackendID: backendID,
		State:     mpv1.SandboxLifecycleState_SCRATCH,
		ExpiresAt: now.Add(ttl),
		CreatedAt: now,
	}
	s.mu.Lock()
	s.byID[id] = clone(l)
	s.mu.Unlock()
	return clone(l), nil
}

// GetScoped returns a lease by ID only within the verified organization,
// optional user owner, and asserted backend id. Not-found and expiry are
// checked first so a stale or foreign lease ID never leaks a backend
// mismatch signal; backendID is compared last and only matters for a
// Space-scoped lease (both sides are empty for the pre-existing path, so it
// trivially matches).
func (s *Store) GetScoped(id, orgID, ownerID, backendID string) (*Lease, error) {
	s.mu.RLock()
	l, ok := s.byID[id]
	s.mu.RUnlock()
	if !ok || l.OrgID != orgID || (ownerID != "" && l.OwnerID != ownerID) {
		return nil, ErrLeaseNotFound
	}
	if l.IsExpired(s.nowFn()) {
		return nil, ErrLeaseExpired
	}
	if l.BackendID != backendID {
		return nil, ErrLeaseBackendMismatch
	}
	return clone(l), nil
}

// ReleaseScoped removes a lease only inside the verified identity scope and
// asserted backend id.
func (s *Store) ReleaseScoped(id, orgID, ownerID, backendID string) (bool, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	l, ok := s.byID[id]
	if !ok || l.OrgID != orgID || (ownerID != "" && l.OwnerID != ownerID) {
		return false, ErrLeaseNotFound
	}
	if l.BackendID != backendID {
		return false, ErrLeaseBackendMismatch
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
