package api

import (
	"fmt"
	"sort"
	"strings"
	"unicode"

	"github.com/triodelab/quarry/internal/actions"
	"github.com/triodelab/quarry/internal/scraper"
)

type researchPresetDefinition struct {
	Name          string
	Limit         int
	TimeoutSec    int
	MaxIterations int
	Sources       []v2SearchSource
	Formats       []string
	FocusTerms    []string
}

var researchPresetDefinitions = map[string]researchPresetDefinition{
	"lead-enrichment": {
		Name:          "lead-enrichment",
		Limit:         5,
		TimeoutSec:    120,
		MaxIterations: 2,
		Sources:       []v2SearchSource{{Type: "web"}},
		Formats:       []string{"markdown", "branding"},
		FocusTerms:    []string{"founders", "leadership", "customers", "contact"},
	},
	"ecommerce-monitor": {
		Name:          "ecommerce-monitor",
		Limit:         8,
		TimeoutSec:    120,
		MaxIterations: 2,
		Sources:       []v2SearchSource{{Type: "web"}, {Type: "images"}},
		Formats:       []string{"markdown", "branding", "pagestatus"},
		FocusTerms:    []string{"pricing", "inventory", "shipping", "returns"},
	},
	"competitive-monitor": {
		Name:          "competitive-monitor",
		Limit:         8,
		TimeoutSec:    150,
		MaxIterations: 3,
		Sources:       []v2SearchSource{{Type: "web"}, {Type: "news"}},
		Formats:       []string{"markdown", "seo", "pagestatus"},
		FocusTerms:    []string{"pricing", "features", "positioning", "customers"},
	},
	"finance-research": {
		Name:          "finance-research",
		Limit:         6,
		TimeoutSec:    150,
		MaxIterations: 3,
		Sources:       []v2SearchSource{{Type: "web"}, {Type: "news"}},
		Formats:       []string{"markdown", "seo", "pagestatus"},
		FocusTerms:    []string{"funding", "revenue", "earnings", "investors"},
	},
	"content-seeding": {
		Name:          "content-seeding",
		Limit:         6,
		TimeoutSec:    120,
		MaxIterations: 2,
		Sources:       []v2SearchSource{{Type: "web"}, {Type: "news"}},
		Formats:       []string{"markdown", "seo"},
		FocusTerms:    []string{"trends", "examples", "quotes", "statistics"},
	},
	"data-migration": {
		Name:          "data-migration",
		Limit:         10,
		TimeoutSec:    180,
		MaxIterations: 2,
		Sources:       []v2SearchSource{{Type: "web"}},
		Formats:       []string{"markdown", "html", "links"},
		FocusTerms:    []string{"docs", "faq", "reference", "help"},
	},
	"site-observability": {
		Name:          "site-observability",
		Limit:         6,
		TimeoutSec:    150,
		MaxIterations: 2,
		Sources:       []v2SearchSource{{Type: "web"}},
		Formats:       []string{"markdown", "seo", "wcag", "pagestatus"},
		FocusTerms:    []string{"uptime", "performance", "errors", "accessibility"},
	},
}

func normalizeResearchPresetName(raw string) string {
	return strings.ToLower(strings.TrimSpace(raw))
}

func lookupResearchPreset(raw string) (researchPresetDefinition, bool) {
	preset, ok := researchPresetDefinitions[normalizeResearchPresetName(raw)]
	return preset, ok
}

func buildV1ResearchScrapeOptions(raw *v2ScrapeOptionsRequest) (*scraper.FormatOptions, error) {
	if raw == nil {
		return nil, nil
	}
	pageOptions, err := parseV2ScrapeOptions(raw)
	if err != nil {
		return nil, err
	}
	formats, _ := selectCrawlFormats(pageOptions.Formats)
	if len(formats) == 0 {
		formats = []string{"markdown"}
	}
	return &scraper.FormatOptions{
		Formats:         formats,
		Headers:         cloneStringMap(pageOptions.Headers),
		WaitFor:         pageOptions.WaitFor,
		MaxAgeMs:        pageOptions.MaxAgeMs,
		OnlyMainContent: pageOptions.OnlyMainContent,
		IncludeTags:     append([]string(nil), pageOptions.IncludeTags...),
		ExcludeTags:     append([]string(nil), pageOptions.ExcludeTags...),
		Actions:         append([]actions.ActionStep(nil), pageOptions.Actions...),
		Mobile:          pageOptions.Mobile,
		Viewport:        pageOptions.Viewport,
		Location:        pageOptions.Location,
		ProxyURL:        pageOptions.ProxyURL,
		BlockAds:        pageOptions.BlockAds,
		ParserMode:      pageOptions.ParserMode,
	}, nil
}

