package eventtrigger

import (
	"context"
	"errors"
	"testing"
)

// fakeRecorder stands in for the fire table's record step.
type fakeRecorder struct {
	events   []WebhookReceived
	recorded int
	err      error
}

func (r *fakeRecorder) record(_ context.Context, event WebhookReceived) (int, error) {
	r.events = append(r.events, event)
	if r.err != nil {
		return 0, r.err
	}
	return r.recorded, nil
}

func TestProcessRecordsAWellFormedEventAndAcknowledgesIt(t *testing.T) {
	rec := &fakeRecorder{recorded: 2}
	if got := newConsumer(rec).process(context.Background(), []byte(validEnvelope)); got != deliveryAck {
		t.Fatalf("delivery = %v; want ack", got)
	}
	if len(rec.events) != 1 || rec.events[0].WebhookEventID != "wh-1" || rec.events[0].OrgID != "org-1" {
		t.Fatalf("recorded events = %+v", rec.events)
	}
}

func TestProcessDropsAMalformedEventWithoutRecordingIt(t *testing.T) {
	rec := &fakeRecorder{}
	if got := newConsumer(rec).process(context.Background(), []byte(`{"organizationId":"org-1"}`)); got != deliveryAck {
		t.Fatalf("delivery = %v; a malformed event is acknowledged and dropped", got)
	}
	if len(rec.events) != 0 {
		t.Fatal("a malformed event reached the fire table")
	}
}

func TestProcessRetriesOnlyAFailedWrite(t *testing.T) {
	rec := &fakeRecorder{err: errors.New("database unavailable")}
	if got := newConsumer(rec).process(context.Background(), []byte(validEnvelope)); got != deliveryRetry {
		t.Fatalf("delivery = %v; a failed write is redelivered", got)
	}
}

func TestNewConsumerNeedsADatabase(t *testing.T) {
	if _, err := NewConsumer(nil); err == nil {
		t.Fatal("a consumer without a database was built")
	}
}

func TestTheIngestionSubjectAndStreamAreThePublishersNames(t *testing.T) {
	if WebhookReceivedSubject != "verevon.ingestion.integration.webhook_received" || WebhookStream != "VEREVON_INGESTION" {
		t.Fatalf("subject %q on stream %q", WebhookReceivedSubject, WebhookStream)
	}
}
