package jobs

import (
	"reflect"
	"testing"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/workflows"
)

// TestCrawlInputFromJob_UsesSharedBuilder: the dispatcher's crawl input is the
// shared builder's output with the dispatcher's resolved bounds, so the scheduled
// wrapper and the dispatcher cannot build different inputs from the same params.
func TestCrawlInputFromJob_UsesSharedBuilder(t *testing.T) {
	t.Parallel()
	j := job{ID: "job_1", Kind: "crawl", Params: map[string]any{
		"url":       "https://example.com",
		"org_id":    "org_a",
		"user_id":   "user_1",
		"max_pages": float64(0),
		"max_depth": float64(2),
	}}
	limits := DefaultCrawlLimits()
	got, err := crawlInputFromJob(j, "run_1", limits)
	if err != nil {
		t.Fatalf("crawlInputFromJob: %v", err)
	}
	resolved := limits.Resolve(0, 2)
	want, err := workflows.CrawlInputFromParams(j.Params, "run_1", "job_1", resolved.Depth, resolved.Pages)
	if err != nil {
		t.Fatalf("shared builder: %v", err)
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("dispatcher input differs from the shared builder:\n got %+v\nwant %+v", got, want)
	}
	if got.MaxPages != defaultCrawlMaxPages {
		t.Fatalf("a zero page request must take the default, got %d", got.MaxPages)
	}
}

// TestBatchInputFromJob_UsesSharedBuilder: the batch input matches the shared builder.
func TestBatchInputFromJob_UsesSharedBuilder(t *testing.T) {
	t.Parallel()
	j := job{ID: "job_2", Kind: "batch", Params: map[string]any{
		"urls":    []any{"https://a.example", "https://b.example"},
		"org_id":  "org_a",
		"ingest":  true,
		"user_id": "user_1",
	}}
	got, err := batchInputFromJob(j, "run_2")
	if err != nil {
		t.Fatalf("batchInputFromJob: %v", err)
	}
	want, err := workflows.BatchInputFromParams(j.Params, "run_2", "job_2")
	if err != nil {
		t.Fatalf("shared builder: %v", err)
	}
	if !reflect.DeepEqual(got, want) || len(got.URLs) != 2 || !got.Ingest {
		t.Fatalf("batch input = %+v, want %+v", got, want)
	}
}

// TestScrapeInputFromJob_RequiresURL: the refusal is the shared builder's.
func TestScrapeInputFromJob_RequiresURL(t *testing.T) {
	t.Parallel()
	if _, err := scrapeInputFromJob(job{ID: "job_3", Kind: "scrape", Params: map[string]any{}}, "run_3"); err == nil {
		t.Fatal("a scrape job without a url must be refused")
	}
}
