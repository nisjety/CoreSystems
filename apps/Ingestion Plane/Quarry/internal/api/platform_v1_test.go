package api

import (
	"context"
	"encoding/json"
	"fmt"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	userv1 "github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/proto/user/v1"
	"github.com/gofiber/fiber/v2"
	"google.golang.org/grpc"
	"google.golang.org/protobuf/types/known/structpb"
	"google.golang.org/protobuf/types/known/timestamppb"

	"github.com/triodelab/quarry/internal/actions"
	"github.com/triodelab/quarry/internal/batch"
	quarrycrawl "github.com/triodelab/quarry/internal/crawl"
	"github.com/triodelab/quarry/internal/dataplane"
	"github.com/triodelab/quarry/internal/jobs"
	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/platform"
	"github.com/triodelab/quarry/internal/scraper"
	quarrysearch "github.com/triodelab/quarry/internal/search"
	"github.com/triodelab/quarry/internal/tracker"
)

type stubBatchScraper struct {
	scrape func(context.Context, *models.ScrapeRequest) (*models.ScrapeResult, error)
}

func (s stubBatchScraper) ScrapeCollection(ctx context.Context, req *models.ScrapeRequest) (*models.ScrapeResult, error) {
	return s.scrape(ctx, req)
}

func TestV1CrawlAppliesPromptGeneratedOptions(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		jobStore:   jobs.NewStore(time.Minute),
		crawlStore: quarrycrawl.NewMemoryStore(time.Minute),
		crawlPreviewFn: func(ctx context.Context, spec quarrycrawl.Spec, sample []string) (map[string]interface{}, error) {
			return map[string]interface{}{
				"includePaths":      []interface{}{"/pricing/**"},
				"limit":             12,
				"crawlEntireDomain": true,
			}, nil
		},
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			switch targetURL {
			case "https://example.com", "https://example.com/":
				return map[string]interface{}{
					"links":    []string{"/pricing", "/blog"},
					"markdown": "# home",
				}, nil, nil
			case "https://example.com/pricing":
				return map[string]interface{}{
					"links":    []string{},
					"markdown": "# pricing",
				}, nil, nil
			default:
				return nil, nil, fmt.Errorf("unexpected url %s", targetURL)
			}
		},
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/crawl", map[string]interface{}{
		"url":     "https://example.com",
		"prompt":  "Only crawl pricing pages",
		"formats": []interface{}{"markdown"},
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var created struct {
		Success         bool                   `json:"success"`
		ID              string                 `json:"id"`
		Resource        string                 `json:"resource"`
		StatusURL       string                 `json:"statusUrl"`
		ResolvedOptions map[string]interface{} `json:"resolvedOptions"`
	}
	decodeJSONResponse(t, resp, &created)

	if !created.Success {
		t.Fatal("success = false, want true")
	}
	if created.Resource != "crawl" {
		t.Fatalf("resource = %q, want crawl", created.Resource)
	}
	if created.StatusURL == "" {
		t.Fatal("statusUrl = empty, want populated")
	}
	includePaths := stringSliceFromAny(created.ResolvedOptions["includePaths"])
	if len(includePaths) != 1 || includePaths[0] != "/pricing/**" {
		t.Fatalf("resolved includePaths = %v, want [/pricing/**]", includePaths)
	}

	status := waitForV1CrawlStatus(t, app, created.ID, func(status crawlStatusEnvelope) bool {
		return status.Status == "completed"
	})
	if status.Completed != 2 {
		t.Fatalf("completed = %d, want 2", status.Completed)
	}
	if len(status.Data) != 2 {
		t.Fatalf("len(data) = %d, want 2", len(status.Data))
	}
	if status.Data[1].URL != "https://example.com/pricing" {
		t.Fatalf("second url = %q, want https://example.com/pricing", status.Data[1].URL)
	}
}

func TestV1CrawlSiteObservabilityPresetTracksPageChanges(t *testing.T) {
	t.Parallel()

	changeTracker := tracker.NewChangeTracker()
	t.Cleanup(func() {
		if err := changeTracker.Close(); err != nil {
			t.Fatalf("changeTracker.Close() error = %v", err)
		}
	})

	handler := &Handler{
		jobStore:      jobs.NewStore(time.Minute),
		crawlStore:    quarrycrawl.NewMemoryStore(time.Minute),
		changeTracker: changeTracker,
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			for _, expected := range []string{"markdown", "seo", "wcag", "pagestatus"} {
				if !containsString(opts.Formats, expected) {
					t.Fatalf("formats = %v, want %s included", opts.Formats, expected)
				}
			}
			return map[string]interface{}{
				"markdown": "# Status\n\nEverything is healthy.",
				"html":     "<html><body><h1>Status</h1><p>Everything is healthy.</p></body></html>",
				"links":    []string{},
				"json": map[string]interface{}{
					"status":      200,
					"contentType": "text/html",
				},
				"seo":        map[string]interface{}{"title": "Status"},
				"wcag":       map[string]interface{}{"score": 1.0},
				"pagestatus": map[string]interface{}{"status": "completed"},
			}, nil, nil
		},
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/crawl", map[string]interface{}{
		"url":    "https://example.com/status",
		"preset": "site-observability",
	})
	if createResp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", createResp.StatusCode, http.StatusOK)
	}

	var created struct {
		ID              string                 `json:"id"`
		Resource        string                 `json:"resource"`
		ResolvedOptions map[string]interface{} `json:"resolvedOptions"`
	}
	decodeJSONResponse(t, createResp, &created)
	if created.Resource != "crawl" {
		t.Fatalf("resource = %q, want crawl", created.Resource)
	}
	if stringFromAny(created.ResolvedOptions["preset"], "") != "site-observability" {
		t.Fatalf("resolved preset = %v, want site-observability", created.ResolvedOptions["preset"])
	}

	first := waitForV1CrawlStatus(t, app, created.ID, func(status crawlStatusEnvelope) bool {
		return status.Status == "completed"
	})
	if len(first.Data) != 1 {
		t.Fatalf("len(data) = %d, want 1", len(first.Data))
	}
	firstMetadata := mapFromAny(first.Data[0].Metadata)
	firstChange := mapFromAny(firstMetadata["changeTracking"])
	if stringFromAny(firstChange["changeStatus"], "") != "new" {
		t.Fatalf("first changeTracking = %v, want changeStatus=new", firstChange)
	}
	if mapFromAny(firstMetadata["pageStatus"])["status"] == nil {
		t.Fatalf("pageStatus metadata = %v, want populated", firstMetadata["pageStatus"])
	}

	secondResp := performJSONRequest(t, app, http.MethodPost, "/v1/crawl", map[string]interface{}{
		"url":    "https://example.com/status",
		"preset": "site-observability",
	})
	var secondCreated struct {
		ID string `json:"id"`
	}
	decodeJSONResponse(t, secondResp, &secondCreated)

	second := waitForV1CrawlStatus(t, app, secondCreated.ID, func(status crawlStatusEnvelope) bool {
		return status.Status == "completed"
	})
	secondMetadata := mapFromAny(second.Data[0].Metadata)
	secondChange := mapFromAny(secondMetadata["changeTracking"])
	if stringFromAny(secondChange["changeStatus"], "") != "same" {
		t.Fatalf("second changeTracking = %v, want changeStatus=same", secondChange)
	}
}

