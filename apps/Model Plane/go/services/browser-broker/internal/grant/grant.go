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
const maxSensitiveScopeValues = 32

var allowedSensitiveActions = map[string]struct{}{
	"respond_dialog": {},
	"upload_ref":     {},
	"download_ref":   {},
}

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
	// Sensitive scopes are empty for a normal run grant. When present they
	// authorize only the exact action and observed opaque resources below.
	AllowedActions     []string
	AllowedFrameIDs    []string
	AllowedDialogIDs   []string
	AllowedArtifactIDs []string
	// ParentGrantID binds a sensitive approval to one ordinary run grant.
	// It is empty only for ordinary grants, never for an approval.
	ParentGrantID string
	ExpiresAt     time.Time
	Revoked       bool
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

// Create issues an ordinary run grant. Sensitive authority is intentionally
// absent; callers must use CreateWithSensitiveScopes for a one-time approval.
func (s *Store) Create(
	orgID, ownerID, sessionKey, scopeURL string,
	allowedDomains []string,
	ttl time.Duration,
) (*Grant, error) {
	return s.CreateWithSensitiveScopes(
		orgID, ownerID, sessionKey, scopeURL, allowedDomains,
		nil, nil, nil, nil, ttl,
	)
}

// CreateWithSensitiveScopes issues a grant with explicit, bounded authority
// for an irreversible browser action. The policy is normalized at the
// authority boundary even when callers have already validated it, so an empty
// or malformed policy cannot become unrestricted stored authority.
func (s *Store) CreateWithSensitiveScopes(
	orgID, ownerID, sessionKey, scopeURL string,
	allowedDomains []string,
	allowedActions, allowedFrameIDs, allowedDialogIDs, allowedArtifactIDs []string,
	ttl time.Duration,
) (*Grant, error) {
	return s.createWithSensitiveScopes(
		orgID, ownerID, sessionKey, scopeURL, allowedDomains,
		allowedActions, allowedFrameIDs, allowedDialogIDs, allowedArtifactIDs,
		"", ttl,
	)
}

// CreateScopedSensitiveApproval binds a one-time sensitive grant to an
// already validated ordinary BrowserBroker grant.
func (s *Store) CreateScopedSensitiveApproval(
	orgID, ownerID, sessionKey, scopeURL string,
	allowedDomains, allowedActions, allowedFrameIDs, allowedDialogIDs, allowedArtifactIDs []string,
	parentGrantID string,
	ttl time.Duration,
) (*Grant, error) {
	parent, err := s.GetScoped(parentGrantID, orgID, ownerID)
	if err != nil {
		return nil, err
	}
	if parent.SessionKey != sessionKey || len(parent.AllowedActions) != 0 {
		return nil, errors.New("sensitive approval parent grant is not an ordinary matching session grant")
	}
	return s.createWithSensitiveScopes(
		orgID, ownerID, sessionKey, scopeURL, allowedDomains,
		allowedActions, allowedFrameIDs, allowedDialogIDs, allowedArtifactIDs,
		parentGrantID, ttl,
	)
}

