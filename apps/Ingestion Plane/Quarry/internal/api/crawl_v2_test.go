package api

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/quarry/internal/actions"
	quarrycrawl "github.com/triodelab/quarry/internal/crawl"
	"github.com/triodelab/quarry/internal/jobs"
	"github.com/triodelab/quarry/internal/scraper"
)

func TestV2CrawlAsyncFlowAndPagination(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		jobStore:   jobs.NewStore(time.Minute),
		crawlStore: quarrycrawl.NewMemoryStore(time.Minute),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			pages := map[string]map[string]interface{}{
				"https://example.com/docs/start": {
					"links":    []string{"/docs/a", "/docs/b"},
					"markdown": "# start",
				},
				"https://example.com/docs/a": {
					"links":    []string{"/docs/c"},
					"markdown": "# a",
				},
				"https://example.com/docs/b": {
					"links":    []string{},
					"markdown": "# b",
				},
				"https://example.com/docs/c": {
					"links":    []string{},
					"markdown": "# c",
				},
			}
			payload, ok := pages[targetURL]
			if !ok {
				return nil, nil, fmt.Errorf("unexpected url %s", targetURL)
			}
			return payload, nil, nil
		},
	}

	app := fiber.New()
	handler.registerV2Crawl(app.Group("/v2"))

	createResp := performJSONRequest(t, app, http.MethodPost, "/v2/crawl", map[string]interface{}{
		"url":               "https://example.com/docs/start",
		"crawlEntireDomain": true,
		"limit":             4,
		"scrapeOptions": map[string]interface{}{
			"formats": []interface{}{"markdown"},
		},
	})

	var created struct {
		Success bool   `json:"success"`
		ID      string `json:"id"`
		JobID   string `json:"job_id"`
		URL     string `json:"url"`
	}
	decodeJSONResponse(t, createResp, &created)

	if !created.Success {
		t.Fatal("success = false, want true")
	}
	if created.ID == "" || created.JobID == "" {
		t.Fatalf("id/job_id = %q/%q, want both populated", created.ID, created.JobID)
	}

	status := waitForV2CrawlStatus(t, app, created.ID, func(status testV2CrawlStatusResponse) bool {
		return status.Status == "completed"
	})
	if status.Completed != 4 {
		t.Fatalf("completed = %d, want 4", status.Completed)
	}
	if len(status.Data) != 4 {
		t.Fatalf("len(data) = %d, want 4", len(status.Data))
	}

	pagedResp := performJSONRequest(t, app, http.MethodGet, "/v2/crawl/"+created.ID+"?skip=1&limit=1", nil)
	var paged testV2CrawlStatusResponse
	decodeJSONResponse(t, pagedResp, &paged)

	if len(paged.Data) != 1 {
		t.Fatalf("len(paged.data) = %d, want 1", len(paged.Data))
	}
	if paged.Next == "" {
		t.Fatal("next = empty, want pagination url")
	}
}

func TestV2CrawlCancelMarksRunCancelled(t *testing.T) {
	t.Parallel()

	blocked := make(chan struct{})
	handler := &Handler{
		jobStore:   jobs.NewStore(time.Minute),
		crawlStore: quarrycrawl.NewMemoryStore(time.Minute),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			close(blocked)
			<-ctx.Done()
			return nil, nil, ctx.Err()
		},
	}

	app := fiber.New()
	handler.registerV2Crawl(app.Group("/v2"))

	createResp := performJSONRequest(t, app, http.MethodPost, "/v2/crawl", map[string]interface{}{
		"url": "https://example.com/docs/start",
	})

	var created struct {
		ID string `json:"id"`
	}
	decodeJSONResponse(t, createResp, &created)

	<-blocked

	cancelResp := performJSONRequest(t, app, http.MethodDelete, "/v2/crawl/"+created.ID, nil)
	var cancelled struct {
		Success bool   `json:"success"`
		Status  string `json:"status"`
	}
	decodeJSONResponse(t, cancelResp, &cancelled)

	if !cancelled.Success || cancelled.Status != "cancelled" {
		t.Fatalf("cancel response = %+v, want success=true status=cancelled", cancelled)
	}

	status := waitForV2CrawlStatus(t, app, created.ID, func(status testV2CrawlStatusResponse) bool {
		return status.Status == "cancelled"
	})
	if status.Status != "cancelled" {
		t.Fatalf("status = %q, want cancelled", status.Status)
	}
}

