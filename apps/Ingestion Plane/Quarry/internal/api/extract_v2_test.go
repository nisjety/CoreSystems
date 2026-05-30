package api

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"sort"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/quarry/internal/actions"
	"github.com/triodelab/quarry/internal/ai"
	"github.com/triodelab/quarry/internal/batch"
	"github.com/triodelab/quarry/internal/jobs"
	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/scraper"
	quarrysearch "github.com/triodelab/quarry/internal/search"
)

func TestV2ExtractAcceptsSchemaObjectAndReturnsIDs(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			return map[string]interface{}{
				"html":     "<html><body><h1>Example</h1></body></html>",
				"markdown": "# Example",
			}, nil, nil
		},
		extractAIDataFn: func(ctx context.Context, req *ai.ExtractRequest) (*ai.ExtractResponse, error) {
			return &ai.ExtractResponse{Data: `{"name":"Example"}`}, nil
		},
	}

	app := fiber.New()
	handler.registerV2ExtractAndSearch(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/extract", map[string]interface{}{
		"urls":   []string{"https://example.com/item"},
		"prompt": "extract the name",
		"schema": map[string]interface{}{
			"type": "object",
			"properties": map[string]interface{}{
				"name": map[string]interface{}{"type": "string"},
			},
		},
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var created struct {
		Success     bool     `json:"success"`
		ID          string   `json:"id"`
		JobID       string   `json:"job_id"`
		Status      string   `json:"status"`
		URLTrace    []string `json:"urlTrace"`
		InvalidURLs []string `json:"invalidURLs"`
	}
	decodeJSONResponse(t, resp, &created)

	if !created.Success {
		t.Fatal("success = false, want true")
	}
	if created.ID == "" || created.JobID == "" {
		t.Fatalf("id/job_id = %q/%q, want both populated", created.ID, created.JobID)
	}
	if created.ID != created.JobID {
		t.Fatalf("id/job_id = %q/%q, want identical ids for compatibility", created.ID, created.JobID)
	}
	if created.Status != "processing" {
		t.Fatalf("status = %q, want processing", created.Status)
	}
	if len(created.URLTrace) != 1 || created.URLTrace[0] != "https://example.com/item" {
		t.Fatalf("urlTrace = %v, want [https://example.com/item]", created.URLTrace)
	}
	if len(created.InvalidURLs) != 0 {
		t.Fatalf("invalidURLs = %v, want empty", created.InvalidURLs)
	}

	status := waitForV2ExtractStatus(t, app, created.ID, "completed")
	raw, ok := status.Data["https://example.com/item"].(map[string]interface{})
	if !ok {
		t.Fatalf("data[url] type = %T, want object", status.Data["https://example.com/item"])
	}
	if raw["name"] != "Example" {
		t.Fatalf("data[url].name = %v, want Example", raw["name"])
	}
}

func TestV2ExtractIgnoreInvalidURLs(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			return map[string]interface{}{
				"html":     "<html><body><p>ok</p></body></html>",
				"markdown": "ok",
			}, nil, nil
		},
	}

	app := fiber.New()
	handler.registerV2ExtractAndSearch(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/extract", map[string]interface{}{
		"urls": []string{
			"https://example.com/valid",
			"notaurl",
			"ftp://example.com/file",
		},
		"ignoreInvalidURLs": true,
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var created struct {
		ID          string   `json:"id"`
		URLTrace    []string `json:"urlTrace"`
		InvalidURLs []string `json:"invalidURLs"`
	}
	decodeJSONResponse(t, resp, &created)

	if len(created.URLTrace) != 1 || created.URLTrace[0] != "https://example.com/valid" {
		t.Fatalf("urlTrace = %v, want [https://example.com/valid]", created.URLTrace)
	}
	if len(created.InvalidURLs) != 2 {
		t.Fatalf("invalidURLs = %v, want 2 invalid urls", created.InvalidURLs)
	}

	status := waitForV2ExtractStatus(t, app, created.ID, "completed")
	if len(status.Data) != 1 {
		t.Fatalf("len(data) = %d, want 1", len(status.Data))
	}
	if _, ok := status.Data["https://example.com/valid"]; !ok {
		t.Fatalf("data keys = %v, want https://example.com/valid present", status.Data)
	}
}