func TestV1CrawlScheduleAtDelaysExecution(t *testing.T) {
	t.Parallel()

	fetchStarted := make(chan time.Time, 1)
	handler := &Handler{
		jobStore:   jobs.NewStore(time.Minute),
		crawlStore: quarrycrawl.NewMemoryStore(time.Minute),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			select {
			case fetchStarted <- time.Now():
			default:
			}
			return map[string]interface{}{
				"markdown": "# Scheduled",
				"links":    []string{},
				"json": map[string]interface{}{
					"status":      200,
					"contentType": "text/html",
				},
			}, nil, nil
		},
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	scheduledAt := time.Now().Add(250 * time.Millisecond).UTC()
	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/crawl", map[string]interface{}{
		"url":        "https://example.com/scheduled",
		"preset":     "site-observability",
		"scheduleAt": scheduledAt.Format(time.RFC3339Nano),
	})
	if createResp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", createResp.StatusCode, http.StatusOK)
	}

	var created struct {
		ID              string                 `json:"id"`
		ResolvedOptions map[string]interface{} `json:"resolvedOptions"`
	}
	decodeJSONResponse(t, createResp, &created)
	if stringFromAny(created.ResolvedOptions["scheduleAt"], "") == "" {
		t.Fatalf("resolved scheduleAt = %v, want populated", created.ResolvedOptions["scheduleAt"])
	}

	startedAt := <-fetchStarted
	if startedAt.Before(scheduledAt.Add(-50 * time.Millisecond)) {
		t.Fatalf("fetch started at %s before scheduled time %s", startedAt.Format(time.RFC3339Nano), scheduledAt.Format(time.RFC3339Nano))
	}

	status := waitForV1CrawlStatus(t, app, created.ID, func(status crawlStatusEnvelope) bool {
		return status.Status == "completed"
	})
	if status.Status != "completed" {
		t.Fatalf("status = %q, want completed", status.Status)
	}
}

func TestV1CrawlRejectsUnknownPreset(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		jobStore:   jobs.NewStore(time.Minute),
		crawlStore: quarrycrawl.NewMemoryStore(time.Minute),
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/crawl", map[string]interface{}{
		"url":    "https://example.com",
		"preset": "not-a-real-preset",
	})
	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusBadRequest)
	}
}

func TestV1CrawlSiteObservabilityWebhookIncludesSummary(t *testing.T) {
	t.Parallel()

	payloads := make(chan models.WebhookPayload, 4)
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

	changeTracker := tracker.NewChangeTracker()
	t.Cleanup(func() {
		if err := changeTracker.Close(); err != nil {
			t.Fatalf("changeTracker.Close() error = %v", err)
		}
	})

	handler := &Handler{
		jobStore:      jobs.NewStore(time.Minute),
		crawlStore:    quarrycrawl.NewMemoryStore(time.Minute),
		changeTracker: changeTracker,
		batchManager:  batch.NewManager(nil, 1, time.Minute, ""),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			return map[string]interface{}{
				"markdown": "# Status\n\nEverything is healthy.",
				"html":     "<html><body><h1>Status</h1><p>Everything is healthy.</p></body></html>",
				"links":    []string{},
				"json": map[string]interface{}{
					"status":      503,
					"contentType": "text/html",
				},
				"seo":        map[string]interface{}{"title": "Status"},
				"wcag":       map[string]interface{}{"score": 1.0},
				"pagestatus": map[string]interface{}{"status": "completed"},
			}, nil, nil
		},
	}
	t.Cleanup(func() {
		if err := handler.batchManager.Close(); err != nil {
			t.Fatalf("batchManager.Close() error = %v", err)
		}
	})

	app := fiber.New()
	handler.registerPlatformV1(app)

	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/crawl", map[string]interface{}{
		"url":    "https://example.com/status",
		"preset": "site-observability",
		"webhook": map[string]interface{}{
			"url": webhookServer.URL,
		},
	})
	var created struct {
		ID string `json:"id"`
	}
	decodeJSONResponse(t, createResp, &created)
	_ = waitForV1CrawlStatus(t, app, created.ID, func(status crawlStatusEnvelope) bool {
		return status.Status == "completed"
	})

	deadline := time.After(2 * time.Second)
	for {
		select {
		case payload := <-payloads:
			if payload.Type != "crawl.completed" {
				continue
			}
			if !payload.Success {
				t.Fatal("payload.success = false, want true")
			}
			if payload.ID != created.ID {
				t.Fatalf("payload.id = %q, want %q", payload.ID, created.ID)
			}
			changes := mapFromAny(payload.Metadata["changes"])
			if intFromAny(changes["new"]) != 1 {
				t.Fatalf("changes = %v, want new=1", changes)
			}
			alerts, _ := payload.Metadata["alerts"].([]interface{})
			if len(alerts) == 0 {
				t.Fatalf("alerts = %v, want at least one alert", payload.Metadata["alerts"])
			}
			return
		case <-deadline:
			t.Fatal("timed out waiting for crawl.completed webhook payload")
		}
	}
}

func TestV1SearchUsesTopLevelFetchOptionsAndAsyncEnvelope(t *testing.T) {
	t.Parallel()

	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[{"title":"Guide","url":"https://example.com/guide","description":"article"}]}}`), nil
		}),
	}, time.Second)

	handler := &Handler{
		jobStore:         jobs.NewStore(time.Minute),
		searchAsyncStore: quarrysearch.NewMemoryAsyncStore(time.Minute),
		searchClient:     searchClient,
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			if opts.WaitFor != 200 {
				t.Fatalf("WaitFor = %d, want 200", opts.WaitFor)
			}
			if !opts.Mobile {
				t.Fatal("Mobile = false, want true")
			}
			if opts.ProxyURL != "http://proxy.local:8080" {
				t.Fatalf("ProxyURL = %q, want http://proxy.local:8080", opts.ProxyURL)
			}
			return map[string]interface{}{"markdown": "# Guide"}, nil, nil
		},
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/search", map[string]interface{}{
		"query":   "quarry",
		"formats": []string{"markdown"},
		"waitFor": 200,
		"mobile":  true,
		"proxy": map[string]interface{}{
			"url": "http://proxy.local:8080",
		},
	})

	var created struct {
		ID           string `json:"id"`
		Resource     string `json:"resource"`
		EventsURL    string `json:"eventsUrl"`
		WebsocketURL string `json:"websocketUrl"`
	}
	decodeJSONResponse(t, resp, &created)

	if created.Resource != "search" {
		t.Fatalf("resource = %q, want search", created.Resource)
	}
	if created.EventsURL == "" || created.WebsocketURL == "" {
		t.Fatalf("events/ws urls = %q / %q, want populated", created.EventsURL, created.WebsocketURL)
	}

	status := waitForV1SearchStatus(t, app, created.ID, "completed")
	if len(status.Data) != 1 || status.Data[0].Content != "# Guide" {
		t.Fatalf("data = %+v, want one markdown result", status.Data)
	}
}

func TestV1SearchPresetResolvesOptions(t *testing.T) {
	t.Parallel()

	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[{"title":"Quarry team","url":"https://example.com/team","description":"leadership and customers"}]}}`), nil
		}),
	}, time.Second)

	handler := &Handler{
		jobStore:         jobs.NewStore(time.Minute),
		searchAsyncStore: quarrysearch.NewMemoryAsyncStore(time.Minute),
		searchClient:     searchClient,
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			if !containsString(opts.Formats, "branding") {
				t.Fatalf("formats = %v, want branding preset format", opts.Formats)
			}
			return map[string]interface{}{"markdown": "# Quarry team"}, nil, nil
		},
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/search", map[string]interface{}{
		"query":  "Quarry founders",
		"preset": "lead-enrichment",
	})
	if createResp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", createResp.StatusCode, http.StatusOK)
	}

	var created struct {
		ID              string                 `json:"id"`
		Resource        string                 `json:"resource"`
		ResolvedOptions map[string]interface{} `json:"resolvedOptions"`
	}
	decodeJSONResponse(t, createResp, &created)
	if created.Resource != "search" {
		t.Fatalf("resource = %q, want search", created.Resource)
	}
	if stringFromAny(created.ResolvedOptions["preset"], "") != "lead-enrichment" {
		t.Fatalf("resolved preset = %v, want lead-enrichment", created.ResolvedOptions["preset"])
	}

	status := waitForV1SearchStatus(t, app, created.ID, "completed")
	if len(status.Data) != 1 || status.Data[0].Content != "# Quarry team" {
		t.Fatalf("data = %+v, want one branded result", status.Data)
	}
}

