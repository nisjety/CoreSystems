package workspace

import (
	"context"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/siteintegrations"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"time"
)

// RunConnections observes the organization-scoped Integration registry even
// after a browser closes. Unchanged grants cause no provider discovery, metrics
// collection or mapping rewrites. Persisted per-site checkpoints survive restart.
func RunConnections(ctx context.Context, cfg pulse.Config, registry *websites.Service, changed func(websites.Website)) {
	broker := siteintegrations.New(siteintegrations.Config{IntegrationCoreURL: cfg.IntegrationCoreURL, BearerTokenFile: cfg.IntegrationBearerTokenFile})
	for {
		if boundToOrganization(cfg.IntegrationBearerTokenFile, registry.OrganizationID()) {
			bounded, cancel := context.WithTimeout(ctx, 90*time.Second)
			if fp, err := broker.ConnectionFingerprint(bounded); err == nil {
				_ = registry.RefreshConnections(bounded, fp, changed)
			}
			cancel()
		}
		timer := time.NewTimer(time.Minute)
		select {
		case <-ctx.Done():
			timer.Stop()
			return
		case <-timer.C:
		}
	}
}