func TestV2ExtractRejectsInvalidURLsWhenIgnoreDisabled(t *testing.T) {
	t.Parallel()

	handler := &Handler{extractionJobStore: jobs.NewInMemoryJobStore(time.Minute)}
	app := fiber.New()
	handler.registerV2ExtractAndSearch(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/extract", map[string]interface{}{
		"urls": []string{"notaurl"},
	})
	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusBadRequest)
	}

	var payload ErrorEnvelope
	decodeJSONResponse(t, resp, &payload)
	if payload.Error != "invalid url: url must be a valid absolute URL" {
		t.Fatalf("error = %q, want invalid url error", payload.Error)
	}
}

func TestV2ExtractDeliversWebhookOnCompletion(t *testing.T) {
	t.Parallel()

	payloads := make(chan models.WebhookPayload, 1)
	webhookServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		defer r.Body.Close()
		var payload models.WebhookPayload
		if err := json.NewDecoder(r.Body).Decode(&payload); err != nil {
			t.Fatalf("Decode() error = %v", err)
		}
		payloads <- payload
		w.WriteHeader(http.StatusOK)
	}))
	defer webhookServer.Close()

	handler := &Handler{
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
		batchManager:       batch.NewManager(nil, 1, time.Minute, ""),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			return map[string]interface{}{
				"html":     "<html><body><p>ok</p></body></html>",
				"markdown": "ok",
			}, nil, nil
		},
	}
	t.Cleanup(func() {
		if err := handler.batchManager.Close(); err != nil {
			t.Fatalf("batchManager.Close() error = %v", err)
		}
	})

	app := fiber.New()
	handler.registerV2ExtractAndSearch(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/extract", map[string]interface{}{
		"urls": []string{"https://example.com/item"},
		"webhook": map[string]interface{}{
			"url": webhookServer.URL,
		},
	})

	var created struct {
		ID string `json:"id"`
	}
	decodeJSONResponse(t, resp, &created)
	_ = waitForV2ExtractStatus(t, app, created.ID, "completed")

	select {
	case payload := <-payloads:
		if !payload.Success {
			t.Fatal("payload.success = false, want true")
		}
		if payload.Type != "extract.completed" {
			t.Fatalf("payload.type = %q, want extract.completed", payload.Type)
		}
		if payload.ID != created.ID {
			t.Fatalf("payload.id = %q, want %q", payload.ID, created.ID)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for webhook payload")
	}
}

func TestV2ExtractWildcardUsesCrawlerDiscoveryAndIgnoreSitemap(t *testing.T) {
	t.Parallel()

	var server *httptest.Server
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/robots.txt":
			_, _ = w.Write([]byte("User-agent: *\nAllow: /\n"))
		case "/sitemap.xml":
			_, _ = w.Write([]byte(`<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>` + server.URL + `/docs/from-sitemap</loc></url>
</urlset>`))
		default:
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	defer server.Close()

	handler := &Handler{
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			switch targetURL {
			case server.URL + "/docs":
				return map[string]interface{}{
					"links": []string{"/docs/from-page", "/blog/outside"},
				}, nil, nil
			case server.URL + "/docs/from-page":
				return map[string]interface{}{
					"links":    []string{},
					"html":     "<html><body>page</body></html>",
					"markdown": "# from page",
				}, nil, nil
			case server.URL + "/docs/from-sitemap":
				return map[string]interface{}{
					"links":    []string{},
					"html":     "<html><body>sitemap</body></html>",
					"markdown": "# from sitemap",
				}, nil, nil
			default:
				return map[string]interface{}{
					"links":    []string{},
					"html":     "<html><body>other</body></html>",
					"markdown": "# other",
				}, nil, nil
			}
		},
	}

	app := fiber.New()
	handler.registerV2ExtractAndSearch(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/extract", map[string]interface{}{
		"urls":          []string{server.URL + "/docs/*"},
		"ignoreSitemap": true,
		"limit":         10,
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var created struct {
		ID       string   `json:"id"`
		URLTrace []string `json:"urlTrace"`
	}
	decodeJSONResponse(t, resp, &created)

	if len(created.URLTrace) != 1 || created.URLTrace[0] != server.URL+"/docs/from-page" {
		t.Fatalf("urlTrace = %v, want only crawler-discovered in-scope page", created.URLTrace)
	}

	status := waitForV2ExtractStatus(t, app, created.ID, "completed")
	if len(status.Data) != 1 {
		t.Fatalf("len(data) = %d, want 1", len(status.Data))
	}
	if _, ok := status.Data[server.URL+"/docs/from-page"]; !ok {
		t.Fatalf("data = %v, want %s/docs/from-page present", status.Data, server.URL)
	}
	if _, ok := status.Data[server.URL+"/docs/from-sitemap"]; ok {
		t.Fatalf("data unexpectedly includes sitemap result: %v", status.Data)
	}
}

