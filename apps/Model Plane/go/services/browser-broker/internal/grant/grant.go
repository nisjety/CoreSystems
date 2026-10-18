// Package grant implements browser grant lifecycle: a grant is a time-bounded,
// revocable authorization for an agent to access a scoped browser endpoint.
//
// Storage sits behind Repository so the in-memory implementation can be routed
// next to a durable one (WS1.C) without touching issuance. Zero Data
// Retention grants are never handed to a durable store.
package grant

import (
	"context"
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
	// ErrTooManyGrants indicates the owner already holds the maximum number of
	// active grants.
	ErrTooManyGrants = errors.New("too many active browser grants")
	// ErrParentNotDerivable means the parent grant cannot seed a run grant: it
	// is not a derivable kind, carries sensitive actions, or has too little
	// lifetime left. The message never says which.
	ErrParentNotDerivable = errors.New("browser grant is not derivable")
	// ErrDeriveWidensParent means a requested run host is not equal to or
	// below any host of the parent grant.
	ErrDeriveWidensParent = errors.New("derived browser grant would widen its parent")
	// ErrZdrPostureChanged means a Zero Data Retention principal asked to
	// derive from a grant that is not ZDR, which would hand it a grant whose
	// run may be persisted.
	ErrZdrPostureChanged = errors.New("zero data retention posture differs from the parent grant")
	// ErrDeriveNeedsExplicitDomains means the run must name its hosts: the
	// parent is wider than one run may inherit, or its kind requires it.
	ErrDeriveNeedsExplicitDomains = errors.New("derived browser grant requires an explicit host list")
	// ErrDeriveConflict means (parent, session_key) already names a run grant
	// with a different host list. A replay never changes an issued grant.
	ErrDeriveConflict = errors.New("run grant already derived with a different policy")
)

// DefaultMaxRunDomains bounds the hosts one run grant may hold, and how many
// it may inherit implicitly (BROWSER_BROKER_MAX_RUN_DOMAINS).
const DefaultMaxRunDomains = 8

// MinDerivedLifetime is the shortest run grant Derive issues. A grant that
// would die mid-step is refused instead.
const MinDerivedLifetime = 30 * time.Second

// DefaultMaxActiveRunsPerParent bounds the live run grants one session grant
// may hold at once. Run grants do not count toward the per-owner cap (that
// cap bounds sessions, and a burst of AI runs must not lock a user out of
// opening a browser session); they are bounded here instead, so one owner's
// total stays at most MaxActivePerOwner * (1 + this).
const DefaultMaxActiveRunsPerParent = 8

// derivePolicy says how a run grant may be derived from a parent kind.
type derivePolicy struct {
	// explicitHosts requires the caller to name a non-empty host list (R-16:
	// any parent other than a session grant, e.g. the future human_browse).
	explicitHosts bool
}

// derivableParents lists the parent kinds a run grant may derive from. A run
// grant is not here (no chains), nor is an approval (single-use authority).
var derivableParents = map[Kind]derivePolicy{
	KindSession: {explicitHosts: false},
}

// approvalParents lists the kinds a sensitive approval may bind to.
var approvalParents = map[Kind]struct{}{
	KindSession: {},
	KindRun:     {},
}

const maxAllowedDomains = 32
const maxSensitiveScopeValues = 32
const maxRevokeReasonBytes = 128

// zdrIDPrefix marks a Zero Data Retention grant id, so a log line or a stray
// id is enough to tell that nothing about it may be persisted.
const zdrIDPrefix = "z_"

var allowedSensitiveActions = map[string]struct{}{
	"respond_dialog": {},
	"upload_ref":     {},
	"download_ref":   {},
}

// Kind says what a grant authorizes.
type Kind string

const (
	// KindSession is held by a signed-in user for one browser session. It
	// carries the navigation policy and is what a run grant derives from.
	KindSession Kind = "session"
	// KindRun is derived from a session grant for one agent run (WS1.B).
	KindRun Kind = "run"
	// KindSensitiveApproval is a one-time approval for one irreversible
	// action, bound to its parent grant.
	KindSensitiveApproval Kind = "sensitive_approval"
)

