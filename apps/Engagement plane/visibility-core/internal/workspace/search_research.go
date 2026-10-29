package workspace

import (
	"context"
	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/searchresearch"
	"github.com/triodelab/engagement-plane/visibility-core/internal/visibilityanalysis"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

func (s *Service) WithSearchResearch(value *searchresearch.Service) *Service {
	next := *s
	next.research = value
	return &next
}
func (s *Service) SearchResearch() *searchresearch.Service { return s.research }
func (s *Service) CollectSearchResearch(ctx context.Context, r websites.Record) pulse.SearchResearch {
	value := s.source(r.Website.Site).GetSearchResearch(ctx, r.Website.Site)
	kept := []pulse.SearchObservation{}
	indexes := map[string]int{}
	for _, row := range value.Rows {
		if !discovery.SafeURL(r.Website.Site, row.Page) {
			value.Truncated = true
			continue
		}
		row.Page = visibilityanalysis.Key(r.Website.Site, row.Page)
		key := row.Date + "\x00" + row.Query + "\x00" + row.Page
		if i, ok := indexes[key]; ok {
			old := kept[i]
			total := old.Impressions + row.Impressions
			position := 0.0
			if total > 0 {
				position = (old.Position*float64(old.Impressions) + row.Position*float64(row.Impressions)) / float64(total)
			}
			old.Impressions = total
			old.Clicks += row.Clicks
			old.Position = position
			kept[i] = old
		} else {
			indexes[key] = len(kept)
			kept = append(kept, row)
		}
	}
	value.Rows = kept
	return value
}
