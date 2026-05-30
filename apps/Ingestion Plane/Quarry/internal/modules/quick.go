package modules

import (
	"context"

	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/scraper"
)

type QuickModule struct{}

func (m *QuickModule) Name() string { return "quick" }

func (m *QuickModule) Run(ctx context.Context, engine *scraper.Scraper, req *models.ScrapeRequest) (*models.ScrapeResult, error) {
	if req == nil {
		req = &models.ScrapeRequest{}
	}
	copyReq := *req
	copyReq.MaxPages = 1
	copyReq.Enrich = false
	copyReq.EnrichLimit = 0
	return engine.ScrapeCollection(ctx, &copyReq)
}
