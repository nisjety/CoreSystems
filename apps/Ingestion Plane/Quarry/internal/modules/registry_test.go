package modules

import (
	"context"
	"testing"

	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/scraper"
)

type stubModule struct {
	name string
}

func (m *stubModule) Name() string { return m.name }
func (m *stubModule) Run(ctx context.Context, engine *scraper.Scraper, req *models.ScrapeRequest) (*models.ScrapeResult, error) {
	_ = ctx
	_ = engine
	_ = req
	return &models.ScrapeResult{Count: 0, Products: []*models.Product{}}, nil
}

func TestRegistry_RegisterGetNames(t *testing.T) {
	r := NewRegistry()
	if err := r.Register(&stubModule{name: "Quick"}); err != nil {
		t.Fatalf("register quick: %v", err)
	}
	if err := r.Register(&stubModule{name: "seo"}); err != nil {
		t.Fatalf("register seo: %v", err)
	}

	if _, ok := r.Get("quick"); !ok {
		t.Fatalf("expected quick module")
	}
	if _, ok := r.Get("SEO"); !ok {
		t.Fatalf("expected seo module with case-insensitive lookup")
	}

	names := r.Names()
	if len(names) != 2 {
		t.Fatalf("expected 2 names, got %d", len(names))
	}
	if names[0] != "quick" || names[1] != "seo" {
		t.Fatalf("unexpected names order: %#v", names)
	}
}

func TestRegistry_MustGetOrDefault(t *testing.T) {
	r := NewDefaultRegistry()
	if _, ok := r.MustGetOrDefault("not-existing", "multi"); !ok {
		t.Fatalf("expected fallback module")
	}
}