func TestV1SearchSupportsIndexSource(t *testing.T) {
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
				{"knowledge_id":"kn-1","document_id":"doc-1","text":"Quarry stores crawl telemetry in the index.","score":0.98,"metadata":{"title":"Telemetry"}},
				{"knowledge_id":"kn-2","document_id":"doc-2","text":"Quarry records extract steps for research workflows.","score":0.88,"metadata":{"title":"Research"}}
			],
			"sources":[
				{"document_id":"doc-1","title":"Telemetry","source":"docs","type":"guide"},
				{"document_id":"doc-2","title":"Research","source":"docs","type":"guide"}
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
	handler.registerPlatformV1(app)

	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/search", map[string]interface{}{
		"query": "quarry",
		"sources": []map[string]interface{}{
			{"type": "index"},
		},
	})
	if createResp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", createResp.StatusCode, http.StatusOK)
	}

	var created asyncCreateEnvelope
	decodeJSONResponse(t, createResp, &created)

	status := waitForV1SearchStatus(t, app, created.ID, "completed")
	if len(status.Data) != 2 {
		t.Fatalf("data = %+v, want two index results", status.Data)
	}
	if status.Data[0].Type != "index" {
		t.Fatalf("first type = %q, want index", status.Data[0].Type)
	}
	if !strings.HasPrefix(status.Data[0].URL, "dataplane://knowledge/") {
		t.Fatalf("first url = %q, want dataplane knowledge url", status.Data[0].URL)
	}
	if status.Data[0].Snippet == "" {
		t.Fatal("first snippet = empty, want indexed content")
	}
}

func TestV1SearchRejectsUnknownPreset(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		jobStore:         jobs.NewStore(time.Minute),
		searchAsyncStore: quarrysearch.NewMemoryAsyncStore(time.Minute),
		searchClient:     quarrysearch.NewBraveClient("test-key", "https://unit.test", time.Second),
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/search", map[string]interface{}{
		"query":  "Quarry",
		"preset": "not-a-real-preset",
	})
	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusBadRequest)
	}
}

