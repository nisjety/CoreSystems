package grant

// Origin-extension grants (E3.2-e1, OD13).
//
// A run grant names the hosts a run may reach. When the run needs one more
// origin, the person is asked; if they consent, the broker issues an
// origin_extension grant: a child of the run (or session) grant that adds the
// consented hosts and nothing else. It is the broker's record of the consent,
// so Quarry can widen its egress policy (configure_egress_policy) only while
// the extension validates, and the widening ends with the grant.
//
// What the kind guarantees (each pinned by a test):
//
//   - parent-bound: it is a child of one live session or run grant of the same
//     owner, org and session key; revoking the parent revokes it (the ordinary
//     ParentGrantID cascade) and its expiry is never past the parent's;
//   - exact hosts only: no wildcard, no suffix pattern (see
//     ConsentPatternLanguage), at most MaxExtensionHosts hosts across the live
//     extensions of one parent (OD13: the start host plus at most eight
//     user-listed domains);
//   - deny beats approval: a host equal to, below or above a denied entry is
//     dropped, and an extension left with no host is refused;
//   - never an approval or derive parent, never renewed, never a key grant's
//     child: no person stands behind a key to consent;
//   - Zero Data Retention: turn scope only, the child of a ZDR parent lives in
//     memory with a "z_" id.
//
// Everything is dark behind BROWSER_BROKER_ORIGIN_EXTENSION_ENABLED.

import (
	"context"
	"errors"
	"sort"
	"strings"
	"time"
)

// KindOriginExtension is a parent-bound grant that adds consented hosts to a
// run (E3.2-e1). A consumer that does not know the kind treats the grant as
// not valid, which is the safe reading.
const KindOriginExtension Kind = "origin_extension"

// Consent scopes: how long the person meant the consent to last. The broker
// stores the label and caps the grant's lifetime by its parent; "conversation"
// and "always" are renewed by the caller asking again, and persisting them is
// Control Plane's browser_consent setting (E3.2-e3), not the broker's.
const (
	ConsentScopeTurn         = "turn"
	ConsentScopeConversation = "conversation"
	ConsentScopeAlways       = "always"
)

// MaxExtensionHosts is the most hosts the live extensions of one parent may
// add together, and the most live extension grants one parent may hold.
const MaxExtensionHosts = 8

// ConsentPatternLanguageApproved is false: the pattern language below is a
// draft that legal counsel has not approved (OD13). Nothing in the broker
// parses a pattern; every consent host is an exact canonical host, and a
// wildcard is refused (ErrWildcardRefused). Turning this true is a reviewed
// code change that also changes isCanonicalDomain, NormalizeExtensionHosts,
// the Quarry BrowserEgressPolicy and the Control Plane browser_consent
// setting together; flipping the constant alone changes nothing.
const ConsentPatternLanguageApproved = false

// ConsentPatternLanguage is the draft grammar of what a person may consent to
// (OD13), kept here as the one place the proposal is written. It is a
// constant for review, not a parser input.
const ConsentPatternLanguage = `consent-pattern-v0 (DRAFT, not approved: pending counsel, OD13)

  consent    = origin *( "," origin )          ; at most 8 per parent grant
  origin     = host                            ; exact DNS host, lowercase
  host       = label 1*( "." label )           ; as isCanonicalDomain
  scope      = "turn" / "conversation" / "always"
  denied     = host *( "," host )              ; same decision, optional

  The start host is the parent grant's and is not listed.
  No scheme, port, path, query, IP literal, "*", "?" or leading ".".
  A listed host covers itself and its subdomains (a dot boundary), exactly
  as a session grant's allowed_domains do.
  Deny beats approval: an approved host equal to, below or above a denied
  entry is dropped. An empty remainder is refused.
  ZDR: scope "turn" only.
  Wildcard and suffix patterns: NOT AVAILABLE until counsel approves this
  language and isCanonicalDomain, the broker policy and the Quarry
  BrowserEgressPolicy change in one reviewed change.
`

