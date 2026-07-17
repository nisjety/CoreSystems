// Package grant implements in-memory storage for browser grant lifecycle.
// A grant represents a time-bounded, revocable authorization for an agent to
// access a scoped browser endpoint.
package grant

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"net"
	"sort"
	"strings"
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
	// ErrInvalidDomainPolicy indicates a browser grant has no safe, bounded
	// navigation policy. An empty policy never means unrestricted access.
	ErrInvalidDomainPolicy = errors.New("invalid browser allowed_domains policy")
)

const maxAllowedDomains = 32

// Grant is a trusted browser access grant.
type Grant struct {
	ID         string
	OrgID      string
	OwnerID    string
	SessionKey string
	ScopeURL   string
	// AllowedDomains is the broker-owned, canonical navigation policy. It is
	// copied at every storage boundary so no caller can mutate a persisted grant.
	AllowedDomains []string
	ExpiresAt      time.Time
	Revoked        bool
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

// Create issues a new grant with the provided TTL. The policy is normalized at
// the authority boundary even when callers have already validated it, so an
// empty or malformed policy cannot become an unrestricted stored grant.
func (s *Store) Create(
	orgID, ownerID, sessionKey, scopeURL string,
	allowedDomains []string,
	ttl time.Duration,
) (*Grant, error) {
	canonicalDomains, err := NormalizeAllowedDomains(allowedDomains)
	if err != nil {
		return nil, err
	}
	id, err := newID()
	if err != nil {
		return nil, err
	}
	g := &Grant{
		ID:             id,
		OrgID:          orgID,
		OwnerID:        ownerID,
		SessionKey:     sessionKey,
		ScopeURL:       scopeURL,
		AllowedDomains: canonicalDomains,
		ExpiresAt:      time.Now().Add(ttl),
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
	revoked := clone(g)
	revoked.Revoked = true
	s.grants[id] = revoked
	return nil
}

func clone(grant *Grant) *Grant {
	copy := *grant
	copy.AllowedDomains = append([]string(nil), grant.AllowedDomains...)
	return &copy
}

// NormalizeAllowedDomains canonicalizes a bounded host-only allowlist. It
// deliberately rejects URL, path, wildcard, IP, and empty entries: callers
// must receive a concrete browser policy rather than relying on a permissive
// default in a downstream browser executor.
func NormalizeAllowedDomains(domains []string) ([]string, error) {
	if len(domains) == 0 || len(domains) > maxAllowedDomains {
		return nil, ErrInvalidDomainPolicy
	}

	canonical := make(map[string]struct{}, len(domains))
	for _, raw := range domains {
		domain := strings.ToLower(strings.TrimSpace(raw))
		if !isCanonicalDomain(domain) {
			return nil, ErrInvalidDomainPolicy
		}
		canonical[domain] = struct{}{}
	}
	if len(canonical) == 0 {
		return nil, ErrInvalidDomainPolicy
	}

	result := make([]string, 0, len(canonical))
	for domain := range canonical {
		result = append(result, domain)
	}
	sort.Strings(result)
	return result, nil
}

func isCanonicalDomain(domain string) bool {
	if len(domain) == 0 || len(domain) > 253 || net.ParseIP(domain) != nil {
		return false
	}
	// Require a DNS hostname, not localhost or a bare internal label. This is a
	// browser-navigation boundary and accepts only explicit public-style host
	// policies; URL schemes, ports, paths, wildcards, and unicode are rejected
	// by the label parser below.
	if !strings.Contains(domain, ".") || strings.HasPrefix(domain, ".") || strings.HasSuffix(domain, ".") {
		return false
	}
	for _, label := range strings.Split(domain, ".") {
		if len(label) == 0 || len(label) > 63 || label[0] == '-' || label[len(label)-1] == '-' {
			return false
		}
		for _, char := range label {
			if (char < 'a' || char > 'z') && (char < '0' || char > '9') && char != '-' {
				return false
			}
		}
	}
	return true
}

func newID() (string, error) {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return hex.EncodeToString(b), nil
}
