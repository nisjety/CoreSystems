// Package grant implements in-memory storage for browser grant lifecycle.
// A grant represents a time-bounded, revocable authorization for an agent to
// access a scoped browser endpoint.
package grant

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"sync"
	"time"
)

var (
	// ErrGrantNotFound indicates the grant ID does not exist.
	ErrGrantNotFound = errors.New("grant not found")
	// ErrGrantExpired indicates the grant TTL has elapsed.
	ErrGrantExpired = errors.New("grant expired")
	// ErrGrantRevoked indicates the grant was explicitly revoked.
	ErrGrantRevoked = errors.New("grant revoked")
)

// Grant is a trusted browser access grant.
type Grant struct {
	ID         string
	OrgID      string
	OwnerID    string
	SessionKey string
	ScopeURL   string
	ExpiresAt  time.Time
	Revoked    bool
}

// Store is a thread-safe in-memory grant store.
type Store struct {
	mu     sync.Mutex
	grants map[string]*Grant
}

// NewStore constructs an empty Store.
func NewStore() *Store {
	return &Store{grants: make(map[string]*Grant)}
}

// Create issues a new grant with the provided TTL.
func (s *Store) Create(orgID, ownerID, sessionKey, scopeURL string, ttl time.Duration) (*Grant, error) {
	id, err := newID()
	if err != nil {
		return nil, err
	}
	g := &Grant{
		ID:         id,
		OrgID:      orgID,
		OwnerID:    ownerID,
		SessionKey: sessionKey,
		ScopeURL:   scopeURL,
		ExpiresAt:  time.Now().Add(ttl),
	}
	s.mu.Lock()
	s.grants[id] = clone(g)
	s.mu.Unlock()
	return clone(g), nil
}

// GetScoped returns a grant only within the verified organization. A non-empty
// ownerID additionally restricts access to the owning user.
func (s *Store) GetScoped(id, orgID, ownerID string) (*Grant, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	g, ok := s.grants[id]
	if !ok || g.OrgID != orgID || (ownerID != "" && g.OwnerID != ownerID) {
		return nil, ErrGrantNotFound
	}
	if g.Revoked {
		return nil, ErrGrantRevoked
	}
	if time.Now().After(g.ExpiresAt) {
		return nil, ErrGrantExpired
	}
	return clone(g), nil
}

// RevokeScoped immutably revokes a grant inside the verified identity scope.
func (s *Store) RevokeScoped(id, orgID, ownerID string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	g, ok := s.grants[id]
	if !ok || g.OrgID != orgID || (ownerID != "" && g.OwnerID != ownerID) {
		return ErrGrantNotFound
	}
	revoked := *g
	revoked.Revoked = true
	s.grants[id] = &revoked
	return nil
}

func clone(grant *Grant) *Grant {
	copy := *grant
	return &copy
}

func newID() (string, error) {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return hex.EncodeToString(b), nil
}