// Grant is a trusted browser access grant.
type Grant struct {
	ID         string
	Kind       Kind
	OrgID      string
	OwnerID    string
	SessionKey string
	ScopeURL   string
	// Zdr marks a Zero Data Retention grant. It can be raised, never lowered,
	// and a derived or approval grant inherits it.
	Zdr bool
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
	// RevokedReason is a short caller-supplied label, never a domain or URL.
	RevokedReason string
}

// CreateSessionParams describes a session grant.
type CreateSessionParams struct {
	OrgID, OwnerID, SessionKey, ScopeURL string
	AllowedDomains                       []string
	TTL                                  time.Duration
	Zdr                                  bool
	// MaxActivePerOwner > 0 refuses creation with ErrTooManyGrants when the
	// owner already holds that many active ordinary grants. The check and the
	// insert are one atomic step.
	MaxActivePerOwner int
}

// ApprovalParams describes a one-time sensitive approval.
type ApprovalParams struct {
	OrgID, OwnerID, SessionKey, ScopeURL                                                  string
	AllowedDomains, AllowedActions, AllowedFrameIDs, AllowedDialogIDs, AllowedArtifactIDs []string
	ParentGrantID                                                                         string
	TTL                                                                                   time.Duration
	// Zdr raises the approval's flag; it never lowers the parent's.
	Zdr bool
}

// DeriveParams describes a run grant derived from a session grant.
type DeriveParams struct {
	// OrgID is the verified organization. OwnerID is the owner filter: a
	// user's own id, or empty for a scoped service. The run grant's owner is
	// always the parent's owner.
	OrgID, OwnerID, ParentGrantID string
	// SessionKey is the run's idempotency key.
	SessionKey string
	// RunDomains narrows the parent's hosts; empty inherits them.
	RunDomains []string
	// TTL is the requested lifetime; the expiry is capped by the parent's.
	TTL time.Duration
	// PrincipalZdr is the verified caller's ZDR flag.
	PrincipalZdr bool
	// MaxRunDomains bounds the run's hosts; zero or negative uses
	// DefaultMaxRunDomains.
	MaxRunDomains int
	// MaxActivePerParent bounds the parent's live run grants (checked
	// atomically with the insert); zero or negative uses
	// DefaultMaxActiveRunsPerParent. A replay of an issued key is not new.
	MaxActivePerParent int
	// Policy, when set, re-checks the run's hosts (defence in depth).
	Policy PolicyResolver
}

// Repository stores grants. Every method is scoped by the verified
// organization; a non-empty ownerID additionally restricts access to that user.
type Repository interface {
	CreateSession(ctx context.Context, p CreateSessionParams) (*Grant, error)
	CreateScopedSensitiveApproval(ctx context.Context, p ApprovalParams) (*Grant, error)
	// Derive issues a run grant narrower than its session grant (WS1.B).
	Derive(ctx context.Context, p DeriveParams) (*Grant, error)
	GetScoped(ctx context.Context, id, orgID, ownerID string) (*Grant, error)
	// RevokeScoped revokes the grant and every grant derived from it.
	RevokeScoped(ctx context.Context, id, orgID, ownerID, reason string) error
}

// MemoryRepository is a thread-safe in-memory Repository.
type MemoryRepository struct {
	mu     sync.Mutex
	grants map[string]*Grant
	now    func() time.Time
	// betweenLocks, when set (tests only), runs after Derive and
	// CreateScopedSensitiveApproval have read the parent and released the
	// lock, before they re-take it to insert: it forces the interleaving a
	// concurrent revoke or expiry would produce.
	betweenLocks func()
}

var (
	_ Repository = (*MemoryRepository)(nil)
	_ Purger     = (*MemoryRepository)(nil)
)

// NewMemoryRepository constructs an empty MemoryRepository.
func NewMemoryRepository() *MemoryRepository {
	return &MemoryRepository{grants: make(map[string]*Grant), now: time.Now}
}

