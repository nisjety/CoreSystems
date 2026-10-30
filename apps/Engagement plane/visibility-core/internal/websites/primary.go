package websites

import (
	"context"
	"strings"
)

// Where the primary site came from, as a list says it. A primary site has one source: org-core.
const (
	PrimaryFromOrgCore   = "org-core"
	PrimaryUnavailable   = "unavailable"
	PrimaryNotConfigured = "not_configured"
)

// PrimarySource returns the website org-core holds for the organisation. Empty means it holds none.
type PrimarySource func(ctx context.Context) (string, error)

// SetPrimarySource connects the registry to org-core. Without a source the registry lists its sites with
// none marked as primary, as it always did.
func (s *Service) SetPrimarySource(source PrimarySource) { s.primary = source }

// ListWithPrimary lists the registered websites and marks the organisation's primary one: the website org-core
// holds for it. A primary site that is not registered yet is added once, so it can carry integrations like any
// other site. The other sites are listed as they were. The organisation can still add as many as it needs.
// The status says where the primary came from, and a failure reading org-core still lists every site.
func (s *Service) ListWithPrimary(ctx context.Context) ([]Website, string) {
	sites := s.List()
	if s.primary == nil {
		return sites, PrimaryNotConfigured
	}
	site, err := s.primary(ctx)
	if err != nil {
		return sites, PrimaryUnavailable
	}
	if strings.TrimSpace(site) == "" {
		return sites, PrimaryFromOrgCore
	}
	canonical, err := canonicalInput(site)
	if err != nil {
		return sites, PrimaryUnavailable
	}
	if !listed(sites, canonical) {
		if _, _, err := s.Add(ctx, canonical); err != nil {
			return sites, PrimaryUnavailable
		}
		sites = s.List()
	}
	for i := range sites {
		sites[i].Primary = sites[i].Site == canonical
	}
	return sites, PrimaryFromOrgCore
}

func listed(sites []Website, site string) bool {
	for _, website := range sites {
		if website.Site == site {
			return true
		}
	}
	return false
}
