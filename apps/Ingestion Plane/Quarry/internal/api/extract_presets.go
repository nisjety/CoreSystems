package api

import (
	"fmt"
	"strings"

	quarrycrawl "github.com/triodelab/quarry/internal/crawl"
	"github.com/triodelab/quarry/internal/scraper"
)

type extractPresetDefinition struct {
	Name               string
	Limit              int
	TimeoutSec         int
	EnableWebSearch    bool
	IncludeSubdomains  bool
	AllowExternalLinks bool
	IgnoreRobotsTxt    bool
	MaxDiscoveryDepth  int
	Sitemap            quarrycrawl.SitemapMode
	Formats            []string
}

var extractPresetDefinitions = map[string]extractPresetDefinition{
	"lead-enrichment": {
		Name:            "lead-enrichment",
		Limit:           5,
		TimeoutSec:      120,
		EnableWebSearch: true,
		Formats:         []string{"markdown", "branding"},
	},
	"ecommerce-monitor": {
		Name:              "ecommerce-monitor",
		Limit:             8,
		TimeoutSec:        120,
		MaxDiscoveryDepth: 1,
		Sitemap:           quarrycrawl.SitemapInclude,
		Formats:           []string{"markdown", "branding", "pagestatus"},
	},
	"competitive-monitor": {
		Name:              "competitive-monitor",
		Limit:             8,
		TimeoutSec:        150,
		MaxDiscoveryDepth: 1,
		Sitemap:           quarrycrawl.SitemapInclude,
		Formats:           []string{"markdown", "seo", "pagestatus"},
	},
	"finance-research": {
		Name:            "finance-research",
		Limit:           6,
		TimeoutSec:      150,
		EnableWebSearch: true,
		Formats:         []string{"markdown", "seo", "pagestatus"},
	},
	"content-seeding": {
		Name:            "content-seeding",
		Limit:           6,
		TimeoutSec:      120,
		EnableWebSearch: true,
		Formats:         []string{"markdown", "seo"},
	},
	"data-migration": {
		Name:              "data-migration",
		Limit:             10,
		TimeoutSec:        180,
		MaxDiscoveryDepth: 2,
		Sitemap:           quarrycrawl.SitemapInclude,
		Formats:           []string{"markdown", "html", "links"},
	},
	"site-observability": {
		Name:              "site-observability",
		Limit:             6,
		TimeoutSec:        150,
		MaxDiscoveryDepth: 1,
		Sitemap:           quarrycrawl.SitemapInclude,
		Formats:           []string{"markdown", "seo", "wcag", "pagestatus"},
	},
}

func normalizeExtractPresetName(raw string) string {
	return strings.ToLower(strings.TrimSpace(raw))
}

func lookupExtractPreset(raw string) (extractPresetDefinition, bool) {
	preset, ok := extractPresetDefinitions[normalizeExtractPresetName(raw)]
	return preset, ok
}

func resolveV1ExtractRequest(req V2ExtractRequest, present map[string]struct{}, scrapeOpts *scraper.FormatOptions) (V2ExtractRequest, *scraper.FormatOptions, map[string]any, error) {
	req.Preset = normalizeExtractPresetName(req.Preset)
	if req.Preset != "" {
		preset, ok := lookupExtractPreset(req.Preset)
		if !ok {
			return V2ExtractRequest{}, nil, nil, fmt.Errorf("preset contains unsupported value")
		}
		if !fieldPresent(present, "limit") {
			req.Limit = preset.Limit
		}
		if !fieldPresent(present, "timeout") {
			req.TimeoutSec = preset.TimeoutSec
		}
		if !fieldPresent(present, "enableWebSearch") {
			req.EnableWebSearch = preset.EnableWebSearch
		}
		if !fieldPresent(present, "includeSubdomains") {
			req.IncludeSubdomains = preset.IncludeSubdomains
		}
		if !fieldPresent(present, "allowExternalLinks") {
			req.AllowExternalLinks = preset.AllowExternalLinks
		}
		if !fieldPresent(present, "ignoreRobotsTxt") {
			req.IgnoreRobotsTxt = preset.IgnoreRobotsTxt
		}
		if !fieldPresent(present, "maxDiscoveryDepth") && req.MaxDiscoveryDepth == nil && preset.MaxDiscoveryDepth > 0 {
			depth := preset.MaxDiscoveryDepth
			req.MaxDiscoveryDepth = &depth
		}
		if !fieldPresent(present, "sitemap") && !fieldPresent(present, "ignoreSitemap") && req.Sitemap == "" && preset.Sitemap != "" {
			req.Sitemap = preset.Sitemap
		}
		if !fieldPresent(present, "scrapeOptions") && !fieldPresent(present, "formats") {
			scrapeOpts = &scraper.FormatOptions{Formats: append([]string(nil), preset.Formats...)}
		}
	}

	if req.Limit <= 0 {
		req.Limit = 5
	}
	if req.Limit > 50 {
		req.Limit = 50
	}
	if req.TimeoutSec <= 0 {
		req.TimeoutSec = 60
	}
	if req.TimeoutSec > 300 {
		req.TimeoutSec = 300
	}
	if scrapeOpts == nil {
		scrapeOpts = &scraper.FormatOptions{Formats: []string{"html", "markdown"}}
	}
	if !sliceContainsFold(scrapeOpts.Formats, "html") {
		scrapeOpts.Formats = append(scrapeOpts.Formats, "html")
	}
	if !sliceContainsFold(scrapeOpts.Formats, "markdown") {
		scrapeOpts.Formats = append(scrapeOpts.Formats, "markdown")
	}

	resolved := map[string]any{
		"preset":          req.Preset,
		"limit":           req.Limit,
		"timeout":         req.TimeoutSec,
		"enableWebSearch": req.EnableWebSearch,
		"sitemap":         req.Sitemap,
	}
	if req.MaxDiscoveryDepth != nil {
		resolved["maxDiscoveryDepth"] = *req.MaxDiscoveryDepth
	}
	if scrapeOpts != nil {
		resolved["scrapeOptions"] = map[string]any{
			"formats": append([]string(nil), scrapeOpts.Formats...),
		}
	}
	return req, scrapeOpts, resolved, nil
}

func sliceContainsFold(items []string, want string) bool {
	normalizedWant := strings.TrimSpace(strings.ToLower(want))
	for _, item := range items {
		if strings.TrimSpace(strings.ToLower(item)) == normalizedWant {
			return true
		}
	}
	return false
}
