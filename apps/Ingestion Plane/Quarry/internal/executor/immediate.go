package executor

import (
	"context"
	"fmt"
	"strings"

	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/modules"
	"github.com/triodelab/quarry/internal/scraper"
)

type ImmediateExecutor struct {
	scraper  *scraper.Scraper
	registry *modules.Registry
}

func NewImmediateExecutor(scraperEngine *scraper.Scraper, registry *modules.Registry) *ImmediateExecutor {
	return &ImmediateExecutor{scraper: scraperEngine, registry: registry}
}

func (e *ImmediateExecutor) Execute(ctx context.Context, req *models.CrawlAPIRequest) (*models.ScrapeResult, error) {
	if req == nil {
		return nil, fmt.Errorf("request is nil")
	}
	if e.scraper == nil {
		return nil, fmt.Errorf("scraper is not initialized")
	}
	if e.registry == nil {
		return nil, fmt.Errorf("module registry is not initialized")
	}

	selectedModule := strings.ToLower(strings.TrimSpace(req.Module))
	if selectedModule == "" {
		selectedModule = "multi"
	}
	module, ok := e.registry.MustGetOrDefault(selectedModule, "multi")
	if !ok {
		return nil, fmt.Errorf("module is not available: %s", selectedModule)
	}

	scrapeReq := &models.ScrapeRequest{
		BaseURL:     req.URL,
		Collection:  inferCollection(req.URL),
		MaxPages:    req.MaxPages,
		Enrich:      req.Enrich,
		EnrichLimit: req.EnrichLimit,
		MaxAge:      req.MaxAge,
		EventSink:   req.EventSink,
		// Pass through smart-crawl fields
		IncludePaths: req.IncludePaths,
		ExcludePaths: req.ExcludePaths,
		Schema:       req.Schema,
		Prompt:       req.Prompt,
	}

	if scrapeReq.MaxPages <= 0 {
		scrapeReq.MaxPages = 1
	}

	// Smart-crawl prompt support: currently implemented via keyword scoring inside ScrapeCollection
	// (scoreAndSortByPrompt). If needed, this block can be extended to call an AI ranking
	// endpoint to pre-filter links before scraping.

	return module.Run(ctx, e.scraper, scrapeReq)
}

func inferCollection(inputURL string) string {
	if inputURL == "" {
		return ""
	}
	parts := strings.Split(strings.Trim(inputURL, "/"), "/")
	for idx, part := range parts {
		if part == "produktkategori" || part == "collections" {
			if idx+1 < len(parts) {
				return parts[idx+1]
			}
		}
	}
	return ""
}