func TestV2CrawlErrorsAndRobotsBlocked(t *testing.T) {
	t.Parallel()

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/robots.txt":
			_, _ = w.Write([]byte("User-agent: *\nDisallow: /private\n"))
		default:
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	defer server.Close()

	handler := &Handler{
		jobStore:   jobs.NewStore(time.Minute),
		crawlStore: quarrycrawl.NewMemoryStore(time.Minute),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			switch targetURL {
			case server.URL + "/root":
				return map[string]interface{}{
					"links":    []string{"/public", "/private", "/broken"},
					"markdown": "# root",
				}, nil, nil
			case server.URL + "/public":
				return map[string]interface{}{
					"links":    []string{},
					"markdown": "# public",
				}, nil, nil
			case server.URL + "/broken":
				return nil, nil, fmt.Errorf("boom")
			default:
				return nil, nil, fmt.Errorf("unexpected url %s", targetURL)
			}
		},
	}

	app := fiber.New()
	handler.registerV2Crawl(app.Group("/v2"))

	createResp := performJSONRequest(t, app, http.MethodPost, "/v2/crawl", map[string]interface{}{
		"url":               server.URL + "/root",
		"crawlEntireDomain": true,
		"limit":             10,
	})

	var created struct {
		ID string `json:"id"`
	}
	decodeJSONResponse(t, createResp, &created)

	_ = waitForV2CrawlStatus(t, app, created.ID, func(status testV2CrawlStatusResponse) bool {
		return status.Status == "completed"
	})

	errorsResp := performJSONRequest(t, app, http.MethodGet, "/v2/crawl/"+created.ID+"/errors", nil)
	var payload struct {
		Success       bool                    `json:"success"`
		Errors        []quarrycrawl.PageError `json:"errors"`
		RobotsBlocked []string                `json:"robotsBlocked"`
	}
	decodeJSONResponse(t, errorsResp, &payload)

	if len(payload.Errors) != 1 {
		t.Fatalf("len(errors) = %d, want 1", len(payload.Errors))
	}
	if payload.Errors[0].URL != server.URL+"/broken" {
		t.Fatalf("error url = %q, want %q", payload.Errors[0].URL, server.URL+"/broken")
	}
	if len(payload.RobotsBlocked) != 1 || payload.RobotsBlocked[0] != server.URL+"/private" {
		t.Fatalf("robotsBlocked = %v, want [%s/private]", payload.RobotsBlocked, server.URL)
	}
}

func TestV2MapReturnsDiscoveredLinks(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			switch targetURL {
			case "https://example.com/start":
				return map[string]interface{}{"links": []string{"/a", "/b"}}, nil, nil
			case "https://example.com/a":
				return map[string]interface{}{"links": []string{"/c"}}, nil, nil
			case "https://example.com/b":
				return map[string]interface{}{"links": []string{}}, nil, nil
			case "https://example.com/c":
				return map[string]interface{}{"links": []string{}}, nil, nil
			default:
				return nil, nil, fmt.Errorf("unexpected url %s", targetURL)
			}
		},
	}

	app := fiber.New()
	handler.registerV2Crawl(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/map", map[string]interface{}{
		"url":               "https://example.com/start",
		"crawlEntireDomain": true,
		"limit":             4,
	})

	var payload struct {
		Success bool     `json:"success"`
		Count   int      `json:"count"`
		Links   []string `json:"links"`
	}
	decodeJSONResponse(t, resp, &payload)

	if !payload.Success {
		t.Fatal("success = false, want true")
	}
	if payload.Count != 4 {
		t.Fatalf("count = %d, want 4", payload.Count)
	}
}

func TestV2CrawlParamsPreviewUsesInjectedPlanner(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		crawlPreviewFn: func(ctx context.Context, spec quarrycrawl.Spec, sample []string) (map[string]interface{}, error) {
			return map[string]interface{}{
				"url":               spec.URL,
				"includePaths":      []string{"/pricing/**"},
				"crawlEntireDomain": true,
				"limit":             250,
			}, nil
		},
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			return map[string]interface{}{"links": []string{"/pricing", "/about"}}, nil, nil
		},
	}

	app := fiber.New()
	handler.registerV2Crawl(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/crawl/params-preview", map[string]interface{}{
		"url":    "https://example.com",
		"prompt": "crawl pricing pages",
	})

	var payload struct {
		Success bool                   `json:"success"`
		Data    map[string]interface{} `json:"data"`
	}
	decodeJSONResponse(t, resp, &payload)

	if !payload.Success {
		t.Fatal("success = false, want true")
	}
	if payload.Data["limit"] == nil {
		t.Fatal("limit missing from preview response")
	}
}