func TestV1SearchRankedBlendDeduplicatesAndAppliesWeights(t *testing.T) {
	t.Parallel()

	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[
				{"title":"Quarry Repo","url":"https://github.com/triodelab/quarry","description":"web mention"},
				{"title":"Quarry Docs","url":"https://example.com/docs","description":"docs"}
			]}}`), nil
		}),
	}, time.Second)
	githubClient := quarrysearch.NewGitHubClientWithHTTPClient("", "https://api.github.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusOK, `{"items":[
				{"full_name":"triodelab/quarry","html_url":"https://github.com/triodelab/quarry","description":"repo description"},
				{"full_name":"triodelab/platform","html_url":"https://github.com/triodelab/platform","description":"platform repo"}
			]}`), nil
		}),
	}, time.Second)

	handler := &Handler{
		jobStore:           jobs.NewStore(time.Minute),
		searchAsyncStore:   quarrysearch.NewMemoryAsyncStore(time.Minute),
		searchClient:       searchClient,
		githubSearchClient: githubClient,
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/search", map[string]interface{}{
		"query":     "quarry",
		"blendMode": "ranked",
		"sources": []map[string]interface{}{
			{"type": "web", "weight": 1},
			{"type": "github", "site": "triodelab", "weight": 3},
		},
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var created asyncCreateEnvelope
	decodeJSONResponse(t, resp, &created)
	resolved := mapFromAny(created.ResolvedOptions)
	if stringFromAny(resolved["blendMode"], "") != "ranked" {
		t.Fatalf("resolved blendMode = %v, want ranked", resolved["blendMode"])
	}

	status := waitForV1SearchStatus(t, app, created.ID, "completed")
	if len(status.Data) != 3 {
		t.Fatalf("data = %+v, want 3 unique results", status.Data)
	}
	if status.Data[0].URL != "https://github.com/triodelab/quarry" {
		t.Fatalf("first url = %q, want deduped github repo first", status.Data[0].URL)
	}
	if status.Data[0].Source != "github" {
		t.Fatalf("first source = %q, want github", status.Data[0].Source)
	}
	if status.Data[0].Score <= status.Data[1].Score {
		t.Fatalf("scores = %v <= %v, want descending weighted rank", status.Data[0].Score, status.Data[1].Score)
	}
}

func TestV1SearchInterleaveBlendRespectsPerSourceLimits(t *testing.T) {
	t.Parallel()

	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[
				{"title":"Web One","url":"https://example.com/one","description":"one"},
				{"title":"Web Two","url":"https://example.com/two","description":"two"}
			]}}`), nil
		}),
	}, time.Second)
	githubClient := quarrysearch.NewGitHubClientWithHTTPClient("", "https://api.github.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusOK, `{"items":[
				{"full_name":"triodelab/one","html_url":"https://github.com/triodelab/one","description":"one"},
				{"full_name":"triodelab/two","html_url":"https://github.com/triodelab/two","description":"two"}
			]}`), nil
		}),
	}, time.Second)

	handler := &Handler{
		jobStore:           jobs.NewStore(time.Minute),
		searchAsyncStore:   quarrysearch.NewMemoryAsyncStore(time.Minute),
		searchClient:       searchClient,
		githubSearchClient: githubClient,
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/search", map[string]interface{}{
		"query":     "quarry",
		"blendMode": "interleave",
		"limit":     5,
		"sources": []map[string]interface{}{
			{"type": "web", "limit": 1},
			{"type": "github", "site": "triodelab", "limit": 2},
		},
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var created asyncCreateEnvelope
	decodeJSONResponse(t, resp, &created)

	status := waitForV1SearchStatus(t, app, created.ID, "completed")
	if len(status.Data) != 3 {
		t.Fatalf("data = %+v, want 3 interleaved results", status.Data)
	}
	if status.Data[0].URL != "https://example.com/one" || status.Data[1].URL != "https://github.com/triodelab/one" || status.Data[2].URL != "https://github.com/triodelab/two" {
		t.Fatalf("order = %+v, want web then github then github", status.Data)
	}
}

func TestV1CrawlActiveListsRunningJobs(t *testing.T) {
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
	handler.registerPlatformV1(app)

	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/crawl", map[string]interface{}{
		"url": "https://example.com/docs/start",
	})

	var created struct {
		ID string `json:"id"`
	}
	decodeJSONResponse(t, createResp, &created)
	<-blocked

	activeResp := performJSONRequest(t, app, http.MethodGet, "/v1/crawl/active", nil)
	var active activeCrawlsEnvelope
	decodeJSONResponse(t, activeResp, &active)

	if !active.Success || active.Count != 1 {
		t.Fatalf("active = %+v, want count=1", active)
	}
	if active.Data[0].ID != created.ID {
		t.Fatalf("active id = %q, want %q", active.Data[0].ID, created.ID)
	}

	_ = performJSONRequest(t, app, http.MethodDelete, "/v1/crawl/"+created.ID, nil)
}

func TestV1CrawlJobsListsOnlyCrawlJobs(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		jobStore:   jobs.NewStore(time.Minute),
		crawlStore: quarrycrawl.NewMemoryStore(time.Minute),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			return map[string]interface{}{
				"links":    []string{},
				"markdown": "# page",
			}, nil, nil
		},
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/crawl", map[string]interface{}{
		"url": "https://example.com/docs/start",
	})

	var created struct {
		ID string `json:"id"`
	}
	decodeJSONResponse(t, createResp, &created)

	_ = waitForV1CrawlStatus(t, app, created.ID, func(status crawlStatusEnvelope) bool {
		return status.Status == "completed"
	})

	_ = handler.jobStore.New(map[string]string{"kind": "search", "query": "docs"})

	jobsResp := performJSONRequest(t, app, http.MethodGet, "/v1/crawl/jobs", nil)
	if jobsResp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", jobsResp.StatusCode, http.StatusOK)
	}

	var listed struct {
		Success bool                  `json:"success"`
		Count   int                   `json:"count"`
		Data    []activeCrawlResource `json:"data"`
	}
	decodeJSONResponse(t, jobsResp, &listed)

	if !listed.Success {
		t.Fatal("success = false, want true")
	}
	if listed.Count != 1 {
		t.Fatalf("count = %d, want 1", listed.Count)
	}
	if len(listed.Data) != 1 {
		t.Fatalf("len(data) = %d, want 1", len(listed.Data))
	}
	if listed.Data[0].ID != created.ID {
		t.Fatalf("listed id = %q, want %q", listed.Data[0].ID, created.ID)
	}
	if listed.Data[0].Status != "completed" {
		t.Fatalf("listed status = %q, want completed", listed.Data[0].Status)
	}
}

func TestV1BatchScrapeStatusErrorsAndCancel(t *testing.T) {
	t.Parallel()

	blocked := make(chan struct{})
	handler := &Handler{
		jobStore: jobs.NewStore(time.Minute),
		batchManager: batch.NewManager(stubBatchScraper{scrape: func(ctx context.Context, req *models.ScrapeRequest) (*models.ScrapeResult, error) {
			switch req.Collection {
			case "success":
				return &models.ScrapeResult{
					Count: 1,
					Products: []*models.Product{
						{Name: "ok"},
					},
				}, nil
			case "failure":
				return nil, fmt.Errorf("boom")
			case "slow":
				close(blocked)
				<-ctx.Done()
				return nil, ctx.Err()
			default:
				return nil, fmt.Errorf("unexpected collection %s", req.Collection)
			}
		}}, 2, time.Minute, ""),
	}
	t.Cleanup(func() {
		if err := handler.batchManager.Close(); err != nil {
			t.Fatalf("batchManager.Close() error = %v", err)
		}
	})

	app := fiber.New()
	handler.registerPlatformV1(app)

	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/batch/scrape", map[string]interface{}{
		"urls": []string{
			"https://example.com/collections/success",
			"https://example.com/collections/failure",
		},
	})

	var created struct {
		ID       string `json:"id"`
		Resource string `json:"resource"`
	}
	decodeJSONResponse(t, createResp, &created)
	if created.Resource != "batch/scrape" {
		t.Fatalf("resource = %q, want batch/scrape", created.Resource)
	}

	status := waitForV1BatchStatus(t, app, created.ID, func(status batchStatusEnvelope) bool {
		return status.Status == "completed"
	})
	if status.Completed != 1 || status.Failed != 1 {
		t.Fatalf("completed/failed = %d/%d, want 1/1", status.Completed, status.Failed)
	}

	errorsResp := performJSONRequest(t, app, http.MethodGet, "/v1/batch/scrape/"+created.ID+"/errors", nil)
	var errs batchErrorsEnvelope
	decodeJSONResponse(t, errorsResp, &errs)
	if len(errs.Errors) != 1 || errs.Errors[0].Error != "boom" {
		t.Fatalf("errors = %+v, want one boom error", errs.Errors)
	}

	cancelCreate := performJSONRequest(t, app, http.MethodPost, "/v1/batch/scrape", map[string]interface{}{
		"urls": []string{
			"https://example.com/collections/slow",
		},
	})
	var cancelJob struct {
		ID string `json:"id"`
	}
	decodeJSONResponse(t, cancelCreate, &cancelJob)
	<-blocked

	cancelResp := performJSONRequest(t, app, http.MethodDelete, "/v1/batch/scrape/"+cancelJob.ID, nil)
	var cancelled struct {
		Success bool   `json:"success"`
		Status  string `json:"status"`
	}
	decodeJSONResponse(t, cancelResp, &cancelled)
	if !cancelled.Success || cancelled.Status != "cancelled" {
		t.Fatalf("cancel response = %+v, want cancelled", cancelled)
	}

	cancelStatus := waitForV1BatchStatus(t, app, cancelJob.ID, func(status batchStatusEnvelope) bool {
		return status.Status == "cancelled"
	})
	if cancelStatus.Status != "cancelled" {
		t.Fatalf("status = %q, want cancelled", cancelStatus.Status)
	}
}

func TestV1ExtractUsesAsyncEnvelopeAndCanBeCancelled(t *testing.T) {
	t.Parallel()

	blocked := make(chan struct{})
	handler := &Handler{
		jobStore:           jobs.NewStore(time.Minute),
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			close(blocked)
			<-ctx.Done()
			return nil, nil, ctx.Err()
		},
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/extract", map[string]interface{}{
		"urls": []string{"https://example.com/report"},
	})

	if createResp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", createResp.StatusCode, http.StatusOK)
	}

	var created asyncCreateEnvelope
	decodeJSONResponse(t, createResp, &created)
	if created.Resource != "extract" {
		t.Fatalf("resource = %q, want extract", created.Resource)
	}
	if created.EventsURL == "" || created.WebsocketURL == "" {
		t.Fatalf("events/ws urls = %q / %q, want populated", created.EventsURL, created.WebsocketURL)
	}

	<-blocked

	cancelResp := performJSONRequest(t, app, http.MethodDelete, "/v1/extract/"+created.ID, nil)
	var cancelled struct {
		Success bool   `json:"success"`
		Status  string `json:"status"`
	}
	decodeJSONResponse(t, cancelResp, &cancelled)
	if !cancelled.Success || cancelled.Status != "cancelled" {
		t.Fatalf("cancel response = %+v, want cancelled", cancelled)
	}

	status := waitForV1ExtractStatus(t, app, created.ID, "cancelled")
	if status.Status != "cancelled" {
		t.Fatalf("status = %q, want cancelled", status.Status)
	}
}

func TestV1ExtractPresetResolvesOptions(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		jobStore:           jobs.NewStore(time.Minute),
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			if !containsString(opts.Formats, "branding") {
				t.Fatalf("formats = %v, want branding preset format", opts.Formats)
			}
			return map[string]interface{}{
				"markdown": "# Quarry company",
				"html":     "<html><body><h1>Quarry company</h1></body></html>",
				"branding": map[string]interface{}{"name": "Quarry"},
			}, nil, nil
		},
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/extract", map[string]interface{}{
		"urls":   []string{"https://example.com/company"},
		"preset": "lead-enrichment",
	})
	if createResp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", createResp.StatusCode, http.StatusOK)
	}

	var created struct {
		ID              string                 `json:"id"`
		Resource        string                 `json:"resource"`
		ResolvedOptions map[string]interface{} `json:"resolvedOptions"`
	}
	decodeJSONResponse(t, createResp, &created)
	if created.Resource != "extract" {
		t.Fatalf("resource = %q, want extract", created.Resource)
	}
	if stringFromAny(created.ResolvedOptions["preset"], "") != "lead-enrichment" {
		t.Fatalf("resolved preset = %v, want lead-enrichment", created.ResolvedOptions["preset"])
	}

	status := waitForV1ExtractStatus(t, app, created.ID, "completed")
	if stringFromAny(status.Data["preset"], "") != "lead-enrichment" {
		t.Fatalf("status preset = %v, want lead-enrichment", status.Data["preset"])
	}
	raw, ok := status.Data["https://example.com/company"].(map[string]interface{})
	if !ok {
		t.Fatalf("data[url] type = %T, want object", status.Data["https://example.com/company"])
	}
	if stringFromAny(raw["content"], "") != "# Quarry company" {
		t.Fatalf("content = %v, want # Quarry company", raw["content"])
	}
}

func TestV1ExtractRejectsUnknownPreset(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		jobStore:           jobs.NewStore(time.Minute),
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/extract", map[string]interface{}{
		"urls":   []string{"https://example.com/company"},
		"preset": "not-a-real-preset",
	})
	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusBadRequest)
	}
}

func TestV1ResearchCreatesAsyncJobAndReturnsSources(t *testing.T) {
	t.Parallel()

	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[{"title":"Quarry docs","url":"https://example.com/docs","description":"product docs"}]}}`), nil
		}),
	}, time.Second)

	handler := &Handler{
		jobStore:     jobs.NewStore(time.Minute),
		searchClient: searchClient,
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			return map[string]interface{}{
				"html":     "<html><body><h1>Quarry docs</h1><p>Quarry indexes content.</p></body></html>",
				"markdown": "# Quarry docs\n\nQuarry indexes content.",
			}, nil, nil
		},
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/research", map[string]interface{}{
		"query": "What is Quarry?",
		"limit": 1,
	})

	if createResp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", createResp.StatusCode, http.StatusOK)
	}

	var created asyncCreateEnvelope
	decodeJSONResponse(t, createResp, &created)
	if created.Resource != "research" {
		t.Fatalf("resource = %q, want research", created.Resource)
	}

	status := waitForV1ResearchStatus(t, app, created.ID, "completed")
	if len(status.Sources) != 1 {
		t.Fatalf("sources = %+v, want one source", status.Sources)
	}
	if status.Report == "" {
		t.Fatal("report = empty, want synthesized report")
	}
	if len(status.Steps) == 0 {
		t.Fatal("steps = empty, want execution steps")
	}
}