func TestV2ExtractWildcardSupportsDiscoveryScopeControls(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			switch targetURL {
			case "https://example.com/docs":
				return map[string]interface{}{
					"links": []string{
						"https://sub.example.com/docs/sub",
						"https://outside.test/page",
					},
				}, nil, nil
			case "https://sub.example.com/docs/sub":
				return map[string]interface{}{
					"links":    []string{},
					"html":     "<html><body>sub</body></html>",
					"markdown": "# sub",
				}, nil, nil
			case "https://outside.test/page":
				return map[string]interface{}{
					"links":    []string{},
					"html":     "<html><body>outside</body></html>",
					"markdown": "# outside",
				}, nil, nil
			default:
				return map[string]interface{}{
					"links":    []string{},
					"html":     "<html><body>root</body></html>",
					"markdown": "# root",
				}, nil, nil
			}
		},
	}

	app := fiber.New()
	handler.registerV2ExtractAndSearch(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/extract", map[string]interface{}{
		"urls":               []string{"https://example.com/docs/*"},
		"ignoreSitemap":      true,
		"ignoreRobotsTxt":    true,
		"includeSubdomains":  true,
		"allowExternalLinks": true,
		"limit":              10,
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var created struct {
		ID       string   `json:"id"`
		URLTrace []string `json:"urlTrace"`
	}
	decodeJSONResponse(t, resp, &created)

	sort.Strings(created.URLTrace)
	want := []string{"https://outside.test/page", "https://sub.example.com/docs/sub"}
	if len(created.URLTrace) != len(want) {
		t.Fatalf("urlTrace = %v, want %v", created.URLTrace, want)
	}
	for i := range want {
		if created.URLTrace[i] != want[i] {
			t.Fatalf("urlTrace = %v, want %v", created.URLTrace, want)
		}
	}

	status := waitForV2ExtractStatus(t, app, created.ID, "completed")
	if len(status.Data) != 2 {
		t.Fatalf("len(data) = %d, want 2", len(status.Data))
	}
}

func TestV2ExtractWildcardSupportsIncludeAndExcludePaths(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			switch targetURL {
			case "https://example.com/docs":
				return map[string]interface{}{
					"links": []string{
						"https://example.com/docs/public/one",
						"https://example.com/docs/private/two",
						"https://example.com/docs/public/three",
					},
				}, nil, nil
			default:
				return map[string]interface{}{
					"links":    []string{},
					"html":     "<html><body>ok</body></html>",
					"markdown": "# ok",
				}, nil, nil
			}
		},
	}

	app := fiber.New()
	handler.registerV2ExtractAndSearch(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/extract", map[string]interface{}{
		"urls":          []string{"https://example.com/docs/*"},
		"ignoreSitemap": true,
		"includePaths":  []string{"/docs/public/*"},
		"excludePaths":  []string{"/docs/public/three"},
		"limit":         10,
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var created struct {
		ID       string   `json:"id"`
		URLTrace []string `json:"urlTrace"`
	}
	decodeJSONResponse(t, resp, &created)

	wantTrace := []string{"https://example.com/docs/public/one"}
	if len(created.URLTrace) != len(wantTrace) {
		t.Fatalf("urlTrace = %v, want %v", created.URLTrace, wantTrace)
	}
	for i := range wantTrace {
		if created.URLTrace[i] != wantTrace[i] {
			t.Fatalf("urlTrace = %v, want %v", created.URLTrace, wantTrace)
		}
	}

	status := waitForV2ExtractStatus(t, app, created.ID, "completed")
	if len(status.Data) != 1 {
		t.Fatalf("len(data) = %d, want 1", len(status.Data))
	}
	if _, ok := status.Data["https://example.com/docs/public/one"]; !ok {
		t.Fatalf("data = %v, want only included page", status.Data)
	}
}

