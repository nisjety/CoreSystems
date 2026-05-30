package compat

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"sync"
	"testing"

	"github.com/triodelab/model-plane/pkg/envelope"
)

type capturingPublisher struct {
	mu       sync.Mutex
	subjects []string
	payloads [][]byte
	failWith error
}

func (c *capturingPublisher) Publish(subject string, data []byte) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.failWith != nil {
		return c.failWith
	}
	buf := make([]byte, len(data))
	copy(buf, data)
	c.subjects = append(c.subjects, subject)
	c.payloads = append(c.payloads, buf)
	return nil
}

func (c *capturingPublisher) snapshot() ([]string, [][]byte) {
	c.mu.Lock()
	defer c.mu.Unlock()
	return append([]string(nil), c.subjects...), append([][]byte(nil), c.payloads...)
}

func newTestSubscriber() (*Subscriber, *capturingPublisher) {
	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	pub := &capturingPublisher{}
	return NewSubscriber(pub, logger), pub
}

func TestHandleLegacyMessage_TranslatesAndRepublishes(t *testing.T) {
	sub, pub := newTestSubscriber()

	body := map[string]any{
		"event_id":       "evt-1",
		"event_type":     "RUN_STARTED",
		"correlation_id": "corr-1",
		"causation_id":   "cause-1",
		"org_id":         "org-1",
		"user_id":        "user-1",
		"run_id":         "run-123",
	}
	data, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	if err := sub.HandleLegacyMessage(context.Background(), "velion.agent.run.run-123.event", data); err != nil {
		t.Fatalf("HandleLegacyMessage: %v", err)
	}

	subjects, payloads := pub.snapshot()
	if len(subjects) != 1 {
		t.Fatalf("expected 1 published subject, got %d", len(subjects))
	}
	if subjects[0] != "mp.v1.run.run-123.event" {
		t.Errorf("translated subject = %q, want mp.v1.run.run-123.event", subjects[0])
	}

	var env envelope.Envelope
	if err := json.Unmarshal(payloads[0], &env); err != nil {
		t.Fatalf("decode envelope: %v", err)
	}
	if env.EventID != "evt-1" {
		t.Errorf("EventID = %q, want evt-1", env.EventID)
	}
	if env.EventType != "RUN_STARTED" {
		t.Errorf("EventType = %q, want RUN_STARTED", env.EventType)
	}
	if env.CorrelationID != "corr-1" {
		t.Errorf("CorrelationID = %q, want corr-1", env.CorrelationID)
	}
	if env.OrgID != "org-1" {
		t.Errorf("OrgID = %q, want org-1", env.OrgID)
	}
	if env.ResourceRef != "run-123" {
		t.Errorf("ResourceRef = %q, want run-123", env.ResourceRef)
	}
	if env.Producer != "compat-adapter" {
		t.Errorf("Producer = %q, want compat-adapter", env.Producer)
	}
	if env.SchemaVersion != 1 {
		t.Errorf("SchemaVersion = %d, want 1", env.SchemaVersion)
	}
}

func TestHandleLegacyMessage_AqenciaSubject_RepublishesToIngress(t *testing.T) {
	sub, pub := newTestSubscriber()

	body := map[string]any{
		"event_id":       "evt-aq-1",
		"event_type":     "usage.recorded",
		"correlation_id": "corr-aq-1",
		"org_id":         "org-aq-1",
		"user_id":        "user-aq-1",
	}
	data, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	if err := sub.HandleLegacyMessage(context.Background(), "aqencia.reasoning.usage.recorded", data); err != nil {
		t.Fatalf("HandleLegacyMessage: %v", err)
	}

	subjects, payloads := pub.snapshot()
	if len(subjects) != 1 {
		t.Fatalf("expected 1 published subject, got %d", len(subjects))
	}
	if subjects[0] != "mp.v1.ingress.usage" {
		t.Errorf("translated subject = %q, want mp.v1.ingress.usage", subjects[0])
	}

	var env envelope.Envelope
	if err := json.Unmarshal(payloads[0], &env); err != nil {
		t.Fatalf("decode envelope: %v", err)
	}
	if env.EventID != "evt-aq-1" {
		t.Errorf("EventID = %q, want evt-aq-1", env.EventID)
	}
	if env.CorrelationID != "corr-aq-1" {
		t.Errorf("CorrelationID = %q, want corr-aq-1", env.CorrelationID)
	}
	if env.OrgID != "org-aq-1" {
		t.Errorf("OrgID = %q, want org-aq-1", env.OrgID)
	}
	if env.Producer != "compat-adapter" {
		t.Errorf("Producer = %q, want compat-adapter", env.Producer)
	}
}

func TestHandleLegacyMessage_UnmappedSubject_IsNoop(t *testing.T) {
	sub, pub := newTestSubscriber()

	err := sub.HandleLegacyMessage(context.Background(), "some.unknown.subject", []byte(`{}`))
	if err != nil {
		t.Fatalf("HandleLegacyMessage: %v", err)
	}

	subjects, _ := pub.snapshot()
	if len(subjects) != 0 {
		t.Errorf("expected no publishes for unmapped subject, got %d", len(subjects))
	}
}

func TestHandleLegacyMessage_InvalidJSON_ReturnsError(t *testing.T) {
	sub, pub := newTestSubscriber()

	err := sub.HandleLegacyMessage(context.Background(), "velion.agent.run.r1.event", []byte("{not json"))
	if err == nil {
		t.Fatal("expected parse error, got nil")
	}

	subjects, _ := pub.snapshot()
	if len(subjects) != 0 {
		t.Errorf("expected no publishes on parse error, got %d", len(subjects))
	}
}

func TestHandleLegacyMessage_PublishError_Propagates(t *testing.T) {
	sub, pub := newTestSubscriber()
	pub.failWith = errors.New("nats down")

	data, _ := json.Marshal(map[string]any{"run_id": "r1"})
	err := sub.HandleLegacyMessage(context.Background(), "velion.agent.run.r1.event", data)
	if err == nil || err.Error() != "nats down" {
		t.Fatalf("err = %v, want nats down", err)
	}
}
