package grant

// Human-browse grants, renewal and attach derivation (WS8.C).
//
// A human_browse grant is a signed-in person's own open-web browsing session:
// no allow-list, every public host except the broker's deny-list. It is never
// lent to an agent as it is. A run grant (ai_run) or an attach grant derived
// from it must name its hosts explicitly, so open web is never inherited.
//
// The grant is renewable to a hard absolute cap (BROWSER_BROKER_HUMAN_ABSOLUTE_
// TTL_SECONDS, four hours by default). An attach grant is a short child bound
// to one run (resource_id); it is never renewable.

import (
	"context"
	"errors"
	"sort"
	"strings"
	"time"
)

var (
	// ErrAbsoluteCapReached means a renewal cannot extend the grant: it already
	// expires at its absolute backstop.
	ErrAbsoluteCapReached = errors.New("browser grant is at its absolute expiry")
	// ErrRenewNotAllowed means the grant's kind cannot be renewed (only session
	// and human_browse can) or the caller is not a user.
	ErrRenewNotAllowed = errors.New("browser grant cannot be renewed")
	// ErrHumanBrowseDisabled means the broker's policy does not issue
	// human_browse grants (BROWSER_BROKER_HUMAN_BROWSE_ENABLED is off).
	ErrHumanBrowseDisabled = errors.New("human browse grants are disabled")
	// ErrAttachZdrRefused means an attach grant was requested under a Zero Data
	// Retention parent: an attach lane persists receipts a ZDR run must not.
	ErrAttachZdrRefused = errors.New("an attach grant cannot derive from a zero data retention grant")
	// ErrAttachNeedsOwner means an attach grant was requested without a user
	// owner; only the person who holds the parent may attach an agent to it.
	ErrAttachNeedsOwner = errors.New("an attach grant needs a user owner")
	// ErrAttachResourceInvalid means the resource_id of an attach grant is
	// missing or unbounded, or an ai_run derive carried one.
	ErrAttachResourceInvalid = errors.New("attach grant needs a bounded resource_id, and only an attach grant takes one")
)

// Policy modes a grant reports.
const (
	// PolicyModeAllowlist: AllowedDomains is the whole navigation policy.
	PolicyModeAllowlist = "allowlist"
	// PolicyModeOpenWeb: every public host except DeniedDomains.
	PolicyModeOpenWeb = "open_web"
)

// DeriveRunGrant purposes.
const (
	// PurposeAIRun derives a run grant (the WS1.B path).
	PurposeAIRun = "ai_run"
	// PurposeAttach derives an attach grant bound to one run (WS8.C).
	PurposeAttach = "attach"
)

const (
	// DefaultHumanAbsoluteTTL is the hard cap of a human_browse session
	// (BROWSER_BROKER_HUMAN_ABSOLUTE_TTL_SECONDS).
	DefaultHumanAbsoluteTTL = 4 * time.Hour
	// DefaultMaxAttachDomains bounds the hosts of one attach grant
	// (BROWSER_BROKER_MAX_ATTACH_DOMAINS).
	DefaultMaxAttachDomains = 8
	// AttachMaxTTL is the longest attach grant; it is also capped by its parent.
	AttachMaxTTL = 15 * time.Minute

	maxDeniedDomains   = 64
	maxResourceIDBytes = 256
)

// DefaultDeniedSuffixes are the namespaces an open-web grant may never reach:
// they resolve inside a deployment or name a cloud metadata service. A name
// with a leading dot is a suffix; one without matches itself and its
// subdomains.
var DefaultDeniedSuffixes = []string{
	".internal",
	".local",
	".localhost",
	".localdomain",
	".lan",
	".home.arpa",
	".svc",
	"metadata.google.internal",
	"metadata.goog",
}

// HumanBrowsePolicy is what a PolicyResolver says about human_browse grants.
type HumanBrowsePolicy struct {
	// Allowed is false when human_browse grants are not issued.
	Allowed bool
	// DeniedSuffixes is the deny-list of the issued grant (never empty when
	// Allowed).
	DeniedSuffixes []string
}

