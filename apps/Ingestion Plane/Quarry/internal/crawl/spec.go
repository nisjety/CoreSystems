package crawl

import (
	"fmt"
	"strings"
	"time"

	"github.com/triodelab/quarry/internal/actions"
	"github.com/triodelab/quarry/internal/models"
)

const (
	DefaultLimit          = 10000
	DefaultMapLimit       = 5000
	DefaultMaxConcurrency = 4
)

type NormalizeInput struct {
	URL                    string
	Preset                 string
	IncludePaths           []string
	ExcludePaths           []string
	MaxDiscoveryDepth      *int
	MaxDepth               *int
	Limit                  int
	CrawlEntireDomain      bool
	AllowExternalLinks     bool
	AllowSubdomains        bool
	IgnoreRobotsTxt        bool
	Sitemap                SitemapMode
	DeduplicateSimilarURLs *bool
	IgnoreQueryParameters  bool
	RegexOnFullURL         bool
	RegexPaths             bool
	DelayMs                int
	MaxConcurrency         int
	Prompt                 string
	Schema                 string
	Module                 string
	Enrich                 bool
	EnrichLimit            int
	MaxAge                 int64
	ScheduleAt             *time.Time
	ChangeTracking         *models.ChangeTrackingRequest
	PageOptions            PageOptions
	DiscoveryOnly          bool
}

func NormalizeSpec(input NormalizeInput) (Spec, error) {
	if strings.TrimSpace(input.URL) == "" {
		return Spec{}, fmt.Errorf("url is required")
	}

	spec := Spec{
		URL:                   strings.TrimSpace(input.URL),
		Preset:                strings.TrimSpace(input.Preset),
		IncludePaths:          cloneStrings(input.IncludePaths),
		ExcludePaths:          cloneStrings(input.ExcludePaths),
		Limit:                 input.Limit,
		CrawlEntireDomain:     input.CrawlEntireDomain,
		AllowExternalLinks:    input.AllowExternalLinks,
		AllowSubdomains:       input.AllowSubdomains,
		IgnoreRobotsTxt:       input.IgnoreRobotsTxt,
		Sitemap:               input.Sitemap,
		IgnoreQueryParameters: input.IgnoreQueryParameters,
		RegexOnFullURL:        input.RegexOnFullURL,
		RegexPaths:            input.RegexPaths,
		Delay:                 time.Duration(input.DelayMs) * time.Millisecond,
		MaxConcurrency:        input.MaxConcurrency,
		Prompt:                strings.TrimSpace(input.Prompt),
		Module:                strings.TrimSpace(input.Module),
		Enrich:                input.Enrich,
		EnrichLimit:           input.EnrichLimit,
		MaxAge:                input.MaxAge,
		ScheduleAt:            cloneTimePtr(input.ScheduleAt),
		ChangeTracking:        cloneChangeTracking(input.ChangeTracking),
		PageOptions:           clonePageOptions(input.PageOptions),
		DiscoveryOnly:         input.DiscoveryOnly,
	}

	if spec.Sitemap == "" {
		spec.Sitemap = SitemapInclude
	}
	if spec.MaxConcurrency <= 0 {
		spec.MaxConcurrency = DefaultMaxConcurrency
	}
	if spec.Limit <= 0 {
		if spec.DiscoveryOnly {
			spec.Limit = DefaultMapLimit
		} else {
			spec.Limit = DefaultLimit
		}
	}
	if spec.EnrichLimit < 0 {
		return Spec{}, fmt.Errorf("enrichLimit must be >= 0")
	}
	if spec.MaxAge < 0 {
		return Spec{}, fmt.Errorf("maxAge must be >= 0")
	}

	if input.MaxDiscoveryDepth != nil {
		if *input.MaxDiscoveryDepth < 0 {
			return Spec{}, fmt.Errorf("maxDiscoveryDepth must be >= 0")
		}
		value := *input.MaxDiscoveryDepth
		spec.MaxDiscoveryDepth = &value
	} else if input.MaxDepth != nil {
		if *input.MaxDepth < 0 {
			return Spec{}, fmt.Errorf("maxDepth must be >= 0")
		}
		value := *input.MaxDepth
		spec.MaxDiscoveryDepth = &value
	}

	if input.DeduplicateSimilarURLs == nil {
		spec.DeduplicateSimilarURLs = true
	} else {
		spec.DeduplicateSimilarURLs = *input.DeduplicateSimilarURLs
	}

	if spec.PageOptions.MaxAgeMs <= 0 && spec.MaxAge > 0 {
		spec.PageOptions.MaxAgeMs = spec.MaxAge
	}

	if len(spec.PageOptions.Formats) == 0 {
		if strings.TrimSpace(input.Schema) != "" {
			spec.PageOptions.Formats = []Format{{
				Type:   "json",
				Schema: strings.TrimSpace(input.Schema),
				Prompt: strings.TrimSpace(input.Prompt),
			}}
			spec.Prompt = ""
		} else {
			spec.PageOptions.Formats = []Format{{Type: "markdown"}}
		}
	}

	return spec, nil
}

func cloneStrings(src []string) []string {
	if len(src) == 0 {
		return nil
	}
	out := make([]string, len(src))
	copy(out, src)
	return out
}

func cloneHeaders(src map[string]string) map[string]string {
	if len(src) == 0 {
		return nil
	}
	out := make(map[string]string, len(src))
	for key, value := range src {
		out[key] = value
	}
	return out
}

func clonePageOptions(src PageOptions) PageOptions {
	return PageOptions{
		Formats:         append([]Format(nil), src.Formats...),
		Headers:         cloneHeaders(src.Headers),
		WaitFor:         src.WaitFor,
		OnlyMainContent: src.OnlyMainContent,
		IncludeTags:     cloneStrings(src.IncludeTags),
		ExcludeTags:     cloneStrings(src.ExcludeTags),
		Mobile:          src.Mobile,
		Viewport:        src.Viewport,
		Location:        src.Location,
		BlockAds:        src.BlockAds,
		Actions:         append([]actions.ActionStep(nil), src.Actions...),
		MaxAgeMs:        src.MaxAgeMs,
	}
}

func cloneTimePtr(src *time.Time) *time.Time {
	if src == nil {
		return nil
	}
	value := *src
	return &value
}

func cloneChangeTracking(src *models.ChangeTrackingRequest) *models.ChangeTrackingRequest {
	if src == nil {
		return nil
	}
	cloned := *src
	cloned.Modes = append([]string(nil), src.Modes...)
	if len(src.Schema) > 0 {
		cloned.Schema = make(map[string]interface{}, len(src.Schema))
		for key, value := range src.Schema {
			cloned.Schema[key] = value
		}
	}
	return &cloned
}
