package api

import (
	"context"
	"net/http"
	"sync"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/quarry/internal/asyncjobs"
	quarrycrawl "github.com/triodelab/quarry/internal/crawl"
	"github.com/triodelab/quarry/internal/jobs"
	quarrysearch "github.com/triodelab/quarry/internal/search"
)

type stubAsyncDispatcher struct {
	mu       sync.Mutex
	messages []asyncjobs.Message
	cancels  []asyncjobs.CancelMessage
}

func (s *stubAsyncDispatcher) Dispatch(_ context.Context, msg asyncjobs.Message) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.messages = append(s.messages, msg)
	return nil
}

func (s *stubAsyncDispatcher) Cancel(_ context.Context, msg asyncjobs.CancelMessage) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.cancels = append(s.cancels, msg)
	return nil
}

func (s *stubAsyncDispatcher) Close() error {
	return nil
}

func (s *stubAsyncDispatcher) messageCount() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	return len(s.messages)
}

func (s *stubAsyncDispatcher) cancelCount() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	return len(s.cancels)
}

func (s *stubAsyncDispatcher) firstMessage(t *testing.T) asyncjobs.Message {
	t.Helper()
	s.mu.Lock()
	defer s.mu.Unlock()
	if len(s.messages) == 0 {
		t.Fatal("dispatcher captured no messages")
	}
	return s.messages[0]
}

