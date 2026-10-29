package workspace

import (
	"context"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

func (s *Service) GetGA4(ctx context.Context, quarter, site string) pulse.GA4Projection {
	if s.analytics != nil {
		return s.savedAnalytics(ctx, quarter, site).GA4
	}
	period, _ := pulse.QuarterPeriod(quarter, time.Now())
	failure := func(reason string) pulse.GA4Projection { return pulse.UnavailableGA4(site, period, reason) }
	if !boundToOrganization(s.cfg.IntegrationBearerTokenFile, s.registry.OrganizationID()) {
		return failure("not_configured")
	}
	record, err := s.registry.Resolve("", site)
	if err != nil {
		return failure("not_found")
	}
	mapping := record.Website.Integrations.GA4
	if mapping.Status != "connected" {
		reason := mapping.Status
		switch reason {
		case "not_connected", "requires_consent", "needs_selection", "not_found", "source_unavailable":
		default:
			reason = "not_connected"
		}
		return failure(reason)
	}
	cfg := s.cfg
	cfg.GoogleGA4ConnectionID = record.GA4ConnectionID
	cfg.GoogleGA4PropertyID = mapping.PropertyID
	cfg.GoogleGA4StreamID = mapping.StreamID
	cfg.GoogleGA4ReportingSite = mapping.ReportingSite
	return pulse.NewService(cfg, nil, nil).GetGA4(ctx, quarter, site)
}
func (s *Service) GetClarity(ctx context.Context, site string) pulse.ClarityProjection {
	if s.analytics != nil {
		return s.savedClarity(ctx, site)
	}
	if !boundToOrganization(s.cfg.IntegrationBearerTokenFile, s.registry.OrganizationID()) {
		return pulse.UnavailableClarity(site, "not_configured")
	}
	record, err := s.registry.Resolve("", site)
	if err != nil {
		return pulse.UnavailableClarity(site, "not_connected")
	}
	cfg := s.cfg
	// A registry project ID is a routing assertion, not an access credential.
	// Integration independently authorizes its operator-approved site binding.
	cfg.ClarityProjectID = record.Website.Integrations.Clarity.ProjectID
	return pulse.NewService(cfg, nil, nil).GetClarity(ctx, site)
}