func TestV1ResearchSupportsGitHubSource(t *testing.T) {
	t.Parallel()

	githubClient := quarrysearch.NewGitHubClientWithHTTPClient("", "https://api.github.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			if r.URL.Path != "/search/repositories" {
				t.Fatalf("path = %q, want /search/repositories", r.URL.Path)
			}
			if !strings.Contains(r.URL.Query().Get("q"), "org:triodelab") {
				t.Fatalf("q = %q, want org qualifier", r.URL.Query().Get("q"))
			}
			return jsonHTTPResponse(http.StatusOK, `{
				"items":[
					{"full_name":"triodelab/quarry","html_url":"https://github.com/triodelab/quarry","description":"Distributed scraping platform"}
				]
			}`), nil
		}),
	}, time.Second)

	handler := &Handler{
		jobStore:           jobs.NewStore(time.Minute),
		githubSearchClient: githubClient,
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			if targetURL != "https://github.com/triodelab/quarry" {
				t.Fatalf("targetURL = %q, want GitHub repo URL", targetURL)
			}
			return map[string]interface{}{
				"markdown": "# triodelab/quarry\n\nDistributed scraping platform.",
			}, nil, nil
		},
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/research", map[string]interface{}{
		"query":         "quarry source",
		"limit":         1,
		"maxIterations": 1,
		"sources": []map[string]interface{}{
			{"type": "github", "site": "triodelab"},
		},
	})
	if createResp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", createResp.StatusCode, http.StatusOK)
	}

	var created asyncCreateEnvelope
	decodeJSONResponse(t, createResp, &created)

	status := waitForV1ResearchStatus(t, app, created.ID, "completed")
	if len(status.Sources) != 1 {
		t.Fatalf("sources = %+v, want one source", status.Sources)
	}
	if status.Sources[0].Type != "github" {
		t.Fatalf("source type = %q, want github", status.Sources[0].Type)
	}
	if status.Sources[0].Content == "" {
		t.Fatalf("source content = %q, want scraped markdown", status.Sources[0].Content)
	}
}

func TestV1ResearchPresetResolvesOptions(t *testing.T) {
	t.Parallel()

	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[{"title":"Quarry team","url":"https://example.com/team","description":"leadership and customers"}]}}`), nil
		}),
	}, time.Second)

	handler := &Handler{
		jobStore:     jobs.NewStore(time.Minute),
		searchClient: searchClient,
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			if !containsString(opts.Formats, "branding") {
				t.Fatalf("formats = %v, want branding preset format", opts.Formats)
			}
			return map[string]interface{}{
				"markdown": "# Quarry team\n\nFounders and customers.",
				"branding": map[string]interface{}{"name": "Quarry"},
			}, nil, nil
		},
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/research", map[string]interface{}{
		"query":         "Quarry founders",
		"preset":        "lead-enrichment",
		"maxIterations": 1,
	})
	if createResp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", createResp.StatusCode, http.StatusOK)
	}

	var created struct {
		ID              string                 `json:"id"`
		Resource        string                 `json:"resource"`
		ResolvedOptions map[string]interface{} `json:"resolvedOptions"`
	}
	decodeJSONResponse(t, createResp, &created)

	if created.Resource != "research" {
		t.Fatalf("resource = %q, want research", created.Resource)
	}
	if stringFromAny(created.ResolvedOptions["preset"], "") != "lead-enrichment" {
		t.Fatalf("resolved preset = %v, want lead-enrichment", created.ResolvedOptions["preset"])
	}
	if intFromAny(created.ResolvedOptions["maxIterations"]) != 1 {
		t.Fatalf("resolved maxIterations = %v, want 1", created.ResolvedOptions["maxIterations"])
	}

	status := waitForV1ResearchStatus(t, app, created.ID, "completed")
	if stringFromAny(status.Data["preset"], "") != "lead-enrichment" {
		t.Fatalf("status preset = %v, want lead-enrichment", status.Data["preset"])
	}
}

func TestV1ResearchPerformsFollowUpRounds(t *testing.T) {
	t.Parallel()

	var (
		mu      sync.Mutex
		queries []string
	)
	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			query := r.URL.Query().Get("q")
			mu.Lock()
			queries = append(queries, query)
			mu.Unlock()

			switch {
			case query == "quarry monitor":
				return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[{"title":"Overview","url":"https://example.com/overview","description":"pricing and availability overview"}]}}`), nil
			case strings.Contains(query, "pricing"):
				return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[{"title":"Pricing","url":"https://example.com/pricing","description":"pricing and plans"}]}}`), nil
			default:
				return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[]}}`), nil
			}
		}),
	}, time.Second)

	handler := &Handler{
		jobStore:     jobs.NewStore(time.Minute),
		searchClient: searchClient,
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			switch targetURL {
			case "https://example.com/overview":
				return map[string]interface{}{
					"markdown": "# Overview\n\nQuarry pricing availability monitoring for teams.",
				}, nil, nil
			case "https://example.com/pricing":
				return map[string]interface{}{
					"markdown": "# Pricing\n\nEnterprise pricing and plans.",
				}, nil, nil
			default:
				return nil, nil, fmt.Errorf("unexpected url %s", targetURL)
			}
		},
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/research", map[string]interface{}{
		"query":         "quarry monitor",
		"limit":         1,
		"maxIterations": 2,
	})
	if createResp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", createResp.StatusCode, http.StatusOK)
	}

	var created asyncCreateEnvelope
	decodeJSONResponse(t, createResp, &created)

	status := waitForV1ResearchStatus(t, app, created.ID, "completed")
	if len(status.Sources) != 2 {
		t.Fatalf("sources = %+v, want two unique sources", status.Sources)
	}
	if intFromAny(status.Data["iterations"]) != 2 {
		t.Fatalf("iterations = %v, want 2", status.Data["iterations"])
	}

	queryPlan := stringSliceFromAny(status.Data["queryPlan"])
	if len(queryPlan) < 2 {
		t.Fatalf("queryPlan = %v, want at least 2 queries", queryPlan)
	}
	if !containsAnySubstring(queryPlan, "pricing") {
		t.Fatalf("queryPlan = %v, want pricing follow-up query", queryPlan)
	}

	mu.Lock()
	recordedQueries := append([]string(nil), queries...)
	mu.Unlock()
	if len(recordedQueries) < 2 {
		t.Fatalf("queries = %v, want at least two search rounds", recordedQueries)
	}
	if !containsAnySubstring(recordedQueries, "pricing") {
		t.Fatalf("queries = %v, want pricing follow-up query", recordedQueries)
	}
}