// CreateHumanBrowseParams describes a human_browse grant.
type CreateHumanBrowseParams struct {
	OrgID, OwnerID, SessionKey, ScopeURL string
	// DeniedDomains is the grant's deny-list, normally from the policy. It must
	// not be empty: an open-web grant with no deny-list is refused.
	DeniedDomains []string
	// TTL is the initial and per-renewal lifetime.
	TTL time.Duration
	// AbsoluteTTL is the hard cap measured from creation; it is never shorter
	// than TTL (a shorter value is raised to it). Zero means TTL: not renewable
	// past the first expiry.
	AbsoluteTTL time.Duration
	Zdr         bool
	// WebPolicy is the org web-policy snapshot read from the verified token (WS7.K);
	// nil is none. The grant keeps it, so an attach child derived from it must pass
	// the snapshot's deny rules (planDerive).
	WebPolicy *WebPolicySnapshot
	// MaxActivePerOwner > 0 refuses creation with ErrTooManyGrants when the
	// owner already holds that many active ordinary grants (session and
	// human_browse together). The check and the insert are one atomic step.
	MaxActivePerOwner int
}

// RenewParams describes a renewal.
type RenewParams struct {
	OrgID, OwnerID, GrantID string
	// TTL is the lifetime the renewal asks for, counted from now; the new
	// expiry is never past the grant's absolute expiry nor before its current
	// one.
	TTL time.Duration
}

// absoluteFrom is the absolute expiry of a grant created at now.
func absoluteFrom(now time.Time, ttl, absoluteTTL time.Duration) time.Time {
	if absoluteTTL < ttl {
		absoluteTTL = ttl
	}
	return now.Add(absoluteTTL)
}

// renewable reports whether a grant can be renewed: a session or human_browse
// grant, never a key grant (INNH-10: its lifetime is fixed at issue, even for
// the key's owner).
func renewable(g *Grant) bool {
	return (g.Kind == KindSession || g.Kind == KindHumanBrowse) && g.KeyID == ""
}

// renewedExpiry computes the expiry a renewal at now gives g, or
// ErrAbsoluteCapReached when there is nothing to give. The caller has already
// checked the grant is active and renewable.
func renewedExpiry(g *Grant, now time.Time, ttl time.Duration) (time.Time, error) {
	abs := g.AbsoluteExpiresAt
	if abs.IsZero() {
		abs = g.ExpiresAt
	}
	if !g.ExpiresAt.Before(abs) {
		return time.Time{}, ErrAbsoluteCapReached
	}
	next := now.Add(ttl)
	if next.After(abs) {
		next = abs
	}
	if next.Before(g.ExpiresAt) {
		next = g.ExpiresAt
	}
	return next, nil
}

// NormalizeDeniedDomains canonicalizes a deny-list: lowercase, trimmed,
// de-duplicated and sorted. An entry is a DNS name, optionally with one
// leading dot (a pure suffix). It never contains a scheme, path, port,
// wildcard or IP literal.
func NormalizeDeniedDomains(entries []string) ([]string, error) {
	if len(entries) == 0 || len(entries) > maxDeniedDomains {
		return nil, ErrInvalidDomainPolicy
	}
	canonical := make(map[string]struct{}, len(entries))
	for _, raw := range entries {
		entry := strings.ToLower(strings.TrimSpace(raw))
		name := strings.TrimPrefix(entry, ".")
		if !validDeniedName(name) {
			return nil, ErrInvalidDomainPolicy
		}
		canonical[entry] = struct{}{}
	}
	out := make([]string, 0, len(canonical))
	for entry := range canonical {
		out = append(out, entry)
	}
	sort.Strings(out)
	return out, nil
}

