package modules

import (
	"context"

	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/scraper"
)

type SEOModule struct{}

func (m *SEOModule) Name() string { return "seo" }

func (m *SEOModule) Run(ctx context.Context, engine *scraper.Scraper, req *models.ScrapeRequest) (*models.ScrapeResult, error) {
	if req == nil {
		req = &models.ScrapeRequest{}
	}
	copyReq := *req
	if copyReq.MaxPages <= 0 {
		copyReq.MaxPages = 3
	}
	copyReq.Enrich = false
	copyReq.EnrichLimit = 0
	return engine.ScrapeCollection(ctx, &copyReq)
}
