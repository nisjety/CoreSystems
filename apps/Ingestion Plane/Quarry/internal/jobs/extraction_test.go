package jobs

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"testing"
	"time"
)

func TestInMemoryExtractionStoreLifecycle(t *testing.T) {
	t.Parallel()

	store := NewInMemoryExtractionStore(time.Minute)
	t.Cleanup(func() {
		if err := store.Close(); err != nil {
			t.Fatalf("Close() error = %v", err)
		}
	})

	ctx := context.Background()
	original := &ExtractionJob{
		ID:     "job-1",
		URL:    "https://example.com/item",
		Status: ExtractionQueued,
		Result: map[string]interface{}{
			"name": "alpha",
			"meta": map[string]interface{}{"category": "serum"},
		},
	}

	if err := store.Create(ctx, original); err != nil {
		t.Fatalf("Create() error = %v", err)
	}

	original.Result["name"] = "mutated-after-create"

	stored, err := store.Get(ctx, original.ID)
	if err != nil {
		t.Fatalf("Get() error = %v", err)
	}
	if got := stored.Result["name"]; got != "alpha" {
		t.Fatalf("Create() should deep-clone input, got name %v", got)
	}

	stored.Result["name"] = "mutated-after-get"
	storedAgain, err := store.Get(ctx, original.ID)
	if err != nil {
		t.Fatalf("Get() second call error = %v", err)
	}
	if got := storedAgain.Result["name"]; got != "alpha" {
		t.Fatalf("Get() should return clones, got name %v", got)
	}

	time.Sleep(2 * time.Millisecond)
	storedAgain.Status = ExtractionCompleted
	storedAgain.Result["name"] = "completed"
	if err := store.Update(ctx, storedAgain); err != nil {
		t.Fatalf("Update() error = %v", err)
	}

	storedAgain.Result["name"] = "mutated-after-update"
	completed, err := store.Get(ctx, original.ID)
	if err != nil {
		t.Fatalf("Get() after update error = %v", err)
	}
	if completed.Status != ExtractionCompleted {
		t.Fatalf("status = %q, want %q", completed.Status, ExtractionCompleted)
	}
	if got := completed.Result["name"]; got != "completed" {
		t.Fatalf("Update() should deep-clone input, got name %v", got)
	}
	if completed.Duration <= 0 {
		t.Fatalf("Duration = %d, want > 0 for terminal status", completed.Duration)
	}

	second := &ExtractionJob{ID: "job-2", URL: "https://example.com/other", Status: ExtractionQueued}
	if err := store.Create(ctx, second); err != nil {
		t.Fatalf("Create(job-2) error = %v", err)
	}

	list, err := store.List(ctx, 1)
	if err != nil {
		t.Fatalf("List() error = %v", err)
	}
	if len(list) != 1 {
		t.Fatalf("List() len = %d, want 1", len(list))
	}
	if list[0].ID != second.ID {
		t.Fatalf("List() first job = %q, want %q", list[0].ID, second.ID)
	}

	if err := store.Delete(ctx, second.ID); err != nil {
		t.Fatalf("Delete() error = %v", err)
	}
	if _, err := store.Get(ctx, second.ID); !errors.Is(err, ErrJobNotFound) {
		t.Fatalf("Get() after Delete() error = %v, want %v", err, ErrJobNotFound)
	}
}

func TestInMemoryExtractionStoreDuplicateAndExpiry(t *testing.T) {
	t.Parallel()

	store := NewInMemoryExtractionStore(20 * time.Millisecond)
	t.Cleanup(func() {
		if err := store.Close(); err != nil {
			t.Fatalf("Close() error = %v", err)
		}
	})

	ctx := context.Background()
	job := &ExtractionJob{ID: "job-dup", URL: "https://example.com/item"}
	if err := store.Create(ctx, job); err != nil {
		t.Fatalf("Create() error = %v", err)
	}
	if err := store.Create(ctx, job); !errors.Is(err, ErrJobExists) {
		t.Fatalf("Create() duplicate error = %v, want %v", err, ErrJobExists)
	}

	time.Sleep(35 * time.Millisecond)

	if _, err := store.Get(ctx, job.ID); !errors.Is(err, ErrJobNotFound) {
		t.Fatalf("Get() after TTL error = %v, want %v", err, ErrJobNotFound)
	}

	list, err := store.List(ctx, 10)
	if err != nil {
		t.Fatalf("List() error = %v", err)
	}
	if len(list) != 0 {
		t.Fatalf("List() len = %d, want 0 after expiry", len(list))
	}
}

func TestInMemoryExtractionStoreConcurrentUpdates(t *testing.T) {
	t.Parallel()

	store := NewInMemoryExtractionStore(time.Minute)
	t.Cleanup(func() {
		if err := store.Close(); err != nil {
			t.Fatalf("Close() error = %v", err)
		}
	})

	ctx := context.Background()
	if err := store.Create(ctx, &ExtractionJob{
		ID:     "job-concurrent",
		URL:    "https://example.com/item",
		Status: ExtractionQueued,
	}); err != nil {
		t.Fatalf("Create() error = %v", err)
	}

	var wg sync.WaitGroup
	errCh := make(chan error, 32)

	for i := 0; i < 32; i++ {
		i := i
		wg.Add(1)
		go func() {
			defer wg.Done()

			job, err := store.Get(ctx, "job-concurrent")
			if err != nil {
				errCh <- fmt.Errorf("Get(): %w", err)
				return
			}

			job.Status = ExtractionProcessing
			job.Result = map[string]interface{}{
				"worker": i,
				"nested": map[string]interface{}{"value": i},
			}

			if i%2 == 0 {
				job.Status = ExtractionCompleted
			}

			if err := store.Update(ctx, job); err != nil {
				errCh <- fmt.Errorf("Update(): %w", err)
			}
		}()
	}

	wg.Wait()
	close(errCh)

	for err := range errCh {
		if err != nil {
			t.Fatal(err)
		}
	}

	job, err := store.Get(ctx, "job-concurrent")
	if err != nil {
		t.Fatalf("Get() final error = %v", err)
	}
	if job.Result == nil {
		t.Fatalf("Result should be populated after concurrent updates")
	}
}

func TestInMemoryExtractionStoreCloseIsIdempotent(t *testing.T) {
	t.Parallel()

	store := NewInMemoryExtractionStore(time.Minute)
	if err := store.Close(); err != nil {
		t.Fatalf("Close() first call error = %v", err)
	}
	if err := store.Close(); err != nil {
		t.Fatalf("Close() second call error = %v", err)
	}
}
