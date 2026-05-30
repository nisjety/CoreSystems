package client

import "time"

func LeadEnrichmentSearch(query string) SearchRequest {
	return SearchRequest{
		Query:  query,
		Preset: "lead-enrichment",
	}
}

func LeadEnrichmentExtract(prompt string) ExtractRequest {
	return ExtractRequest{
		Prompt:          prompt,
		Preset:          "lead-enrichment",
		EnableWebSearch: true,
	}
}

func CompetitiveMonitorResearch(query string) ResearchRequest {
	return ResearchRequest{
		Query:  query,
		Preset: "competitive-monitor",
	}
}

func SiteObservabilityCrawl(targetURL string, scheduleAt *time.Time) CrawlRequest {
	req := CrawlRequest{
		URL:    targetURL,
		Preset: "site-observability",
	}
	if scheduleAt != nil {
		normalized := scheduleAt.UTC()
		req.ScheduleAt = &normalized
	}
	return req
}