func TestV2ExtractWildcardSupportsSitemapOnlyMode(t *testing.T) {
	t.Parallel()

	var server *httptest.Server
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/robots.txt":
			_, _ = w.Write([]byte("User-agent: *\nAllow: /\n"))
		case "/sitemap.xml":
			_, _ = w.Write([]byte(`<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>` + server.URL + `/docs/from-sitemap</loc></url>
</urlset>`))
		default:
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	defer server.Close()

	handler := &Handler{
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			switch targetURL {
			case server.URL + "/docs":
				return map[string]interface{}{
					"links": []string{"/docs/from-page"},
				}, nil, nil
			case server.URL + "/docs/from-sitemap":
				return map[string]interface{}{
					"links":    []string{},
					"html":     "<html><body>sitemap</body></html>",
					"markdown": "# sitemap",
				}, nil, nil
			case server.URL + "/docs/from-page":
				return map[string]interface{}{
					"links":    []string{},
					"html":     "<html><body>page</body></html>",
					"markdown": "# page",
				}, nil, nil
			default:
				return map[string]interface{}{
					"links":    []string{},
					"html":     "<html><body>other</body></html>",
					"markdown": "# other",
				}, nil, nil
			}
		},
	}

	app := fiber.New()
	handler.registerV2ExtractAndSearch(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/extract", map[string]interface{}{
		"urls":    []string{server.URL + "/docs/*"},
		"sitemap": "only",
		"limit":   10,
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var created struct {
		ID       string   `json:"id"`
		URLTrace []string `json:"urlTrace"`
	}
	decodeJSONResponse(t, resp, &created)

	wantTrace := []string{server.URL + "/docs/from-sitemap"}
	if len(created.URLTrace) != len(wantTrace) {
		t.Fatalf("urlTrace = %v, want %v", created.URLTrace, wantTrace)
	}
	for i := range wantTrace {
		if created.URLTrace[i] != wantTrace[i] {
			t.Fatalf("urlTrace = %v, want %v", created.URLTrace, wantTrace)
		}
	}

	status := waitForV2ExtractStatus(t, app, created.ID, "completed")
	if len(status.Data) != 1 {
		t.Fatalf("len(data) = %d, want 1", len(status.Data))
	}
	if _, ok := status.Data[server.URL+"/docs/from-sitemap"]; !ok {
		t.Fatalf("data = %v, want only sitemap page", status.Data)
	}
	if _, ok := status.Data[server.URL+"/docs/from-page"]; ok {
		t.Fatalf("data unexpectedly includes page-discovered url: %v", status.Data)
	}
}

func TestV2ExtractPassesSystemPromptToAIExtractor(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			return map[string]interface{}{
				"html":     "<html><body><h1>Example</h1></body></html>",
				"markdown": "# Example",
			}, nil, nil
		},
		extractAIDataFn: func(ctx context.Context, req *ai.ExtractRequest) (*ai.ExtractResponse, error) {
			if req.SystemPrompt != "Return strict JSON only." {
				t.Fatalf("SystemPrompt = %q, want strict system prompt", req.SystemPrompt)
			}
			if req.Prompt != "extract the page name" {
				t.Fatalf("Prompt = %q, want original user prompt", req.Prompt)
			}
			return &ai.ExtractResponse{Data: `{"name":"Example"}`}, nil
		},
	}

	app := fiber.New()
	handler.registerV2ExtractAndSearch(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/extract", map[string]interface{}{
		"urls":         []string{"https://example.com/item"},
		"systemPrompt": "Return strict JSON only.",
		"prompt":       "extract the page name",
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var created struct {
		ID string `json:"id"`
	}
	decodeJSONResponse(t, resp, &created)

	status := waitForV2ExtractStatus(t, app, created.ID, "completed")
	raw, ok := status.Data["https://example.com/item"].(map[string]interface{})
	if !ok {
		t.Fatalf("data[url] type = %T, want object", status.Data["https://example.com/item"])
	}
	if raw["name"] != "Example" {
		t.Fatalf("data[url].name = %v, want Example", raw["name"])
	}
}