func TestV1ResearchRejectsUnknownPreset(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		jobStore:     jobs.NewStore(time.Minute),
		searchClient: quarrysearch.NewBraveClient("test-key", "https://unit.test", time.Second),
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/research", map[string]interface{}{
		"query":  "Quarry",
		"preset": "not-a-real-preset",
	})
	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusBadRequest)
	}
}

func TestV1SearchRequiresIdempotencyKeyWhenEnabled(t *testing.T) {
	t.Parallel()

	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[]}}`), nil
		}),
	}, time.Second)

	handler := &Handler{
		jobStore:         jobs.NewStore(time.Minute),
		searchAsyncStore: quarrysearch.NewMemoryAsyncStore(time.Minute),
		searchClient:     searchClient,
		idempotencyStore: platform.NewIdempotencyStore(""),
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/search", map[string]interface{}{
		"query": "quarry",
	})
	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusBadRequest)
	}
}

func TestV1SearchReplaysIdempotentCreateResponse(t *testing.T) {
	t.Parallel()

	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[{"title":"Guide","url":"https://example.com/guide","description":"article"}]}}`), nil
		}),
	}, time.Second)

	handler := &Handler{
		jobStore:         jobs.NewStore(time.Minute),
		searchAsyncStore: quarrysearch.NewMemoryAsyncStore(time.Minute),
		searchClient:     searchClient,
		idempotencyStore: platform.NewIdempotencyStore(""),
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	headers := map[string]string{"Idempotency-Key": "search-key-1"}
	firstResp := performJSONRequestWithHeaders(t, app, http.MethodPost, "/v1/search", map[string]interface{}{
		"query": "quarry",
	}, headers)
	secondResp := performJSONRequestWithHeaders(t, app, http.MethodPost, "/v1/search", map[string]interface{}{
		"query": "quarry",
	}, headers)

	var firstCreated asyncCreateEnvelope
	var secondCreated asyncCreateEnvelope
	decodeJSONResponse(t, firstResp, &firstCreated)
	decodeJSONResponse(t, secondResp, &secondCreated)

	if firstCreated.ID == "" || secondCreated.ID == "" {
		t.Fatal("id = empty, want populated ids")
	}
	if firstCreated.ID != secondCreated.ID {
		t.Fatalf("ids = %q / %q, want identical replayed response", firstCreated.ID, secondCreated.ID)
	}
	if secondResp.Header.Get("X-Idempotent-Replayed") != "true" {
		t.Fatalf("X-Idempotent-Replayed = %q, want true", secondResp.Header.Get("X-Idempotent-Replayed"))
	}
}

func TestV1TeamEndpointsExposeOrgScopedStatus(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		jobStore: jobs.NewStore(time.Minute),
	}
	handler.jobStore.Upsert(&jobs.Job{
		ID:        "job-1",
		Status:    jobs.StatusRunning,
		CreatedAt: time.Now().Add(-time.Minute),
		UpdatedAt: time.Now().Add(-30 * time.Second),
		Meta: map[string]string{
			"kind":   "search",
			"org_id": "org-1",
			"query":  "quarry",
		},
		Result: map[string]any{"tokensUsed": 42},
	})
	handler.jobStore.Upsert(&jobs.Job{
		ID:        "job-2",
		Status:    jobs.StatusPending,
		CreatedAt: time.Now().Add(-2 * time.Minute),
		UpdatedAt: time.Now().Add(-90 * time.Second),
		Meta: map[string]string{
			"kind":      "crawl",
			"org_id":    "org-1",
			"url":       "https://example.com",
			"url_count": "2",
		},
		Result: map[string]any{},
	})
	handler.jobStore.Upsert(&jobs.Job{
		ID:        "job-3",
		Status:    jobs.StatusRunning,
		CreatedAt: time.Now().Add(-time.Minute),
		UpdatedAt: time.Now().Add(-time.Second),
		Meta: map[string]string{
			"kind":   "search",
			"org_id": "org-2",
		},
		Result: map[string]any{"tokensUsed": 999},
	})

	app := fiber.New()
	app.Use(func(c *fiber.Ctx) error {
		c.Locals(platform.PrincipalContextKey, &platform.Principal{
			UserID:         "user-1",
			OrganizationID: "org-1",
			Tier:           "pro",
		})
		return c.Next()
	})
	handler.registerPlatformV1(app)

	queueResp := performJSONRequest(t, app, http.MethodGet, "/v1/team/queue-status", nil)
	var queue teamQueueStatusEnvelope
	decodeJSONResponse(t, queueResp, &queue)
	if queue.Organization != "org-1" {
		t.Fatalf("organization = %q, want org-1", queue.Organization)
	}
	if queue.Counts["running"] != 1 || queue.Counts["pending"] != 1 {
		t.Fatalf("counts = %+v, want running=1 pending=1", queue.Counts)
	}

	tokenResp := performJSONRequest(t, app, http.MethodGet, "/v1/team/token-usage", nil)
	var tokenUsage teamTokenUsageEnvelope
	decodeJSONResponse(t, tokenResp, &tokenUsage)
	if tokenUsage.TotalTokens != 42 {
		t.Fatalf("totalTokens = %d, want 42", tokenUsage.TotalTokens)
	}

	concurrencyResp := performJSONRequest(t, app, http.MethodGet, "/v1/team/concurrency", nil)
	var concurrency teamConcurrencyEnvelope
	decodeJSONResponse(t, concurrencyResp, &concurrency)
	if concurrency.Active != 2 || concurrency.Limit != 10 {
		t.Fatalf("concurrency = %+v, want active=2 limit=10", concurrency)
	}

	activityResp := performJSONRequest(t, app, http.MethodGet, "/v1/team/activity", nil)
	var activity teamActivityEnvelope
	decodeJSONResponse(t, activityResp, &activity)
	if activity.Count != 2 {
		t.Fatalf("activity count = %d, want 2", activity.Count)
	}
}

