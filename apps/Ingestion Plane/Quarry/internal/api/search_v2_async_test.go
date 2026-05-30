package api

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/quarry/internal/actions"
	"github.com/triodelab/quarry/internal/batch"
	"github.com/triodelab/quarry/internal/dataplane"
	"github.com/triodelab/quarry/internal/jobs"
	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/platform"
	"github.com/triodelab/quarry/internal/scraper"
	quarrysearch "github.com/triodelab/quarry/internal/search"
)

func TestV2SearchAsyncFlowAndStatus(t *testing.T) {
	t.Parallel()

	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			if r.URL.Path != "/res/v1/web/search" {
				t.Fatalf("path = %q, want /res/v1/web/search", r.URL.Path)
			}
			if r.URL.Query().Get("q") != "quarry" {
				t.Fatalf("q = %q, want quarry", r.URL.Query().Get("q"))
			}
			return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[{"title":"Guide","url":"https://example.com/guide","description":"article"}]}}`), nil
		}),
	}, time.Second)

	handler := &Handler{
		jobStore:         jobs.NewStore(time.Minute),
		searchAsyncStore: quarrysearch.NewMemoryAsyncStore(time.Minute),
		searchClient:     searchClient,
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			if targetURL != "https://example.com/guide" {
				t.Fatalf("targetURL = %q, want https://example.com/guide", targetURL)
			}
			if len(opts.Formats) != 1 || opts.Formats[0] != "markdown" {
				t.Fatalf("Formats = %v, want [markdown]", opts.Formats)
			}
			return map[string]interface{}{"markdown": "# Guide"}, nil, nil
		},
	}

	app := fiber.New()
	handler.registerV2ExtractAndSearch(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/search", map[string]interface{}{
		"query":         "quarry",
		"asyncScraping": true,
		"scrapeOptions": map[string]interface{}{
			"formats": []interface{}{"markdown"},
		},
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var created struct {
		Success bool   `json:"success"`
		ID      string `json:"id"`
		JobID   string `json:"job_id"`
		Status  string `json:"status"`
		Query   string `json:"query"`
	}
	decodeJSONResponse(t, resp, &created)

	if !created.Success {
		t.Fatal("success = false, want true")
	}
	if created.ID == "" || created.JobID == "" {
		t.Fatalf("id/job_id = %q/%q, want both populated", created.ID, created.JobID)
	}
	if created.Status != "processing" {
		t.Fatalf("status = %q, want processing", created.Status)
	}
	if created.Query != "quarry" {
		t.Fatalf("query = %q, want quarry", created.Query)
	}

	status := waitForV2SearchStatus(t, app, created.ID, "completed")
	if status.Completed != 1 || status.Total != 1 || status.Count != 1 {
		t.Fatalf("completed/total/count = %d/%d/%d, want 1/1/1", status.Completed, status.Total, status.Count)
	}
	if len(status.Data) != 1 {
		t.Fatalf("len(data) = %d, want 1", len(status.Data))
	}
	if status.Data[0].Content != "# Guide" {
		t.Fatalf("content = %q, want # Guide", status.Data[0].Content)
	}
	if job, ok := handler.jobStore.Get(created.ID); !ok || job == nil {
		t.Fatalf("job %q not found in job store", created.ID)
	} else if _, exists := job.Result["results"]; exists {
		t.Fatalf("job.Result unexpectedly stores full results payload: %+v", job.Result)
	}
}

func TestV2SearchAsyncStatusPagination(t *testing.T) {
	t.Parallel()

	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[
				{"title":"One","url":"https://example.com/one","description":"one"},
				{"title":"Two","url":"https://example.com/two","description":"two"},
				{"title":"Three","url":"https://example.com/three","description":"three"}
			]}}`), nil
		}),
	}, time.Second)

	handler := &Handler{
		jobStore:         jobs.NewStore(time.Minute),
		searchAsyncStore: quarrysearch.NewMemoryAsyncStore(time.Minute),
		searchClient:     searchClient,
	}

	app := fiber.New()
	handler.registerV2ExtractAndSearch(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/search", map[string]interface{}{
		"query":         "quarry",
		"asyncScraping": true,
		"limit":         3,
	})

	var created struct {
		ID string `json:"id"`
	}
	decodeJSONResponse(t, resp, &created)
	_ = waitForV2SearchStatus(t, app, created.ID, "completed")

	pageResp := performJSONRequest(t, app, http.MethodGet, "/v2/search/"+created.ID+"?skip=1&limit=1", nil)
	var page struct {
		Success bool             `json:"success"`
		Count   int              `json:"count"`
		Total   int              `json:"total"`
		Next    string           `json:"next"`
		Data    []V2SearchResult `json:"data"`
	}
	decodeJSONResponse(t, pageResp, &page)

	if !page.Success {
		t.Fatal("success = false, want true")
	}
	if page.Total != 3 || page.Count != 1 {
		t.Fatalf("total/count = %d/%d, want 3/1", page.Total, page.Count)
	}
	if len(page.Data) != 1 || page.Data[0].Title != "Two" {
		t.Fatalf("page data = %+v, want second result only", page.Data)
	}
	if page.Next == "" {
		t.Fatal("next = empty, want pagination url")
	}
}

