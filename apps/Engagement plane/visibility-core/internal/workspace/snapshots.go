package workspace

import (
	"context"
	"strconv"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/analyticssync"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/siteintegrations"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

func currentSnapshotQuarter() string {
	return "Q" + strconv.Itoa((int(time.Now().UTC().Month())-1)/3+1)
}

func (s *Service) savedAnalytics(ctx context.Context, quarter, site string) analyticssync.Bundle {
	if quarter == "" {
		quarter = currentSnapshotQuarter()
	}
	failure := analyticssync.MissingBundle(site, quarter, time.Now(), "source_unavailable")
	record, err := s.registry.Resolve("", site)
	if err != nil {
		return failure
	}
	bundle, err := s.analytics.Read(ctx, record.Website.ID, site, quarter)
	if err != nil {
		return failure
	}
	return bundle
}

// Clarity's rolling 72-hour report is independent of the selected GSC quarter.
// A newer failed collection supersedes an older success; never hide that failure.
func (s *Service) savedClarity(ctx context.Context, site string) pulse.ClarityProjection {
	record, err := s.registry.Resolve("", site)
	if err != nil {
		return pulse.UnavailableClarity(site, "source_unavailable")
	}
	quarter, latest := currentSnapshotQuarter(), time.Time{}
	for _, q := range []string{"Q1", "Q2", "Q3", "Q4"} {
		state, err := s.analytics.Get(record.Website.ID, site, q)
		if err != nil {
			continue
		}
		// Include queued/running requests, so a deliberate refresh hides stale data.
		for _, stamp := range []string{state.CompletedAt, state.QueuedAt} {
			observed, _ := time.Parse(time.RFC3339Nano, stamp)
			if observed.After(latest) {
				quarter, latest = q, observed
			}
		}
	}
	return s.savedAnalytics(ctx, quarter, site).Clarity
}

type SnapshotAuthority struct {
	cfg      pulse.Config
	registry *websites.Service
	broker   *siteintegrations.Broker
}

func NewSnapshotAuthority(cfg pulse.Config, registry *websites.Service) *SnapshotAuthority {
	return &SnapshotAuthority{cfg: cfg, registry: registry, broker: siteintegrations.New(siteintegrations.Config{IntegrationCoreURL: cfg.IntegrationCoreURL, BearerTokenFile: cfg.IntegrationBearerTokenFile})}
}

func (a *SnapshotAuthority) Check(ctx context.Context, record websites.Record) analyticssync.Access {
	if !boundToOrganization(a.cfg.IntegrationBearerTokenFile, a.registry.OrganizationID()) {
		return analyticssync.Access{}
	}
	current, err := a.registry.Resolve(record.Website.ID, record.Website.Site)
	if err != nil || current.Website.Revision != record.Website.Revision {
		return analyticssync.Access{}
	}
	value := a.broker.CurrentAccess(ctx, a.registry.OrganizationID(), record)
	current, err = a.registry.Resolve(record.Website.ID, record.Website.Site)
	if err != nil || current.Website.Revision != record.Website.Revision {
		return analyticssync.Access{}
	}
	return analyticssync.Access{SearchConsole: value.SearchConsole, GA4: value.GA4, Clarity: value.Clarity, ClarityProjectID: value.ClarityProjectID, ClarityVersion: value.ClarityVersion}
}

// The collector receives the uncached adapter. Only background jobs use it;
// normal panels and the work queue receive the saved-data adapter instead.
type SnapshotCollector struct {
	source    *Service
	authority analyticssync.Authority
}

func NewSnapshotCollector(source *Service, authority analyticssync.Authority) *SnapshotCollector {
	return &SnapshotCollector{source: source, authority: authority}
}
func (c *SnapshotCollector) Collect(ctx context.Context, record websites.Record, quarter string) (analyticssync.Bundle, error) {
	bundle := analyticssync.MissingBundle(record.Website.Site, quarter, time.Now(), "authorization_required")
	access := c.authority.Check(ctx, record)
	if access.SearchConsole {
		bundle.Pulse = c.source.Get(ctx, quarter, record.Website.Site)
		if ctx.Err() != nil {
			return analyticssync.Bundle{}, ctx.Err()
		}
		bundle.Search = c.source.GetSearch(ctx, quarter, record.Website.Site)
	}
	if ctx.Err() != nil {
		return analyticssync.Bundle{}, ctx.Err()
	}
	if access.GA4 {
		bundle.GA4 = c.source.GetGA4(ctx, quarter, record.Website.Site)
	}
	if ctx.Err() != nil {
		return analyticssync.Bundle{}, ctx.Err()
	}
	if access.Clarity {
		bundle.Clarity = c.source.GetClarity(ctx, record.Website.Site)
	}
	return bundle, ctx.Err()
}