type testAPILogger struct {
	userv1.UnimplementedUserServiceServer
	mu         sync.Mutex
	activities []*userv1.Activity
}

func (s *testAPILogger) LogActivity(_ context.Context, req *userv1.LogActivityRequest) (*userv1.LogActivityResponse, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	item := &userv1.Activity{
		Id:        "activity-" + req.GetAction(),
		UserId:    req.GetUserId(),
		Action:    req.GetAction(),
		Resource:  req.GetResource(),
		Details:   req.GetDetails(),
		CreatedAt: timestamppb.New(time.Now().UTC()),
	}
	s.activities = append(s.activities, item)
	return &userv1.LogActivityResponse{Activity: item}, nil
}

func (s *testAPILogger) ListActivities(_ context.Context, req *userv1.ListActivitiesRequest) (*userv1.ListActivitiesResponse, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := make([]*userv1.Activity, 0, len(s.activities))
	for _, item := range s.activities {
		if item.GetUserId() == req.GetUserId() {
			out = append(out, item)
		}
	}
	return &userv1.ListActivitiesResponse{Activities: out, Total: int32(len(out))}, nil
}

func startAPITestUserCore(t *testing.T) (string, *testAPILogger, func()) {
	t.Helper()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	server := grpc.NewServer()
	logger := &testAPILogger{}
	userv1.RegisterUserServiceServer(server, logger)
	go func() {
		_ = server.Serve(listener)
	}()
	return listener.Addr().String(), logger, func() {
		server.GracefulStop()
		_ = listener.Close()
	}
}

func TestV1TeamCreditUsageUsesControlPlane(t *testing.T) {
	t.Parallel()

	billingServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api/v1/billing/orgs/org-1/account":
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"org_id":"org-1","plan":"pro","subscription_state":"active","credits":120,"entitlements":{"feature.api_keys":true},"quota_limits":{"crawl_credits":500},"metadata":{"concurrency_limit":7}}`))
		case "/api/v1/billing/orgs/org-1/quotas/crawl_credits":
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"quota":{"org_id":"org-1","metric":"crawl_credits","limit":500,"used":123,"remaining":377,"is_exceeded":false,"utilization":0.246}}`))
		default:
			http.NotFound(w, r)
		}
	}))
	defer billingServer.Close()

	orgServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/orgs/org-1/entitlements" {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"organization_id":"org-1","entitlements":[{"key":"feature.api_keys","enabled":true},{"key":"feature.sso","enabled":false}]}`))
	}))
	defer orgServer.Close()

	handler := &Handler{
		jobStore: jobs.NewStore(time.Minute),
		controlPlane: platform.NewControlPlaneService(platform.ControlPlaneConfig{
			BillingBaseURL: billingServer.URL,
			OrgBaseURL:     orgServer.URL,
			CacheTTL:       time.Minute,
		}),
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
	handler.registerPlatformV1(app)

	resp := performJSONRequest(t, app, http.MethodGet, "/v1/team/credit-usage", nil)
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var payload teamCreditUsageEnvelope
	decodeJSONResponse(t, resp, &payload)
	if payload.Plan != "pro" || payload.Credits != 120 {
		t.Fatalf("payload = %+v, want plan=pro credits=120", payload)
	}
	if payload.Quota == nil || payload.Quota.Remaining != 377 {
		t.Fatalf("quota = %+v, want remaining=377", payload.Quota)
	}
	if !payload.Entitlements["feature.api_keys"] || payload.Entitlements["feature.sso"] {
		t.Fatalf("entitlements = %+v, want api_keys=true sso=false", payload.Entitlements)
	}
}

func TestV1TeamTokenUsageUsesControlPlaneQuota(t *testing.T) {
	t.Parallel()

	billingServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api/v1/billing/orgs/org-1/quotas/llm_tokens":
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"quota":{"org_id":"org-1","metric":"llm_tokens","limit":100000,"used":4242,"remaining":95758,"is_exceeded":false,"utilization":0.04242}}`))
		default:
			http.NotFound(w, r)
		}
	}))
	defer billingServer.Close()

	handler := &Handler{
		jobStore: jobs.NewStore(time.Minute),
		controlPlane: platform.NewControlPlaneService(platform.ControlPlaneConfig{
			BillingBaseURL: billingServer.URL,
			CacheTTL:       time.Minute,
		}),
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
	handler.registerPlatformV1(app)

	resp := performJSONRequest(t, app, http.MethodGet, "/v1/team/token-usage", nil)
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var payload teamTokenUsageEnvelope
	decodeJSONResponse(t, resp, &payload)
	if payload.TotalTokens != 4242 || payload.Source != "control-plane" || payload.Metric != "llm_tokens" {
		t.Fatalf("payload = %+v, want control-plane llm_tokens=4242", payload)
	}
}

func TestV1TeamActivityMergesUserCoreActivity(t *testing.T) {
	t.Parallel()

	target, logger, cleanup := startAPITestUserCore(t)
	defer cleanup()

	details, _ := structpb.NewStruct(map[string]interface{}{
		"jobId":   "job-user-1",
		"summary": "search queued",
		"query":   "quarry",
	})
	logger.activities = []*userv1.Activity{{
		Id:        "activity-1",
		UserId:    "user-1",
		Action:    "search.created",
		Resource:  "search",
		Details:   details,
		CreatedAt: timestamppb.New(time.Now().UTC().Add(-time.Second)),
	}}

	handler := &Handler{
		jobStore: jobs.NewStore(time.Minute),
		controlPlane: platform.NewControlPlaneService(platform.ControlPlaneConfig{
			UserBaseURL: target,
			CacheTTL:    time.Minute,
		}),
	}
	handler.jobStore.Upsert(&jobs.Job{
		ID:        "job-1",
		Status:    jobs.StatusRunning,
		CreatedAt: time.Now().Add(-2 * time.Second),
		UpdatedAt: time.Now().Add(-2 * time.Second),
		Meta: map[string]string{
			"kind":   "crawl",
			"org_id": "org-1",
			"url":    "https://example.com",
		},
	})

	app := fiber.New()
	app.Use(func(c *fiber.Ctx) error {
		c.Locals(platform.PrincipalContextKey, &platform.Principal{
			UserID:         "user-1",
			OrganizationID: "org-1",
			Tier:           "pro",
		})
		return c.Next()
	})
	handler.registerPlatformV1(app)

	resp := performJSONRequest(t, app, http.MethodGet, "/v1/team/activity", nil)
	var payload teamActivityEnvelope
	decodeJSONResponse(t, resp, &payload)
	if payload.Count != 2 {
		t.Fatalf("activity count = %d, want 2", payload.Count)
	}
	if payload.Data[0].Status != "search.created" {
		t.Fatalf("first activity status = %q, want search.created", payload.Data[0].Status)
	}
}

func TestV1ResearchBlockedWithoutEntitlement(t *testing.T) {
	t.Parallel()

	orgServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/orgs/org-1/entitlements" {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"organization_id":"org-1","entitlements":[{"key":"feature.research","enabled":false}]}`))
	}))
	defer orgServer.Close()

	billingServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/billing/orgs/org-1/account" {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"org_id":"org-1","plan":"pro","credits":50}`))
	}))
	defer billingServer.Close()

	service := platform.NewControlPlaneService(platform.ControlPlaneConfig{
		BillingBaseURL: billingServer.URL,
		OrgBaseURL:     orgServer.URL,
		CacheTTL:       time.Minute,
	})

	handler := &Handler{
		controlPlane: service,
	}

	app := fiber.New()
	app.Use(func(c *fiber.Ctx) error {
		c.Locals(platform.PrincipalContextKey, &platform.Principal{
			UserID:         "user-1",
			OrganizationID: "org-1",
			Tier:           "free",
		})
		return c.Next()
	})
	app.Use(platform.ControlPlanePolicyMiddleware(platform.ControlPlanePolicyConfig{Service: service}))
	handler.registerPlatformV1(app)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/research", map[string]interface{}{
		"query": "quarry",
	})
	if resp.StatusCode != http.StatusForbidden {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusForbidden)
	}
}

