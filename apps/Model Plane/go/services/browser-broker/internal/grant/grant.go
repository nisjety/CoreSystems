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
	ID        string
	OrgID     string
	AgentID   string
	ScopeURL  string
	ExpiresAt time.Time
	Revoked   bool
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
func (s *Store) Create(orgID, agentID, scopeURL string, ttl time.Duration) (*Grant, error) {
	id, err := newID()
	if err != nil {
		return nil, err
	}
	g := &Grant{
		ID:        id,
		OrgID:     orgID,
		AgentID:   agentID,
		ScopeURL:  scopeURL,
		ExpiresAt: time.Now().Add(ttl),
	}
	s.mu.Lock()
	s.grants[id] = g
	s.mu.Unlock()
	return g, nil
}

// Get returns a grant by ID, or an error if missing, expired, or revoked.
func (s *Store) Get(id string) (*Grant, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	g, ok := s.grants[id]
	if !ok {
		return nil, ErrGrantNotFound
	}
	if g.Revoked {
		return nil, ErrGrantRevoked
	}
	if time.Now().After(g.ExpiresAt) {
		return nil, ErrGrantExpired
	}
	return g, nil
}

// Revoke marks a grant as revoked. Returns ErrGrantNotFound if missing.
func (s *Store) Revoke(id string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	g, ok := s.grants[id]
	if !ok {
		return ErrGrantNotFound
	}
	g.Revoked = true
	return nil
}

func newID() (string, error) {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return hex.EncodeToString(b), nil
}
