// Package workspace resolves website-specific mappings before producing panels.
package workspace

import (
	"context"
	"github.com/triodelab/engagement-plane/visibility-core/internal/analyticssync"
	"github.com/triodelab/engagement-plane/visibility-core/internal/competitors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/labreports"
	"github.com/triodelab/engagement-plane/visibility-core/internal/outcomes"
	"github.com/triodelab/engagement-plane/visibility-core/internal/searchresearch"

	"github.com/triodelab/engagement-plane/visibility-core/internal/algoliamapping"
	"github.com/triodelab/engagement-plane/visibility-core/internal/preparation"
	"github.com/triodelab/engagement-plane/visibility-core/internal/publicsignals"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
	"github.com/triodelab/engagement-plane/visibility-core/internal/sitesources"
	"github.com/triodelab/engagement-plane/visibility-core/internal/trends"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"github.com/triodelab/engagement-plane/visibility-core/internal/webvitals"
	"github.com/triodelab/engagement-plane/visibility-core/internal/workqueue"
)

type Service struct {
	research        *searchresearch.Service
	competitors     *competitors.Service
	cfg             pulse.Config
	outcomes        *outcomes.Service
	registry        *websites.Service
	work            *workqueue.Service
	discovery       *discovery.Service
	audit           *siteaudit.Service
	monitoring      *siteaudit.Monitor
	preparing       *preparation.Service
	labReports      *labreports.Service
	analytics       *analyticssync.Service
	publicSignals   *publicsignals.Service
	automaticTrends *trends.Automatic
	webVitals       *webvitals.Service
	siteSources     *sitesources.Service
	algoliaMapping  *algoliamapping.Service
}

func New(cfg pulse.Config, registry *websites.Service) *Service {
	return &Service{cfg: cfg, registry: registry}
}
func (s *Service) WebsiteRegistry() *websites.Service      { return s.registry }
func (s *Service) WorkQueue() *workqueue.Service           { return s.work }
func (s *Service) Discovery() *discovery.Service           { return s.discovery }
func (s *Service) Audit() *siteaudit.Service               { return s.audit }
func (s *Service) Monitoring() *siteaudit.Monitor          { return s.monitoring }
func (s *Service) Preparation() *preparation.Service       { return s.preparing }
func (s *Service) AnalyticsSync() *analyticssync.Service   { return s.analytics }
func (s *Service) PublicSignals() *publicsignals.Service   { return s.publicSignals }
func (s *Service) WebVitals() *webvitals.Service           { return s.webVitals }
func (s *Service) SiteSources() *sitesources.Service       { return s.siteSources }
func (s *Service) AlgoliaMapping() *algoliamapping.Service { return s.algoliaMapping }
func (s *Service) WithAlgoliaMapping(mapping *algoliamapping.Service) *Service {
	next := *s
	next.algoliaMapping = mapping
	return &next
}
func (s *Service) WithSiteSources(sources *sitesources.Service) *Service {
	next := *s
	next.siteSources = sources
	return &next
}
func (s *Service) WithWebVitals(vitals *webvitals.Service) *Service {
	next := *s
	next.webVitals = vitals
	return &next
}
func (s *Service) WithPublicSignals(jobs *publicsignals.Service) *Service {
	next := *s
	next.publicSignals = jobs
	return &next
}
func (s *Service) WithAnalyticsSync(jobs *analyticssync.Service) *Service {
	next := *s
	next.analytics = jobs
	return &next
}
func (s *Service) WithLabReports(reports *labreports.Service) *Service {
	next := *s
	next.labReports = reports
	return &next
}
func (s *Service) WithPreparation(jobs *preparation.Service) *Service {
	next := *s
	next.preparing = jobs
	return &next
}
func (s *Service) WithMonitoring(monitor *siteaudit.Monitor) *Service {
	next := *s
	next.monitoring = monitor
	return &next
}
func (s *Service) WithAudit(audit *siteaudit.Service) *Service {
	next := *s
	next.audit = audit
	return &next
}
func (s *Service) WithDiscovery(service *discovery.Service) *Service {
	next := *s
	next.discovery = service
	return &next
}
func (s *Service) WithWorkQueue(queue *workqueue.Service) *Service {
	next := *s
	next.work = queue
	return &next
}
func (s *Service) source(site string) *pulse.Service {
	cfg := s.cfg
	cfg.GoogleConnectionID = ""
	cfg.GoogleGSCProperty = ""
	if !boundToOrganization(cfg.IntegrationBearerTokenFile, s.registry.OrganizationID()) {
		cfg.IntegrationBearerTokenFile = ""
		return pulse.NewService(cfg, nil, nil)
	}
	if record, err := s.registry.Resolve("", site); err == nil && record.Website.Integrations.SearchConsole.Status == "connected" {
		cfg.GoogleConnectionID = record.GSCConnectionID
		cfg.GoogleGSCProperty = record.Website.Integrations.SearchConsole.Property
	}
	return pulse.NewService(cfg, nil, nil)
}
func (s *Service) Get(ctx context.Context, quarter, site string) pulse.Projection {
	if s.analytics != nil {
		return s.savedAnalytics(ctx, quarter, site).Pulse
	}
	return s.source(site).Get(ctx, quarter, site)
}
func (s *Service) GetSearch(ctx context.Context, quarter, site string) pulse.SearchProjection {
	if s.analytics != nil {
		return s.savedAnalytics(ctx, quarter, site).Search
	}
	return s.source(site).GetSearch(ctx, quarter, site)
}
func (s *Service) GetSearchComparison(ctx context.Context, site, dimension string, current, previous pulse.Period, rowLimit int) pulse.SearchComparisonProjection {
	return s.source(site).GetSearchComparison(ctx, site, dimension, current, previous, rowLimit)
}
func (s *Service) GetPageSpeed(ctx context.Context, site, strategy string) pulse.PageSpeedProjection {
	if s.labReports != nil {
		return s.labReports.Get(ctx, site, strategy)
	}
	return s.source(site).GetPageSpeed(ctx, site, strategy)
}
func (s *Service) StartPageSpeed(ctx context.Context, site, strategy string) pulse.PageSpeedProjection {
	if s.labReports != nil {
		return s.labReports.Queue(site, strategy)
	}
	return s.source(site).StartPageSpeed(ctx, site, strategy)
}

func (s *Service) WithBusinessOutcomes(value *outcomes.Service) *Service {
	next := *s
	next.outcomes = value
	return &next
}
func (s *Service) BusinessOutcomes(ctx context.Context, id, site, q, env string) (outcomes.Projection, error) {
	source := s.outcomes
	if source == nil {
		source, _ = outcomes.New(s.registry, "", "")
	}
	return source.Get(ctx, id, site, q, env)
}

func (s *Service) Competitors() *competitors.Service { return s.competitors }
func (s *Service) WithCompetitors(value *competitors.Service) *Service {
	next := *s
	next.competitors = value
	return &next
}
