package publisher_test

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/pkg/publisher"
)

func newTestEnvelope(t *testing.T, id string) *envelope.Envelope {
	t.Helper()
	return &envelope.Envelope{
		EventID:        id,
		EventType:      "model.request.started",
		SchemaVersion:  1,
		Ts:             time.Unix(1_700_000_000, 0).UTC(),
		Producer:       "test-producer",
		ResourceRef:    "model://test",
		IdempotencyKey: "idem-" + id,
		Payload:        json(`{}`),
	}
}

// json is a tiny helper so we don't pull in encoding/json in every test.
func json(s string) []byte { return []byte(s) }

func TestInMemoryPublisher_SatisfiesInterface(t *testing.T) {
	var _ publisher.EventPublisher = publisher.NewInMemoryPublisher()
}

func TestInMemoryPublisher_PublishAppends(t *testing.T) {
	p := publisher.NewInMemoryPublisher()
	ctx := context.Background()

	env := newTestEnvelope(t, "evt-1")
	if err := p.Publish(ctx, "events.model.request.started", env); err != nil {
		t.Fatalf("Publish returned error: %v", err)
	}

	records := p.Drain()
	if len(records) != 1 {
		t.Fatalf("expected 1 record, got %d", len(records))
	}
	if records[0].Subject != "events.model.request.started" {
		t.Errorf("unexpected subject: %q", records[0].Subject)
	}
	if records[0].Envelope.EventID != "evt-1" {
		t.Errorf("unexpected event_id: %q", records[0].Envelope.EventID)
	}
}

func TestInMemoryPublisher_DrainClearsBuffer(t *testing.T) {
	p := publisher.NewInMemoryPublisher()
	ctx := context.Background()

	for i, id := range []string{"a", "b", "c"} {
		_ = i
		if err := p.Publish(ctx, "events.test", newTestEnvelope(t, id)); err != nil {
			t.Fatalf("Publish failed: %v", err)
		}
	}

	first := p.Drain()
	if len(first) != 3 {
		t.Fatalf("expected 3 records on first drain, got %d", len(first))
	}
	second := p.Drain()
	if len(second) != 0 {
		t.Fatalf("expected 0 records on second drain, got %d", len(second))
	}
}

func TestInMemoryPublisher_ConcurrentPublishIsSafe(t *testing.T) {
	p := publisher.NewInMemoryPublisher()
	ctx := context.Background()

	const goroutines = 16
	const perGoroutine = 32

	var wg sync.WaitGroup
	wg.Add(goroutines)
	for g := 0; g < goroutines; g++ {
		go func(g int) {
			defer wg.Done()
			for i := 0; i < perGoroutine; i++ {
				env := newTestEnvelope(t, "x")
				if err := p.Publish(ctx, "events.test", env); err != nil {
					t.Errorf("Publish failed in goroutine %d: %v", g, err)
					return
				}
			}
		}(g)
	}
	wg.Wait()

	records := p.Drain()
	if got, want := len(records), goroutines*perGoroutine; got != want {
		t.Errorf("expected %d records, got %d", want, got)
	}
}

func TestInMemoryPublisher_NilEnvelopeReturnsSerializationError(t *testing.T) {
	p := publisher.NewInMemoryPublisher()
	err := p.Publish(context.Background(), "events.test", nil)
	if err == nil {
		t.Fatal("expected error for nil envelope, got nil")
	}
	var pubErr *publisher.PublishError
	if !errors.As(err, &pubErr) {
		t.Fatalf("expected *PublishError, got %T", err)
	}
	if pubErr.Kind != publisher.KindSerialization {
		t.Errorf("expected KindSerialization, got %v", pubErr.Kind)
	}
}

func TestPublishError_KindsAreDistinguishable(t *testing.T) {
	serErr := publisher.NewSerializationError(errors.New("bad json"))
	trErr := publisher.NewTransportError(errors.New("conn refused"))

	if serErr.Kind == trErr.Kind {
		t.Fatal("serialization and transport kinds must differ")
	}
	if serErr.Unwrap() == nil {
		t.Error("Unwrap should return wrapped error")
	}
	if trErr.Error() == "" {
		t.Error("Error() should be non-empty")
	}
}