func TestV2SearchAsyncCancel(t *testing.T) {
	t.Parallel()

	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[{"title":"Guide","url":"https://example.com/guide","description":"article"}]}}`), nil
		}),
	}, time.Second)

	blocked := make(chan struct{})
	handler := &Handler{
		jobStore:         jobs.NewStore(time.Minute),
		searchAsyncStore: quarrysearch.NewMemoryAsyncStore(time.Minute),
		searchClient:     searchClient,
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			close(blocked)
			<-ctx.Done()
			return nil, nil, ctx.Err()
		},
	}

	app := fiber.New()
	handler.registerV2ExtractAndSearch(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/search", map[string]interface{}{
		"query":         "quarry",
		"asyncScraping": true,
		"scrapeOptions": map[string]interface{}{
			"formats": []interface{}{"markdown"},
		},
	})

	var created struct {
		ID string `json:"id"`
	}
	decodeJSONResponse(t, resp, &created)

	<-blocked

	cancelResp := performJSONRequest(t, app, http.MethodDelete, "/v2/search/"+created.ID, nil)
	var cancelled struct {
		Success bool   `json:"success"`
		Status  string `json:"status"`
	}
	decodeJSONResponse(t, cancelResp, &cancelled)

	if !cancelled.Success || cancelled.Status != "cancelled" {
		t.Fatalf("cancel response = %+v, want success=true status=cancelled", cancelled)
	}

	status := waitForV2SearchStatus(t, app, created.ID, "cancelled")
	if status.Status != "cancelled" {
		t.Fatalf("status = %q, want cancelled", status.Status)
	}
}

func TestV2SearchAsyncWebhookOnCompletion(t *testing.T) {
	t.Parallel()

	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[{"title":"Guide","url":"https://example.com/guide","description":"article"}]}}`), nil
		}),
	}, time.Second)

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
		jobStore:         jobs.NewStore(time.Minute),
		searchAsyncStore: quarrysearch.NewMemoryAsyncStore(time.Minute),
		searchClient:     searchClient,
		batchManager:     batch.NewManager(nil, 1, time.Minute, ""),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			return map[string]interface{}{"markdown": "# Guide"}, nil, nil
		},
	}
	t.Cleanup(func() {
		if err := handler.batchManager.Close(); err != nil {
			t.Fatalf("batchManager.Close() error = %v", err)
		}
	})

	app := fiber.New()
	handler.registerV2ExtractAndSearch(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/search", map[string]interface{}{
		"query":         "quarry",
		"asyncScraping": true,
		"scrapeOptions": map[string]interface{}{
			"formats": []interface{}{"markdown"},
		},
		"webhook": map[string]interface{}{
			"url": webhookServer.URL,
		},
	})

	var created struct {
		ID string `json:"id"`
	}
	decodeJSONResponse(t, resp, &created)
	_ = waitForV2SearchStatus(t, app, created.ID, "completed")

	select {
	case payload := <-payloads:
		if !payload.Success {
			t.Fatal("payload.success = false, want true")
		}
		if payload.Type != "search.completed" {
			t.Fatalf("payload.type = %q, want search.completed", payload.Type)
		}
		if payload.ID != created.ID {
			t.Fatalf("payload.id = %q, want %q", payload.ID, created.ID)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for webhook payload")
	}
}

