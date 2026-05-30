package batch

import (
	"context"
	"fmt"
	"testing"
	"time"

	"github.com/triodelab/quarry/internal/models"
)

type fakeBatchScraper struct{}

func (f *fakeBatchScraper) ScrapeCollection(_ context.Context, req *models.ScrapeRequest) (*models.ScrapeResult, error) {
	if req == nil || req.Collection == "" {
		return nil, fmt.Errorf("missing collection")
	}
	return &models.ScrapeResult{Count: 1, Products: []*models.Product{}}, nil
}

func TestExtractCollection(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		url  string
		want string
	}{
		{name: "collections path", url: "https://example.com/collections/serum", want: "serum"},
		{name: "produktkategori path", url: "https://shop.no/produktkategori/rens", want: "rens"},
		{name: "fallback last segment", url: "https://example.com/category/toner", want: "toner"},
		{name: "invalid url", url: "://", want: ""},
	}

	for _, tt := range tests {
		tt := tt
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if got := extractCollection(tt.url); got != tt.want {
				t.Fatalf("extractCollection() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestBatchManagerProcesses100PlusURLs(t *testing.T) {
	t.Parallel()

	manager := NewManager(&fakeBatchScraper{}, 25, 2*time.Hour, "")
	urls := make([]string, 0, 120)
	for i := 0; i < 120; i++ {
		urls = append(urls, fmt.Sprintf("https://example.com/collections/serum-%d", i))
	}

	job := manager.CreateJob(&models.BatchScrapeRequest{URLs: urls, MaxAge: 1000})
	if err := manager.StartJob(context.Background(), job.ID); err != nil {
		t.Fatalf("StartJob() error = %v", err)
	}

	status, err := manager.WaitForCompletion(context.Background(), job.ID, 10*time.Second, 20*time.Millisecond)
	if err != nil {
		t.Fatalf("WaitForCompletion() error = %v", err)
	}
	if status.Status != "completed" {
		t.Fatalf("status = %q, want completed", status.Status)
	}
	if status.Total != 120 {
		t.Fatalf("total = %d, want 120", status.Total)
	}
	if status.Completed != 120 {
		t.Fatalf("completed = %d, want 120", status.Completed)
	}
	if status.Failed != 0 {
		t.Fatalf("failed = %d, want 0", status.Failed)
	}
}