func cloneResearchScrapeOptions(opts *scraper.FormatOptions) *scraper.FormatOptions {
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

func resolveV1ResearchConfig(req v1ResearchRequest, present map[string]struct{}, sources []v2SearchSource, scrapeOpts *scraper.FormatOptions) (v1ResearchRunConfig, map[string]any, error) {
	cfg := v1ResearchRunConfig{
		Query:         req.Query,
		Prompt:        req.Prompt,
		SystemPrompt:  req.SystemPrompt,
		Limit:         req.Limit,
		BlendMode:     normalizeBlendMode(req.BlendMode),
		Sources:       append([]v2SearchSource(nil), sources...),
		ScrapeOpts:    cloneResearchScrapeOptions(scrapeOpts),
		Webhook:       req.Webhook,
		TimeoutSec:    req.TimeoutSec,
		Preset:        normalizeResearchPresetName(req.Preset),
		MaxIterations: req.MaxIterations,
	}

	if cfg.Preset != "" {
		preset, ok := lookupResearchPreset(cfg.Preset)
		if !ok {
			return v1ResearchRunConfig{}, nil, fmt.Errorf("preset contains unsupported value")
		}
		if _, ok := present["limit"]; !ok {
			cfg.Limit = preset.Limit
		}
		if _, ok := present["timeout"]; !ok {
			cfg.TimeoutSec = preset.TimeoutSec
		}
		if _, ok := present["maxIterations"]; !ok {
			cfg.MaxIterations = preset.MaxIterations
		}
		if _, ok := present["sources"]; !ok && len(req.Sources) == 0 {
			cfg.Sources = append([]v2SearchSource(nil), preset.Sources...)
		}
		if cfg.ScrapeOpts == nil && !fieldPresent(present, "scrapeOptions") && !fieldPresent(present, "formats") {
			cfg.ScrapeOpts = &scraper.FormatOptions{Formats: append([]string(nil), preset.Formats...)}
		}
		cfg.FocusTerms = append([]string(nil), preset.FocusTerms...)
	}

	if cfg.Limit <= 0 {
		cfg.Limit = 5
	}
	if cfg.Limit > 10 {
		cfg.Limit = 10
	}
	if cfg.TimeoutSec <= 0 {
		cfg.TimeoutSec = 90
	}
	if cfg.TimeoutSec > 300 {
		cfg.TimeoutSec = 300
	}
	if cfg.MaxIterations <= 0 {
		cfg.MaxIterations = 1
	}
	if cfg.MaxIterations > 4 {
		cfg.MaxIterations = 4
	}
	if len(cfg.Sources) == 0 {
		cfg.Sources = []v2SearchSource{{Type: "web"}}
	}
	if cfg.BlendMode == "" {
		cfg.BlendMode = searchBlendRanked
	}
	if cfg.ScrapeOpts == nil {
		cfg.ScrapeOpts = &scraper.FormatOptions{Formats: []string{"markdown", "html"}}
	}
	if len(cfg.ScrapeOpts.Formats) == 0 {
		cfg.ScrapeOpts.Formats = []string{"markdown"}
	}

	resolved := map[string]any{
		"preset":        cfg.Preset,
		"limit":         cfg.Limit,
		"timeout":       cfg.TimeoutSec,
		"maxIterations": cfg.MaxIterations,
		"blendMode":     cfg.BlendMode,
		"sources":       researchSourcesJSON(cfg.Sources),
		"scrapeOptions": map[string]any{
			"formats": append([]string(nil), cfg.ScrapeOpts.Formats...),
		},
	}
	return cfg, resolved, nil
}

func fieldPresent(present map[string]struct{}, key string) bool {
	if len(present) == 0 {
		return false
	}
	_, ok := present[key]
	return ok
}

func researchSourcesJSON(sources []v2SearchSource) []map[string]any {
	if len(sources) == 0 {
		return nil
	}
	out := make([]map[string]any, 0, len(sources))
	for _, source := range sources {
		out = append(out, map[string]any{
			"type":          source.Type,
			"site":          source.Site,
			"weight":        source.Weight,
			"limit":         source.Limit,
			"country":       source.Country,
			"searchLang":    source.SearchLang,
			"uiLang":        source.UILang,
			"freshness":     source.Freshness,
			"safeSearch":    source.SafeSearch,
			"extraSnippets": source.ExtraSnippets,
		})
	}
	return out
}

func initialResearchData(cfg v1ResearchRunConfig) map[string]any {
	return map[string]any{
		"preset":        cfg.Preset,
		"blendMode":     cfg.BlendMode,
		"iterations":    0,
		"maxIterations": cfg.MaxIterations,
		"queryPlan":     []string{},
	}
}

func buildResearchFollowUpQueries(cfg v1ResearchRunConfig, sources []researchSource, seen map[string]struct{}, remaining int) []string {
	if remaining <= 0 {
		return nil
	}
	candidates := make([]string, 0, remaining+len(cfg.FocusTerms))
	for _, focus := range cfg.FocusTerms {
		focus = strings.TrimSpace(focus)
		if focus == "" {
			continue
		}
		candidates = append(candidates, strings.TrimSpace(cfg.Query+" "+focus))
	}
	for _, term := range researchKeywords(sources) {
		candidates = append(candidates, strings.TrimSpace(cfg.Query+" "+term))
	}

	out := make([]string, 0, remaining)
	for _, candidate := range candidates {
		normalized := strings.ToLower(strings.TrimSpace(candidate))
		if normalized == "" {
			continue
		}
		if _, ok := seen[normalized]; ok {
			continue
		}
		out = append(out, candidate)
		if len(out) == remaining {
			break
		}
	}
	return out
}

func researchKeywords(sources []researchSource) []string {
	counts := map[string]int{}
	for _, source := range sources {
		for _, token := range tokenizeResearchText(source.Title + " " + source.Snippet + " " + source.Content) {
			counts[token]++
		}
	}

	type scored struct {
		token string
		count int
	}
	scoredTokens := make([]scored, 0, len(counts))
	for token, count := range counts {
		scoredTokens = append(scoredTokens, scored{token: token, count: count})
	}
	sort.SliceStable(scoredTokens, func(i, j int) bool {
		if scoredTokens[i].count == scoredTokens[j].count {
			return scoredTokens[i].token < scoredTokens[j].token
		}
		return scoredTokens[i].count > scoredTokens[j].count
	})

	out := make([]string, 0, min(4, len(scoredTokens)))
	for _, item := range scoredTokens {
		out = append(out, item.token)
		if len(out) == 4 {
			break
		}
	}
	return out
}

func tokenizeResearchText(raw string) []string {
	stopwords := map[string]struct{}{
		"about": {}, "after": {}, "availability": {}, "their": {}, "there": {}, "these": {}, "those": {},
		"quarry": {}, "monitor": {}, "overview": {}, "guide": {}, "docs": {}, "with": {},
		"from": {}, "into": {}, "have": {}, "that": {}, "this": {}, "what": {}, "when": {}, "where": {},
		"which": {}, "while": {}, "will": {}, "would": {}, "your": {}, "ours": {}, "ourselves": {},
		"example": {}, "team": {}, "teams": {}, "product": {}, "products": {}, "company": {},
	}

	normalized := strings.Map(func(r rune) rune {
		if unicode.IsLetter(r) || unicode.IsNumber(r) || unicode.IsSpace(r) {
			return unicode.ToLower(r)
		}
		return ' '
	}, raw)

	fields := strings.Fields(normalized)
	out := make([]string, 0, len(fields))
	for _, field := range fields {
		if len(field) < 4 {
			continue
		}
		if _, blocked := stopwords[field]; blocked {
			continue
		}
		out = append(out, field)
	}
	return out
}