func TestV2SearchSupportsObjectSourcesAndScrapeOptions(t *testing.T) {
	t.Parallel()

	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			if r.URL.Path != "/res/v1/web/search" {
				t.Fatalf("path = %q, want /res/v1/web/search", r.URL.Path)
			}
			if r.URL.Query().Get("q") != "site:example.com quarry" {
				t.Fatalf("q = %q, want site-scoped query", r.URL.Query().Get("q"))
			}
			if r.URL.Query().Get("country") != "DE" {
				t.Fatalf("country = %q, want DE", r.URL.Query().Get("country"))
			}
			if r.URL.Query().Get("search_lang") != "de" {
				t.Fatalf("search_lang = %q, want de", r.URL.Query().Get("search_lang"))
			}
			if r.URL.Query().Get("freshness") != "pw" {
				t.Fatalf("freshness = %q, want pw", r.URL.Query().Get("freshness"))
			}
			return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[{"title":"Guide","url":"https://example.com/guide","description":"article"}]}}`), nil
		}),
	}, time.Second)

	handler := &Handler{
		searchClient: searchClient,
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			if targetURL != "https://example.com/guide" {
				t.Fatalf("targetURL = %q, want https://example.com/guide", targetURL)
			}
			if len(opts.Formats) != 1 || opts.Formats[0] != "markdown" {
				t.Fatalf("Formats = %v, want [markdown]", opts.Formats)
			}
			if opts.WaitFor != 200 {
				t.Fatalf("WaitFor = %d, want 200", opts.WaitFor)
			}
			return map[string]interface{}{"markdown": "# Guide"}, nil, nil
		},
	}

	app := fiber.New()
	handler.registerV2ExtractAndSearch(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/search", map[string]interface{}{
		"query": "quarry",
		"sources": []map[string]interface{}{
			{
				"type":    "web",
				"site":    "example.com",
				"country": "DE",
				"lang":    "de",
				"tbs":     "pw",
			},
		},
		"scrapeOptions": map[string]interface{}{
			"formats": []interface{}{"markdown"},
			"waitFor": 200,
		},
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var payload V2SearchResponse
	decodeJSONResponse(t, resp, &payload)

	if !payload.Success {
		t.Fatal("success = false, want true")
	}
	if payload.Count != 1 {
		t.Fatalf("count = %d, want 1", payload.Count)
	}
	if len(payload.Data.Web) != 1 {
		t.Fatalf("len(data.web) = %d, want 1", len(payload.Data.Web))
	}
	if payload.Data.Web[0].Content != "# Guide" {
		t.Fatalf("content = %q, want # Guide", payload.Data.Web[0].Content)
	}
}

type testV2ExtractStatusResponse struct {
	Success bool                   `json:"success"`
	ID      string                 `json:"id"`
	JobID   string                 `json:"job_id"`
	Status  string                 `json:"status"`
	Data    map[string]interface{} `json:"data"`
	Error   string                 `json:"error"`
}

func waitForV2ExtractStatus(t *testing.T, app *fiber.App, jobID, wantStatus string) testV2ExtractStatusResponse {
	t.Helper()

	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		resp := performJSONRequest(t, app, http.MethodGet, "/v2/extract/"+jobID, nil)
		if resp.StatusCode != http.StatusOK {
			var payload ErrorEnvelope
			decodeJSONResponse(t, resp, &payload)
			t.Fatalf("unexpected status response: %+v", payload)
		}

		var status testV2ExtractStatusResponse
		decodeJSONResponse(t, resp, &status)
		if status.Status == wantStatus {
			return status
		}
		time.Sleep(10 * time.Millisecond)
	}

	t.Fatalf("timed out waiting for v2 extract status %q", wantStatus)
	return testV2ExtractStatusResponse{}
}