func TestV2CrawlSchemaAliasProducesStructuredJSON(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		jobStore:   jobs.NewStore(time.Minute),
		crawlStore: quarrycrawl.NewMemoryStore(time.Minute),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			return map[string]interface{}{
				"links": []string{},
				"html":  "<html><body>hi</body></html>",
			}, nil, nil
		},
		extractStructuredFn: func(ctx context.Context, targetURL string, opts *scraper.StructuredExtractOptions) (map[string]interface{}, error) {
			if opts.Schema == "" {
				t.Fatal("schema alias was not forwarded to structured extraction")
			}
			return map[string]interface{}{"name": "example"}, nil
		},
	}

	app := fiber.New()
	handler.registerV2Crawl(app.Group("/v2"))

	createResp := performJSONRequest(t, app, http.MethodPost, "/v2/crawl", map[string]interface{}{
		"url":    "https://example.com",
		"schema": `{"type":"object","properties":{"name":{"type":"string"}}}`,
		"prompt": "extract the page name",
	})

	var created struct {
		ID string `json:"id"`
	}
	decodeJSONResponse(t, createResp, &created)

	status := waitForV2CrawlStatus(t, app, created.ID, func(status testV2CrawlStatusResponse) bool {
		return status.Status == "completed"
	})
	if len(status.Data) != 1 {
		t.Fatalf("len(data) = %d, want 1", len(status.Data))
	}
	jsonOutput, ok := status.Data[0].Outputs["json"].(map[string]interface{})
	if !ok {
		t.Fatalf("json output type = %T, want map[string]interface{}", status.Data[0].Outputs["json"])
	}
	if jsonOutput["name"] != "example" {
		t.Fatalf("json output = %v, want name=example", jsonOutput)
	}
}

func TestV2CrawlRejectsInvalidRegex(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		jobStore:   jobs.NewStore(time.Minute),
		crawlStore: quarrycrawl.NewMemoryStore(time.Minute),
	}

	app := fiber.New()
	handler.registerV2Crawl(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/crawl", map[string]interface{}{
		"url":            "https://example.com",
		"regexOnFullURL": true,
		"includePaths":   []string{"["},
	})

	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusBadRequest)
	}
}

func TestPreviewHelpersAndFormatParsing(t *testing.T) {
	t.Parallel()

	formats, err := parseV2Formats([]json.RawMessage{
		json.RawMessage(`"markdown"`),
		json.RawMessage(`{"type":"json","schema":{"type":"object"},"prompt":"extract"}`),
	})
	if err != nil {
		t.Fatalf("parseV2Formats() error = %v", err)
	}
	if len(formats) != 2 {
		t.Fatalf("len(formats) = %d, want 2", len(formats))
	}

	rawFormats, structured := selectCrawlFormats(formats)
	if len(rawFormats) != 1 || rawFormats[0] != "markdown" {
		t.Fatalf("rawFormats = %v, want [markdown]", rawFormats)
	}
	if structured == nil || structured.Schema == "" {
		t.Fatalf("structured format = %+v, want populated json schema format", structured)
	}

	links := extractLinks([]interface{}{"https://example.com/a", 42, "https://example.com/b"})
	if len(links) != 2 {
		t.Fatalf("len(links) = %d, want 2", len(links))
	}

	preview := heuristicPreview(quarrycrawl.Spec{
		URL:    "https://example.com",
		Prompt: "crawl pricing pages",
	}, []string{"https://example.com/pricing", "https://example.com/about"})
	if preview["includePaths"] == nil {
		t.Fatal("includePaths missing from heuristic preview")
	}

	plan := `{"targets":[{"url":"https://example.com/docs/intro"},{"url":"https://example.com/docs/api"}],"estimatedPages":250}`
	derived := previewFromPlan(quarrycrawl.Spec{URL: "https://example.com"}, plan)
	if derived["limit"] != 250 {
		t.Fatalf("derived limit = %v, want 250", derived["limit"])
	}
	if derived["includePaths"] == nil {
		t.Fatal("includePaths missing from preview plan")
	}

	prompt := buildPreviewPrompt("crawl docs", []string{"https://example.com/docs", "https://example.com/api"})
	if prompt == "crawl docs" {
		t.Fatal("buildPreviewPrompt() did not include sample urls")
	}
}

type testV2CrawlStatusResponse struct {
	Success   bool                   `json:"success"`
	Status    string                 `json:"status"`
	Completed int                    `json:"completed"`
	Total     int                    `json:"total"`
	Next      string                 `json:"next"`
	Data      []quarrycrawl.Document `json:"data"`
	Error     string                 `json:"error"`
}

func waitForV2CrawlStatus(t *testing.T, app *fiber.App, jobID string, done func(testV2CrawlStatusResponse) bool) testV2CrawlStatusResponse {
	t.Helper()

	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		resp := performJSONRequest(t, app, http.MethodGet, "/v2/crawl/"+jobID, nil)
		var status testV2CrawlStatusResponse
		decodeJSONResponse(t, resp, &status)
		if done(status) {
			return status
		}
		time.Sleep(25 * time.Millisecond)
	}

	t.Fatalf("crawl %s did not reach desired status before timeout", jobID)
	return testV2CrawlStatusResponse{}
}