func (s *Store) createWithSensitiveScopes(
	orgID, ownerID, sessionKey, scopeURL string,
	allowedDomains, allowedActions, allowedFrameIDs, allowedDialogIDs, allowedArtifactIDs []string,
	parentGrantID string,
	ttl time.Duration,
) (*Grant, error) {
	canonicalDomains, err := NormalizeAllowedDomains(allowedDomains)
	if err != nil {
		return nil, err
	}
	approvalScopes, err := NormalizeSensitiveScopes(
		allowedActions,
		allowedFrameIDs,
		allowedDialogIDs,
		allowedArtifactIDs,
	)
	if err != nil {
		return nil, err
	}
	if len(approvalScopes.actions) > 0 && parentGrantID == "" {
		return nil, errors.New("sensitive grant requires parent browser grant")
	}
	id, err := newID()
	if err != nil {
		return nil, err
	}
	g := &Grant{
		ID:                 id,
		OrgID:              orgID,
		OwnerID:            ownerID,
		SessionKey:         sessionKey,
		ScopeURL:           scopeURL,
		AllowedDomains:     canonicalDomains,
		AllowedActions:     approvalScopes.actions,
		AllowedFrameIDs:    approvalScopes.frames,
		AllowedDialogIDs:   approvalScopes.dialogs,
		AllowedArtifactIDs: approvalScopes.artifacts,
		ParentGrantID:      parentGrantID,
		ExpiresAt:          time.Now().Add(ttl),
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
	copy.AllowedActions = append([]string(nil), grant.AllowedActions...)
	copy.AllowedFrameIDs = append([]string(nil), grant.AllowedFrameIDs...)
	copy.AllowedDialogIDs = append([]string(nil), grant.AllowedDialogIDs...)
	copy.AllowedArtifactIDs = append([]string(nil), grant.AllowedArtifactIDs...)
	return &copy
}

type sensitiveScopes struct {
	actions   []string
	frames    []string
	dialogs   []string
	artifacts []string
}

// NormalizeSensitiveScopes makes a sensitive grant explicit and bounded. A
// broker can issue an ordinary domain-scoped run grant with every sensitive
// scope empty; any non-empty scope must name a recognized irreversible action
// and its exact observed resource(s), never a wildcard.
func NormalizeSensitiveScopes(actions, frames, dialogs, artifacts []string) (sensitiveScopes, error) {
	if len(actions) == 0 {
		if len(frames) != 0 || len(dialogs) != 0 || len(artifacts) != 0 {
			return sensitiveScopes{}, errors.New("sensitive resource scope requires an action")
		}
		return sensitiveScopes{}, nil
	}
	canonicalActions, err := normalizeScopeValues(actions, "action", func(value string) bool {
		_, ok := allowedSensitiveActions[value]
		return ok
	})
	if err != nil {
		return sensitiveScopes{}, err
	}
	if len(canonicalActions) != 1 {
		return sensitiveScopes{}, errors.New("sensitive grant requires exactly one action")
	}
	canonicalFrames, err := normalizeScopeValues(frames, "frame", validOpaqueScopeValue)
	if err != nil {
		return sensitiveScopes{}, err
	}
	canonicalDialogs, err := normalizeScopeValues(dialogs, "dialog", validOpaqueScopeValue)
	if err != nil {
		return sensitiveScopes{}, err
	}
	canonicalArtifacts, err := normalizeScopeValues(artifacts, "artifact", func(value string) bool {
		return strings.HasPrefix(value, "art_") && validOpaqueScopeValue(value)
	})
	if err != nil {
		return sensitiveScopes{}, err
	}
	switch canonicalActions[0] {
	case "respond_dialog":
		if len(canonicalDialogs) != 1 || len(canonicalFrames) != 1 || len(canonicalArtifacts) != 0 {
			return sensitiveScopes{}, errors.New("respond_dialog grant requires one dialog, one frame, and no artifacts")
		}
	case "upload_ref":
		if len(canonicalArtifacts) != 1 || len(canonicalFrames) != 1 || len(canonicalDialogs) != 0 {
			return sensitiveScopes{}, errors.New("upload_ref grant requires one artifact, one frame, and no dialogs")
		}
	case "download_ref":
		if len(canonicalArtifacts) != 0 || len(canonicalDialogs) != 0 || len(canonicalFrames) != 1 {
			return sensitiveScopes{}, errors.New("download_ref grant requires one frame and no dialog or artifact scopes")
		}
	}
	return sensitiveScopes{canonicalActions, canonicalFrames, canonicalDialogs, canonicalArtifacts}, nil
}

func normalizeScopeValues(values []string, name string, valid func(string) bool) ([]string, error) {
	if len(values) > maxSensitiveScopeValues {
		return nil, errors.New("too many sensitive " + name + " scope values")
	}
	canonical := make(map[string]struct{}, len(values))
	for _, raw := range values {
		value := strings.TrimSpace(raw)
		if !valid(value) {
			return nil, errors.New("invalid sensitive " + name + " scope value")
		}
		canonical[value] = struct{}{}
	}
	result := make([]string, 0, len(canonical))
	for value := range canonical {
		result = append(result, value)
	}
	sort.Strings(result)
	return result, nil
}

func validOpaqueScopeValue(value string) bool {
	if len(value) == 0 || len(value) > 256 {
		return false
	}
	for _, char := range value {
		if char <= 0x1f || char == 0x7f {
			return false
		}
	}
	return true
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
