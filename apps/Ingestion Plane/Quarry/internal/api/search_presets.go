package api

import (
	"fmt"
	"strings"

	"github.com/triodelab/quarry/internal/scraper"
)

type searchPresetDefinition struct {
	Name       string
	Limit      int
	TimeoutSec int
	Sources    []v2SearchSource
	Formats    []string
}

var searchPresetDefinitions = map[string]searchPresetDefinition{
	"lead-enrichment": {
		Name:       "lead-enrichment",
		Limit:      5,
		TimeoutSec: 120,
		Sources:    []v2SearchSource{{Type: "web"}},
		Formats:    []string{"markdown", "branding"},
	},
	"ecommerce-monitor": {
		Name:       "ecommerce-monitor",
		Limit:      8,
		TimeoutSec: 120,
		Sources:    []v2SearchSource{{Type: "web"}, {Type: "images"}},
		Formats:    []string{"markdown", "branding", "pagestatus"},
	},
	"competitive-monitor": {
		Name:       "competitive-monitor",
		Limit:      8,
		TimeoutSec: 150,
		Sources:    []v2SearchSource{{Type: "web"}, {Type: "news"}},
		Formats:    []string{"markdown", "seo", "pagestatus"},
	},
	"finance-research": {
		Name:       "finance-research",
		Limit:      6,
		TimeoutSec: 150,
		Sources:    []v2SearchSource{{Type: "web"}, {Type: "news"}},
		Formats:    []string{"markdown", "seo", "pagestatus"},
	},
	"content-seeding": {
		Name:       "content-seeding",
		Limit:      6,
		TimeoutSec: 120,
		Sources:    []v2SearchSource{{Type: "web"}, {Type: "news"}},
		Formats:    []string{"markdown", "seo"},
	},
	"data-migration": {
		Name:       "data-migration",
		Limit:      10,
		TimeoutSec: 180,
		Sources:    []v2SearchSource{{Type: "web"}},
		Formats:    []string{"markdown", "html", "links"},
	},
	"site-observability": {
		Name:       "site-observability",
		Limit:      6,
		TimeoutSec: 150,
		Sources:    []v2SearchSource{{Type: "web"}},
		Formats:    []string{"markdown", "seo", "wcag", "pagestatus"},
	},
}

func normalizeSearchPresetName(raw string) string {
	return strings.ToLower(strings.TrimSpace(raw))
}

func lookupSearchPreset(raw string) (searchPresetDefinition, bool) {
	preset, ok := searchPresetDefinitions[normalizeSearchPresetName(raw)]
	return preset, ok
}

func cloneSearchScrapeOptions(opts *scraper.FormatOptions) *scraper.FormatOptions {
	if opts == nil {
		return nil
	}
	cloned := *opts
	cloned.Formats = append([]string(nil), opts.Formats...)
	cloned.Headers = cloneStringMap(opts.Headers)
	cloned.IncludeTags = append([]string(nil), opts.IncludeTags...)
	cloned.ExcludeTags = append([]string(nil), opts.ExcludeTags...)
	cloned.Actions = append(cloned.Actions[:0:0], opts.Actions...)
	return &cloned
}

func resolveV1SearchConfig(req V2SearchRequest, present map[string]struct{}, sources []v2SearchSource, scrapeOpts *scraper.FormatOptions, shouldScrape bool) (v2SearchRunConfig, map[string]any, error) {
	cfg := v2SearchRunConfig{
		Preset:       normalizeSearchPresetName(req.Preset),
		BlendMode:    normalizeBlendMode(req.BlendMode),
		Query:        req.Query,
		Limit:        req.Limit,
		Sources:      append([]v2SearchSource(nil), sources...),
		ScrapeOpts:   cloneSearchScrapeOptions(scrapeOpts),
		ShouldScrape: shouldScrape,
		TimeoutSec:   req.TimeoutSec,
		Webhook:      req.Webhook,
	}

	if cfg.Preset != "" {
		preset, ok := lookupSearchPreset(cfg.Preset)
		if !ok {
			return v2SearchRunConfig{}, nil, fmt.Errorf("preset contains unsupported value")
		}
		if !fieldPresent(present, "limit") {
			cfg.Limit = preset.Limit
		}
		if !fieldPresent(present, "timeout") {
			cfg.TimeoutSec = preset.TimeoutSec
		}
		if !fieldPresent(present, "sources") && len(req.Sources) == 0 {
			cfg.Sources = append([]v2SearchSource(nil), preset.Sources...)
		}
		if !fieldPresent(present, "scrapeOptions") && !fieldPresent(present, "formats") && !fieldPresent(present, "scrape") {
			cfg.ScrapeOpts = &scraper.FormatOptions{Formats: append([]string(nil), preset.Formats...)}
			cfg.ShouldScrape = true
		}
	}

	if cfg.Limit <= 0 {
		cfg.Limit = 10
	}
	if cfg.Limit > 50 {
		cfg.Limit = 50
	}
	if cfg.TimeoutSec <= 0 {
		cfg.TimeoutSec = 15
	}
	if cfg.TimeoutSec > 300 {
		cfg.TimeoutSec = 300
	}
	if len(cfg.Sources) == 0 {
		cfg.Sources = []v2SearchSource{{Type: "web"}}
	}
	if cfg.BlendMode == "" {
		cfg.BlendMode = searchBlendRanked
	}
	if cfg.ShouldScrape && cfg.ScrapeOpts == nil {
		cfg.ScrapeOpts = &scraper.FormatOptions{Formats: []string{"markdown"}}
	}
	if cfg.ScrapeOpts != nil && len(cfg.ScrapeOpts.Formats) == 0 {
		cfg.ScrapeOpts.Formats = []string{"markdown"}
	}

	resolved := map[string]any{
		"preset":    cfg.Preset,
		"query":     cfg.Query,
		"limit":     cfg.Limit,
		"timeout":   cfg.TimeoutSec,
		"blendMode": cfg.BlendMode,
		"sources":   researchSourcesJSON(cfg.Sources),
		"scrape":    cfg.ShouldScrape,
	}
	if cfg.ScrapeOpts != nil {
		resolved["scrapeOptions"] = map[string]any{
			"formats": append([]string(nil), cfg.ScrapeOpts.Formats...),
		}
	}
	return cfg, resolved, nil
}