func TestV2SearchAsyncSupportsDocumentsSourceAndSkipsDataplaneScrape(t *testing.T) {
	t.Parallel()

	retrievalServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1/retrieve" {
			http.NotFound(w, r)
			return
		}
		if r.Header.Get("x-internal-key") != "retrieval-test-key" {
			t.Fatalf("x-internal-key = %q, want retrieval-test-key", r.Header.Get("x-internal-key"))
		}
		if r.Header.Get("x-org-id") != "org-1" {
			t.Fatalf("x-org-id = %q, want org-1", r.Header.Get("x-org-id"))
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"facts":[
				{"knowledge_id":"kn-1","document_id":"doc-1","text":"Quarry stores crawl telemetry in internal documents.","score":0.91,"metadata":{"title":"Telemetry"}}
			],
			"sources":[
				{"document_id":"doc-1","title":"Telemetry","source":"docs","type":"guide"}
			],
			"query":"quarry",
			"org_id":"org-1"
		}`))
	}))
	defer retrievalServer.Close()

	dataplaneClient := dataplane.NewClient("http://documents.invalid")
	dataplaneClient.SetRetrievalBaseURL(retrievalServer.URL)
	dataplaneClient.SetInternalAPIKey("retrieval-test-key")

	handler := &Handler{
		jobStore:         jobs.NewStore(time.Minute),
		searchAsyncStore: quarrysearch.NewMemoryAsyncStore(time.Minute),
		dataplaneClient:  dataplaneClient,
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			t.Fatalf("fetchFormatsFn should not be called for dataplane url %q", targetURL)
			return nil, nil, nil
		},
	}

	app := fiber.New()
	app.Use(func(c *fiber.Ctx) error {
		c.Locals(platform.PrincipalContextKey, &platform.Principal{
			UserID:         "user-1",
			OrganizationID: "org-1",
			Tier:           "pro",
		})
		return c.Next()
	})
	handler.registerV2ExtractAndSearch(app.Group("/v2"))

	resp := performJSONRequest(t, app, http.MethodPost, "/v2/search", map[string]interface{}{
		"query":         "quarry",
		"asyncScraping": true,
		"sources": []map[string]interface{}{
			{"type": "documents"},
		},
		"scrapeOptions": map[string]interface{}{
			"formats": []interface{}{"markdown"},
		},
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var created struct {
		ID string `json:"id"`
	}
	decodeJSONResponse(t, resp, &created)

	status := waitForV2SearchStatus(t, app, created.ID, "completed")
	if len(status.Data) != 1 {
		t.Fatalf("data = %+v, want one documents result", status.Data)
	}
	if status.Data[0].Type != "documents" {
		t.Fatalf("type = %q, want documents", status.Data[0].Type)
	}
	if status.Data[0].Content != "" {
		t.Fatalf("content = %q, want empty because dataplane urls are not scraped", status.Data[0].Content)
	}
	if status.Data[0].URL != "dataplane://documents/doc-1" {
		t.Fatalf("url = %q, want dataplane://documents/doc-1", status.Data[0].URL)
	}
}

type testV2SearchStatusResponse struct {
	Success   bool             `json:"success"`
	ID        string           `json:"id"`
	JobID     string           `json:"job_id"`
	Status    string           `json:"status"`
	Query     string           `json:"query"`
	Completed int              `json:"completed"`
	Total     int              `json:"total"`
	Count     int              `json:"count"`
	Next      string           `json:"next"`
	Data      []V2SearchResult `json:"data"`
	Error     string           `json:"error"`
}

func waitForV2SearchStatus(t *testing.T, app *fiber.App, jobID, wantStatus string) testV2SearchStatusResponse {
	t.Helper()

	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		resp := performJSONRequest(t, app, http.MethodGet, "/v2/search/"+jobID, nil)
		if resp.StatusCode != http.StatusOK {
			var payload ErrorEnvelope
			decodeJSONResponse(t, resp, &payload)
			t.Fatalf("unexpected status response: %+v", payload)
		}

		var status testV2SearchStatusResponse
		decodeJSONResponse(t, resp, &status)
		if status.Status == wantStatus {
			return status
		}
		time.Sleep(10 * time.Millisecond)
	}

	t.Fatalf("timed out waiting for v2 search status %q", wantStatus)
	return testV2SearchStatusResponse{}
}
