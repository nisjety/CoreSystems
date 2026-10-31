package workspace

import (
	"context"
	"github.com/triodelab/engagement-plane/visibility-core/internal/labreports"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

// Raw provider adapter bypasses the workspace cache to avoid recursive collection.
// It retains the same organization-scoped Integration Core authority.
type labSource struct{ workspace *Service }

func NewLabSource(source *Service) labreports.Source { return labSource{source} }
func (s labSource) GetPageSpeed(ctx context.Context, site, strategy string) pulse.PageSpeedProjection {
	return s.workspace.source(site).GetPageSpeed(ctx, site, strategy)
}
func (s labSource) StartPageSpeed(ctx context.Context, site, strategy string) pulse.PageSpeedProjection {
	return s.workspace.source(site).StartPageSpeed(ctx, site, strategy)
}
