package workspace

import (
	"context"
	"github.com/triodelab/engagement-plane/visibility-core/internal/siteintegrations"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"sync"
	"time"
)

// SourceStatuses is a fresh metadata projection, independent of report freshness.
// Opening the extension picker never runs analytics, audits or provider discovery.
func (s *Service) SourceStatuses(ctx context.Context, w websites.Website) map[string]string {
	out := map[string]string{}
	if w.Integrations.SearchConsole.Status == "connected" {
		out["search_console"] = "authorization_required"
	}
	if w.Integrations.GA4.Status == "connected" {
		out["ga4"] = "authorization_required"
	}
	if w.Integrations.Clarity.ProjectID != "" {
		out["clarity"] = "authorization_required"
	}
	if !boundToOrganization(s.cfg.IntegrationBearerTokenFile, s.registry.OrganizationID()) {
		return out
	}
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	broker := siteintegrations.New(siteintegrations.Config{IntegrationCoreURL: s.cfg.IntegrationCoreURL, BearerTokenFile: s.cfg.IntegrationBearerTokenFile})
	record, err := s.registry.Resolve(w.ID, w.Site)
	if err != nil || record.Website.Revision != w.Revision {
		return out
	}
	access := broker.CurrentAccess(ctx, s.registry.OrganizationID(), record)
	for key, allowed := range map[string]bool{"search_console": access.SearchConsole, "ga4": access.GA4, "clarity": access.Clarity} {
		if allowed {
			out[key] = "connected"
		} else if key == "search_console" && w.Integrations.SearchConsole.Status == "connected" || key == "ga4" && w.Integrations.GA4.Status == "connected" || key == "clarity" && w.Integrations.Clarity.ProjectID != "" {
			out[key] = "authorization_required"
		}
	}
	var mu sync.Mutex
	var wg sync.WaitGroup
	for _, key := range []string{"cookiebot", "algolia", "web_vitals"} {
		wg.Add(1)
		go func(key string) {
			defer wg.Done()
			configured, err := broker.InstallationConfigured(ctx, w, key)
			status := "not_connected"
			if err != nil {
				status = "source_unavailable"
			} else if configured {
				status = "connected"
			}
			if key == "algolia" && s.algoliaMapping != nil {
				mapping, mapErr := s.algoliaMapping.Get(ctx, w.ID, w.Site)
				if mapErr == nil && mapping.Status != "not_mapped" {
					if mapping.Status == "mapped" {
						status = "connected"
					} else {
						status = "authorization_required"
					}
				}
			}
			mu.Lock()
			out[key] = status
			mu.Unlock()
		}(key)
	}
	wg.Wait()
	return out
}
