package grant

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"

	"github.com/triodelab/model-plane/pkg/authctx"
)

// ErrDomainNotGrantable means a host passed the syntax check but may not be
// put in a grant: it is internal, or the policy bounds were exceeded. The
// message never echoes the hosts.
var ErrDomainNotGrantable = errors.New("browser host is not grantable")

// PolicyResolver decides whether a set of canonical hosts may be granted to an
// organization. It runs after NormalizeAllowedDomains and before storage, so a
// later policy source (per-org allowlists, ADR-1) replaces StaticPolicy
// without touching issuance.
type PolicyResolver interface {
	CheckSession(ctx context.Context, orgID string, domains []string) error
	// ResolveHumanBrowse says whether human_browse (open-web) grants are issued
	// to this user and which deny-list they carry (WS8.C). The org override is
	// the ADR-1 hook and is not implemented: StaticPolicy ignores both ids.
	ResolveHumanBrowse(ctx context.Context, orgID, ownerID string) (HumanBrowsePolicy, error)
}

// DefaultMaxSessionDomains bounds one session grant.
const DefaultMaxSessionDomains = 8

// deniedSuffixes are namespaces that resolve inside a deployment, never on the
// public internet. A browser that can navigate them can reach private services.
var deniedSuffixes = []string{
	".internal",
	".local",
	".localhost",
	".localdomain",
	".svc",
}

// deniedHosts are cloud metadata names that are not under a denied suffix.
var deniedHosts = []string{
	"metadata.goog",
}

// StaticPolicy is the default PolicyResolver: a domain-count cap and a
// deny-list of internal namespaces. It is stateless and org-independent.
type StaticPolicy struct {
	// MaxDomains caps hosts per grant. Zero or negative uses
	// DefaultMaxSessionDomains.
	MaxDomains int
	// HumanBrowseEnabled turns human_browse issuance on
	// (BROWSER_BROKER_HUMAN_BROWSE_ENABLED). The zero value is off.
	HumanBrowseEnabled bool
	// DeniedSuffixes is the deny-list of an issued human_browse grant. Nil or
	// empty uses DefaultDeniedSuffixes, so enabling the kind can never issue an
	// open-web grant with no deny-list.
	DeniedSuffixes []string
}

// CheckSession implements PolicyResolver.
func (p StaticPolicy) CheckSession(_ context.Context, _ string, domains []string) error {
	limit := p.MaxDomains
	if limit <= 0 {
		limit = DefaultMaxSessionDomains
	}
	if len(domains) > limit {
		return ErrDomainNotGrantable
	}
	for _, domain := range domains {
		host := strings.ToLower(domain)
		for _, suffix := range deniedSuffixes {
			if strings.HasSuffix(host, suffix) {
				return ErrDomainNotGrantable
			}
		}
		for _, denied := range deniedHosts {
			if host == denied || strings.HasSuffix(host, "."+denied) {
				return ErrDomainNotGrantable
			}
		}
	}
	return nil
}

// ResolveHumanBrowse implements PolicyResolver.
func (p StaticPolicy) ResolveHumanBrowse(_ context.Context, _, _ string) (HumanBrowsePolicy, error) {
	if !p.HumanBrowseEnabled {
		return HumanBrowsePolicy{}, nil
	}
	denied := p.DeniedSuffixes
	if len(denied) == 0 {
		denied = DefaultDeniedSuffixes
	}
	return HumanBrowsePolicy{Allowed: true, DeniedSuffixes: append([]string(nil), denied...)}, nil
}

// ErrDomainDenied means a requested host is refused by the org web policy
// (WS7.K): the claim's deny rules name it or a host under it, or no host of the
// request falls under the claim's allow rules. It wraps ErrDomainNotGrantable,
// so it maps to the same refusal; the message never echoes the hosts.
var ErrDomainDenied = fmt.Errorf("%w: the org web policy denies it", ErrDomainNotGrantable)

// ErrWebPolicyUnreadable means the verified token carries a web_policy claim
// the broker cannot read: malformed JSON, a version other than 1, a bad
// revision or an unbounded entry. Acquire refuses rather than mint a grant
// without the org's rules.
var ErrWebPolicyUnreadable = errors.New("web_policy_unreadable")

// WebPolicyClaimVersion is the only web_policy claim version.
const WebPolicyClaimVersion = 1

// Bounds on a web_policy snapshot. Auth Core caps the claim at 2 KiB, so these
// are guards, not code paths.
const (
	maxWebPolicyCaps        = 64
	maxWebPolicyCapBytes    = 128
	maxWebPolicyDomains     = 256
	maxWebPolicyDomainBytes = 253
	maxWebPolicyRevBytes    = 12
)

