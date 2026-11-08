package activities

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"

	"github.com/stretchr/testify/require"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/errs"
)

// INNH-6a: SitemapSeeds asks the edge for the sitemap URLs of a crawl and either returns
// them or enqueues them itself. The edge is a test server here.

type fakeSitemapEdge struct {
	mu          sync.Mutex
	sitemapReqs []map[string]any
	sitemapSigs []string
	sitemapAuth []string
	batches     [][]FrontierBatchItem
	urls        []string
	status      int
}

func (f *fakeSitemapEdge) handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/v1/internal/crawl/sitemap", func(w http.ResponseWriter, r *http.Request) {
		raw, _ := io.ReadAll(r.Body)
		var body map[string]any
		_ = json.Unmarshal(raw, &body)
		f.mu.Lock()
		f.sitemapReqs = append(f.sitemapReqs, body)
		f.sitemapSigs = append(f.sitemapSigs, r.Header.Get("X-Quarry-Run-Sig"))
		f.sitemapAuth = append(f.sitemapAuth, r.Header.Get("Authorization"))
		status := f.status
		urls := f.urls
		f.mu.Unlock()
		if status != 0 {
			w.WriteHeader(status)
			_, _ = w.Write([]byte(`{"error":{"code":"UNSUPPORTED"}}`))
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"urls":             urls,
			"discovered":       len(urls) + 3,
			"sitemaps_fetched": 2,
			"nested_fetched":   1,
			"skipped":          map[string]any{"out_of_scope": 2, "duplicate": 1},
			"truncated":        false,
		})
	})
	mux.HandleFunc("/v1/internal/queues/", func(w http.ResponseWriter, r *http.Request) {
		raw, _ := io.ReadAll(r.Body)
		var body struct {
			Items []FrontierBatchItem `json:"items"`
		}
		_ = json.Unmarshal(raw, &body)
		f.mu.Lock()
		f.batches = append(f.batches, body.Items)
		f.mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(fmt.Sprintf(`{"data":{"accepted":%d}}`, len(body.Items))))
	})
	return mux
}

func newSitemapActivities(t *testing.T, edge *fakeSitemapEdge) *Activities {
	t.Helper()
	srv := httptest.NewServer(edge.handler())
	t.Cleanup(srv.Close)
	return New(Config{
		RuntimeBaseURL:   srv.URL,
		RuntimeAuthToken: "runtime-token",
		EdgeBaseURL:      srv.URL,
		EdgeAuthToken:    "edge-token",
		EdgeRunSecret:    "run-secret",
	})
}

func sitemapCrawlContext() *CrawlContext {
	return &CrawlContext{
		V: CrawlContractVersion, Kind: CrawlKindCrawl, MaxDepth: 3,
		SeedHosts: []string{"site.test"}, SeedURL: "https://site.test/",
		Scope:     CrawlScope{CrawlEntireDomain: true},
		ScopeMode: ScopeModeEnforce, Robots: RobotsRespect,
	}
}

func TestSitemapSeeds_ReturnsTheURLsWhenThereIsNoQueue(t *testing.T) {
	edge := &fakeSitemapEdge{urls: []string{"https://site.test/a", "https://site.test/b"}}
	a := newSitemapActivities(t, edge)

	res, err := a.SitemapSeeds(context.Background(), SitemapSeedsInput{
		RunID: "run_1", OrgID: "org_a", Seed: "https://site.test/", Crawl: sitemapCrawlContext(), MaxURLs: 50,
	})
	require.NoError(t, err)
	require.Equal(t, []string{"https://site.test/a", "https://site.test/b"}, res.URLs)
	require.Equal(t, uint32(2), res.Seeded)
	require.Equal(t, uint32(5), res.Discovered)
	require.Equal(t, uint32(2), res.SitemapsFetched)
	require.Equal(t, uint32(1), res.NestedFetched)
	require.Equal(t, uint32(2), res.Skipped.OutOfScope)
	require.Equal(t, uint32(1), res.Skipped.Duplicate)

	require.Len(t, edge.sitemapReqs, 1)
	req := edge.sitemapReqs[0]
	require.Equal(t, "https://site.test/", req["url"])
	require.Equal(t, "org_a", req["org_id"])
	require.Equal(t, float64(50), req["max_urls"])
	crawl, _ := req["crawl"].(map[string]any)
	require.NotNil(t, crawl, "the crawl scope travels with the request")
	require.Equal(t, "enforce", crawl["scope_mode"])
	// The org binding is the one run_page uses: the org and the seed URL, signed.
	require.Equal(t, runBindingSig("run-secret", "org_a", "https://site.test/"), edge.sitemapSigs[0])
	require.Equal(t, "Bearer runtime-token", edge.sitemapAuth[0])
}

func TestSitemapSeeds_EnqueuesInBatchesOf500WithTheFrontierRequestIDs(t *testing.T) {
	var urls []string
	for i := 0; i < 1100; i++ {
		urls = append(urls, fmt.Sprintf("https://site.test/p/%d", i))
	}
	edge := &fakeSitemapEdge{urls: urls}
	a := newSitemapActivities(t, edge)

	res, err := a.SitemapSeeds(context.Background(), SitemapSeedsInput{
		RunID: "run_1", OrgID: "org_a", Seed: "https://site.test/", Crawl: sitemapCrawlContext(),
		MaxURLs: 2000, Queue: "crawl-q", Depth: 1,
	})
	require.NoError(t, err)
	require.Empty(t, res.URLs, "with a queue the URLs never travel back through the workflow history")
	require.Equal(t, uint32(1100), res.Seeded)

	require.Len(t, edge.batches, 3)
	require.Len(t, edge.batches[0], 500)
	require.Len(t, edge.batches[2], 100)
	first := edge.batches[0][0]
	require.Equal(t, "https://site.test/p/0", first.URL)
	require.Equal(t, uint32(1), first.Depth, "sitemap URLs are queued one level below the seed")
	require.Equal(t, FrontierRequestID("run_1", first.URL), first.RequestID)
}

