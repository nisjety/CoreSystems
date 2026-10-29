package publisher_test

import (
	"context"
	encjson "encoding/json"
	"errors"
	"sync"
	"testing"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/pkg/publisher"
)

// fakeRaw is a test double for the byte-level publisher that NATSPublisher
// delegates to after serializing the envelope.
type fakeRaw struct {
	mu      sync.Mutex
	calls   []fakeRawCall
	failErr error
}

type fakeRawCall struct {
	Subject string
	Data    []byte
}

func (f *fakeRaw) Publish(subject string, data []byte) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	// copy data so the caller can reuse the buffer
	buf := make([]byte, len(data))
	copy(buf, data)
	f.calls = append(f.calls, fakeRawCall{Subject: subject, Data: buf})
	return f.failErr
}

func (f *fakeRaw) Calls() []fakeRawCall {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := make([]fakeRawCall, len(f.calls))
	copy(out, f.calls)
	return out
}

func TestNATSPublisher_SatisfiesInterface(t *testing.T) {
	var _ publisher.EventPublisher = publisher.NewNATSPublisher(&fakeRaw{})
}

func TestNATSPublisher_PublishMarshalsEnvelopeAndDelegates(t *testing.T) {
	raw := &fakeRaw{}
	p := publisher.NewNATSPublisher(raw)

	env := newTestEnvelope(t, "evt-nats-1")
	subject := "events.model.request.started"

	if err := p.Publish(context.Background(), subject, env); err != nil {
		t.Fatalf("Publish returned error: %v", err)
	}

	calls := raw.Calls()
	if len(calls) != 1 {
		t.Fatalf("expected 1 raw call, got %d", len(calls))
	}
	if calls[0].Subject != subject {
		t.Errorf("subject mismatch: got %q, want %q", calls[0].Subject, subject)
	}

	// Round-trip the JSON: it must decode back to an equivalent envelope.
	var got envelope.Envelope
	if err := encjson.Unmarshal(calls[0].Data, &got); err != nil {
		t.Fatalf("delegated bytes not valid envelope JSON: %v (bytes=%s)", err, string(calls[0].Data))
	}
	if got.EventID != env.EventID {
		t.Errorf("round-trip event_id: got %q, want %q", got.EventID, env.EventID)
	}
	if got.IdempotencyKey != env.IdempotencyKey {
		t.Errorf("round-trip idempotency_key: got %q, want %q", got.IdempotencyKey, env.IdempotencyKey)
	}
}

func TestNATSPublisher_NilEnvelopeReturnsSerializationError(t *testing.T) {
	raw := &fakeRaw{}
	p := publisher.NewNATSPublisher(raw)

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
	if len(raw.Calls()) != 0 {
		t.Errorf("expected raw publisher not to be called on nil envelope, got %d calls", len(raw.Calls()))
	}
}

func TestNATSPublisher_RawFailureReturnsTransportError(t *testing.T) {
	transportErr := errors.New("nats: connection refused")
	raw := &fakeRaw{failErr: transportErr}
	p := publisher.NewNATSPublisher(raw)

	err := p.Publish(context.Background(), "events.test", newTestEnvelope(t, "evt-fail"))
	if err == nil {
		t.Fatal("expected error when raw publisher fails, got nil")
	}
	var pubErr *publisher.PublishError
	if !errors.As(err, &pubErr) {
		t.Fatalf("expected *PublishError, got %T", err)
	}
	if pubErr.Kind != publisher.KindTransport {
		t.Errorf("expected KindTransport, got %v", pubErr.Kind)
	}
	if !errors.Is(err, transportErr) {
		t.Errorf("expected wrapped transport error, got %v", err)
	}
}