// WebPolicySnapshot is the org web policy a grant carries (WS7.K, ADR-1
// outcome C): the verified principal's web_policy claim, mapped 1:1. It is
// built from the verified token only, never from a request field, and a
// derived grant copies its parent's unchanged, so it can never loosen. Nil is
// no snapshot (the token carried no claim), which a consumer reads as no
// grant layer.
type WebPolicySnapshot struct {
	// V is the claim version (1).
	V uint32
	// Rev is the issuer's revision: "0" (no policy) or 1-12 lowercase hex.
	Rev string
	// Caps maps a capability key to its mode or ceiling name, as signed.
	Caps map[string]string
	// RobotsMax and ProxyMax are the org's ceilings, "" when none is stated.
	RobotsMax, ProxyMax string
	// DomainsAllow is the allow rule; empty means no allow rule.
	DomainsAllow []string
	// DomainsDeny is the deny rule.
	DomainsDeny []string
}

// Clone returns a deep copy, or nil for nil.
func (s *WebPolicySnapshot) Clone() *WebPolicySnapshot {
	if s == nil {
		return nil
	}
	out := *s
	if s.Caps != nil {
		out.Caps = make(map[string]string, len(s.Caps))
		for k, v := range s.Caps {
			out.Caps[k] = v
		}
	}
	out.DomainsAllow = append([]string(nil), s.DomainsAllow...)
	out.DomainsDeny = append([]string(nil), s.DomainsDeny...)
	return &out
}

// webPolicyClaim is the web_policy claim exactly as Auth Core signs it:
// {"v":1,"rev":"...","caps":{...},"robots_max":...,"proxy_max":...,
// "domains":{"allow":[...],"deny":[...]}}.
type webPolicyClaim struct {
	V         *uint32           `json:"v"`
	Rev       *string           `json:"rev"`
	Caps      map[string]string `json:"caps"`
	RobotsMax string            `json:"robots_max"`
	ProxyMax  string            `json:"proxy_max"`
	Domains   *struct {
		Allow []string `json:"allow"`
		Deny  []string `json:"deny"`
	} `json:"domains"`
}

// ClaimPolicyResolver reads the org web policy from the verified principal's
// web_policy claim (WS7.K). It is stateless and asks no service: the claim is
// already signed by Auth Core. A web_policy in gRPC metadata or a request body
// is never read.
type ClaimPolicyResolver struct{}

// Snapshot builds the grant snapshot from p.WebPolicy. It returns nil, nil
// when the token carries no claim, and ErrWebPolicyUnreadable when the claim
// is malformed, its version is not 1, its revision is not "0" or 1-12
// lowercase hex, or an entry is empty or unbounded.
func (ClaimPolicyResolver) Snapshot(p authctx.Principal) (*WebPolicySnapshot, error) {
	raw := bytes.TrimSpace(p.WebPolicy)
	if len(raw) == 0 || bytes.Equal(raw, []byte("null")) {
		return nil, nil
	}
	var claim webPolicyClaim
	if err := json.Unmarshal(raw, &claim); err != nil {
		return nil, ErrWebPolicyUnreadable
	}
	if claim.V == nil || *claim.V != WebPolicyClaimVersion || claim.Rev == nil || !validWebPolicyRev(*claim.Rev) {
		return nil, ErrWebPolicyUnreadable
	}
	snapshot := &WebPolicySnapshot{V: *claim.V, Rev: *claim.Rev}
	if len(claim.Caps) > maxWebPolicyCaps {
		return nil, ErrWebPolicyUnreadable
	}
	if len(claim.Caps) > 0 {
		snapshot.Caps = make(map[string]string, len(claim.Caps))
		for key, mode := range claim.Caps {
			if !boundedClaimText(key, maxWebPolicyCapBytes) || !boundedClaimText(mode, maxWebPolicyCapBytes) {
				return nil, ErrWebPolicyUnreadable
			}
			snapshot.Caps[key] = mode
		}
	}
	for _, ceiling := range []string{claim.RobotsMax, claim.ProxyMax} {
		if ceiling != "" && !boundedClaimText(ceiling, maxWebPolicyCapBytes) {
			return nil, ErrWebPolicyUnreadable
		}
	}
	snapshot.RobotsMax, snapshot.ProxyMax = claim.RobotsMax, claim.ProxyMax
	if claim.Domains != nil {
		allow, err := claimDomains(claim.Domains.Allow)
		if err != nil {
			return nil, err
		}
		deny, err := claimDomains(claim.Domains.Deny)
		if err != nil {
			return nil, err
		}
		snapshot.DomainsAllow, snapshot.DomainsDeny = allow, deny
	}
	return snapshot, nil
}

