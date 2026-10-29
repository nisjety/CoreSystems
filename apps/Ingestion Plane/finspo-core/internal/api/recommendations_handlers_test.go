package api

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/google/uuid"

	"github.com/triodelab/finspo/internal/store"
)

type fakeRecommender struct {
	dupDrafts    []store.RecommendationDraft
	dupErr       error
	inactive     store.RecommendationDraft
	inactiveErr  error
	dupCall      struct {
		org       string
		minBytes  int64
		maxGroups int
	}
}

func (f *fakeRecommender) Duplicates(_ context.Context, org string, minBytes int64, maxGroups int) ([]store.RecommendationDraft, error) {
	f.dupCall.org = org
	f.dupCall.minBytes = minBytes
	f.dupCall.maxGroups = maxGroups
	return f.dupDrafts, f.dupErr
}

func (f *fakeRecommender) Inactive(_ context.Context, _ string, _ time.Duration, _ int) (store.RecommendationDraft, error) {
	return f.inactive, f.inactiveErr
}

func newRecommendationServer(rec Recommender) interface {
	Test(*http.Request, ...int) (*http.Response, error)
} {
	return NewServer(ServerConfig{APIKey: "key", Recommender: rec})
}

func doRec(t *testing.T, app interface {
	Test(*http.Request, ...int) (*http.Response, error)
}, path string) *http.Response {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, path, nil)
	req.Header.Set("X-API-Key", "key")
	req.Header.Set("X-Org-ID", "org-1")
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	return resp
}

func TestRecommendationsCombinesDuplicatesAndInactive(t *testing.T) {
	t.Parallel()

	rec := &fakeRecommender{
		dupDrafts: []store.RecommendationDraft{
			{Kind: "delete", Reason: "dup", ItemPKs: []uuid.UUID{uuid.New()}, EstimatedBytes: 100},
		},
		inactive: store.RecommendationDraft{Kind: "archive", Reason: "stale", ItemPKs: []uuid.UUID{uuid.New()}, EstimatedBytes: 50},
	}
	resp := doRec(t, newRecommendationServer(rec), "/api/v1/recommendations")
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d", resp.StatusCode)
	}
	var payload struct {
		Data struct {
			Count               int   `json:"count"`
			EstimatedBytesTotal int64 `json:"estimated_bytes_total"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&payload); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if payload.Data.Count != 2 {
		t.Errorf("count = %d, want 2", payload.Data.Count)
	}
	if payload.Data.EstimatedBytesTotal != 150 {
		t.Errorf("estimated total = %d, want 150", payload.Data.EstimatedBytesTotal)
	}
}

func TestRecommendationsOmitsInactiveWhenNone(t *testing.T) {
	t.Parallel()

	rec := &fakeRecommender{
		dupDrafts:   []store.RecommendationDraft{{Kind: "delete", ItemPKs: []uuid.UUID{uuid.New()}}},
		inactiveErr: store.ErrNotFound,
	}
	resp := doRec(t, newRecommendationServer(rec), "/api/v1/recommendations")
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d", resp.StatusCode)
	}
	var payload struct {
		Data struct {
			Count int `json:"count"`
		} `json:"data"`
	}
	_ = json.NewDecoder(resp.Body).Decode(&payload)
	if payload.Data.Count != 1 {
		t.Errorf("count = %d, want 1 (inactive omitted)", payload.Data.Count)
	}
}

func TestRecommendationsRejectsBadDuration(t *testing.T) {
	t.Parallel()

	resp := doRec(t, newRecommendationServer(&fakeRecommender{}), "/api/v1/recommendations?older_than=nonsense")
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400", resp.StatusCode)
	}
}
