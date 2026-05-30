package api

import (
	"context"
	"net/http"
	"testing"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/quarry/internal/actions"
	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/modules"
	"github.com/triodelab/quarry/internal/scraper"
)

type stubModule struct {
	name string
	run  func(context.Context, *scraper.Scraper, *models.ScrapeRequest) (*models.ScrapeResult, error)
}

func (m *stubModule) Name() string { return m.name }

func (m *stubModule) Run(ctx context.Context, engine *scraper.Scraper, req *models.ScrapeRequest) (*models.ScrapeResult, error) {
	return m.run(ctx, engine, req)
}

func TestResolveModule_DefaultsToMulti(t *testing.T) {
	h := &Handler{moduleRegistry: modules.NewDefaultRegistry()}

	m, selected, ok := h.resolveModule("")
	if !ok {
		t.Fatalf("expected resolver to succeed")
	}
	if selected != "multi" {
		t.Fatalf("expected selected module multi, got %s", selected)
	}
	if m.Name() != "multi" {
		t.Fatalf("expected module multi, got %s", m.Name())
	}
}

func TestResolveModule_UnknownFallsBackToMulti(t *testing.T) {
	h := &Handler{moduleRegistry: modules.NewDefaultRegistry()}

	m, selected, ok := h.resolveModule("unknown")
	if !ok {
		t.Fatalf("expected resolver to succeed with fallback")
	}
	if selected != "unknown" {
		t.Fatalf("expected selected module unknown, got %s", selected)
	}
	if m.Name() != "multi" {
		t.Fatalf("expected fallback module multi, got %s", m.Name())
	}
}

func TestResolveModule_ExplicitModule(t *testing.T) {
	h := &Handler{moduleRegistry: modules.NewDefaultRegistry()}

	m, selected, ok := h.resolveModule("quick")
	if !ok {
		t.Fatalf("expected resolver to succeed")
	}
	if selected != "quick" {
		t.Fatalf("expected selected module quick, got %s", selected)
	}
	if m.Name() != "quick" {
		t.Fatalf("expected module quick, got %s", m.Name())
	}
}

func TestScrapeAcceptsTopLevelProxyConfig(t *testing.T) {
	t.Parallel()

	registry := modules.NewRegistry()
	if err := registry.Register(&stubModule{
		name: "stub",
		run: func(ctx context.Context, engine *scraper.Scraper, req *models.ScrapeRequest) (*models.ScrapeResult, error) {
			return &models.ScrapeResult{Count: 1}, nil
		},
	}); err != nil {
		t.Fatalf("registry.Register() error = %v", err)
	}

	handler := &Handler{
		moduleRegistry: registry,
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			if opts.ProxyURL != "http://proxy.scrape.local:8080" {
				t.Fatalf("ProxyURL = %q, want http://proxy.scrape.local:8080", opts.ProxyURL)
			}
			return map[string]interface{}{"markdown": "# proxy ok"}, nil, nil
		},
	}

	app := fiber.New()
	handler.Register(app)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/scrape", map[string]interface{}{
		"url":        "https://example.com/collections/widgets",
		"collection": "widgets",
		"module":     "stub",
		"formats":    []string{"markdown"},
		"proxy": map[string]interface{}{
			"url": "http://proxy.scrape.local:8080",
		},
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var payload models.ScrapeAPIResponse
	decodeJSONResponse(t, resp, &payload)
	if !payload.Success {
		t.Fatal("success = false, want true")
	}
	if payload.Outputs["markdown"] != "# proxy ok" {
		t.Fatalf("markdown = %#v, want # proxy ok", payload.Outputs["markdown"])
	}
}
