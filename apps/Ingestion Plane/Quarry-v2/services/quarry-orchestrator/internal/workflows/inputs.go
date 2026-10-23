package workflows

// Pure input builders for the three executable job kinds (WS15.S2). The job
// dispatcher (internal/jobs) and the scheduled-job wrapper (ScheduledJobWF)
// both build a child workflow input from a job's params through these
// functions, so the two paths cannot drift. Nothing here reads the clock, the
// environment or randomness: ScheduledJobWF runs them inside deterministic
// workflow code.
//
// Crawl bounds are resolved by the caller. The dispatcher resolves them with
// its CrawlLimits at dispatch; the schedule reconciler resolves them when it
// materializes a crawl schedule and stores the result in params (max_pages,
// max_depth), so the wrapper reads the same bounds on every fire.

import (
	"errors"
)

// ScrapeInputFromParams builds a ScrapeJobWF input from a job's params.
func ScrapeInputFromParams(params map[string]any, runID, jobID string) (ScrapeJobInput, error) {
	url, _ := params["url"].(string)
	if url == "" {
		return ScrapeJobInput{}, errors.New("scrape job missing params.url")
	}
	return ScrapeJobInput{
		UserID: UserFromParams(params),
		Ingest: IngestFromParams(params),
		RunID:  runID,
		JobID:  jobID,
		OrgID:  OrgFromParams(params),
		URL:    url,
	}, nil
}

// CrawlInputFromParams builds a CrawlJobWF input. Seeds come from params.seeds,
// or from a single params.url. maxDepth and maxPages are the already-resolved
// bounds; a zero bound is refused, because zero used to mean "unlimited".
func CrawlInputFromParams(params map[string]any, runID, jobID string, maxDepth, maxPages uint32) (CrawlJobInput, error) {
	seeds := StringsFromParam(params["seeds"])
	if len(seeds) == 0 {
		if single, ok := params["url"].(string); ok && single != "" {
			seeds = []string{single}
		}
	}
	if len(seeds) == 0 {
		return CrawlJobInput{}, errors.New("crawl job missing params.url or params.seeds")
	}
	if maxDepth == 0 || maxPages == 0 {
		return CrawlJobInput{}, errors.New("crawl job has no resolved depth or page bound")
	}
	return CrawlJobInput{
		UserID:   UserFromParams(params),
		Ingest:   IngestFromParams(params),
		RunID:    runID,
		JobID:    jobID,
		OrgID:    OrgFromParams(params),
		Seeds:    seeds,
		MaxDepth: maxDepth,
		MaxPages: maxPages,
	}, nil
}

// BatchInputFromParams builds a BatchJobWF input from params.urls.
func BatchInputFromParams(params map[string]any, runID, jobID string) (BatchJobInput, error) {
	urls := StringsFromParam(params["urls"])
	if len(urls) == 0 {
		return BatchJobInput{}, errors.New("batch job missing params.urls")
	}
	return BatchJobInput{
		UserID: UserFromParams(params),
		Ingest: IngestFromParams(params),
		RunID:  runID,
		JobID:  jobID,
		OrgID:  OrgFromParams(params),
		URLs:   urls,
	}, nil
}

// IngestFromParams reads the selective-ingest flag (Phase 2) from a job's
// params. The edge handoff stamps `ingest`; false or absent means the working
// set only (default NEVER).
func IngestFromParams(params map[string]any) bool {
	if b, ok := params["ingest"].(bool); ok {
		return b
	}
	return false
}

// UserFromParams extracts the initiating user id from a job's params. "" when
// absent, which makes the job a system or legacy job (org-visible).
func UserFromParams(params map[string]any) string {
	if s, ok := params["user_id"].(string); ok {
		return s
	}
	return ""
}

// OrgFromParams extracts the originating tenant id from a job's params. "" when
// absent (legacy jobs); the edge enforces a non-empty org_id itself.
func OrgFromParams(params map[string]any) string {
	if s, ok := params["org_id"].(string); ok {
		return s
	}
	return ""
}

// StringsFromParam returns the non-empty strings of a JSON array param.
func StringsFromParam(v any) []string {
	arr, ok := v.([]any)
	if !ok {
		return nil
	}
	out := make([]string, 0, len(arr))
	for _, x := range arr {
		if s, ok := x.(string); ok && s != "" {
			out = append(out, s)
		}
	}
	return out
}

// UintFromParam reads a non-negative integer param; anything else is zero.
func UintFromParam(v any) uint32 {
	switch n := v.(type) {
	case float64:
		if n < 0 {
			return 0
		}
		return uint32(n)
	case int:
		if n < 0 {
			return 0
		}
		return uint32(n)
	case int64:
		if n < 0 {
			return 0
		}
		return uint32(n)
	case uint32:
		return n
	case uint64:
		return uint32(n)
	}
	return 0
}
