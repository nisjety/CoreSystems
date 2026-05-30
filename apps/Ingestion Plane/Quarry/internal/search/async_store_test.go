package search

import (
	"context"
	"testing"
	"time"

	"github.com/triodelab/quarry/internal/config"
)

func TestMemoryAsyncStorePagination(t *testing.T) {
	t.Parallel()

	store := NewMemoryAsyncStore(time.Minute)
	run := &AsyncRun{
		ID:        "search-1",
		Query:     "quarry",
		CreatedAt: time.Now(),
		UpdatedAt: time.Now(),
		ExpiresAt: time.Now().Add(time.Minute),
	}
	if err := store.CreateRun(context.Background(), run); err != nil {
		t.Fatalf("CreateRun() error = %v", err)
	}

	if err := store.ReplaceResults(context.Background(), run.ID, []StoredResult{
		{Title: "One", URL: "https://example.com/one"},
		{Title: "Two", URL: "https://example.com/two"},
		{Title: "Three", URL: "https://example.com/three"},
	}); err != nil {
		t.Fatalf("ReplaceResults() error = %v", err)
	}

	results, total, err := store.ListResults(context.Background(), run.ID, 1, 1)
	if err != nil {
		t.Fatalf("ListResults() error = %v", err)
	}
	if total != 3 {
		t.Fatalf("total = %d, want 3", total)
	}
	if len(results) != 1 || results[0].Title != "Two" {
		t.Fatalf("results = %+v, want second item only", results)
	}
}

func TestNewAsyncStoreFallsBackToMemoryWhenBackendUnavailable(t *testing.T) {
	tests := []struct {
		name string
		cfg  *config.Config
	}{
		{
			name: "redis without url",
			cfg: &config.Config{
				JobStoreBackend: "redis",
			},
		},
		{
			name: "postgres without dsn",
			cfg: &config.Config{
				JobStoreBackend: "postgres",
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			store := NewAsyncStore(tt.cfg, time.Minute)
			defer func() { _ = store.Close() }()

			if _, ok := store.(*memoryAsyncStore); !ok {
				t.Fatalf("store type = %T, want *memoryAsyncStore fallback", store)
			}
		})
	}
}
