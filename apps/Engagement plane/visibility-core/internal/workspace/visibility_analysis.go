package workspace

import (
	"context"
	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
	"github.com/triodelab/engagement-plane/visibility-core/internal/visibilityanalysis"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"time"
)

func (s *Service) VisibilityAnalysis(ctx context.Context, id, site, quarter string) (visibilityanalysis.Snapshot, error) {
	w, err := s.registry.Resolve(id, site)
	if err != nil {
		return visibilityanalysis.Snapshot{}, err
	}
	search := s.GetSearch(ctx, quarter, site)
	audit := siteaudit.Snapshot{Status: "unavailable"}
	if s.audit != nil {
		audit, err = s.audit.Get(id, site)
		if err != nil {
			return visibilityanalysis.Snapshot{}, err
		}
	}
	inventory := discovery.Snapshot{Status: "unavailable"}
	if s.discovery != nil {
		inventory, _ = s.discovery.Get(id, site)
	}
	current, err := s.registry.Resolve(id, site)
	if err != nil || current.Website.Revision != w.Website.Revision {
		return visibilityanalysis.Snapshot{}, websites.ErrConflict
	}
	return visibilityanalysis.Build(w.Website, search, audit, inventory, time.Now()), nil
}
