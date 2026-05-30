package client

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestStartSearchPostsPresetRequest(t *testing.T) {
	t.Parallel()

	type capturedRequest struct {
		Path          string
		Method        string
		Authorization string
		Query         string
		Preset        string
	}

	received := make(chan capturedRequest, 1)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		defer r.Body.Close()

		var payload SearchRequest
		if err := json.NewDecoder(r.Body).Decode(&payload); err != nil {
			t.Fatalf("Decode() error = %v", err)
		}
		received <- capturedRequest{
			Path:          r.URL.Path,
			Method:        r.Method,
			Authorization: r.Header.Get("Authorization"),
			Query:         payload.Query,
			Preset:        payload.Preset,
		}

		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(AsyncCreateResponse{
			Success:  true,
			ID:       "job-123",
			Resource: "search",
			Status:   "queued",
		})
	}))
	defer server.Close()

	client := New(server.URL, "secret", WithHTTPClient(server.Client()))
	resp, err := client.StartSearch(context.Background(), LeadEnrichmentSearch("Quarry founders"))
	if err != nil {
		t.Fatalf("StartSearch() error = %v", err)
	}
	if resp.ID != "job-123" || resp.Resource != "search" {
		t.Fatalf("response = %+v, want queued search envelope", resp)
	}

	got := <-received
	if got.Method != http.MethodPost {
		t.Fatalf("method = %q, want POST", got.Method)
	}
	if got.Path != "/v1/search" {
		t.Fatalf("path = %q, want /v1/search", got.Path)
	}
	if got.Authorization != "Bearer secret" {
		t.Fatalf("authorization = %q, want Bearer secret", got.Authorization)
	}
	if got.Query != "Quarry founders" {
		t.Fatalf("query = %q, want Quarry founders", got.Query)
	}
	if got.Preset != "lead-enrichment" {
		t.Fatalf("preset = %q, want lead-enrichment", got.Preset)
	}
}

func TestPresetHelpersExposeWorkflowDefaults(t *testing.T) {
	t.Parallel()

	scheduledAt := time.Date(2026, 4, 3, 8, 0, 0, 0, time.UTC)
	crawlReq := SiteObservabilityCrawl("https://example.com", &scheduledAt)
	if crawlReq.Preset != "site-observability" {
		t.Fatalf("crawl preset = %q, want site-observability", crawlReq.Preset)
	}
	if crawlReq.ScheduleAt == nil || !crawlReq.ScheduleAt.Equal(scheduledAt) {
		t.Fatalf("crawl scheduleAt = %v, want %v", crawlReq.ScheduleAt, scheduledAt)
	}

	extractReq := LeadEnrichmentExtract("Find company contacts")
	if extractReq.Preset != "lead-enrichment" {
		t.Fatalf("extract preset = %q, want lead-enrichment", extractReq.Preset)
	}
	if !extractReq.EnableWebSearch {
		t.Fatal("extract enableWebSearch = false, want true")
	}

	researchReq := CompetitiveMonitorResearch("Quarry alternatives")
	if researchReq.Preset != "competitive-monitor" {
		t.Fatalf("research preset = %q, want competitive-monitor", researchReq.Preset)
	}
}

func TestStartSearchPostsBlendModeAndWeightedSources(t *testing.T) {
	t.Parallel()

	type capturedRequest struct {
		BlendMode string
		Sources   []SearchSource
	}

	received := make(chan capturedRequest, 1)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		defer r.Body.Close()

		var payload SearchRequest
		if err := json.NewDecoder(r.Body).Decode(&payload); err != nil {
			t.Fatalf("Decode() error = %v", err)
		}
		received <- capturedRequest{
			BlendMode: payload.BlendMode,
			Sources:   payload.Sources,
		}

		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(AsyncCreateResponse{
			Success:  true,
			ID:       "job-456",
			Resource: "search",
			Status:   "queued",
		})
	}))
	defer server.Close()

	client := New(server.URL, "secret", WithHTTPClient(server.Client()))
	_, err := client.StartSearch(context.Background(), SearchRequest{
		Query:     "quarry",
		BlendMode: "interleave",
		Sources: []SearchSource{
			{Type: "web", Weight: 2, Limit: 1},
			{Type: "github", Site: "triodelab", Weight: 1.5, Limit: 2},
		},
	})
	if err != nil {
		t.Fatalf("StartSearch() error = %v", err)
	}

	got := <-received
	if got.BlendMode != "interleave" {
		t.Fatalf("blendMode = %q, want interleave", got.BlendMode)
	}
	if len(got.Sources) != 2 || got.Sources[0].Weight != 2 || got.Sources[0].Limit != 1 {
		t.Fatalf("sources = %+v, want weighted limited source payload", got.Sources)
	}
}
