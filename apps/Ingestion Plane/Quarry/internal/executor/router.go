package executor

import (
	"strings"

	"github.com/triodelab/quarry/internal/models"
)

type Mode string

const (
	ModeImmediate Mode = "immediate"
	ModeScheduled Mode = "scheduled"
)

type Router struct {
	temporalEnabled bool
}

func NewRouter(temporalEnabled bool) *Router {
	return &Router{temporalEnabled: temporalEnabled}
}

func (r *Router) Select(req *models.CrawlAPIRequest) Mode {
	if req == nil {
		return ModeImmediate
	}

	explicit := strings.ToLower(strings.TrimSpace(req.Mode))
	switch explicit {
	case string(ModeImmediate):
		return ModeImmediate
	case string(ModeScheduled):
		if r.temporalEnabled {
			return ModeScheduled
		}
		return ModeImmediate
	}

	if req.ScheduleAt != nil && r.temporalEnabled {
		return ModeScheduled
	}

	if req.MaxDepth > 1 && r.temporalEnabled {
		return ModeScheduled
	}

	return ModeImmediate
}
