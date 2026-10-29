package workspace

import (
	"context"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/trends"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"sort"
	"time"
)

func (s *Service) AutomaticTrends() *trends.Automatic { return s.automaticTrends }
func (s *Service) WithAutomaticTrends(value *trends.Automatic) *Service {
	next := *s
	next.automaticTrends = value
	return &next
}

// Reads saved, permission-checked GSC projections only. The newest retained
// quarter supplies keyword evidence, independent of Pulse's display filter.
func (s *Service) TrendsSearch(ctx context.Context, r websites.Record) (pulse.SearchProjection, error) {
	if s.analytics == nil {
		return pulse.SearchProjection{}, trends.ErrUnavailable
	}
	type candidate struct {
		quarter   string
		completed time.Time
	}
	candidates := []candidate{}
	for _, q := range []string{"Q1", "Q2", "Q3", "Q4"} {
		state, err := s.analytics.Get(r.Website.ID, r.Website.Site, q)
		expires, _ := time.Parse(time.RFC3339Nano, state.ExpiresAt)
		completed, _ := time.Parse(time.RFC3339Nano, state.CompletedAt)
		if err == nil && expires.After(time.Now()) {
			candidates = append(candidates, candidate{q, completed})
		}
	}
	sort.SliceStable(candidates, func(i, j int) bool { return candidates[i].completed.After(candidates[j].completed) })
	for _, candidate := range candidates {
		bundle, err := s.analytics.Read(ctx, r.Website.ID, r.Website.Site, candidate.quarter)
		if err != nil {
			return pulse.SearchProjection{}, err
		}
		if bundle.Search.Status == "connected" {
			return bundle.Search, nil
		}
		// Don't fall back to earlier grants after authority was denied.
		if bundle.Search.Reason == "authorization_required" || ctx.Err() != nil {
			return pulse.SearchProjection{}, trends.ErrUnavailable
		}
	}
	return pulse.SearchProjection{}, trends.ErrUnavailable
}