var (
	// ErrConsentScopeInvalid means consent_scope is missing or not one of the
	// three scopes.
	ErrConsentScopeInvalid = errors.New("consent_scope must be turn, conversation or always")
	// ErrConsentScopeZdr means a Zero Data Retention consent asked for more than
	// turn scope: a longer consent would have to persist host names.
	ErrConsentScopeZdr = errors.New("a zero data retention consent is turn scope only")
	// ErrWildcardRefused means a consent origin carries pattern syntax. No
	// pattern language is approved (ConsentPatternLanguageApproved).
	ErrWildcardRefused = errors.New("consent origins must be exact hostnames; wildcard patterns are not available")
	// ErrExtensionParentInvalid means the parent cannot take an origin
	// extension: not a live session or run grant of the same session key, a
	// sensitive scope, an open-web parent, or a key grant. The message never
	// says which.
	ErrExtensionParentInvalid = errors.New("browser grant cannot take an origin extension")
	// ErrExtensionNothingNew means every requested host is already covered by
	// the parent grant.
	ErrExtensionNothingNew = errors.New("the parent grant already covers every requested origin")
	// ErrExtensionHostCap means the live extensions of the parent would pass
	// MaxExtensionHosts hosts together.
	ErrExtensionHostCap = errors.New("origin extensions of this grant would exceed the host cap")
	// ErrExtensionDenied means no requested origin survived the denies.
	ErrExtensionDenied = errors.New("every requested origin is denied")
)

// ValidConsentScope reports one of the three consent scopes.
func ValidConsentScope(scope string) bool {
	switch scope {
	case ConsentScopeTurn, ConsentScopeConversation, ConsentScopeAlways:
		return true
	default:
		return false
	}
}

// CheckConsentScope validates scope and applies the ZDR rule: a Zero Data
// Retention consent is turn scope only.
func CheckConsentScope(scope string, zdr bool) error {
	if !ValidConsentScope(scope) {
		return ErrConsentScopeInvalid
	}
	if zdr && scope != ConsentScopeTurn {
		return ErrConsentScopeZdr
	}
	return nil
}

// NormalizeExtensionHosts canonicalizes the origins of one consent: one to
// MaxExtensionHosts exact hosts. Pattern syntax is refused with
// ErrWildcardRefused before anything else, so a wildcard can never be
// mistaken for a malformed host and retried in another form. The hosts go
// through the same isCanonicalDomain check as a session grant's, so an
// extension can never name a host a session grant could not.
func NormalizeExtensionHosts(domains []string) ([]string, error) {
	if len(domains) == 0 || len(domains) > MaxExtensionHosts {
		return nil, ErrInvalidDomainPolicy
	}
	for _, raw := range domains {
		host := strings.TrimSpace(raw)
		if strings.ContainsAny(host, "*?[]{}") || strings.HasPrefix(host, ".") {
			return nil, ErrWildcardRefused
		}
	}
	return NormalizeAllowedDomains(domains)
}

// ApplyConsentDenies applies "deny beats approval" to the approved hosts of
// one decision. An approved host is dropped when it overlaps a denied entry in
// any direction: equal to it, below it, or above it (a carve-out such as
// approving example.com while denying pay.example.com cannot be expressed in
// an allow-list grant, so the whole approval yields). Hosts are canonical;
// entries come from NormalizeDeniedDomains. It returns the kept and dropped
// hosts, each in the input order.
func ApplyConsentDenies(approved, denied []string) (kept, dropped []string) {
	for _, host := range approved {
		if consentOverlap(host, denied) {
			dropped = append(dropped, host)
			continue
		}
		kept = append(kept, host)
	}
	return kept, dropped
}

func consentOverlap(host string, denied []string) bool {
	for _, entry := range denied {
		name := strings.TrimPrefix(entry, ".")
		if host == name || strings.HasSuffix(host, "."+name) || strings.HasSuffix(name, "."+host) {
			return true
		}
	}
	return false
}

// ExtensionParams describes an origin-extension grant.
type ExtensionParams struct {
	OrgID, OwnerID, SessionKey, ScopeURL string
	// ParentGrantID is the session or run grant the consent is for.
	ParentGrantID string
	// AllowedDomains are the consented hosts, already stripped of denied
	// ones by the caller; the store normalizes and checks them again.
	AllowedDomains []string
	// ConsentScope is one of the three scopes (CheckConsentScope).
	ConsentScope string
	// TTL is the requested lifetime; the expiry is capped by the parent's.
	TTL time.Duration
	// Zdr is the caller's ZDR flag. A ZDR caller under a non-ZDR parent is
	// ErrZdrPostureChanged, never a ZDR child of a persisted parent.
	Zdr bool
}