// CreateSession issues an ordinary session grant. Sensitive authority is
// intentionally absent; use CreateScopedSensitiveApproval for a one-time
// approval. The policy is normalized at the authority boundary even when
// callers have already validated it, so an empty or malformed policy cannot
// become unrestricted stored authority.
func (s *MemoryRepository) CreateSession(_ context.Context, p CreateSessionParams) (*Grant, error) {
	domains, err := NormalizeAllowedDomains(p.AllowedDomains)
	if err != nil {
		return nil, err
	}
	id, err := newID(p.Zdr)
	if err != nil {
		return nil, err
	}
	g := &Grant{
		ID:             id,
		Kind:           KindSession,
		OrgID:          p.OrgID,
		OwnerID:        p.OwnerID,
		SessionKey:     p.SessionKey,
		ScopeURL:       p.ScopeURL,
		Zdr:            p.Zdr,
		AllowedDomains: domains,
		ExpiresAt:      s.now().Add(p.TTL),
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if p.MaxActivePerOwner > 0 && s.activeLocked(p.OrgID, p.OwnerID) >= p.MaxActivePerOwner {
		return nil, ErrTooManyGrants
	}
	s.grants[id] = clone(g)
	return clone(g), nil
}

// CreateScopedSensitiveApproval binds a one-time sensitive grant to an
// already validated ordinary BrowserBroker grant. The approval is never wider
// than its parent: it inherits the parent's ZDR flag, each of its hosts must
// equal or sit below a parent host (ErrDeriveWidensParent), and its expiry is
// capped at the parent's. The parent is re-read under the insert lock, so a
// parent revoked or expired meanwhile leaves no approval behind.
func (s *MemoryRepository) CreateScopedSensitiveApproval(_ context.Context, p ApprovalParams) (*Grant, error) {
	s.mu.Lock()
	parent, err := s.getScopedLocked(p.ParentGrantID, p.OrgID, p.OwnerID)
	s.mu.Unlock()
	if err != nil {
		return nil, err
	}
	if _, ok := approvalParents[parent.Kind]; !ok || parent.SessionKey != p.SessionKey || len(parent.AllowedActions) != 0 {
		return nil, errors.New("sensitive approval parent grant is not an ordinary matching session or run grant")
	}
	domains, err := NormalizeAllowedDomains(p.AllowedDomains)
	if err != nil {
		return nil, err
	}
	for _, host := range domains {
		if !hostWithin(host, parent.AllowedDomains) {
			return nil, ErrDeriveWidensParent
		}
	}
	scopes, err := NormalizeSensitiveScopes(
		p.AllowedActions, p.AllowedFrameIDs, p.AllowedDialogIDs, p.AllowedArtifactIDs,
	)
	if err != nil {
		return nil, err
	}
	if len(scopes.actions) == 0 {
		return nil, errors.New("sensitive grant requires an action")
	}
	zdr := parent.Zdr || p.Zdr
	id, err := newID(zdr)
	if err != nil {
		return nil, err
	}
	g := &Grant{
		ID:                 id,
		Kind:               KindSensitiveApproval,
		OrgID:              p.OrgID,
		OwnerID:            p.OwnerID,
		SessionKey:         p.SessionKey,
		ScopeURL:           p.ScopeURL,
		Zdr:                zdr,
		AllowedDomains:     domains,
		AllowedActions:     scopes.actions,
		AllowedFrameIDs:    scopes.frames,
		AllowedDialogIDs:   scopes.dialogs,
		AllowedArtifactIDs: scopes.artifacts,
		ParentGrantID:      p.ParentGrantID,
	}
	if s.betweenLocks != nil {
		s.betweenLocks()
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	parent, err = s.getScopedLocked(p.ParentGrantID, p.OrgID, p.OwnerID)
	if err != nil {
		return nil, err
	}
	g.ExpiresAt = s.now().Add(p.TTL)
	if parent.ExpiresAt.Before(g.ExpiresAt) {
		g.ExpiresAt = parent.ExpiresAt
	}
	s.grants[id] = clone(g)
	return clone(g), nil
}

// Derive issues a run grant that is never wider than its parent session grant:
// same tenant and owner, hosts equal to or below the parent's, the parent's
// ZDR flag, an expiry no later than the parent's, and a ParentGrantID so a
// revocation of the parent cascades to it. It is idempotent on
// (parent, SessionKey): a replay returns the issued grant unchanged.
//
// The parent is looked up and the run grant inserted under one lock, so a
// parent revoked concurrently can never leave a live child behind.
func (s *MemoryRepository) Derive(ctx context.Context, p DeriveParams) (*Grant, error) {
	if strings.TrimSpace(p.SessionKey) == "" || p.ParentGrantID == "" || p.OrgID == "" {
		return nil, ErrParentNotDerivable
	}
	s.mu.Lock()
	parent, err := s.getScopedLocked(p.ParentGrantID, p.OrgID, p.OwnerID)
	s.mu.Unlock()
	if err != nil {
		return nil, err
	}
	policy, ok := derivableParents[parent.Kind]
	if !ok || len(parent.AllowedActions) != 0 {
		return nil, ErrParentNotDerivable
	}
	// ZDR follows the parent. A ZDR caller deriving from a non-ZDR grant
	// would obtain a grant whose run may be retained: refuse it.
	if p.PrincipalZdr && !parent.Zdr {
		return nil, ErrZdrPostureChanged
	}
	maxRun := p.MaxRunDomains
	if maxRun <= 0 {
		maxRun = DefaultMaxRunDomains
	}
	domains, err := deriveDomains(parent.AllowedDomains, p.RunDomains, maxRun, policy.explicitHosts)
	if err != nil {
		return nil, err
	}
	if p.Policy != nil {
		if err := p.Policy.CheckSession(ctx, p.OrgID, domains); err != nil {
			return nil, err
		}
	}
	id, err := newID(parent.Zdr)
	if err != nil {
		return nil, err
	}
	if s.betweenLocks != nil {
		s.betweenLocks()
	}

	s.mu.Lock()
	defer s.mu.Unlock()
	// Re-read the parent under the insert lock: it may have been revoked or
	// have expired since the first read.
	parent, err = s.getScopedLocked(p.ParentGrantID, p.OrgID, p.OwnerID)
	if err != nil {
		return nil, err
	}
	now := s.now()
	if existing := s.derivedLocked(parent.ID, p.OrgID, p.SessionKey); existing != nil {
		switch {
		case existing.Revoked:
			return nil, ErrGrantRevoked
		case now.After(existing.ExpiresAt):
			return nil, ErrGrantExpired
		case !equalStrings(existing.AllowedDomains, domains):
			return nil, ErrDeriveConflict
		}
		return clone(existing), nil
	}
	expires := now.Add(p.TTL)
	if parent.ExpiresAt.Before(expires) {
		expires = parent.ExpiresAt
	}
	if expires.Sub(now) < MinDerivedLifetime {
		return nil, ErrParentNotDerivable
	}
	maxRuns := p.MaxActivePerParent
	if maxRuns <= 0 {
		maxRuns = DefaultMaxActiveRunsPerParent
	}
	if s.activeRunsLocked(parent.ID, parent.OrgID) >= maxRuns {
		return nil, ErrTooManyGrants
	}
	g := &Grant{
		ID:             id,
		Kind:           KindRun,
		OrgID:          parent.OrgID,
		OwnerID:        parent.OwnerID,
		SessionKey:     p.SessionKey,
		ScopeURL:       parent.ScopeURL,
		Zdr:            parent.Zdr,
		AllowedDomains: domains,
		ParentGrantID:  parent.ID,
		ExpiresAt:      expires,
	}
	s.grants[id] = clone(g)
	return clone(g), nil
}

// derivedLocked returns the run grant already derived from parentID under
// sessionKey, revoked or not, or nil.
func (s *MemoryRepository) derivedLocked(parentID, orgID, sessionKey string) *Grant {
	for _, g := range s.grants {
		if g.Kind == KindRun && g.ParentGrantID == parentID && g.OrgID == orgID && g.SessionKey == sessionKey {
			return g
		}
	}
	return nil
}

// deriveDomains computes a run grant's hosts. An empty request inherits the
// parent's hosts unless the parent kind requires an explicit list or the
// parent holds more hosts than one run may inherit. An explicit list must be
// canonical, at most maxRun hosts, and each host must equal or be a subdomain
// of a parent host.
func deriveDomains(parent, requested []string, maxRun int, explicitRequired bool) ([]string, error) {
	if len(requested) == 0 {
		if explicitRequired || len(parent) > maxRun {
			return nil, ErrDeriveNeedsExplicitDomains
		}
		return append([]string(nil), parent...), nil
	}
	domains, err := NormalizeAllowedDomains(requested)
	if err != nil {
		return nil, err
	}
	if len(domains) > maxRun {
		return nil, ErrInvalidDomainPolicy
	}
	for _, host := range domains {
		if !hostWithin(host, parent) {
			return nil, ErrDeriveWidensParent
		}
	}
	return domains, nil
}

// hostWithin reports whether host equals one of the policy hosts or is a
// subdomain of one (a dot boundary, so "badexample.com" is not under
// "example.com").
func hostWithin(host string, policy []string) bool {
	for _, allowed := range policy {
		if host == allowed || strings.HasSuffix(host, "."+allowed) {
			return true
		}
	}
	return false
}

func equalStrings(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

// GetScoped returns a grant only within the verified organization. A non-empty
// ownerID additionally restricts access to the owning user.
func (s *MemoryRepository) GetScoped(_ context.Context, id, orgID, ownerID string) (*Grant, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.getScopedLocked(id, orgID, ownerID)
}

func (s *MemoryRepository) getScopedLocked(id, orgID, ownerID string) (*Grant, error) {
	g, ok := s.grants[id]
	if !ok || g.OrgID != orgID || (ownerID != "" && g.OwnerID != ownerID) {
		return nil, ErrGrantNotFound
	}
	if g.Revoked {
		return nil, ErrGrantRevoked
	}
	if s.now().After(g.ExpiresAt) {
		return nil, ErrGrantExpired
	}
	return clone(g), nil
}

// RevokeScoped revokes a grant inside the verified identity scope, and every
// grant derived from it (a sensitive approval or a derived run grant), so an
// approval cannot outlive the grant it was bound to.
func (s *MemoryRepository) RevokeScoped(_ context.Context, id, orgID, ownerID, reason string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	g, ok := s.grants[id]
	if !ok || g.OrgID != orgID || (ownerID != "" && g.OwnerID != ownerID) {
		return ErrGrantNotFound
	}
	reason = truncateReason(reason)
	revoked := map[string]struct{}{id: {}}
	for changed := true; changed; {
		changed = false
		for childID, child := range s.grants {
			if _, done := revoked[childID]; done || child.OrgID != orgID || child.ParentGrantID == "" {
				continue
			}
			if _, parentRevoked := revoked[child.ParentGrantID]; parentRevoked {
				revoked[childID] = struct{}{}
				changed = true
			}
		}
	}
	for revokedID := range revoked {
		next := clone(s.grants[revokedID])
		next.Revoked = true
		next.RevokedReason = reason
		s.grants[revokedID] = next
	}
	return nil
}

// PurgeExpired drops grants that expired before the cutoff, and any grant
// whose parent it dropped, so the in-memory store (all grants in development,
// ZDR grants behind Routed) stays bounded.
func (s *MemoryRepository) PurgeExpired(_ context.Context, before time.Time) (int64, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	var purged int64
	for id, g := range s.grants {
		if g.ExpiresAt.Before(before) {
			delete(s.grants, id)
			purged++
		}
	}
	for changed := true; changed; {
		changed = false
		for id, g := range s.grants {
			if g.ParentGrantID == "" {
				continue
			}
			if _, ok := s.grants[g.ParentGrantID]; !ok {
				delete(s.grants, id)
				purged++
				changed = true
			}
		}
	}
	return purged, nil
}

// activeLocked counts the owner's session grants that are neither revoked nor
// expired. Approvals and run grants are children of those and are bounded per
// parent instead (see DefaultMaxActiveRunsPerParent).
func (s *MemoryRepository) activeLocked(orgID, ownerID string) int {
	now := s.now()
	active := 0
	for _, g := range s.grants {
		if g.OrgID == orgID && g.OwnerID == ownerID && g.Kind != KindSensitiveApproval && g.Kind != KindRun &&
			!g.Revoked && !now.After(g.ExpiresAt) {
			active++
		}
	}
	return active
}

// activeRunsLocked counts the live run grants derived from parentID.
func (s *MemoryRepository) activeRunsLocked(parentID, orgID string) int {
	now := s.now()
	active := 0
	for _, g := range s.grants {
		if g.Kind == KindRun && g.ParentGrantID == parentID && g.OrgID == orgID &&
			!g.Revoked && !now.After(g.ExpiresAt) {
			active++
		}
	}
	return active
}

func truncateReason(reason string) string {
	reason = strings.TrimSpace(reason)
	if len(reason) <= maxRevokeReasonBytes {
		return reason
	}
	cut := maxRevokeReasonBytes
	for cut > 0 && reason[cut]&0xC0 == 0x80 {
		cut--
	}
	return reason[:cut]
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

func newID(zdr bool) (string, error) {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	id := hex.EncodeToString(b)
	if zdr {
		return zdrIDPrefix + id, nil
	}
	return id, nil
}
