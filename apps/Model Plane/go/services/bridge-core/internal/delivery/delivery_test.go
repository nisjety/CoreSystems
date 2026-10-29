package delivery

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"
)

func TestMemoryStore_EnqueueAndClaim(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore()

	rec, err := s.Enqueue(ctx, Record{SessionID: "sess", Channel: "web", Destination: "http://x", Payload: []byte(`{"a":1}`), MaxAttempts: 3})
	if err != nil {
		t.Fatalf("enqueue: %v", err)
	}
	if rec.ID == "" {
		t.Fatal("expected generated ID")
	}
	if rec.Status != StatusPending {
		t.Fatalf("expected pending, got %s", rec.Status)
	}

	due, err := s.ClaimDue(ctx, time.Now().UTC().Add(time.Second), 10)
	if err != nil {
		t.Fatalf("claim: %v", err)
	}
	if len(due) != 1 {
		t.Fatalf("expected 1 due record, got %d", len(due))
	}
}

func TestMemoryStore_ClaimLeasesPreventDoubleProcessing(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore()
	if _, err := s.Enqueue(ctx, Record{SessionID: "s", Channel: "web", Destination: "u", MaxAttempts: 3}); err != nil {
		t.Fatal(err)
	}

	first, _ := s.ClaimDue(ctx, time.Now().UTC().Add(time.Second), 10)
	second, _ := s.ClaimDue(ctx, time.Now().UTC().Add(time.Second), 10)
	if len(first) != 1 {
		t.Fatalf("first claim expected 1, got %d", len(first))
	}
	if len(second) != 0 {
		t.Fatalf("second claim should see nothing (leased), got %d", len(second))
	}
}

func TestMemoryStore_FutureRecordsNotClaimed(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore()
	rec, _ := s.Enqueue(ctx, Record{SessionID: "s", Channel: "web", Destination: "u", MaxAttempts: 3})
	// Re-arm into the future.
	if err := s.MarkRetry(ctx, rec.ID, "boom", time.Now().UTC().Add(time.Hour)); err != nil {
		t.Fatal(err)
	}
	due, _ := s.ClaimDue(ctx, time.Now().UTC(), 10)
	if len(due) != 0 {
		t.Fatalf("future record should not be due, got %d", len(due))
	}
}

func TestWorker_DeliversOnSuccess(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore()
	rec, _ := s.Enqueue(ctx, Record{SessionID: "s", Channel: "web", Destination: "u", MaxAttempts: 3})

	var sent int
	w := NewWorker(s, SenderFunc(func(_ context.Context, _ Record) error {
		sent++
		return nil
	}), DefaultConfig())

	n := w.RunOnce(ctx)
	if n != 1 {
		t.Fatalf("expected to process 1, got %d", n)
	}
	got, _ := s.Get(ctx, rec.ID)
	if got.Status != StatusDelivered {
		t.Fatalf("expected delivered, got %s", got.Status)
	}
	if sent != 1 {
		t.Fatalf("expected 1 send, got %d", sent)
	}
}

func TestWorker_RetriesThenDeadLetters(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore()
	rec, _ := s.Enqueue(ctx, Record{SessionID: "s", Channel: "web", Destination: "u", MaxAttempts: 3})

	failErr := errors.New("upstream down")
	w := NewWorker(s, SenderFunc(func(_ context.Context, _ Record) error {
		return failErr
	}), Config{BaseBackoff: time.Nanosecond, MaxBackoff: time.Nanosecond})

	// Attempt 1 -> retry, attempt 2 -> retry, attempt 3 -> dead.
	for i := 0; i < 3; i++ {
		// Each pass the record is due (backoff is ~0).
		time.Sleep(time.Millisecond)
		w.RunOnce(ctx)
	}

	got, _ := s.Get(ctx, rec.ID)
	if got.Status != StatusDead {
		t.Fatalf("expected dead after exhausting attempts, got %s (attempts=%d)", got.Status, got.Attempts)
	}
	if got.Attempts != 3 {
		t.Fatalf("expected 3 attempts, got %d", got.Attempts)
	}
	if got.LastError != failErr.Error() {
		t.Fatalf("expected last error recorded, got %q", got.LastError)
	}
}

func TestWorker_BackoffGrowsAndCaps(t *testing.T) {
	w := NewWorker(NewMemoryStore(), SenderFunc(func(context.Context, Record) error { return nil }), Config{
		BaseBackoff: time.Second,
		MaxBackoff:  10 * time.Second,
	})
	if got := w.backoff(1); got != time.Second {
		t.Fatalf("attempt 1 backoff: want 1s, got %s", got)
	}
	if got := w.backoff(2); got != 2*time.Second {
		t.Fatalf("attempt 2 backoff: want 2s, got %s", got)
	}
	if got := w.backoff(3); got != 4*time.Second {
		t.Fatalf("attempt 3 backoff: want 4s, got %s", got)
	}
	// Large attempt must cap at MaxBackoff and never overflow negative.
	if got := w.backoff(50); got != 10*time.Second {
		t.Fatalf("attempt 50 backoff: want cap 10s, got %s", got)
	}
}

func TestWorker_ConcurrentSafety(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore()
	for i := 0; i < 100; i++ {
		if _, err := s.Enqueue(ctx, Record{SessionID: "s", Channel: "web", Destination: "u", MaxAttempts: 2}); err != nil {
			t.Fatal(err)
		}
	}
	var mu sync.Mutex
	delivered := map[string]int{}
	w := NewWorker(s, SenderFunc(func(_ context.Context, r Record) error {
		mu.Lock()
		delivered[r.ID]++
		mu.Unlock()
		return nil
	}), DefaultConfig())

	var wg sync.WaitGroup
	for i := 0; i < 8; i++ {
		wg.Add(1)
		go func() { defer wg.Done(); w.RunOnce(ctx) }()
	}
	wg.Wait()

	// Every record delivered exactly once despite concurrent drains.
	for id, c := range delivered {
		if c != 1 {
			t.Fatalf("record %s delivered %d times, want 1", id, c)
		}
	}
	pending, _ := s.PendingCount(ctx)
	if pending != 0 {
		t.Fatalf("expected 0 pending after drain, got %d", pending)
	}
}
