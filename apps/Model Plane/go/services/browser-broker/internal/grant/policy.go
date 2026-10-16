package grant

import (
	"context"
	"errors"
	"strings"
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
