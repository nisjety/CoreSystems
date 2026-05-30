package modules

import (
	"context"

	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/scraper"
)

type MultiModule struct{}

func (m *MultiModule) Name() string { return "multi" }

func (m *MultiModule) Run(ctx context.Context, engine *scraper.Scraper, req *models.ScrapeRequest) (*models.ScrapeResult, error) {
	if req == nil {
		req = &models.ScrapeRequest{}
	}
	copyReq := *req
	if copyReq.MaxPages <= 0 {
		copyReq.MaxPages = 5
	}
	if copyReq.EnrichLimit <= 0 {
		copyReq.EnrichLimit = 50
	}
	copyReq.Enrich = true
	return engine.ScrapeCollection(ctx, &copyReq)
}

func NewDefaultRegistry() *Registry {
	r := NewRegistry()
	_ = r.Register(&QuickModule{})
	_ = r.Register(&SEOModule{})
	_ = r.Register(&MultiModule{})
	return r
}
