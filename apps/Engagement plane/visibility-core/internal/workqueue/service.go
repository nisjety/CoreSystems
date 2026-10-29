package workqueue

import (
	"context"
	"errors"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type Registry interface {
	Resolve(string, string) (websites.Record, error)
	OrganizationID() string
}
type SearchSource interface {
	GetSearch(context.Context, string, string) pulse.SearchProjection
}
type Service struct {
	repo     *Repository
	registry Registry
	source   SearchSource
	now      func() time.Time
	audit    AuditSource
	research ResearchSource
}

func New(repo *Repository, registry Registry, source SearchSource, now func() time.Time) *Service {
	if now == nil {
		now = time.Now
	}
	return &Service{repo: repo, registry: registry, source: source, now: now}
}
func (s *Service) binding(id, site string, revision int64) (websites.Website, error) {
	if s.repo.org != s.registry.OrganizationID() {
		return websites.Website{}, ErrConflict
	}
	canonical, err := pulse.CanonicalSite(site)
	if err != nil || canonical != site || !websitePattern.MatchString(id) {
		return websites.Website{}, ErrInvalid
	}
	record, err := s.registry.Resolve(id, site)
	if errors.Is(err, websites.ErrConflict) {
		return websites.Website{}, ErrConflict
	}
	if errors.Is(err, websites.ErrNotFound) {
		return websites.Website{}, ErrNotFound
	}
	if err != nil {
		return websites.Website{}, err
	}
	if revision > 0 && record.Website.Revision != revision {
		return websites.Website{}, ErrConflict
	}
	return record.Website, nil
}
func (s *Service) List(id, site string) (Snapshot, error) {
	if _, err := s.binding(id, site, 0); err != nil {
		return Snapshot{}, err
	}
	return s.repo.snapshot(id, site), nil
}
func (s *Service) Save(ctx context.Context, id, site string, websiteRevision int64, quarter, key string) (Snapshot, bool, error) {
	if websiteRevision < 1 || !validKey(key) || (quarter != "Q1" && quarter != "Q2" && quarter != "Q3" && quarter != "Q4" && quarter != "FY") {
		return Snapshot{}, false, ErrInvalid
	}
	if _, err := pulse.QuarterPeriod(quarter, s.now()); err != nil {
		return Snapshot{}, false, ErrInvalid
	}
	if _, err := s.binding(id, site, websiteRevision); err != nil {
		return Snapshot{}, false, err
	}
	if s.source == nil {
		return Snapshot{}, false, ErrSourceUnavailable
	}
	projection := s.source.GetSearch(ctx, quarter, site)
	if projection.Status != pulse.StatusConnected || projection.Site != site || projection.Source != "google_search_console" {
		return Snapshot{}, false, ErrSourceUnavailable
	}
	var evidence Evidence
	for _, candidate := range projection.Opportunities {
		if candidate.Key == key {
			evidence = Evidence{Kind: "gsc_opportunity", Key: key, OpportunityType: candidate.Type, Period: projection.Period, Clicks: candidate.Clicks, Impressions: candidate.Impressions, CTR: candidate.CTR, Position: candidate.Position, ObservedAt: projection.UpdatedAt}
			break
		}
	}
	if evidence.Key == "" {
		return Snapshot{}, false, ErrNotFound
	}
	if !validEvidence(evidence) {
		return Snapshot{}, false, ErrSourceUnavailable
	}
	if _, err := s.binding(id, site, websiteRevision); err != nil {
		return Snapshot{}, false, err
	}
	created, err := s.repo.create(id, site, evidence, s.now().UTC().Format(time.RFC3339Nano))
	if err != nil {
		return Snapshot{}, false, err
	}
	result, err := s.List(id, site)
	return result, created, err
}
func (s *Service) Change(id, site string, websiteRevision int64, taskID string, revision int64, status string) (Snapshot, error) {
	if websiteRevision < 1 || revision < 1 || !taskPattern.MatchString(taskID) || !validStatus(status) {
		return Snapshot{}, ErrInvalid
	}
	if _, err := s.binding(id, site, websiteRevision); err != nil {
		return Snapshot{}, err
	}
	if err := s.repo.change(id, site, taskID, revision, status, s.now().UTC().Format(time.RFC3339Nano)); err != nil {
		return Snapshot{}, err
	}
	return s.List(id, site)
}