func TestV1SearchAllowedWhenFeatureDecisionIsUnknown(t *testing.T) {
	t.Parallel()

	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[]}}`), nil
		}),
	}, time.Second)

	billingServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/billing/orgs/org-1/account" {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"org_id":"org-1","plan":"pro","credits":50,"entitlements":{}}`))
	}))
	defer billingServer.Close()

	service := platform.NewControlPlaneService(platform.ControlPlaneConfig{
		BillingBaseURL: billingServer.URL,
		CacheTTL:       time.Minute,
	})

	handler := &Handler{
		jobStore:         jobs.NewStore(time.Minute),
		searchAsyncStore: quarrysearch.NewMemoryAsyncStore(time.Minute),
		searchClient:     searchClient,
		controlPlane:     service,
		idempotencyStore: platform.NewIdempotencyStore(""),
	}

	app := fiber.New()
	app.Use(func(c *fiber.Ctx) error {
		c.Locals(platform.PrincipalContextKey, &platform.Principal{
			UserID:         "user-1",
			OrganizationID: "org-1",
			Tier:           "free",
		})
		return c.Next()
	})
	app.Use(platform.ControlPlanePolicyMiddleware(platform.ControlPlanePolicyConfig{Service: service}))
	handler.registerPlatformV1(app)

	resp := performJSONRequestWithHeaders(t, app, http.MethodPost, "/v1/search", map[string]interface{}{
		"query": "quarry",
	}, map[string]string{
		"Idempotency-Key": "unknown-feature-search",
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}
}

func TestV1SearchRedactsStoredQueryForZDROrg(t *testing.T) {
	t.Parallel()

	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[{"title":"Guide","url":"https://example.com/guide","description":"article"}]}}`), nil
		}),
	}, time.Second)

	billingServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/billing/orgs/org-1/account" {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"org_id":"org-1","plan":"pro","credits":50,"metadata":{"zdr_mode":true}}`))
	}))
	defer billingServer.Close()

	service := platform.NewControlPlaneService(platform.ControlPlaneConfig{
		BillingBaseURL: billingServer.URL,
		CacheTTL:       time.Minute,
	})

	handler := &Handler{
		jobStore:         jobs.NewStore(time.Minute),
		searchAsyncStore: quarrysearch.NewMemoryAsyncStore(time.Minute),
		searchClient:     searchClient,
		controlPlane:     service,
		idempotencyStore: platform.NewIdempotencyStore(""),
	}

	app := fiber.New()
	app.Use(func(c *fiber.Ctx) error {
		c.Locals(platform.PrincipalContextKey, &platform.Principal{
			UserID:         "user-1",
			OrganizationID: "org-1",
			Tier:           "free",
		})
		return c.Next()
	})
	app.Use(platform.ControlPlanePolicyMiddleware(platform.ControlPlanePolicyConfig{Service: service}))
	handler.registerPlatformV1(app)

	resp := performJSONRequestWithHeaders(t, app, http.MethodPost, "/v1/search", map[string]interface{}{
		"query": "quarry internal roadmap",
	}, map[string]string{
		"Idempotency-Key": "zdr-search-1",
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var created asyncCreateEnvelope
	decodeJSONResponse(t, resp, &created)

	job, ok := handler.jobStore.Get(created.ID)
	if !ok || job == nil {
		t.Fatalf("job %s not found", created.ID)
	}
	if job.Meta["query"] != "" {
		t.Fatalf("stored query meta = %q, want redacted empty string", job.Meta["query"])
	}
	if resultQuery, _ := job.Result["query"].(string); resultQuery != "" {
		t.Fatalf("stored query result = %q, want redacted empty string", resultQuery)
	}

	run, err := handler.searchAsyncStore.GetRun(context.Background(), created.ID)
	if err != nil {
		t.Fatalf("GetRun: %v", err)
	}
	if run.Query != "" {
		t.Fatalf("search run query = %q, want redacted empty string", run.Query)
	}
}

func waitForV1CrawlStatus(t *testing.T, app *fiber.App, jobID string, done func(crawlStatusEnvelope) bool) crawlStatusEnvelope {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		resp := performJSONRequest(t, app, http.MethodGet, "/v1/crawl/"+jobID, nil)
		var status crawlStatusEnvelope
		decodeJSONResponse(t, resp, &status)
		if done(status) {
			return status
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for crawl status for %s", jobID)
	return crawlStatusEnvelope{}
}

func waitForV1SearchStatus(t *testing.T, app *fiber.App, jobID, want string) searchStatusEnvelope {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		resp := performJSONRequest(t, app, http.MethodGet, "/v1/search/"+jobID, nil)
		var status searchStatusEnvelope
		decodeJSONResponse(t, resp, &status)
		if status.Status == want {
			return status
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for search status %q", want)
	return searchStatusEnvelope{}
}

func waitForV1BatchStatus(t *testing.T, app *fiber.App, jobID string, done func(batchStatusEnvelope) bool) batchStatusEnvelope {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		resp := performJSONRequest(t, app, http.MethodGet, "/v1/batch/scrape/"+jobID, nil)
		var status batchStatusEnvelope
		decodeJSONResponse(t, resp, &status)
		if done(status) {
			return status
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for batch status for %s", jobID)
	return batchStatusEnvelope{}
}

func waitForV1ExtractStatus(t *testing.T, app *fiber.App, jobID, want string) extractStatusEnvelope {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		resp := performJSONRequest(t, app, http.MethodGet, "/v1/extract/"+jobID, nil)
		var status extractStatusEnvelope
		decodeJSONResponse(t, resp, &status)
		if status.Status == want {
			return status
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for extract status %q", want)
	return extractStatusEnvelope{}
}

func waitForV1ResearchStatus(t *testing.T, app *fiber.App, jobID, want string) researchStatusEnvelope {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		resp := performJSONRequest(t, app, http.MethodGet, "/v1/research/"+jobID, nil)
		var status researchStatusEnvelope
		decodeJSONResponse(t, resp, &status)
		if status.Status == want {
			return status
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for research status %q", want)
	return researchStatusEnvelope{}
}

func containsString(values []string, expected string) bool {
	for _, value := range values {
		if value == expected {
			return true
		}
	}
	return false
}

func containsAnySubstring(values []string, expected string) bool {
	for _, value := range values {
		if strings.Contains(value, expected) {
			return true
		}
	}
	return false
}