func TestV1CrawlDispatchesToWorkerQueue(t *testing.T) {
	t.Parallel()

	dispatcher := &stubAsyncDispatcher{}
	handler := &Handler{
		jobStore:        jobs.NewStore(time.Minute),
		crawlStore:      quarrycrawl.NewMemoryStore(time.Minute),
		asyncDispatcher: dispatcher,
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	scheduledAt := time.Now().Add(time.Minute).UTC()
	resp := performJSONRequest(t, app, http.MethodPost, "/v1/crawl", map[string]any{
		"url":        "https://example.com/docs",
		"preset":     "site-observability",
		"scheduleAt": scheduledAt.Format(time.RFC3339Nano),
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}
	if dispatcher.messageCount() != 1 {
		t.Fatalf("dispatch count = %d, want 1", dispatcher.messageCount())
	}

	msg := dispatcher.firstMessage(t)
	if msg.Kind != asyncjobs.KindCrawl {
		t.Fatalf("kind = %q, want crawl", msg.Kind)
	}

	var payload asyncjobs.CrawlPayload
	if err := msg.DecodePayload(&payload); err != nil {
		t.Fatalf("DecodePayload() error = %v", err)
	}
	if payload.Spec.URL != "https://example.com/docs" {
		t.Fatalf("payload url = %q, want https://example.com/docs", payload.Spec.URL)
	}
	if payload.Spec.Preset != "site-observability" {
		t.Fatalf("payload preset = %q, want site-observability", payload.Spec.Preset)
	}
	if payload.Spec.ScheduleAt == nil {
		t.Fatal("payload scheduleAt = nil, want populated")
	}
	if payload.Spec.ChangeTracking == nil || !payload.Spec.ChangeTracking.Enabled {
		t.Fatalf("payload changeTracking = %+v, want enabled preset change tracking", payload.Spec.ChangeTracking)
	}
	if job, ok := handler.jobStore.Get(msg.JobID); !ok || job == nil || job.Status != jobs.StatusPending {
		t.Fatalf("job status = %+v, want pending", job)
	}
}

func TestV1SearchDispatchesToWorkerQueue(t *testing.T) {
	t.Parallel()

	dispatcher := &stubAsyncDispatcher{}
	handler := &Handler{
		jobStore:         jobs.NewStore(time.Minute),
		searchAsyncStore: quarrysearch.NewMemoryAsyncStore(time.Minute),
		searchClient:     quarrysearch.NewBraveClient("test-key", "https://unit.test", time.Second),
		asyncDispatcher:  dispatcher,
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/search", map[string]any{
		"query":     "quarry",
		"blendMode": "interleave",
		"formats":   []string{"markdown"},
		"sources": []map[string]any{
			{"type": "web", "weight": 2, "limit": 1},
		},
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}
	if dispatcher.messageCount() != 1 {
		t.Fatalf("dispatch count = %d, want 1", dispatcher.messageCount())
	}

	msg := dispatcher.firstMessage(t)
	if msg.Kind != asyncjobs.KindSearch {
		t.Fatalf("kind = %q, want search", msg.Kind)
	}

	var payload asyncjobs.SearchPayload
	if err := msg.DecodePayload(&payload); err != nil {
		t.Fatalf("DecodePayload() error = %v", err)
	}
	if payload.Query != "quarry" {
		t.Fatalf("query = %q, want quarry", payload.Query)
	}
	if payload.BlendMode != "interleave" {
		t.Fatalf("blendMode = %q, want interleave", payload.BlendMode)
	}
	if payload.OrgID != "internal" {
		t.Fatalf("orgID = %q, want internal", payload.OrgID)
	}
	if !payload.ShouldScrape {
		t.Fatal("ShouldScrape = false, want true")
	}
	if len(payload.Sources) != 1 || payload.Sources[0].Weight != 2 || payload.Sources[0].Limit != 1 {
		t.Fatalf("sources = %+v, want weighted limited source", payload.Sources)
	}
}

func TestV1ExtractDispatchesToWorkerQueue(t *testing.T) {
	t.Parallel()

	dispatcher := &stubAsyncDispatcher{}
	handler := &Handler{
		jobStore:           jobs.NewStore(time.Minute),
		extractionJobStore: jobs.NewInMemoryJobStore(time.Minute),
		asyncDispatcher:    dispatcher,
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/extract", map[string]any{
		"urls": []string{"https://example.com/report"},
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}
	if dispatcher.messageCount() != 1 {
		t.Fatalf("dispatch count = %d, want 1", dispatcher.messageCount())
	}

	msg := dispatcher.firstMessage(t)
	if msg.Kind != asyncjobs.KindExtract {
		t.Fatalf("kind = %q, want extract", msg.Kind)
	}

	var payload asyncjobs.ExtractPayload
	if err := msg.DecodePayload(&payload); err != nil {
		t.Fatalf("DecodePayload() error = %v", err)
	}
	if len(payload.URLTrace) != 1 || payload.URLTrace[0] != "https://example.com/report" {
		t.Fatalf("urlTrace = %v, want [https://example.com/report]", payload.URLTrace)
	}
}

func TestV1ResearchDispatchesToWorkerQueue(t *testing.T) {
	t.Parallel()

	dispatcher := &stubAsyncDispatcher{}
	handler := &Handler{
		jobStore:        jobs.NewStore(time.Minute),
		searchClient:    quarrysearch.NewBraveClient("test-key", "https://unit.test", time.Second),
		asyncDispatcher: dispatcher,
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/research", map[string]any{
		"query":         "What is Quarry?",
		"limit":         2,
		"blendMode":     "interleave",
		"preset":        "competitive-monitor",
		"maxIterations": 3,
	})
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}
	if dispatcher.messageCount() != 1 {
		t.Fatalf("dispatch count = %d, want 1", dispatcher.messageCount())
	}

	msg := dispatcher.firstMessage(t)
	if msg.Kind != asyncjobs.KindResearch {
		t.Fatalf("kind = %q, want research", msg.Kind)
	}

	var payload asyncjobs.ResearchPayload
	if err := msg.DecodePayload(&payload); err != nil {
		t.Fatalf("DecodePayload() error = %v", err)
	}
	if payload.Query != "What is Quarry?" {
		t.Fatalf("query = %q, want What is Quarry?", payload.Query)
	}
	if payload.Limit != 2 {
		t.Fatalf("limit = %d, want 2", payload.Limit)
	}
	if payload.Preset != "competitive-monitor" {
		t.Fatalf("preset = %q, want competitive-monitor", payload.Preset)
	}
	if payload.BlendMode != "interleave" {
		t.Fatalf("blendMode = %q, want interleave", payload.BlendMode)
	}
	if payload.OrgID != "internal" {
		t.Fatalf("orgID = %q, want internal", payload.OrgID)
	}
	if payload.MaxIterations != 3 {
		t.Fatalf("maxIterations = %d, want 3", payload.MaxIterations)
	}
}

func TestV1SearchCancelPublishesWorkerCancel(t *testing.T) {
	t.Parallel()

	dispatcher := &stubAsyncDispatcher{}
	handler := &Handler{
		jobStore:         jobs.NewStore(time.Minute),
		searchAsyncStore: quarrysearch.NewMemoryAsyncStore(time.Minute),
		searchClient:     quarrysearch.NewBraveClient("test-key", "https://unit.test", time.Second),
		asyncDispatcher:  dispatcher,
	}

	app := fiber.New()
	handler.registerPlatformV1(app)

	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/search", map[string]any{
		"query": "quarry",
	})
	var created struct {
		ID string `json:"id"`
	}
	decodeJSONResponse(t, createResp, &created)

	cancelResp := performJSONRequest(t, app, http.MethodDelete, "/v1/search/"+created.ID, nil)
	if cancelResp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", cancelResp.StatusCode, http.StatusOK)
	}
	if dispatcher.cancelCount() != 1 {
		t.Fatalf("cancel count = %d, want 1", dispatcher.cancelCount())
	}
}
