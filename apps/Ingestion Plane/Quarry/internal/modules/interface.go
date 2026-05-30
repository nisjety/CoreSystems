package modules

import (
	"context"

	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/scraper"
)

type Module interface {
	Name() string
	Run(ctx context.Context, engine *scraper.Scraper, req *models.ScrapeRequest) (*models.ScrapeResult, error)
}