func validDeniedName(name string) bool {
	if name == "" || len(name) > 253 || strings.HasPrefix(name, ".") || strings.HasSuffix(name, ".") {
		return false
	}
	for _, label := range strings.Split(name, ".") {
		if len(label) == 0 || len(label) > 63 || label[0] == '-' || label[len(label)-1] == '-' {
			return false
		}
		for _, c := range label {
			if (c < 'a' || c > 'z') && (c < '0' || c > '9') && c != '-' {
				return false
			}
		}
	}
	return true
}

// hostDenied reports whether host is under an entry of a deny-list.
func hostDenied(host string, denied []string) bool {
	for _, entry := range denied {
		if strings.HasPrefix(entry, ".") {
			if strings.HasSuffix(host, entry) {
				return true
			}
			continue
		}
		if host == entry || strings.HasSuffix(host, "."+entry) {
			return true
		}
	}
	return false
}

func validResourceID(id string) bool {
	if id == "" || len(id) > maxResourceIDBytes || !validText(id) {
		return false
	}
	return !strings.ContainsFunc(id, func(r rune) bool { return r <= 0x1f || r == 0x7f })
}

// attachParents lists the kinds an attach grant may derive from: the person's
// own session or human_browse grant. The host list is always explicit.
var attachParents = map[Kind]derivePolicy{
	KindSession:     {explicitHosts: true},
	KindHumanBrowse: {explicitHosts: true},
}

// derivePlan is what both stores need to know before they write a child: the
// kind, the hosts, the claim the request asked for, the bound resource and the
// lifetime. planDerive holds every rule that does not depend on storage.
type derivePlan struct {
	kind            Kind
	domains         []string
	requestedBudget Budget
	resourceID      string
	ttl             time.Duration
}

// planDerive checks a derive request against its parent (already read, active
// and scoped) and returns the child it describes.
func planDerive(ctx context.Context, p DeriveParams, parent *Grant) (derivePlan, error) {
	plan := derivePlan{kind: KindRun, ttl: p.TTL}
	policy, ok := derivableParents[parent.Kind]
	switch p.Purpose {
	case "", PurposeAIRun:
		if p.ResourceID != "" {
			return derivePlan{}, ErrAttachResourceInvalid
		}
	case PurposeAttach:
		plan.kind = KindAttach
		policy, ok = attachParents[parent.Kind]
	default:
		return derivePlan{}, ErrParentNotDerivable
	}
	// A key grant (INNH-10) is never a parent: neither the key nor its owner
	// can widen or lengthen it through a child.
	if !ok || len(parent.AllowedActions) != 0 || parent.KeyID != "" {
		return derivePlan{}, ErrParentNotDerivable
	}
	if plan.kind == KindAttach {
		if parent.Zdr {
			return derivePlan{}, ErrAttachZdrRefused
		}
		if p.OwnerID == "" {
			return derivePlan{}, ErrAttachNeedsOwner
		}
		if !validResourceID(p.ResourceID) {
			return derivePlan{}, ErrAttachResourceInvalid
		}
		plan.resourceID = p.ResourceID
		plan.ttl = min(p.TTL, AttachMaxTTL)
	}
	// ZDR follows the parent. A ZDR caller deriving from a non-ZDR grant
	// would obtain a grant whose run may be retained: refuse it.
	if p.PrincipalZdr && !parent.Zdr {
		return derivePlan{}, ErrZdrPostureChanged
	}
	maxRun := p.MaxRunDomains
	if maxRun <= 0 {
		maxRun = DefaultMaxRunDomains
	}
	domains, err := deriveDomainsFrom(parent, p.RunDomains, maxRun, policy.explicitHosts)
	if err != nil {
		return derivePlan{}, err
	}
	// The child keeps the parent's snapshot, so its hosts must pass the
	// parent's deny rules too (WS7.K), inherited or named alike.
	if err := parent.WebPolicy.checkDeny(domains); err != nil {
		return derivePlan{}, err
	}
	plan.domains = domains
	plan.requestedBudget, err = NormalizeBudget(p.Budget)
	if err != nil {
		return derivePlan{}, err
	}
	if p.Policy != nil {
		if err := p.Policy.CheckSession(ctx, p.OrgID, domains); err != nil {
			return derivePlan{}, err
		}
	}
	return plan, nil
}