// planExtension checks everything about an extension that both stores decide
// from the parent alone, and returns the hosts that are new to it. The parent
// is already scoped (live, same org and owner).
func planExtension(p ExtensionParams, parent *Grant) ([]string, error) {
	if (parent.Kind != KindSession && parent.Kind != KindRun) || parent.SessionKey != p.SessionKey ||
		len(parent.AllowedActions) != 0 || parent.KeyID != "" || parent.PolicyMode == PolicyModeOpenWeb {
		return nil, ErrExtensionParentInvalid
	}
	if p.Zdr && !parent.Zdr {
		return nil, ErrZdrPostureChanged
	}
	if err := CheckConsentScope(p.ConsentScope, parent.Zdr || p.Zdr); err != nil {
		return nil, err
	}
	hosts, err := NormalizeExtensionHosts(p.AllowedDomains)
	if err != nil {
		return nil, err
	}
	fresh := make([]string, 0, len(hosts))
	for _, host := range hosts {
		if !hostWithin(host, parent.AllowedDomains) {
			fresh = append(fresh, host)
		}
	}
	if len(fresh) == 0 {
		return nil, ErrExtensionNothingNew
	}
	return fresh, nil
}

// decideExtension looks at the parent's live extension children: an identical
// request (same hosts, same scope) returns the grant already issued; otherwise
// the child count and the union of hosts must stay within MaxExtensionHosts.
func decideExtension(children []*Grant, fresh []string, scope string) (*Grant, error) {
	for _, child := range children {
		if equalStrings(child.AllowedDomains, fresh) && child.ConsentScope == scope {
			return child, nil
		}
	}
	if len(children) >= MaxExtensionHosts {
		return nil, ErrTooManyGrants
	}
	union := make(map[string]struct{}, MaxExtensionHosts)
	for _, child := range children {
		for _, host := range child.AllowedDomains {
			union[host] = struct{}{}
		}
	}
	for _, host := range fresh {
		union[host] = struct{}{}
	}
	if len(union) > MaxExtensionHosts {
		return nil, ErrExtensionHostCap
	}
	return nil, nil
}

// extensionExpiry is the grant's expiry: the requested lifetime, never past
// the parent's.
func extensionExpiry(now time.Time, ttl time.Duration, parent *Grant) time.Time {
	expires := now.Add(ttl)
	if parent.ExpiresAt.Before(expires) {
		expires = parent.ExpiresAt
	}
	return expires
}

// newExtensionGrant builds the grant both stores insert.
func newExtensionGrant(id string, p ExtensionParams, parent *Grant, hosts []string, expires time.Time) *Grant {
	return &Grant{
		ID:                id,
		Kind:              KindOriginExtension,
		OrgID:             parent.OrgID,
		OwnerID:           parent.OwnerID,
		SessionKey:        p.SessionKey,
		ScopeURL:          p.ScopeURL,
		Zdr:               parent.Zdr,
		ReadOnly:          parent.ReadOnly,
		AllowedDomains:    append([]string(nil), hosts...),
		PolicyMode:        PolicyModeAllowlist,
		ParentGrantID:     parent.ID,
		ConsentScope:      p.ConsentScope,
		ExpiresAt:         expires,
		AbsoluteExpiresAt: expires,
	}
}

// CreateOriginExtension issues an origin-extension grant under a live session
// or run grant of the same owner, org and session key. The parent is read and
// the child inserted under one lock, so a parent revoked concurrently never
// leaves a live extension behind. An identical request returns the grant
// already issued.
func (s *MemoryRepository) CreateOriginExtension(_ context.Context, p ExtensionParams) (*Grant, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	parent, err := s.getScopedLocked(p.ParentGrantID, p.OrgID, p.OwnerID)
	if err != nil {
		return nil, err
	}
	fresh, err := planExtension(p, parent)
	if err != nil {
		return nil, err
	}
	reuse, err := decideExtension(s.liveExtensionsLocked(parent.ID, parent.OrgID), fresh, p.ConsentScope)
	if err != nil {
		return nil, err
	}
	if reuse != nil {
		return clone(reuse), nil
	}
	id, err := newID(parent.Zdr)
	if err != nil {
		return nil, err
	}
	g := newExtensionGrant(id, p, parent, fresh, extensionExpiry(s.now(), p.TTL, parent))
	s.grants[id] = clone(g)
	return clone(g), nil
}

// liveExtensionsLocked returns the live extension grants of one parent, in a
// stable order.
func (s *MemoryRepository) liveExtensionsLocked(parentID, orgID string) []*Grant {
	now := s.now()
	var out []*Grant
	for _, g := range s.grants {
		if g.Kind == KindOriginExtension && g.ParentGrantID == parentID && g.OrgID == orgID &&
			!g.Revoked && !now.After(g.ExpiresAt) {
			out = append(out, g)
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].ID < out[j].ID })
	return out
}