func TestSitemapSeeds_ARetryEnqueuesTheSameRequestIDs(t *testing.T) {
	edge := &fakeSitemapEdge{urls: []string{"https://site.test/a", "https://site.test/b"}}
	a := newSitemapActivities(t, edge)
	in := SitemapSeedsInput{
		RunID: "run_1", OrgID: "org_a", Seed: "https://site.test/", Crawl: sitemapCrawlContext(),
		MaxURLs: 10, Queue: "crawl-q", Depth: 1,
	}
	_, err := a.SitemapSeeds(context.Background(), in)
	require.NoError(t, err)
	_, err = a.SitemapSeeds(context.Background(), in)
	require.NoError(t, err)
	require.Len(t, edge.batches, 2)
	require.Equal(t, edge.batches[0], edge.batches[1], "the queue drops what it already holds by request id")
}

func TestSitemapSeeds_CutsAResponseThatExceedsMaxURLs(t *testing.T) {
	edge := &fakeSitemapEdge{urls: []string{"https://site.test/a", "https://site.test/b", "https://site.test/c"}}
	a := newSitemapActivities(t, edge)
	res, err := a.SitemapSeeds(context.Background(), SitemapSeedsInput{
		RunID: "run_1", OrgID: "org_a", Seed: "https://site.test/", Crawl: sitemapCrawlContext(), MaxURLs: 2,
	})
	require.NoError(t, err)
	require.Len(t, res.URLs, 2)
	require.Equal(t, uint32(2), res.Seeded)
}

func TestSitemapSeeds_ADisabledEdgeIsANonRetryableError(t *testing.T) {
	for _, status := range []int{http.StatusNotImplemented, http.StatusNotFound, http.StatusBadRequest} {
		edge := &fakeSitemapEdge{status: status}
		a := newSitemapActivities(t, edge)
		_, err := a.SitemapSeeds(context.Background(), SitemapSeedsInput{
			RunID: "run_1", OrgID: "org_a", Seed: "https://site.test/", Crawl: sitemapCrawlContext(), MaxURLs: 10,
		})
		require.Error(t, err, "status %d", status)
		require.True(t, errs.IsNonRetryable(err), "status %d must not be retried", status)
	}
	// A server error is retried.
	edge := &fakeSitemapEdge{status: http.StatusBadGateway}
	a := newSitemapActivities(t, edge)
	_, err := a.SitemapSeeds(context.Background(), SitemapSeedsInput{
		RunID: "run_1", OrgID: "org_a", Seed: "https://site.test/", Crawl: sitemapCrawlContext(), MaxURLs: 10,
	})
	require.Error(t, err)
	require.False(t, errs.IsNonRetryable(err))
}

func TestSitemapSeeds_RefusesACrawlWithoutAScope(t *testing.T) {
	edge := &fakeSitemapEdge{}
	a := newSitemapActivities(t, edge)
	_, err := a.SitemapSeeds(context.Background(), SitemapSeedsInput{RunID: "run_1", OrgID: "org_a", Seed: "https://site.test/"})
	require.Error(t, err)
	require.True(t, errs.IsNonRetryable(err))
	require.Empty(t, edge.sitemapReqs, "nothing is sent without a scope")
}

// INNH-6b: the scrape options ride the run_page body only when the crawl has them.
func TestRunPage_SendsScrapeOptionsOnlyWhenTheCrawlHasThemAndReadsTheEcho(t *testing.T) {
	var mu sync.Mutex
	var raws []string
	mux := http.NewServeMux()
	mux.HandleFunc("/v1/internal/run_page", func(w http.ResponseWriter, r *http.Request) {
		raw, _ := io.ReadAll(r.Body)
		mu.Lock()
		raws = append(raws, string(raw))
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		if json.Valid(raw) && containsKey(raw, "scrape") {
			_, _ = w.Write([]byte(`{"status":200,"fingerprint":"blake3:x","links":[],"scrape_applied":true}`))
			return
		}
		_, _ = w.Write([]byte(`{"status":200,"fingerprint":"blake3:x","links":[]}`))
	})
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	a := New(Config{RuntimeBaseURL: srv.URL, RuntimeAuthToken: "t"})

	plain, err := a.RunPage(context.Background(), RunPageInput{RunID: "r", URL: "https://site.test/p", OrgID: "org_a", Kind: "crawl"})
	require.NoError(t, err)
	require.Nil(t, plain.ScrapeApplied)

	withOptions, err := a.RunPage(context.Background(), RunPageInput{
		RunID: "r", URL: "https://site.test/p", OrgID: "org_a", Kind: "crawl",
		Scrape: json.RawMessage(`{"exclude_tags":["nav"],"extractor":"readability","include_tags":[],"only_main_content":true,"remove_base64_images":false}`),
	})
	require.NoError(t, err)
	require.NotNil(t, withOptions.ScrapeApplied)
	require.True(t, *withOptions.ScrapeApplied)

	mu.Lock()
	defer mu.Unlock()
	require.Len(t, raws, 2)
	require.NotContains(t, raws[0], "scrape", "a page without options is byte-identical to before")
	require.Contains(t, raws[1], `"scrape":{"exclude_tags":["nav"]`)
}

func containsKey(raw []byte, key string) bool {
	var m map[string]json.RawMessage
	if err := json.Unmarshal(raw, &m); err != nil {
		return false
	}
	_, ok := m[key]
	return ok
}