// deriveDomainsFrom is deriveDomains for a parent grant: an open-web parent has
// no host list to narrow, so its child must name hosts (never inherited). The
// org web policy the grant carries narrows those hosts as it narrows an ordinary
// grant's (Narrow): an allow rule narrows them to the hosts it covers and a deny
// rule refuses them. None may be under the parent's deny-list.
func deriveDomainsFrom(parent *Grant, requested []string, maxRun int, explicitRequired bool) ([]string, error) {
	if parent.PolicyMode != PolicyModeOpenWeb {
		return deriveDomains(parent, requested, maxRun, explicitRequired)
	}
	if len(requested) == 0 {
		return nil, ErrDeriveNeedsExplicitDomains
	}
	domains, err := NormalizeAllowedDomains(requested)
	if err != nil {
		return nil, err
	}
	// Narrowing can name an allowed host below a requested one, so the run bound
	// is read from what it returns.
	domains, err = Narrow(domains, parent.WebPolicy)
	if err != nil {
		return nil, err
	}
	if len(domains) > maxRun {
		return nil, ErrInvalidDomainPolicy
	}
	for _, host := range domains {
		if hostDenied(host, parent.DeniedDomains) {
			return nil, ErrDomainNotGrantable
		}
	}
	return domains, nil
}

// CreateHumanBrowse issues a human_browse grant: open web minus a deny-list,
// renewable to an absolute cap. Its host list is empty by design.
func (s *MemoryRepository) CreateHumanBrowse(_ context.Context, p CreateHumanBrowseParams) (*Grant, error) {
	denied, err := NormalizeDeniedDomains(p.DeniedDomains)
	if err != nil {
		return nil, err
	}
	id, err := newID(p.Zdr)
	if err != nil {
		return nil, err
	}
	now := s.now()
	g := &Grant{
		ID:                id,
		Kind:              KindHumanBrowse,
		OrgID:             p.OrgID,
		OwnerID:           p.OwnerID,
		SessionKey:        p.SessionKey,
		ScopeURL:          p.ScopeURL,
		Zdr:               p.Zdr,
		WebPolicy:         p.WebPolicy.Clone(),
		PolicyMode:        PolicyModeOpenWeb,
		DeniedDomains:     denied,
		ExpiresAt:         now.Add(p.TTL),
		AbsoluteExpiresAt: absoluteFrom(now, p.TTL, p.AbsoluteTTL),
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if p.MaxActivePerOwner > 0 && s.activeLocked(p.OrgID, p.OwnerID) >= p.MaxActivePerOwner {
		return nil, ErrTooManyGrants
	}
	s.grants[id] = clone(g)
	return clone(g), nil
}

// Renew extends a session or human_browse grant by one TTL, never past its
// absolute expiry. It needs a user owner (OwnerID) and the grant must be
// active; every other kind is ErrRenewNotAllowed.
func (s *MemoryRepository) Renew(_ context.Context, p RenewParams) (*Grant, error) {
	if p.OwnerID == "" {
		return nil, ErrRenewNotAllowed
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	g, err := s.getScopedLocked(p.GrantID, p.OrgID, p.OwnerID)
	if err != nil {
		return nil, err
	}
	if !renewable(g) {
		return nil, ErrRenewNotAllowed
	}
	next, err := renewedExpiry(g, s.now(), p.TTL)
	if err != nil {
		return nil, err
	}
	g.ExpiresAt = next
	s.grants[g.ID] = clone(g)
	return g, nil
}

// NewMemoryRepositoryWithClock is NewMemoryRepository reading now instead of
// time.Now. Tests of the layers above (the gRPC server) use it to step a
// renewal to its absolute cap without sleeping.
func NewMemoryRepositoryWithClock(now func() time.Time) *MemoryRepository {
	r := NewMemoryRepository()
	r.now = now
	return r
}