// Narrow applies a snapshot's domain rules to a canonical requested host list
// (the output of NormalizeAllowedDomains). A nil snapshot changes nothing. A
// requested host under a deny rule (equal or below it, on a dot boundary) is
// refused with ErrDomainDenied, never silently dropped, and so is a kept host
// that reaches a denied host: a grant covers the subdomains of its hosts, so a
// deny rule below the host is reached too (reachesDenied). With an allow rule
// the result is the intersection: a requested host equal to or below an allowed
// host is kept, and an allowed host below a requested host replaces it; an
// allowed host that reaches a denied host is never added. An empty result is
// ErrDomainDenied. The result is canonical (sorted, unique).
func Narrow(requested []string, s *WebPolicySnapshot) ([]string, error) {
	if s == nil {
		return append([]string(nil), requested...), nil
	}
	allow := ruleNames(s.DomainsAllow)
	deny := ruleNames(s.DomainsDeny)
	kept := make([]string, 0, len(requested))
	for _, raw := range requested {
		host := strings.ToLower(strings.TrimSpace(raw))
		if hostWithin(host, deny) {
			return nil, ErrDomainDenied
		}
		if len(s.DomainsAllow) == 0 || hostWithin(host, allow) {
			if reachesDenied(host, deny) {
				return nil, ErrDomainDenied
			}
			kept = append(kept, host)
			continue
		}
		for _, allowed := range allow {
			if strings.HasSuffix(allowed, "."+host) && isCanonicalDomain(allowed) && !reachesDenied(allowed, deny) {
				kept = append(kept, allowed)
			}
		}
	}
	if len(kept) == 0 {
		return nil, ErrDomainDenied
	}
	return NormalizeAllowedDomains(kept)
}

// ruleNames turns claim domain rules into names to match on a dot boundary: a
// leading "*." or "." marks a suffix rule, which matches the same hosts.
func ruleNames(rules []string) []string {
	out := make([]string, 0, len(rules))
	for _, rule := range rules {
		name := strings.ToLower(strings.TrimSpace(rule))
		name = strings.TrimPrefix(name, "*.")
		name = strings.TrimPrefix(name, ".")
		if name != "" {
			out = append(out, name)
		}
	}
	return out
}

// reachesDenied reports whether a grant naming host would reach a denied host:
// host is a deny rule or sits below one (hostWithin), or a deny rule sits below
// host. A grant covers the subdomains of its hosts, so a grant for example.com
// reaches evil.example.com when that is a deny rule.
func reachesDenied(host string, deny []string) bool {
	if hostWithin(host, deny) {
		return true
	}
	for _, rule := range deny {
		if strings.HasSuffix(rule, "."+host) {
			return true
		}
	}
	return false
}

// checkDeny returns ErrDomainDenied when the snapshot's deny rules refuse any of
// hosts. A derived or approval grant copies its parent's snapshot, so the
// parent's deny rules hold for its hosts whether they were inherited or named.
// A nil snapshot refuses nothing.
func (s *WebPolicySnapshot) checkDeny(hosts []string) error {
	if s == nil {
		return nil
	}
	deny := ruleNames(s.DomainsDeny)
	for _, host := range hosts {
		if reachesDenied(host, deny) {
			return ErrDomainDenied
		}
	}
	return nil
}

// claimDomains copies a claim domain list after bounding it.
func claimDomains(list []string) ([]string, error) {
	if len(list) > maxWebPolicyDomains {
		return nil, ErrWebPolicyUnreadable
	}
	out := make([]string, 0, len(list))
	for _, entry := range list {
		if !boundedClaimText(entry, maxWebPolicyDomainBytes) {
			return nil, ErrWebPolicyUnreadable
		}
		out = append(out, entry)
	}
	if len(out) == 0 {
		return nil, nil
	}
	return out, nil
}

// validWebPolicyRev reports "0" or 1-12 lowercase hex.
func validWebPolicyRev(rev string) bool {
	if rev == "0" {
		return true
	}
	if rev == "" || len(rev) > maxWebPolicyRevBytes {
		return false
	}
	for i := 0; i < len(rev); i++ {
		c := rev[i]
		if (c < '0' || c > '9') && (c < 'a' || c > 'f') {
			return false
		}
	}
	return true
}

// boundedClaimText reports a non-empty, bounded, printable ASCII value.
func boundedClaimText(value string, limit int) bool {
	if value == "" || len(value) > limit {
		return false
	}
	for i := 0; i < len(value); i++ {
		if value[i] < 0x21 || value[i] > 0x7e {
			return false
		}
	}
	return true
}
