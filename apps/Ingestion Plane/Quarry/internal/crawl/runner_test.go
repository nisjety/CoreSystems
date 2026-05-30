package crawl

import (
	"context"
	"errors"
	"testing"
	"time"
)

func TestRun_TraversesRecursivelyAndStoresDocuments(t *testing.T) {
	t.Parallel()

	store := NewMemoryStore(time.Hour)
	run := &Run{
		ID:        "crawl-1",
		URL:       "https://example.com/docs/start",
		Status:    StatusRunning,
		CreatedAt: time.Now(),
		UpdatedAt: time.Now(),
		ExpiresAt: time.Now().Add(time.Hour),
	}
	if err := store.CreateRun(context.Background(), run); err != nil {
		t.Fatalf("CreateRun() error = %v", err)
	}

	spec := Spec{
		URL:               "https://example.com/docs/start",
		Limit:             10,
		CrawlEntireDomain: true,
		Sitemap:           SitemapSkip,
		MaxConcurrency:    2,
		PageOptions: PageOptions{
			Formats: []Format{{Type: "markdown"}},
		},
	}

	graph := map[string]*FetchedPage{
		"https://example.com/docs/start": {
			URL: "https://example.com/docs/start",
			Links: []string{
				"/docs/a",
				"/docs/b",
			},
			Outputs: map[string]any{"markdown": "# start"},
		},
		"https://example.com/docs/a": {
			URL: "https://example.com/docs/a",
			Links: []string{
				"/docs/c",
			},
			Outputs: map[string]any{"markdown": "# a"},
		},
		"https://example.com/docs/b": {
			URL:     "https://example.com/docs/b",
			Links:   nil,
			Outputs: map[string]any{"markdown": "# b"},
		},
		"https://example.com/docs/c": {
			URL:     "https://example.com/docs/c",
			Links:   nil,
			Outputs: map[string]any{"markdown": "# c"},
		},
	}

	err := Execute(context.Background(), store, run, spec, func(ctx context.Context, item Item, spec Spec) (*FetchedPage, error) {
		page, ok := graph[item.URL]
		if !ok {
			return nil, errors.New("missing page")
		}
		return page, nil
	})
	if err != nil {
		t.Fatalf("Run() error = %v", err)
	}

	storedRun, err := store.GetRun(context.Background(), run.ID)
	if err != nil {
		t.Fatalf("GetRun() error = %v", err)
	}
	if storedRun.Status != StatusCompleted {
		t.Fatalf("run status = %q, want %q", storedRun.Status, StatusCompleted)
	}
	if storedRun.Completed != 4 {
		t.Fatalf("completed = %d, want 4", storedRun.Completed)
	}

	docs, total, err := store.ListDocuments(context.Background(), run.ID, 0, 10)
	if err != nil {
		t.Fatalf("ListDocuments() error = %v", err)
	}
	if total != 4 {
		t.Fatalf("total = %d, want 4", total)
	}
	if len(docs) != 4 {
		t.Fatalf("len(docs) = %d, want 4", len(docs))
	}
}

func TestRun_StopsAtLimit(t *testing.T) {
	t.Parallel()

	store := NewMemoryStore(time.Hour)
	run := &Run{
		ID:        "crawl-limit",
		URL:       "https://example.com/root",
		Status:    StatusRunning,
		CreatedAt: time.Now(),
		UpdatedAt: time.Now(),
		ExpiresAt: time.Now().Add(time.Hour),
	}
	if err := store.CreateRun(context.Background(), run); err != nil {
		t.Fatalf("CreateRun() error = %v", err)
	}

	spec := Spec{
		URL:               "https://example.com/root",
		Limit:             2,
		MaxConcurrency:    4,
		Sitemap:           SitemapSkip,
		CrawlEntireDomain: true,
	}

	graph := map[string]*FetchedPage{
		"https://example.com/root": {URL: "https://example.com/root", Links: []string{"/a", "/b", "/c"}},
		"https://example.com/a":    {URL: "https://example.com/a"},
		"https://example.com/b":    {URL: "https://example.com/b"},
		"https://example.com/c":    {URL: "https://example.com/c"},
	}

	err := Execute(context.Background(), store, run, spec, func(ctx context.Context, item Item, spec Spec) (*FetchedPage, error) {
		return graph[item.URL], nil
	})
	if err != nil {
		t.Fatalf("Run() error = %v", err)
	}

	storedRun, err := store.GetRun(context.Background(), run.ID)
	if err != nil {
		t.Fatalf("GetRun() error = %v", err)
	}
	if storedRun.Completed != 2 {
		t.Fatalf("completed = %d, want 2", storedRun.Completed)
	}
}

func TestRun_MarksCancelledWhenContextStops(t *testing.T) {
	t.Parallel()

	store := NewMemoryStore(time.Hour)
	run := &Run{
		ID:        "crawl-cancelled",
		URL:       "https://example.com/root",
		Status:    StatusRunning,
		CreatedAt: time.Now(),
		UpdatedAt: time.Now(),
		ExpiresAt: time.Now().Add(time.Hour),
	}
	if err := store.CreateRun(context.Background(), run); err != nil {
		t.Fatalf("CreateRun() error = %v", err)
	}

	spec := Spec{
		URL:            "https://example.com/root",
		Limit:          10,
		MaxConcurrency: 1,
		Sitemap:        SitemapSkip,
	}

	ctx, cancel := context.WithCancel(context.Background())

	err := Execute(ctx, store, run, spec, func(ctx context.Context, item Item, spec Spec) (*FetchedPage, error) {
		cancel()
		<-ctx.Done()
		return nil, ctx.Err()
	})
	if err == nil {
		t.Fatal("Run() error = nil, want cancellation error")
	}

	storedRun, getErr := store.GetRun(context.Background(), run.ID)
	if getErr != nil {
		t.Fatalf("GetRun() error = %v", getErr)
	}
	if storedRun.Status != StatusCancelled {
		t.Fatalf("run status = %q, want %q", storedRun.Status, StatusCancelled)
	}
}
