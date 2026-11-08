package workflows

import "testing"

func TestCrawlInputFromParams_RefusesZeroBounds(t *testing.T) {
	t.Parallel()
	params := map[string]any{"url": "https://example.com"}
	if _, err := CrawlInputFromParams(params, "run_1", "job_1", 0, 10); err == nil {
		t.Fatal("a zero depth must be refused, zero used to mean unlimited")
	}
	if _, err := CrawlInputFromParams(params, "run_1", "job_1", 3, 0); err == nil {
		t.Fatal("a zero page bound must be refused")
	}
}

func TestCrawlInputFromParams_SeedsFromURLOrSeeds(t *testing.T) {
	t.Parallel()
	in, err := CrawlInputFromParams(map[string]any{"url": "https://example.com"}, "run_1", "job_1", 2, 7)
	if err != nil || len(in.Seeds) != 1 || in.MaxPages != 7 || in.MaxDepth != 2 || in.JobID != "job_1" {
		t.Fatalf("url seed input = %+v, err=%v", in, err)
	}
	in, err = CrawlInputFromParams(map[string]any{"seeds": []any{"https://a.example", "", "https://b.example"}}, "run_1", "job_1", 2, 7)
	if err != nil || len(in.Seeds) != 2 {
		t.Fatalf("seeds input = %+v, err=%v", in, err)
	}
	if _, err := CrawlInputFromParams(map[string]any{}, "run_1", "job_1", 2, 7); err == nil {
		t.Fatal("a crawl without seeds must be refused")
	}
}

func TestScrapeAndBatchInputs_RequireTheirParams(t *testing.T) {
	t.Parallel()
	if _, err := ScrapeInputFromParams(map[string]any{}, "run_1", "job_1"); err == nil {
		t.Fatal("scrape without url must be refused")
	}
	if _, err := BatchInputFromParams(map[string]any{}, "run_1", "job_1"); err == nil {
		t.Fatal("batch without urls must be refused")
	}
	in, err := BatchInputFromParams(map[string]any{"urls": []any{"https://a.example"}, "org_id": "org_a", "ingest": true}, "run_1", "job_1")
	if err != nil || len(in.URLs) != 1 || in.OrgID != "org_a" || !in.Ingest {
		t.Fatalf("batch input = %+v, err=%v", in, err)
	}
}

func TestUintFromParam(t *testing.T) {
	t.Parallel()
	cases := map[string]struct {
		in   any
		want uint32
	}{
		"float":    {float64(12), 12},
		"negative": {float64(-4), 0},
		"int":      {7, 7},
		"string":   {"9", 0},
		"nil":      {nil, 0},
	}
	for name, c := range cases {
		if got := UintFromParam(c.in); got != c.want {
			t.Fatalf("%s: got %d want %d", name, got, c.want)
		}
	}
}
