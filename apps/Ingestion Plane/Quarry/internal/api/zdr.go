package api

import (
	"strings"

	quarrycrawl "github.com/triodelab/quarry/internal/crawl"
	"github.com/triodelab/quarry/internal/jobs"
	quarrysearch "github.com/triodelab/quarry/internal/search"
)

func (h *Handler) isJobZDR(jobID string) bool {
	if h == nil || h.jobStore == nil || strings.TrimSpace(jobID) == "" {
		return false
	}
	job, ok := h.jobStore.Get(jobID)
	if !ok || job == nil {
		return false
	}
	return strings.EqualFold(strings.TrimSpace(job.Meta["zdr_mode"]), "true")
}

func sanitizeStoredSearchResultsForZDR(results []quarrysearch.StoredResult) []quarrysearch.StoredResult {
	if results == nil {
		return nil
	}
	sanitized := make([]quarrysearch.StoredResult, 0, len(results))
	for _, result := range results {
		sanitized = append(sanitized, quarrysearch.StoredResult{
			URL:    result.URL,
			Source: result.Source,
			Type:   result.Type,
			Score:  result.Score,
		})
	}
	return sanitized
}

func sanitizeExtractPayloadForZDR(result map[string]interface{}, completed, total int) map[string]interface{} {
	return map[string]interface{}{
		"redacted":  true,
		"completed": completed,
		"total":     total,
	}
}

func sanitizeResearchPayloadForZDR(steps []researchStep) ([]researchSource, string, map[string]any) {
	return nil, "", map[string]any{
		"redacted": true,
		"steps":    steps,
	}
}

func sanitizeCrawlFetchedPageForZDR(page *quarrycrawl.FetchedPage) *quarrycrawl.FetchedPage {
	if page == nil {
		return nil
	}
	page.Outputs = nil
	if page.Metadata == nil {
		page.Metadata = map[string]interface{}{}
	}
	page.Metadata["sourceURL"] = ""
	if pageStatus, ok := page.Metadata["pageStatus"].(map[string]interface{}); ok {
		pageStatus["sourceURL"] = ""
		page.Metadata["pageStatus"] = pageStatus
	}
	return page
}

func sanitizeExtractionJobForZDR(job *jobs.ExtractionJob) *jobs.ExtractionJob {
	if job == nil {
		return nil
	}
	job.URL = ""
	job.Prompt = ""
	job.Schema = ""
	return job
}
