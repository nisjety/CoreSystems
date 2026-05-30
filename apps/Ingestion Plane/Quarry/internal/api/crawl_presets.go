package api

import (
	"fmt"
	"strings"
	"time"

	quarrycrawl "github.com/triodelab/quarry/internal/crawl"
	"github.com/triodelab/quarry/internal/models"
)

type crawlPresetDefinition struct {
	Name              string
	Limit             int
	MaxDiscoveryDepth int
	CrawlEntireDomain bool
	AllowSubdomains   bool
	IgnoreRobotsTxt   bool
	Formats           []string
	ChangeTracking    *models.ChangeTrackingRequest
}

var crawlPresetDefinitions = map[string]crawlPresetDefinition{
	"site-observability": {
		Name:              "site-observability",
		Limit:             25,
		MaxDiscoveryDepth: 2,
		CrawlEntireDomain: true,
		AllowSubdomains:   true,
		Formats:           []string{"markdown", "seo", "wcag", "pagestatus"},
		ChangeTracking: &models.ChangeTrackingRequest{
			Enabled: true,
			Modes:   []string{"git-diff"},
			Tag:     "site-observability",
		},
	},
	"competitive-monitor": {
		Name:              "competitive-monitor",
		Limit:             25,
		MaxDiscoveryDepth: 2,
		CrawlEntireDomain: true,
		Formats:           []string{"markdown", "seo", "pagestatus"},
		ChangeTracking: &models.ChangeTrackingRequest{
			Enabled: true,
			Modes:   []string{"git-diff"},
			Tag:     "competitive-monitor",
		},
	},
	"ecommerce-monitor": {
		Name:              "ecommerce-monitor",
		Limit:             50,
		MaxDiscoveryDepth: 2,
		CrawlEntireDomain: true,
		Formats:           []string{"markdown", "branding", "pagestatus"},
		ChangeTracking: &models.ChangeTrackingRequest{
			Enabled: true,
			Modes:   []string{"git-diff"},
			Tag:     "ecommerce-monitor",
		},
	},
	"data-migration": {
		Name:              "data-migration",
		Limit:             100,
		MaxDiscoveryDepth: 3,
		CrawlEntireDomain: true,
		IgnoreRobotsTxt:   false,
		Formats:           []string{"markdown", "html", "links"},
	},
}

func normalizeCrawlPresetName(raw string) string {
	return strings.ToLower(strings.TrimSpace(raw))
}

func lookupCrawlPreset(raw string) (crawlPresetDefinition, bool) {
	preset, ok := crawlPresetDefinitions[normalizeCrawlPresetName(raw)]
	return preset, ok
}

func resolveV1CrawlSpec(req *v2CrawlRequest, spec quarrycrawl.Spec, present map[string]struct{}) (quarrycrawl.Spec, map[string]any, error) {
	if req == nil {
		return spec, crawlResolvedOptions(spec), nil
	}
	presetName := normalizeCrawlPresetName(req.Preset)
	spec.Preset = presetName
	if presetName == "" {
		return spec, crawlResolvedOptions(spec), nil
	}

	preset, ok := lookupCrawlPreset(presetName)
	if !ok {
		return quarrycrawl.Spec{}, nil, fmt.Errorf("preset contains unsupported value")
	}

	if !fieldPresent(present, "limit") {
		spec.Limit = preset.Limit
	}
	if !fieldPresent(present, "maxDiscoveryDepth") && !fieldPresent(present, "maxDepth") {
		depth := preset.MaxDiscoveryDepth
		spec.MaxDiscoveryDepth = &depth
	}
	if !fieldPresent(present, "crawlEntireDomain") {
		spec.CrawlEntireDomain = preset.CrawlEntireDomain
	}
	if !fieldPresent(present, "allowSubdomains") {
		spec.AllowSubdomains = preset.AllowSubdomains
	}
	if !fieldPresent(present, "ignoreRobotsTxt") {
		spec.IgnoreRobotsTxt = preset.IgnoreRobotsTxt
	}
	if !fieldPresent(present, "scrapeOptions") && !fieldPresent(present, "formats") {
		spec.PageOptions.Formats = make([]quarrycrawl.Format, 0, len(preset.Formats))
		for _, format := range preset.Formats {
			spec.PageOptions.Formats = append(spec.PageOptions.Formats, quarrycrawl.Format{Type: format})
		}
	}
	if !fieldPresent(present, "changeTracking") && preset.ChangeTracking != nil {
		changeTracking := *preset.ChangeTracking
		changeTracking.Modes = append([]string(nil), preset.ChangeTracking.Modes...)
		spec.ChangeTracking = &changeTracking
	}
	if spec.ChangeTracking != nil && strings.TrimSpace(spec.ChangeTracking.Tag) == "" {
		spec.ChangeTracking.Tag = preset.Name
	}
	if req.ScheduleAt != nil {
		value := req.ScheduleAt.UTC()
		spec.ScheduleAt = &value
	}

	return spec, crawlResolvedOptions(spec), nil
}

func crawlResolvedOptions(spec quarrycrawl.Spec) map[string]any {
	resolved := map[string]any{
		"url":                spec.URL,
		"preset":             spec.Preset,
		"limit":              spec.Limit,
		"includePaths":       append([]string(nil), spec.IncludePaths...),
		"excludePaths":       append([]string(nil), spec.ExcludePaths...),
		"crawlEntireDomain":  spec.CrawlEntireDomain,
		"allowExternalLinks": spec.AllowExternalLinks,
		"allowSubdomains":    spec.AllowSubdomains,
		"ignoreRobotsTxt":    spec.IgnoreRobotsTxt,
		"sitemap":            spec.Sitemap,
	}
	if spec.MaxDiscoveryDepth != nil {
		resolved["maxDiscoveryDepth"] = *spec.MaxDiscoveryDepth
	}
	if spec.ScheduleAt != nil && !spec.ScheduleAt.IsZero() {
		resolved["scheduleAt"] = spec.ScheduleAt.UTC().Format(time.RFC3339Nano)
	}
	formats := make([]string, 0, len(spec.PageOptions.Formats))
	for _, format := range spec.PageOptions.Formats {
		formatType := strings.TrimSpace(format.Type)
		if formatType != "" {
			formats = append(formats, formatType)
		}
	}
	if len(formats) > 0 {
		resolved["scrapeOptions"] = map[string]any{
			"formats": formats,
		}
	}
	if spec.ChangeTracking != nil {
		resolved["changeTracking"] = map[string]any{
			"enabled": spec.ChangeTracking.Enabled,
			"modes":   append([]string(nil), spec.ChangeTracking.Modes...),
			"tag":     spec.ChangeTracking.Tag,
			"dryRun":  spec.ChangeTracking.DryRun,
		}
	}
	return resolved
}
